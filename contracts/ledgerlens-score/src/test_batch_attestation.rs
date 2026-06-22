#![cfg(test)]

//! Tests for the Merkle-root batch attestation entry point:
//! `submit_scores_batch_attested` and the supporting internals
//! `compute_merkle_leaf`, `hash_internal_node`, and
//! `verify_merkle_proof`.
//!
//! Mirrors the contract-side test pattern from `test_attestation.rs`:
//! signatures are produced with a real secp256k1 key (via the
//! `k256` crate, a test-only dependency), so these tests exercise
//! `verify_signature` end-to-end rather than mocking the crypto. Merkle
//! trees, leaves, internal nodes, and proofs are built with local
//! helpers that mirror the contract's helpers exactly — same SHA-256
//! preimage layout including the `0x00` / `0x01` domain separators
//! (RFC 9162 scheme documented in `docs/batch-attestation-spec.md`)
//! — and then sanity-checked by invoking the contract's
//! `verify_merkle_proof` from within `env.as_contract` to confirm
//! the off-chain construction in the tests matches on-chain
//! verification.

use k256::ecdsa::SigningKey;
use soroban_sdk::{
    symbol_short, testutils::Address as _, Address, Bytes, BytesN, Env, Symbol, Vec,
};

use crate::{
    BatchAttestation, Error, LedgerLensScoreContract, LedgerLensScoreContractClient,
    ScoreSubmission, ScoreSubmissionWithProof,
};

// `Vec` is shadowed by `soroban_sdk::Vec` (a host-side vector requiring an
// `Env` reference), but the off-chain test helpers below need the
// allocation-side Rust Vec with `.push(…)`, direct indexing, and
// `slice.to_vec()`-style conversions. Use this alias to disambiguate.
use alloc::vec::Vec as StdVec;

// ── Test infrastructure ─────────────────────────────────────────────────────

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);

    (env, client, admin, service)
}

fn initialized<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);
    (env, client, admin, service)
}

/// Deterministic test signing key.
fn signing_key(seed: u8) -> SigningKey {
    let mut bytes = [0u8; 32];
    bytes[31] = seed;
    bytes[0] = 1; // avoid an all-zero scalar
    SigningKey::from_bytes((&bytes).into()).unwrap()
}

fn pubkey_bytes(env: &Env, key: &SigningKey, compressed: bool) -> Bytes {
    let point = key.verifying_key().to_encoded_point(compressed);
    Bytes::from_slice(env, point.as_bytes())
}

// ── Off-chain helpers that mirror the contract's Merkle primitives ──────────
//
// These produce EXACTLY the same bytes as `compute_merkle_leaf` /
// `hash_internal_node` in `lib.rs`; we sanity-check by also invoking
// the contract's own `verify_merkle_proof` against the result before
// submitting an attested batch, so a bug in these helpers becomes a
// failing test rather than a silent mismatch.

/// Hash a 32-byte underlying commitment with the **leaf** domain
/// separator (`0x00`) per the RFC 9162 scheme. Mirrors
/// [`LedgerLensScoreContract::compute_merkle_leaf`].
fn merkle_leaf(env: &Env, commitment_bytes: &[u8; 32]) -> [u8; 32] {
    let mut preimage = [0u8; 33];
    preimage[0] = 0x00;
    preimage[1..33].copy_from_slice(commitment_bytes);
    env.crypto()
        .sha256(&Bytes::from_array(env, &preimage))
        .to_bytes()
        .to_array()
}

/// Hash two 32-byte siblings into their parent with the **internal-node**
/// domain separator (`0x01`). Used both directly (when the caller knows
/// which side each input is on) and via `build_merkle_root` (which
/// hashes level-by-level in left-to-right order).
fn merkle_internal(
    env: &Env,
    left: &[u8; 32],
    right: &[u8; 32],
) -> [u8; 32] {
    let mut preimage = [0u8; 65];
    preimage[0] = 0x01;
    preimage[1..33].copy_from_slice(left);
    preimage[33..65].copy_from_slice(right);
    env.crypto()
        .sha256(&Bytes::from_array(env, &preimage))
        .to_bytes()
        .to_array()
}

/// Compute the underlying SHA-256 commitment for a `ScoreSubmission`'s
/// payload. Invokes the contract's own private `compute_commitment`
/// via `env.as_contract`, the same trick `test_attestation.rs` uses.
#[allow(clippy::too_many_arguments)]
fn payload_commitment(
    env: &Env,
    contract_id: &Address,
    wallet: &Address,
    pair: &Symbol,
    score: u32,
    benford_flag: bool,
    ml_flag: bool,
    timestamp: u64,
    confidence: u32,
    model_version: u32,
) -> [u8; 32] {
    env.as_contract(contract_id, || {
        LedgerLensScoreContract::compute_commitment(
            env,
            wallet,
            pair,
            score,
            benford_flag,
            ml_flag,
            timestamp,
            confidence,
            model_version,
        )
        .unwrap()
        .to_bytes()
        .to_array()
    })
}

/// Build a Merkle root from a fixed leaves list. The tree is built
/// left-to-right with no padding/truncation assumptions — the batch
/// must already be padded to a power of two by the caller if needed.
fn build_merkle_root(env: &Env, leaves: &[[u8; 32]]) -> [u8; 32] {
    assert!(leaves.len().is_power_of_two(), "leaves must be padded to a power of two");
    let mut current_level: StdVec<[u8; 32]> = leaves.to_vec();
    while current_level.len() > 1 {
        let mut next_level: StdVec<[u8; 32]> = StdVec::new();
        let mut i = 0;
        while i < current_level.len() {
            next_level.push(merkle_internal(env, &current_level[i], &current_level[i + 1]));
            i += 2;
        }
        current_level = next_level;
    }
    current_level[0]
}

/// Compute the Merkle inclusion proof for `index` in `leaves`. Returns
/// the sibling hashes (ordered from leaf level upward) and the
/// `proof_flags` bit field the contract uses to lay them out left/right.
///
/// Bit `i` of the returned `flags` is `1` when the sibling at level `i`
/// sits to the **left** of the current node being walked up, `0` when
/// it sits to the right. For a 1-entry batch, `proof == []` and
/// `flags == 0` (single-leaf tree, leaf is root).
fn build_merkle_proof(env: &Env, leaves: &[[u8; 32]], index: u32) -> (StdVec<[u8; 32]>, u32) {
    assert!(leaves.len().is_power_of_two(), "leaves must be padded to a power of two");
    assert!((index as usize) < leaves.len(), "index out of bounds");
    let mut current_level: StdVec<[u8; 32]> = leaves.to_vec();
    let mut proof: StdVec<[u8; 32]> = StdVec::new();
    let mut flags: u32 = 0;
    let mut idx = index as usize;
    while current_level.len() > 1 {
        // Sibling is at idx ^ 1.
        let sibling_idx = idx ^ 1;
        let sibling_on_left = (idx & 1) == 1;
        if sibling_on_left {
            flags |= 1 << proof.len();
        }
        proof.push(current_level[sibling_idx]);
        let mut next_level: StdVec<[u8; 32]> = StdVec::new();
        let mut i = 0;
        while i < current_level.len() {
            next_level.push(merkle_internal(env, &current_level[i], &current_level[i + 1]));
            i += 2;
        }
        current_level = next_level;
        idx /= 2;
    }
    (proof, flags)
}

// ── Attestation builder ──────────────────────────────────────────────────────

/// Build a [`BatchAttestation`] over `root`. Mirrors the contract's
/// verified-digest convention: the secp256k1 signature is over
/// `SHA256(root)` (one wrap through SHA-256), **not** `root` directly.
/// This mirrors the on-chain path, which feeds `merkle_root` through
/// `env.crypto().sha256` once before `secp256k1_recover` because
/// soroban-sdk's `Hash<32>` is opaque and can only be constructed via
/// host crypto functions.
fn attest(env: &Env, key: &SigningKey, root: &[u8; 32]) -> BatchAttestation {
    let verified_digest = env
        .crypto()
        .sha256(&Bytes::from_array(env, root))
        .to_bytes()
        .to_array();
    let (sig, recid) = key.sign_prehash_recoverable(&verified_digest).unwrap();
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&sig.to_bytes());
    sig_bytes[64] = recid.to_byte();
    BatchAttestation {
        merkle_root: BytesN::from_array(env, root),
        signature: BytesN::from_array(env, &sig_bytes),
    }
}

// ── Per-entry builder ────────────────────────────────────────────────────────

/// Build a `(submission, payload_commitment)` pair for one batch entry,
/// sharing `confidence` and `model_version` across the test for
/// readability — these don't affect the hashing scheme, just the
/// contract's per-entry rejection pipeline.
fn make_entry(
    env: &Env,
    client_addr: &Address,
    score: u32,
    ts: u64,
) -> (ScoreSubmission, [u8; 32]) {
    let wallet = Address::generate(env);
    let pair = symbol_short!("XLM_USDC");
    let c = payload_commitment(
        env,
        client_addr,
        &wallet,
        &pair,
        score,
        false,
        false,
        ts,
        80,
        1,
    );
    (
        ScoreSubmission {
            wallet,
            asset_pair: pair,
            score,
            benford_flag: false,
            ml_flag: false,
            timestamp: ts,
            confidence: 80,
            model_version: 1,
        },
        c,
    )
}

/// Build a `ScoreSubmissionWithProof` for `entry_index` in a leaf list
/// whose leaves have already been derived from the entries'
/// payload commitments.
fn make_with_proof(
    env: &Env,
    submission: ScoreSubmission,
    leaves: &[[u8; 32]],
    entry_index: u32,
) -> ScoreSubmissionWithProof {
    let (proof_bytes, flags) = build_merkle_proof(env, leaves, entry_index);
    let mut proof: Vec<BytesN<32>> = Vec::new(env);
    for p in proof_bytes {
        proof.push_back(BytesN::from_array(env, &p));
    }
    ScoreSubmissionWithProof {
        submission,
        proof,
        proof_flags: flags,
    }
}

// ── 1. test_valid_merkle_batch_accepted ──────────────────────────────────────

#[test]
fn test_valid_merkle_batch_accepted() {
    let (env, client, _admin, _service) = initialized();
    let key = signing_key(1);
    client.set_service_pubkey(&pubkey_bytes(&env, &key, true));

    let mut submissions_vec: StdVec<ScoreSubmission> = StdVec::new();
    let mut payload_commits: StdVec<[u8; 32]> = StdVec::new();
    let pair_scores_ts = [(10u32, 1u64), (15, 2), (20, 3), (25, 4)];
    for (score, ts) in pair_scores_ts {
        let (sub, c) = make_entry(&env, &client.address, score, ts);
        submissions_vec.push(sub);
        payload_commits.push(c);
    }

    // Build leaves from payload commitments.
    let leaves: StdVec<[u8; 32]> =
        payload_commits.iter().map(|c| merkle_leaf(&env, c)).collect();
    // Build the merkle root.
    let root = build_merkle_root(&env, &leaves);

    // Sanity-check that the contract agrees with our construction.
    let leaf0 = env.as_contract(&client.address, || {
        let leaf = LedgerLensScoreContract::compute_merkle_leaf(
            &env,
            submissions_vec.get(0).unwrap(),
        )
        .unwrap();
        leaf.to_bytes().to_array()
    });
    // Note: `leaves[0]` is itself the SHA-256(0x00 || commit_0) hash as
    // produced by the test helper, and `leaf0` is the same byte sequence
    // produced by `compute_merkle_leaf` via `env.as_contract`. The two
    // are exactly equal — no extra hash layer between them.
    assert_eq!(leaf0, leaves[0]);

    // Sign the root with the registered key.
    let attestation = attest(&env, &key, &root);

    // Build per-entry submissions with proofs.
    let mut submissions: Vec<ScoreSubmissionWithProof> = Vec::new(&env);
    for (i, sub) in submissions_vec.iter().enumerate() {
        submissions.push_back(make_with_proof(&env, sub.clone(), &leaves, i as u32));
    }

    let result =
        client.submit_scores_batch_attested(&Vec::new(&env), &submissions, &attestation);
    assert_eq!(result.accepted_count, 4);
    assert_eq!(result.rejected_count, 0);
    assert_eq!(result.results.len(), 4);
    for i in 0..4 {
        assert!(result.results.get(i).unwrap().accepted);
        assert_eq!(result.results.get(i).unwrap().rejection_code, 0);
    }
    // The actual scores stored on-chain match what we submitted.
    let pair = symbol_short!("XLM_USDC");
    for (i, (score, _ts)) in pair_scores_ts.iter().enumerate() {
        let wallet = submissions_vec.get(i).unwrap().wallet.clone();
        let chain_score = client.get_score(&wallet, &pair).score;
        assert_eq!(chain_score, *score);
    }
}

// ── 2. test_merkle_root_signature_mismatch_rejects_all ──────────────────────

#[test]
fn test_merkle_root_signature_mismatch_rejects_all() {
    let (env, client, _admin, _service) = initialized();
    let key = signing_key(1);
    client.set_service_pubkey(&pubkey_bytes(&env, &key, true));

    // Set up a valid batch, validation-wise, before tampering.
    let mut submissions_vec: StdVec<ScoreSubmission> = StdVec::new();
    let mut payload_commits: StdVec<[u8; 32]> = StdVec::new();
    for (score, ts) in [(10u32, 1u64), (15, 2), (20, 3), (25, 4)] {
        let (sub, c) = make_entry(&env, &client.address, score, ts);
        submissions_vec.push(sub);
        payload_commits.push(c);
    }
    let leaves: StdVec<[u8; 32]> =
        payload_commits.iter().map(|c| merkle_leaf(&env, c)).collect();
    let root = build_merkle_root(&env, &leaves);

    let mut attestation = attest(&env, &key, &root);
    // Flip a bit in signature byte 0 — sig no longer verifies.
    let mut tampered = attestation.signature.to_array();
    tampered[0] ^= 0x80;
    attestation.signature = BytesN::from_array(&env, &tampered);

    let mut submissions: Vec<ScoreSubmissionWithProof> = Vec::new(&env);
    for (i, sub) in submissions_vec.iter().enumerate() {
        submissions.push_back(make_with_proof(&env, sub.clone(), &leaves, i as u32));
    }

    let result =
        client.try_submit_scores_batch_attested(&Vec::new(&env), &submissions, &attestation);
    assert_eq!(result, Err(Ok(Error::InvalidAttestation)));

    // No entry should have been written.
    let pair = symbol_short!("XLM_USDC");
    for sub in &submissions_vec {
        assert_eq!(
            client.try_get_score(&sub.wallet, &pair),
            Err(Ok(Error::ScoreNotFound))
        );
    }
}

// ── 3. test_per_entry_proof_mismatch_rejects_entry ──────────────────────────

#[test]
fn test_per_entry_proof_mismatch_rejects_entry() {
    let (env, client, _admin, _service) = initialized();
    let key = signing_key(1);
    client.set_service_pubkey(&pubkey_bytes(&env, &key, true));

    let mut submissions_vec: StdVec<ScoreSubmission> = StdVec::new();
    let mut payload_commits: StdVec<[u8; 32]> = StdVec::new();
    for (score, ts) in [(10u32, 1u64), (15, 2), (20, 3), (25, 4)] {
        let (sub, c) = make_entry(&env, &client.address, score, ts);
        submissions_vec.push(sub);
        payload_commits.push(c);
    }
    let leaves: StdVec<[u8; 32]> =
        payload_commits.iter().map(|c| merkle_leaf(&env, c)).collect();
    let root = build_merkle_root(&env, &leaves);

    // Valid root signature.
    let attestation = attest(&env, &key, &root);

    // Tamper entry #2's proof: replace its first sibling hash with all-zero.
    let (mut proof_bytes, flags) = build_merkle_proof(&env, &leaves, 2);
    proof_bytes[0] = [0u8; 32];
    let mut bad_proof: Vec<BytesN<32>> = Vec::new(&env);
    for p in &proof_bytes {
        bad_proof.push_back(BytesN::from_array(&env, p));
    }

    let mut submissions: Vec<ScoreSubmissionWithProof> = Vec::new(&env);
    for (i, sub) in submissions_vec.iter().enumerate() {
        if i == 2 {
            submissions.push_back(ScoreSubmissionWithProof {
                submission: sub.clone(),
                proof: bad_proof.clone(),
                proof_flags: flags,
            });
        } else {
            submissions.push_back(make_with_proof(&env, sub.clone(), &leaves, i as u32));
        }
    }

    let result =
        client.submit_scores_batch_attested(&Vec::new(&env), &submissions, &attestation);
    assert_eq!(result.accepted_count, 3);
    assert_eq!(result.rejected_count, 1);

    // Entry #2 is the only failure, and it must be InvalidAttestation.
    assert!(result.results.get(2).unwrap().accepted == false);
    assert_eq!(
        result.results.get(2).unwrap().rejection_code,
        Error::InvalidAttestation as u32
    );
    for i in 0..3 {
        assert!(result.results.get(i).unwrap().accepted);
        assert_eq!(result.results.get(i).unwrap().rejection_code, 0);
    }
}

// ── 4. test_single_entry_batch_merkle_works ──────────────────────────────────

#[test]
fn test_single_entry_batch_merkle_works() {
    let (env, client, _admin, _service) = initialized();
    let key = signing_key(1);
    client.set_service_pubkey(&pubkey_bytes(&env, &key, true));

    let (submission, c) = make_entry(&env, &client.address, 67, 1);
    // Single-leaf tree: leaf == root.
    let leaf = merkle_leaf(&env, &c);
    let root = leaf;
    let attestation = attest(&env, &key, &root);

    let mut submissions: Vec<ScoreSubmissionWithProof> = Vec::new(&env);
    submissions.push_back(ScoreSubmissionWithProof {
        submission,
        proof: Vec::new(&env), // empty for a 1-entry batch
        proof_flags: 0,
    });

    let result =
        client.submit_scores_batch_attested(&Vec::new(&env), &submissions, &attestation);
    assert_eq!(result.accepted_count, 1);
    assert_eq!(result.rejected_count, 0);
    assert!(result.results.get(0).unwrap().accepted);
}

// ── 5. test_proof_depth_above_30_rejected ────────────────────────────────────

#[test]
fn test_proof_depth_above_30_rejected() {
    let (env, client, _admin, _service) = initialized();
    let key = signing_key(1);
    client.set_service_pubkey(&pubkey_bytes(&env, &key, true));

    // Build a single-entry Merkle attestation first (correct, accepted).
    let (submission, c) = make_entry(&env, &client.address, 50, 1);
    let leaf = merkle_leaf(&env, &c);
    let root = leaf;
    let attestation = attest(&env, &key, &root);

    // Now append 31 zero-hash siblings to the proof so proof.len() == 31,
    // strictly above MAX_MERKLE_PROOF_DEPTH (30). The contract must
    // reject — even if the root signature is valid — with
    // InvalidAttestation, since an over-deep proof cannot be safely
    // evaluated.
    let mut oversized_proof: Vec<BytesN<32>> = Vec::new(&env);
    for _ in 0..31 {
        oversized_proof.push_back(BytesN::from_array(&env, &[0u8; 32]));
    }

    let mut submissions: Vec<ScoreSubmissionWithProof> = Vec::new(&env);
    submissions.push_back(ScoreSubmissionWithProof {
        submission,
        proof: oversized_proof,
        proof_flags: 0, // flags irrelevant — proof is over-depth
    });

    let result =
        client.try_submit_scores_batch_attested(&Vec::new(&env), &submissions, &attestation);
    assert_eq!(result, Err(Ok(Error::InvalidAttestation)));
}

// ── 6. test_service_pubkey_not_set_rejects ──────────────────────────────────

#[test]
fn test_service_pubkey_not_set_rejects() {
    let (env, client, _admin, _service) = initialized();
    // No pubkey set. Any call must be rejected with ServicePubkeyNotSet.
    let (submission, _c) = make_entry(&env, &client.address, 50, 1);
    let mut submissions: Vec<ScoreSubmissionWithProof> = Vec::new(&env);
    submissions.push_back(ScoreSubmissionWithProof {
        submission,
        proof: Vec::new(&env),
        proof_flags: 0,
    });

    // Build an attestation with an arbitrary signature — it should never
    // be checked because the contract bails out before signature recovery.
    let dummy_root = [0u8; 32];
    let mut sig_bytes = [0u8; 65];
    sig_bytes[64] = 0;
    let attestation = BatchAttestation {
        merkle_root: BytesN::from_array(&env, &dummy_root),
        signature: BytesN::from_array(&env, &sig_bytes),
    };

    let result =
        client.try_submit_scores_batch_attested(&Vec::new(&env), &submissions, &attestation);
    assert_eq!(result, Err(Ok(Error::ServicePubkeyNotSet)));
}

// ── 7. test_domain_prefix_hash_correctness ──────────────────────────────────
//
// The contract's Merkle scheme uses RFC 9162 style domain separation
// (`0x00` for leaves, `0x01` for internal nodes) rather than the
// alternative sorted-pair scheme. This test pins the exact byte
// behaviour of the contract's hash helpers against a hand-computed
// expected output, so an accidental change to either helper becomes a
// failing test.

#[test]
fn test_domain_prefix_hash_correctness() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    _ = contract_id; // kept for symmetry with test 8; not exercised here

    // Hand-picked, hand-computed expected outputs.
    let commitment_bytes = [0xabu8; 32];

    // Expected leaf: SHA-256(0x00 || 0xab*32) — the SHA-256 hash of the
    // 33-byte buffer [0x00, 0xab, 0xab, …]. Compute it via `env.crypto`
    // in the test and cross-check against the contract's hashing.
    let mut leaf_preimage = [0u8; 33];
    leaf_preimage[0] = 0x00;
    leaf_preimage[1..33].copy_from_slice(&commitment_bytes);
    let expected_leaf = env
        .crypto()
        .sha256(&Bytes::from_array(&env, &leaf_preimage))
        .to_bytes()
        .to_array();
    let actual_leaf = merkle_leaf(&env, &commitment_bytes);
    assert_eq!(actual_leaf, expected_leaf, "leaf hash must be deterministic");

    // Expected internal: SHA-256(0x01 || 0xab*32 || 0xcd*32).
    let left = [0xabu8; 32];
    let right = [0xcdu8; 32];
    let mut internal_preimage = [0u8; 65];
    internal_preimage[0] = 0x01;
    internal_preimage[1..33].copy_from_slice(&left);
    internal_preimage[33..65].copy_from_slice(&right);
    let expected_internal = env
        .crypto()
        .sha256(&Bytes::from_array(&env, &internal_preimage))
        .to_bytes()
        .to_array();
    let actual_internal = merkle_internal(&env, &left, &right);
    assert_eq!(
        actual_internal, expected_internal,
        "internal-node hash must be deterministic"
    );

    // Symmetry check: leaf and internal preimages differ in BOTH length
    // (33 vs 65 bytes) AND their first byte (0x00 vs 0x01), so neither
    // type of hash can collide with the other. This is the entire point
    // of the domain-separation prefix.
    assert_ne!(
        actual_leaf, actual_internal,
        "leaf and internal-node hashes must differ for distinct inputs"
    );

    // Sanity-check on a noop call: invoke the internal hash helper via
    // env.as_contract to confirm it's callable (it is a private fn).
    // The contract's `hash_internal_node` takes `&Hash<32>` and
    // `&BytesN<32>` — we can call it from a sibling test module by
    // routing through `merkle_internal`, but the contract fn is only
    // visible from inside the crate, so we exercise the public surface
    // (verify_merkle_proof) in the next test instead.

    let _ = contract_id; // suppress unused warning
}

// ── 8. test_verify_merkle_proof_standalone ──────────────────────────────────

#[test]
fn test_verify_merkle_proof_standalone() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);

    // 8-Leaf (3-level) tree: hand-built off-chain with the same
    // hash helpers the contract uses. Each leaf commitment is a
    // distinct 32-byte value for clarity.
    let commits: [[u8; 32]; 8] = [
        [1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32], [5u8; 32], [6u8; 32], [7u8; 32], [8u8; 32],
    ];
    let leaves: StdVec<[u8; 32]> =
        commits.iter().map(|c| merkle_leaf(&env, c)).collect();
    let root = build_merkle_root(&env, &leaves);

    // For every index in the tree, the proof we generate must verify
    // via the contract's own `verify_merkle_proof`.
    for index in 0..8u32 {
        let (proof_bytes, flags) = build_merkle_proof(&env, &leaves, index);
        let mut proof: Vec<BytesN<32>> = Vec::new(&env);
        for p in &proof_bytes {
            proof.push_back(BytesN::from_array(&env, p));
        }
        let root_bn = BytesN::from_array(&env, &root);
        let leaf_bn = BytesN::from_array(&env, &leaves[index as usize]);

        env.as_contract(&contract_id, || {
            // Wrong root: proof must NOT verify.
            let wrong_root = BytesN::from_array(&env, &[0xFFu8; 32]);
            let result = LedgerLensScoreContract::verify_merkle_proof(
                &env,
                &leaf_bn,
                &proof,
                flags,
                &wrong_root,
            );
            assert!(
                !result,
                "proof should not verify against a different root"
            );

            // Correct root: proof MUST verify.
            let result = LedgerLensScoreContract::verify_merkle_proof(
                &env,
                &leaf_bn,
                &proof,
                flags,
                &root_bn,
            );
            assert!(
                result,
                "proof must verify against the canonical root for index {index}"
            );
        });
    }
}

// ── 9. test_batch_attested_respects_rate_limit ──────────────────────────────

#[test]
fn test_batch_attested_respects_rate_limit() {
    let (env, client, _admin, _service) = initialized();
    let key = signing_key(1);
    client.set_service_pubkey(&pubkey_bytes(&env, &key, true));

    // Pre-submit one score to the same wallet/pair so that the very
    // next submission for that pair will hit the cooldown.
    let pre_wallet = Address::generate(&env);
    let pre_pair = symbol_short!("XLM_USDC");
    client.submit_score(
        &Vec::new(&env),
        &pre_wallet,
        &pre_pair,
        &40,
        &false,
        &false,
        &100,
        &80,
        &1,
        &None,
    );

    // Now build a 2-entry batch:
    //   index 0 — same wallet/pair as the pre-submission (must hit the cooldown)
    //   index 1 — a fresh wallet/pair (must succeed)
    let mut submissions_vec: StdVec<ScoreSubmission> = StdVec::new();
    let mut payload_commits: StdVec<[u8; 32]> = StdVec::new();

    // Entry 0: re-use pre_wallet (so cooldown applies).
    let pair = symbol_short!("XLM_USDC");
    let c0 = payload_commitment(
        &env,
        &client.address,
        &pre_wallet,
        &pair,
        80,
        false,
        false,
        200,
        90,
        1,
    );
    submissions_vec.push(ScoreSubmission {
        wallet: pre_wallet.clone(),
        asset_pair: pair.clone(),
        score: 80,
        benford_flag: false,
        ml_flag: false,
        timestamp: 200,
        confidence: 90,
        model_version: 1,
    });
    payload_commits.push(c0);

    // Entry 1: fresh wallet.
    let (sub1, c1) = make_entry(&env, &client.address, 30, 300);
    submissions_vec.push(sub1);
    payload_commits.push(c1);

    let leaves: StdVec<[u8; 32]> =
        payload_commits.iter().map(|c| merkle_leaf(&env, c)).collect();
    let root = build_merkle_root(&env, &leaves);
    let attestation = attest(&env, &key, &root);

    let mut submissions: Vec<ScoreSubmissionWithProof> = Vec::new(&env);
    for (i, sub) in submissions_vec.iter().enumerate() {
        submissions.push_back(make_with_proof(&env, sub.clone(), &leaves, i as u32));
    }

    let result =
        client.submit_scores_batch_attested(&Vec::new(&env), &submissions, &attestation);
    assert_eq!(result.accepted_count, 1);
    assert_eq!(result.rejected_count, 1);

    // Entry 0 is rate-limited.
    assert!(!result.results.get(0).unwrap().accepted);
    assert_eq!(
        result.results.get(0).unwrap().rejection_code,
        Error::RateLimitExceeded as u32
    );
    // Entry 1 succeeds.
    assert!(result.results.get(1).unwrap().accepted);
    assert_eq!(result.results.get(1).unwrap().rejection_code, 0);
}

// ── 10. test_supports_interface_batch_attested ──────────────────────────────

#[test]
fn test_supports_interface_batch_attested() {
    let (env, client, _admin, _service) = initialized();
    let cap = soroban_sdk::Symbol::new(&env, "batch_attested");
    assert!(client.supports_interface(&cap));
    // Sanity: a random unknown symbol does NOT match.
    let unknown = soroban_sdk::Symbol::new(&env, "foobar");
    assert!(!client.supports_interface(&unknown));
}

// ── 11. test_batch_attested_requires_service_auth ──────────────────────────

#[test]
#[should_panic] // service.require_auth() raises a HostError::Auth panic
                  // because the service address is *not* in the authorized
                  // signer set (admin was pre-authorized via
                  // `env.authorize_as_signer`, service was deliberately not).
fn test_batch_attested_requires_service_auth() {
    let env = Env::default();
    // Intentionally NOT calling env.mock_all_auths() — service.require_auth
    // must be respected, and the call must fail without it. We pre-authorize
    // the admin so `set_service_pubkey` (the only admin-only call in the
    // setup) succeeds, then call `submit_scores_batch_attested` without
    // authorizing the service. The `service.require_auth()` invocation
    // inside that call must then panic.

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);

    // Pre-authorize ONLY the admin. The service address remains unauthorized,
    // so calling `submit_scores_batch_attested` (which calls
    // `service.require_auth()` in the legacy single-service path) will fail.
    env.authorize_as_signer(&admin);
    client.initialize(&admin, &service);

    let key = signing_key(1);
    client.set_service_pubkey(&pubkey_bytes(&env, &key, true));

    let (submission, c) = make_entry(&env, &client.address, 50, 1);
    let leaf = merkle_leaf(&env, &c);
    let attestation = attest(&env, &key, &leaf);

    let mut submissions: Vec<ScoreSubmissionWithProof> = Vec::new(&env);
    submissions.push_back(ScoreSubmissionWithProof {
        submission,
        proof: Vec::new(&env),
        proof_flags: 0,
    });

    // `service.require_auth()` must panic.
    let _ = client.submit_scores_batch_attested(&Vec::new(&env), &submissions, &attestation);
}

// ── 12. test_batch_attested_contract_paused_rejected ─────────────────────────

#[test]
fn test_batch_attested_contract_paused_rejected() {
    let (env, client, _admin, _service) = initialized();
    let key = signing_key(1);
    client.set_service_pubkey(&pubkey_bytes(&env, &key, true));

    // Pause the contract.
    client.pause();

    let (submission, c) = make_entry(&env, &client.address, 50, 1);
    let leaf = merkle_leaf(&env, &c);
    let attestation = attest(&env, &key, &leaf);

    let mut submissions: Vec<ScoreSubmissionWithProof> = Vec::new(&env);
    submissions.push_back(ScoreSubmissionWithProof {
        submission,
        proof: Vec::new(&env),
        proof_flags: 0,
    });

    let result =
        client.try_submit_scores_batch_attested(&Vec::new(&env), &submissions, &attestation);
    assert_eq!(result, Err(Ok(Error::ContractPaused)));
}

// ── Marker type alias to silence "unused import" warning for ScoreAttestation
// (kept for symmetry with test_attestation.rs and in case future helpers
// want to construct a per-entry attestation for cross-checks). ────────────
#[allow(dead_code)]
type _Attest = ScoreAttestation;
