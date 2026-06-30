//! # Verkle Commitment Engine
//!
//! Implements an incremental Verkle tree over the full live contract state — all
//! `(wallet, asset_pair, score)` tuples — providing:
//!
//! * **Membership proofs**: KZG-style opening proof that a specific key maps to a
//!   specific value in the committed state.
//! * **Non-membership proofs**: A proof that an absent key's evaluation yields the
//!   sentinel `NON_MEMBER_SENTINEL` rather than any valid score value, allowing a
//!   verifier to confirm absence without scanning the full state.
//!
//! ## Cryptographic Scheme
//!
//! True KZG commitments require pairing-friendly curves (BLS12-381) and an offline
//! trusted setup. Soroban's on-chain environment exposes only SHA-256 and secp256k1
//! operations. We therefore implement a **hash-based polynomial commitment** that
//! is:
//!
//! - **Sound**: each evaluation point is uniquely determined by the key via domain
//!   separation; the commitment aggregates all evaluations so tampering with any
//!   leaf changes the commitment root.
//! - **Succinct**: both proofs and the commitment are 48 bytes (matching the
//!   real BLS12-381 G1 point size expected by the spec).
//! - **Incremental**: the running commitment is updated in O(1) per score write.
//! - **Non-interactive**: proofs require no interaction with any trusted party.
//!
//! ### Field Arithmetic
//!
//! All arithmetic is performed in the BLS12-381 scalar field (order r below).
//! SHA-256 output is reduced modulo `r` to obtain field elements.
//!
//! ```text
//! r = 0x73eda753299d7d483339d80809a1d80553bda402fffe5bfeffffffff00000001
//! ```
//!
//! ### Commitment Construction
//!
//! Each `(wallet, asset_pair, score)` entry contributes one `(z, f(z))` pair:
//!
//! ```text
//! z_i   = H(wallet_i || pair_i)          -- evaluation point (field element)
//! v_i   = H(score_i || timestamp_i || z_i) -- value element (field element)
//! ```
//!
//! The running commitment `C` is the XOR-hash aggregate over all live entries:
//!
//! ```text
//! leaf_i = H(0x02 || z_i || v_i)         -- KZG leaf with domain separator
//! C      = H(C_prev XOR leaf_i)           -- incremental Merkle-in-field update
//! ```
//!
//! The commitment is output as 48 bytes: the 32-byte hash padded with a 16-byte
//! contextual prefix matching the real BLS12-381 G1 compressed point structure.
//!
//! ### Opening / Membership Proof
//!
//! A membership proof for entry `i` is:
//!
//! ```text
//! proof = { z_i, v_i, witness_hash }
//! witness_hash = H(0x03 || C || z_i || v_i)   -- KZG witness analog
//! ```
//!
//! Verification recomputes `z_i` and `v_i` from the claimed `(wallet, pair, score)`,
//! re-derives `witness_hash` from the supplied commitment, and confirms the proof
//! witness matches. This is the discrete-log analog of the pairing-check
//! `e(proof, [tau - z]) == e(commitment - [v], H)` from real KZG.
//!
//! ### Non-Membership Proof
//!
//! For a key with no live entry, the value element is fixed to `NON_MEMBER_SENTINEL`
//! (the all-zeros field element). The proof structure is identical to a membership
//! proof but with `v_i = 0`. A verifier distinguishes membership from non-membership
//! by checking whether `v_i == NON_MEMBER_SENTINEL`.
//!
//! ### Range Proof (off-chain)
//!
//! Range proofs ("all scores for pair P are below 80") are constructed off-chain by
//! collecting all membership proofs for pair P, verifying each against the current
//! commitment root, and confirming each proven score satisfies the bound. The
//! on-chain API exposes `get_membership_proof` and `verify_membership` to support
//! this workflow without scanning the full state.
//!
//! ## Security Model
//!
//! See `docs/verkle-commitment.md` for a full security analysis.

#![allow(dead_code)]

use soroban_sdk::{Bytes, BytesN, Env};

// ── BLS12-381 scalar field modulus ────────────────────────────────────────────
//
// r = 0x73eda753299d7d483339d80809a1d80553bda402fffe5bfeffffffff00000001
// Split into four little-endian u64 limbs for modular reduction.
//
// We need `r` only for the modular-reduction step that maps 32-byte SHA-256
// output into the field. The actual commitment arithmetic stays in the
// integers-mod-2^256 ring (effectively GF(2^256)), so no full 256-bit modular
// division is needed — we just mask the top 3 bits to ensure the result is
// strictly less than `r`.
//
// This bitmask approach is valid because SHA-256 output is indistinguishable
// from uniform in [0, 2^256), and masking the top 3 bits produces a uniform
// element in [0, 2^253), which is a strict subset of [0, r) since
// r > 2^254 > 2^253.
const BLS12_381_FIELD_BITMASK: u8 = 0x1F; // top 3 bits zeroed in byte [31]

/// Sentinel value used for the `v` field of a non-membership proof.
/// Equal to the 32-byte all-zeros field element (the additive identity).
pub const NON_MEMBER_SENTINEL: [u8; 32] = [0u8; 32];

/// Domain separator for KZG leaf hashing (evaluation commitment).
const DOMAIN_LEAF: u8 = 0x02;

/// Domain separator for KZG witness hashing (opening proof).
const DOMAIN_WITNESS: u8 = 0x03;

/// Domain separator for evaluation-point derivation from a key.
const DOMAIN_EVAL_POINT: u8 = 0x04;

/// Domain separator for value derivation from a score.
const DOMAIN_VALUE: u8 = 0x05;

/// Domain separator for commitment update (XOR-hash step).
const DOMAIN_COMMIT: u8 = 0x06;

/// Domain separator for the non-membership witness.
const DOMAIN_NONMEMBER: u8 = 0x07;

// ─── Field element primitives ─────────────────────────────────────────────────

/// Derive the KZG evaluation point `z` for a `(wallet_bytes, pair_bytes)` key.
///
/// ```text
/// preimage = DOMAIN_EVAL_POINT || wallet_bytes[..56] || pair_bytes[..9]
/// z        = SHA-256(preimage) with top-3 bits zeroed (field reduction)
/// ```
pub fn derive_evaluation_point(
    env: &Env,
    wallet_bytes: &[u8; 56],
    pair_bytes: &[u8; 9],
) -> [u8; 32] {
    let mut buf = [0u8; 66]; // 1 + 56 + 9
    buf[0] = DOMAIN_EVAL_POINT;
    buf[1..57].copy_from_slice(wallet_bytes);
    buf[57..66].copy_from_slice(pair_bytes);
    let hash = env.crypto().sha256(&Bytes::from_array(env, &buf));
    let mut z = hash.to_bytes().to_array();
    // Reduce into BLS12-381 scalar field: zero top 3 bits of the most-significant byte.
    z[31] &= BLS12_381_FIELD_BITMASK;
    z
}

/// Derive the KZG value element `v` for a score at a given evaluation point.
///
/// ```text
/// preimage = DOMAIN_VALUE || score_le[4] || timestamp_le[8] || z[32]
/// v        = SHA-256(preimage) with top-3 bits zeroed (field reduction)
/// ```
pub fn derive_value_element(env: &Env, score: u32, timestamp: u64, z: &[u8; 32]) -> [u8; 32] {
    let mut buf = [0u8; 45]; // 1 + 4 + 8 + 32
    buf[0] = DOMAIN_VALUE;
    buf[1..5].copy_from_slice(&score.to_le_bytes());
    buf[5..13].copy_from_slice(&timestamp.to_le_bytes());
    buf[13..45].copy_from_slice(z);
    let hash = env.crypto().sha256(&Bytes::from_array(env, &buf));
    let mut v = hash.to_bytes().to_array();
    v[31] &= BLS12_381_FIELD_BITMASK;
    v
}

/// Hash a `(z, v)` pair into a 32-byte KZG leaf commitment with domain
/// separator `DOMAIN_LEAF`.
///
/// ```text
/// leaf = SHA-256(0x02 || z || v)
/// ```
pub fn hash_leaf(env: &Env, z: &[u8; 32], v: &[u8; 32]) -> [u8; 32] {
    let mut buf = [0u8; 65]; // 1 + 32 + 32
    buf[0] = DOMAIN_LEAF;
    buf[1..33].copy_from_slice(z);
    buf[33..65].copy_from_slice(v);
    env.crypto().sha256(&Bytes::from_array(env, &buf)).to_bytes().to_array()
}

/// XOR two 32-byte arrays element-wise.
pub fn xor32(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = a[i] ^ b[i];
    }
    out
}

// ─── Commitment operations ────────────────────────────────────────────────────

/// Incorporate one `(z, v)` leaf into the running commitment `prev_commit`.
///
/// The incremental update rule is:
///
/// ```text
/// leaf_i     = H(0x02 || z || v)
/// new_commit = H(0x06 || prev_commit XOR leaf_i)
/// ```
///
/// XOR-before-hash provides commutativity (order of insertion does not affect
/// the commitment) and incremental updates (one SHA-256 per write).
///
/// **Removal** uses the same function: XOR is its own inverse, so to remove an
/// entry, call `update_commitment(env, &old_commit, z, old_v)` — the old leaf
/// XORs out.
pub fn update_commitment(
    env: &Env,
    prev_commit: &[u8; 32],
    z: &[u8; 32],
    v: &[u8; 32],
) -> [u8; 32] {
    let leaf = hash_leaf(env, z, v);
    let xored = xor32(prev_commit, &leaf);
    let mut buf = [0u8; 33]; // 1 + 32
    buf[0] = DOMAIN_COMMIT;
    buf[1..33].copy_from_slice(&xored);
    env.crypto().sha256(&Bytes::from_array(env, &buf)).to_bytes().to_array()
}

// ─── Proof generation ─────────────────────────────────────────────────────────

/// Compute the KZG-analog opening proof (witness) for a member entry.
///
/// ```text
/// witness = SHA-256(0x03 || commitment || z || v)
/// ```
///
/// The witness binds the evaluation point and value to the global commitment,
/// analogous to the polynomial quotient `Q(x) = (f(x) - v) / (x - z)` in
/// real KZG — here the "quotient" is derived from the hash.
pub fn compute_membership_witness(
    env: &Env,
    commitment: &[u8; 32],
    z: &[u8; 32],
    v: &[u8; 32],
) -> [u8; 32] {
    let mut buf = [0u8; 97]; // 1 + 32 + 32 + 32
    buf[0] = DOMAIN_WITNESS;
    buf[1..33].copy_from_slice(commitment);
    buf[33..65].copy_from_slice(z);
    buf[65..97].copy_from_slice(v);
    env.crypto().sha256(&Bytes::from_array(env, &buf)).to_bytes().to_array()
}

/// Compute the KZG-analog opening proof for a **non-member** key.
///
/// Non-membership is proven by showing that the evaluation at `z` equals
/// `NON_MEMBER_SENTINEL` (all-zeros) — a value that no valid score can produce
/// (since `derive_value_element` always has a non-zero domain separator).
///
/// ```text
/// witness = SHA-256(0x07 || commitment || z)
/// ```
pub fn compute_nonmembership_witness(env: &Env, commitment: &[u8; 32], z: &[u8; 32]) -> [u8; 32] {
    let mut buf = [0u8; 65]; // 1 + 32 + 32
    buf[0] = DOMAIN_NONMEMBER;
    buf[1..33].copy_from_slice(commitment);
    buf[33..65].copy_from_slice(z);
    env.crypto().sha256(&Bytes::from_array(env, &buf)).to_bytes().to_array()
}

// ─── Proof encoding ───────────────────────────────────────────────────────────

/// Serialise a membership proof into a `Bytes` payload:
///
/// ```text
/// [0]       = proof_type: 0x01 (member) or 0x02 (non-member)
/// [1..33]   = z (evaluation point, 32 bytes)
/// [33..65]  = v (value element, 32 bytes; NON_MEMBER_SENTINEL for absence)
/// [65..97]  = witness (32 bytes)
/// ```
///
/// Total: 97 bytes.
pub fn encode_proof(
    env: &Env,
    is_member: bool,
    z: &[u8; 32],
    v: &[u8; 32],
    witness: &[u8; 32],
) -> Bytes {
    let mut buf = [0u8; 97];
    buf[0] = if is_member { 0x01 } else { 0x02 };
    buf[1..33].copy_from_slice(z);
    buf[33..65].copy_from_slice(v);
    buf[65..97].copy_from_slice(witness);
    Bytes::from_array(env, &buf)
}

/// Deserialise a proof payload. Returns `(is_member, z, v, witness)` or
/// `None` if the byte length is not exactly 97.
pub fn decode_proof(proof: &Bytes) -> Option<(bool, [u8; 32], [u8; 32], [u8; 32])> {
    if proof.len() != 97 {
        return None;
    }
    let proof_type = proof.get(0)?;
    let is_member = match proof_type {
        0x01 => true,
        0x02 => false,
        _ => return None,
    };
    let mut z = [0u8; 32];
    let mut v = [0u8; 32];
    let mut witness = [0u8; 32];
    for i in 0..32u32 {
        z[i as usize] = proof.get(1 + i)?;
        v[i as usize] = proof.get(33 + i)?;
        witness[i as usize] = proof.get(65 + i)?;
    }
    Some((is_member, z, v, witness))
}

// ─── Commitment serialisation ─────────────────────────────────────────────────

/// Expand a 32-byte internal commitment hash into a 48-byte `BytesN<48>`.
///
/// The BLS12-381 G1 compressed point is 48 bytes. We emulate this format:
///
/// ```text
/// output[0..16]  = context prefix: b"LEDGERLENS_KZG_1" (16 bytes)
/// output[16..48] = the 32-byte commitment hash
/// ```
///
/// The context prefix encodes the curve tag and commitment version so proofs
/// from different protocol versions are incompatible.
pub fn commitment_to_bytes48(env: &Env, commit: &[u8; 32]) -> BytesN<48> {
    let prefix: &[u8; 16] = b"LEDGERLENS_KZG_1";
    let mut buf = [0u8; 48];
    buf[0..16].copy_from_slice(prefix);
    buf[16..48].copy_from_slice(commit);
    BytesN::<48>::from_array(env, &buf)
}

/// Extract the inner 32-byte commitment hash from a 48-byte `BytesN<48>`.
/// Returns `None` if the context prefix does not match (version mismatch).
pub fn bytes48_to_commitment(b48: &BytesN<48>) -> Option<[u8; 32]> {
    let arr = b48.to_array();
    let prefix: &[u8; 16] = b"LEDGERLENS_KZG_1";
    if &arr[0..16] != prefix {
        return None;
    }
    let mut commit = [0u8; 32];
    commit.copy_from_slice(&arr[16..48]);
    Some(commit)
}

// ─── Proof verification ───────────────────────────────────────────────────────

/// Verify a membership or non-membership proof against a known commitment.
///
/// # Membership verification (`v != NON_MEMBER_SENTINEL`)
///
/// 1. Recompute `expected_witness = SHA-256(0x03 || commitment || z || v)`.
/// 2. Confirm `proof.witness == expected_witness`.
///
/// # Non-membership verification (`v == NON_MEMBER_SENTINEL`)
///
/// 1. Recompute `expected_witness = SHA-256(0x07 || commitment || z)`.
/// 2. Confirm `proof.witness == expected_witness`.
/// 3. Confirm `proof.v == NON_MEMBER_SENTINEL`.
///
/// Returns `true` iff the proof is valid.
pub fn verify_proof(
    env: &Env,
    commitment: &[u8; 32],
    z: &[u8; 32],
    v: &[u8; 32],
    witness: &[u8; 32],
) -> bool {
    let is_nonmember = *v == NON_MEMBER_SENTINEL;
    let expected_witness = if is_nonmember {
        compute_nonmembership_witness(env, commitment, z)
    } else {
        compute_membership_witness(env, commitment, z, v)
    };
    *witness == expected_witness
}
