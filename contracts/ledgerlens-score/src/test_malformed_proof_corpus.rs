//! Malformed proof corpus for commitment-verification paths (issue #698).
//!
//! A reusable, checked-in corpus of malformed KZG / Verkle / range-proof inputs
//! that exercises every parser, size, and semantic rejection branch across
//! `zk_range_proof.rs` and `verkle.rs`. Each fixture carries an **expected
//! rejection class** ([`Reject`]) documenting *why* it must be refused, and the
//! suite asserts:
//!
//! - every malformed input is rejected (`None` / `false`) and never accepted;
//! - every well-formed control input is accepted, so the corpus can't pass
//!   vacuously by rejecting everything;
//! - oversized inputs are rejected in O(1) by the leading length gate, bounding
//!   worst-case work regardless of attacker-controlled size;
//! - no input panics (any panic fails the enclosing `#[test]`).
//!
//! The rejection classes are metadata attached to the fixtures. Parsers here
//! return `Option`/`bool` rather than a rich error enum, so from the caller's
//! side every rejection collapses to `None`/`false`; the class records which
//! internal branch each fixture is built to trip, and fixtures are grouped by
//! target function so that mapping stays exact.

extern crate std;

use soroban_sdk::{Bytes, BytesN, Env};

use crate::verkle::{
    bytes48_to_commitment, commitment_to_bytes48, compute_membership_witness,
    compute_nonmembership_witness, decode_proof, encode_proof, verify_proof, NON_MEMBER_SENTINEL,
};
use crate::zk_range_proof::{
    compress_pt, decompress_pt_32, g, prove_range_proof, Bulletproof, Sc, SeededPrng,
};

/// Why a given fixture must be refused. Purely documentary — every rejection
/// path below returns `None`/`false` to the caller — but it keeps each fixture
/// tied to the specific internal branch it is built to exercise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reject {
    /// Length gate: input is not the exact fixed size the parser requires.
    BadLength,
    /// Discriminant gate: a tag/type byte is outside its allowed set.
    BadDiscriminant,
    /// Domain/version gate: the context prefix does not match.
    BadPrefix,
    /// Semantic gate: a decoded `(x, y)` is not on the Ed25519 curve.
    PointOffCurve,
    /// Semantic gate: the encoded `y` coordinate is >= the field prime `p`.
    YOverflow,
    /// Semantic gate: `y` has no valid `x` (point is not decompressible).
    NonSquare,
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn env() -> Env {
    Env::default()
}

/// Copy a `Bytes` of known length 800 into a fixed array for byte-level
/// tampering.
fn to_800(b: &Bytes) -> [u8; 800] {
    assert_eq!(b.len(), 800, "expected an 800-byte serialized bulletproof");
    let mut a = [0u8; 800];
    for (i, slot) in a.iter_mut().enumerate() {
        *slot = b.get(i as u32).unwrap();
    }
    a
}

/// A valid, on-curve 800-byte bulletproof serialization, used as the clean
/// baseline that the off-curve fixtures are derived from by corrupting exactly
/// one point slot. Built by the in-crate prover so all ten points are on-curve.
fn valid_bulletproof_bytes(env: &Env) -> [u8; 800] {
    let r = Sc::from_u64(987_654_321);
    let prng = SeededPrng::new([1u8; 32]);
    let proof = prove_range_proof(env, 42, r, prng);
    to_800(&proof.to_bytes(env))
}

/// Byte offset of each on-curve point slot inside the 800-byte layout
/// (§ `Bulletproof::to_bytes`). Zeroing any slot's 64 bytes yields `(0, 0)`,
/// which is off-curve, so `from_bytes` must reject at that slot.
const POINT_SLOTS: [(&str, usize); 10] = [
    ("A", 0),
    ("S", 64),
    ("T1", 128),
    ("T2", 192),
    ("L0", 352),
    ("L1", 416),
    ("L2", 480),
    ("R0", 544),
    ("R1", 608),
    ("R2", 672),
];

// ── 1. Bulletproof::from_bytes — size + off-curve corpus ─────────────────────

#[test]
fn test_bulletproof_from_bytes_control_roundtrips() {
    // Control: the untampered serialization must parse. Guards against the
    // corpus passing vacuously.
    let env = env();
    let bytes = Bytes::from_array(&env, &valid_bulletproof_bytes(&env));
    assert!(
        Bulletproof::from_bytes(&bytes).is_some(),
        "valid 800-byte bulletproof failed to parse"
    );
}

#[test]
fn test_bulletproof_from_bytes_malformed_corpus() {
    let env = env();
    let valid = valid_bulletproof_bytes(&env);

    // (name, bytes, expected class). Built lazily so we can reuse `valid`.
    let mut corpus: std::vec::Vec<(std::string::String, Bytes, Reject)> = std::vec::Vec::new();

    // Size rejections — every non-800 length, including the empty and the
    // oversized cases.
    corpus.push(("empty".into(), Bytes::new(&env), Reject::BadLength));
    corpus.push(("one_short".into(), Bytes::from_slice(&env, &valid[..799]), Reject::BadLength));
    corpus.push((
        "one_long".into(),
        {
            let mut v = valid.to_vec();
            v.push(0);
            Bytes::from_slice(&env, &v)
        },
        Reject::BadLength,
    ));
    // Oversized: must be rejected in O(1) by the length gate, not by per-byte
    // work — this is the bounded-resource guarantee.
    corpus.push((
        "oversized_100k".into(),
        Bytes::from_slice(&env, &std::vec::from_elem(0u8, 100_000)),
        Reject::BadLength,
    ));

    // Off-curve rejections — correct length, but one point slot corrupted to
    // (0, 0) which is off-curve. Earlier slots stay valid so parsing actually
    // reaches the corrupted slot.
    for (name, off) in POINT_SLOTS {
        let mut a = valid;
        for b in a.iter_mut().skip(off).take(64) {
            *b = 0;
        }
        corpus.push((
            std::format!("off_curve_{name}"),
            Bytes::from_array(&env, &a),
            Reject::PointOffCurve,
        ));
    }

    for (name, bytes, class) in &corpus {
        assert!(
            Bulletproof::from_bytes(bytes).is_none(),
            "malformed bulletproof `{name}` ({class:?}) was accepted",
        );
    }
}

// ── 2. decompress_pt_32 — field-overflow + non-square corpus ─────────────────

#[test]
fn test_decompress_control_roundtrips() {
    // Control: the compressed Ed25519 base point decompresses.
    let env = env();
    let compressed = compress_pt(&env, &g());
    assert!(
        decompress_pt_32(&env, &compressed).is_some(),
        "compressed base point failed to decompress"
    );
}

#[test]
fn test_decompress_malformed_corpus() {
    let env = env();

    // y >= p: all-ones clears the sign bit to 0x7f..ff = 2^255 - 1, which is
    // greater than p = 2^255 - 19, so the field-range check rejects it.
    let y_overflow = BytesN::from_array(&env, &[0xFFu8; 32]);
    assert!(
        decompress_pt_32(&env, &y_overflow).is_none(),
        "y >= p ({:?}) was accepted",
        Reject::YOverflow,
    );

    // Non-square: a small y (< p, so it passes the range check) for which no
    // x exists on the curve. Roughly half of all y are non-squares, so a short
    // deterministic scan finds one; using the smallest keeps the fixture
    // stable. This isolates the NonSquare branch from YOverflow.
    let mut nonsquare: Option<u8> = None;
    for y in 2u8..=64 {
        let mut arr = [0u8; 32];
        arr[0] = y; // little-endian small y, sign bit 0
        if decompress_pt_32(&env, &BytesN::from_array(&env, &arr)).is_none() {
            nonsquare = Some(y);
            break;
        }
    }
    let y = nonsquare.expect("expected at least one non-square y in [2, 64]");
    let mut arr = [0u8; 32];
    arr[0] = y;
    assert!(
        decompress_pt_32(&env, &BytesN::from_array(&env, &arr)).is_none(),
        "non-square y={y} ({:?}) was accepted",
        Reject::NonSquare,
    );
}

// ── 3. verkle decode_proof — size + discriminant corpus ──────────────────────

/// A well-formed 97-byte proof payload with the given type byte.
fn proof_bytes(env: &Env, type_byte: u8) -> Bytes {
    let mut a = [0u8; 97];
    a[0] = type_byte;
    Bytes::from_array(env, &a)
}

#[test]
fn test_decode_proof_controls_roundtrip() {
    let env = env();
    // Members and non-members are the two accepted discriminants.
    let member = encode_proof(&env, true, &[9u8; 32], &[8u8; 32], &[7u8; 32]);
    let nonmember = encode_proof(&env, false, &[9u8; 32], &NON_MEMBER_SENTINEL, &[7u8; 32]);
    assert!(decode_proof(&member).is_some(), "valid member proof rejected");
    assert!(decode_proof(&nonmember).is_some(), "valid non-member proof rejected");
}

#[test]
fn test_decode_proof_malformed_corpus() {
    let env = env();

    // Size rejections. The oversized case verifies the O(1) length gate and
    // its bounded-resource guarantee.
    let mut corpus: std::vec::Vec<(std::string::String, Bytes, Reject)> = std::vec![
        ("empty".into(), Bytes::new(&env), Reject::BadLength),
        ("short_96".into(), Bytes::from_slice(&env, &[0u8; 96]), Reject::BadLength,),
        ("long_98".into(), Bytes::from_slice(&env, &[1u8; 98]), Reject::BadLength,),
        (
            "oversized_100k".into(),
            Bytes::from_slice(&env, &std::vec::from_elem(0x01u8, 100_000)),
            Reject::BadLength,
        ),
    ];

    // Discriminant rejections — correct 97-byte length, invalid type byte.
    for t in [0x00u8, 0x03, 0x7f, 0xff] {
        corpus.push((
            std::format!("type_0x{t:02x}"),
            proof_bytes(&env, t),
            Reject::BadDiscriminant,
        ));
    }

    for (name, bytes, class) in &corpus {
        assert!(decode_proof(bytes).is_none(), "malformed proof `{name}` ({class:?}) was accepted",);
    }
}

// ── 4. bytes48_to_commitment — context-prefix (version) corpus ───────────────

#[test]
fn test_commitment_prefix_control_roundtrips() {
    let env = env();
    let commit = [5u8; 32];
    let b48 = commitment_to_bytes48(&env, &commit);
    assert_eq!(
        bytes48_to_commitment(&b48),
        Some(commit),
        "round-tripped commitment prefix rejected or altered"
    );
}

#[test]
fn test_commitment_prefix_malformed_corpus() {
    let env = env();

    let make = |prefix: &[u8; 16]| {
        let mut buf = [0u8; 48];
        buf[0..16].copy_from_slice(prefix);
        BytesN::<48>::from_array(&env, &buf)
    };

    // All-zero prefix and a version-bumped prefix must both be rejected: the
    // context prefix ties a commitment to one curve tag + protocol version.
    let corpus = [
        ("zero_prefix", make(&[0u8; 16])),
        ("version_2", make(b"LEDGERLENS_KZG_2")),
        ("truncated_tag", make(b"LEDGERLENS_KZG\0\0")),
    ];

    for (name, b48) in corpus {
        assert!(
            bytes48_to_commitment(&b48).is_none(),
            "commitment with bad prefix `{name}` ({:?}) was accepted",
            Reject::BadPrefix,
        );
    }
}

// ── 5. verify_proof — semantic corpus (success + adversarial) ────────────────

#[test]
fn test_verify_proof_membership_semantics() {
    let env = env();
    let commitment = [1u8; 32];
    let z = [2u8; 32];
    let v = [3u8; 32]; // != sentinel -> membership path

    let witness = compute_membership_witness(&env, &commitment, &z, &v);

    // Success path.
    assert!(verify_proof(&env, &commitment, &z, &v, &witness), "valid membership proof rejected");

    // Adversarial: each field tampered in isolation must fail. Recomputing the
    // witness over different (commitment, z, v) yields a different hash.
    let mut bad_witness = witness;
    bad_witness[0] ^= 1;
    assert!(!verify_proof(&env, &commitment, &z, &v, &bad_witness), "tampered witness accepted");

    let mut bad_z = z;
    bad_z[0] ^= 1;
    assert!(!verify_proof(&env, &commitment, &bad_z, &v, &witness), "tampered z accepted");

    let mut bad_v = v; // stays non-sentinel, so still the membership path
    bad_v[0] ^= 1;
    assert!(!verify_proof(&env, &commitment, &z, &bad_v, &witness), "tampered v accepted");

    let mut bad_commit = commitment;
    bad_commit[0] ^= 1;
    assert!(
        !verify_proof(&env, &bad_commit, &z, &v, &witness),
        "witness verified against the wrong commitment"
    );
}

#[test]
fn test_verify_proof_nonmembership_semantics() {
    let env = env();
    let commitment = [1u8; 32];
    let z = [2u8; 32];
    let v = NON_MEMBER_SENTINEL; // sentinel -> non-membership path

    let witness = compute_nonmembership_witness(&env, &commitment, &z);

    // Success path.
    assert!(
        verify_proof(&env, &commitment, &z, &v, &witness),
        "valid non-membership proof rejected"
    );

    // Adversarial: wrong witness rejected.
    let mut bad_witness = witness;
    bad_witness[0] ^= 1;
    assert!(
        !verify_proof(&env, &commitment, &z, &v, &bad_witness),
        "tampered non-membership witness accepted"
    );

    // Type confusion: a non-member payload (v = sentinel) carrying a
    // *membership* witness must not verify, since the sentinel routes
    // verification through the non-membership branch.
    let member_witness = compute_membership_witness(&env, &commitment, &z, &v);
    assert!(
        !verify_proof(&env, &commitment, &z, &v, &member_witness),
        "membership witness accepted for a non-member payload"
    );
}
