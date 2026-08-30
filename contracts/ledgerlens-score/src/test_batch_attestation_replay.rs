//! Batch attestation replay isolation across asset pairs and positions
//! (issue #699).
//!
//! Proves that a batch attestation valid for one `(wallet, pair)` at one batch
//! position cannot be replayed into a different pair, wallet, position,
//! timestamp, or model version. The mechanism under test:
//!
//! - Each entry's Merkle leaf is `SHA-256(0x00 || compute_commitment(...))`,
//!   and `compute_commitment` folds in `wallet`, `asset_pair`, `score`,
//!   `timestamp`, `confidence`, and `model_version` (plus the deployment/chain
//!   binding). Mutating any of those changes the leaf, so the position's
//!   inclusion proof no longer reproduces the signed root → the entry is
//!   rejected with `InvalidAttestation`.
//! - The position itself is bound by `(proof, proof_flags)`: reusing a leaf
//!   with a *different* position's proof recomputes a different root → rejected.
//!
//! The root signature (over `SHA256(merkle_root)`) is unchanged by these
//! mutations, so the batch call still returns `Ok`; isolation is enforced
//! per-entry, exactly where a replayed entry would try to sneak in. Each test
//! mutates one field in isolation (the #699 acceptance matrix: wallet, pair,
//! index, timestamp, model) and asserts that entry — and only that entry — is
//! rejected, while its siblings still succeed.
//!
//! Signatures are produced with a real secp256k1 key (`k256`, test-only), and
//! the off-chain Merkle helpers mirror the contract's `compute_merkle_leaf` /
//! `hash_internal_node` byte layout exactly. All helpers are copied locally so
//! this module is self-contained.

extern crate alloc;

use alloc::vec::Vec as StdVec;
use k256::ecdsa::SigningKey;
use soroban_sdk::{
    symbol_short, testutils::Address as _, Address, Bytes, BytesN, Env, Symbol, Vec,
};

use crate::{
    BatchAttestation, BatchResult, Error, LedgerLensScoreContract, LedgerLensScoreContractClient,
    ScoreSubmission, ScoreSubmissionWithProof,
};

// ── Infrastructure (mirrors test_batch_attestation.rs) ───────────────────────

fn initialized<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client, admin, service)
}

fn signing_key(seed: u8) -> SigningKey {
    let mut bytes = [0u8; 32];
    bytes[31] = seed;
    bytes[0] = 1;
    SigningKey::from_bytes((&bytes).into()).unwrap()
}

fn pubkey_bytes(env: &Env, key: &SigningKey) -> Bytes {
    let point = key.verifying_key().to_encoded_point(true);
    Bytes::from_slice(env, point.as_bytes())
}

/// `SHA-256(0x00 || commitment)` — the leaf marker, mirroring
/// `compute_merkle_leaf`.
fn merkle_leaf(env: &Env, commitment_bytes: &[u8; 32]) -> [u8; 32] {
    let mut preimage = [0u8; 33];
    preimage[0] = 0x00;
    preimage[1..33].copy_from_slice(commitment_bytes);
    env.crypto().sha256(&Bytes::from_array(env, &preimage)).to_bytes().to_array()
}

/// `SHA-256(0x01 || left || right)` — the internal-node marker.
fn merkle_internal(env: &Env, left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut preimage = [0u8; 65];
    preimage[0] = 0x01;
    preimage[1..33].copy_from_slice(left);
    preimage[33..65].copy_from_slice(right);
    env.crypto().sha256(&Bytes::from_array(env, &preimage)).to_bytes().to_array()
}

/// The 32-byte payload commitment for a submission, via the contract's own
/// `compute_commitment` (invoked as the deployed contract so the
/// address/network binding resolves).
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
            &BytesN::from_array(env, &[0u8; 32]),
            0,
        )
        .unwrap()
        .to_bytes()
        .to_array()
    })
}

fn build_merkle_root(env: &Env, leaves: &[[u8; 32]]) -> [u8; 32] {
    assert!(leaves.len().is_power_of_two(), "leaves must be padded to a power of two");
    let mut level: StdVec<[u8; 32]> = leaves.to_vec();
    while level.len() > 1 {
        let mut next: StdVec<[u8; 32]> = StdVec::new();
        let mut i = 0;
        while i < level.len() {
            next.push(merkle_internal(env, &level[i], &level[i + 1]));
            i += 2;
        }
        level = next;
    }
    level[0]
}

/// Inclusion proof (sibling hashes + left/right flag field) for `index`.
fn build_merkle_proof(env: &Env, leaves: &[[u8; 32]], index: u32) -> (StdVec<[u8; 32]>, u32) {
    assert!(leaves.len().is_power_of_two(), "leaves must be padded to a power of two");
    assert!((index as usize) < leaves.len(), "index out of bounds");
    let mut level: StdVec<[u8; 32]> = leaves.to_vec();
    let mut proof: StdVec<[u8; 32]> = StdVec::new();
    let mut flags: u32 = 0;
    let mut idx = index as usize;
    while level.len() > 1 {
        let sibling_idx = idx ^ 1;
        let sibling_on_left = (idx & 1) == 1;
        if sibling_on_left {
            flags |= 1 << proof.len();
        }
        proof.push(level[sibling_idx]);
        let mut next: StdVec<[u8; 32]> = StdVec::new();
        let mut i = 0;
        while i < level.len() {
            next.push(merkle_internal(env, &level[i], &level[i + 1]));
            i += 2;
        }
        level = next;
        idx /= 2;
    }
    (proof, flags)
}

/// Sign `SHA256(root)` (the verified-digest convention) with `key`.
fn attest(env: &Env, key: &SigningKey, root: &[u8; 32]) -> BatchAttestation {
    let verified_digest = env.crypto().sha256(&Bytes::from_array(env, root)).to_bytes().to_array();
    let (sig, recid) = key.sign_prehash_recoverable(&verified_digest).unwrap();
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&sig.to_bytes());
    sig_bytes[64] = recid.to_byte();
    BatchAttestation {
        merkle_root: BytesN::from_array(env, root),
        signature: BytesN::from_array(env, &sig_bytes),
    }
}

// ── Fixture model ────────────────────────────────────────────────────────────

/// One batch entry's payload fields. `clone`-and-mutate one field to build the
/// replay attempts below.
#[derive(Clone)]
struct Entry {
    wallet: Address,
    pair: Symbol,
    score: u32,
    timestamp: u64,
    confidence: u32,
    model_version: u32,
}

impl Entry {
    fn submission(&self) -> ScoreSubmission {
        ScoreSubmission {
            wallet: self.wallet.clone(),
            asset_pair: self.pair.clone(),
            score: self.score,
            benford_flag: false,
            ml_flag: false,
            timestamp: self.timestamp,
            confidence: self.confidence,
            model_version: self.model_version,
        }
    }

    fn commitment(&self, env: &Env, contract: &Address) -> [u8; 32] {
        payload_commitment(
            env,
            contract,
            &self.wallet,
            &self.pair,
            self.score,
            false,
            false,
            self.timestamp,
            self.confidence,
            self.model_version,
        )
    }
}

/// A four-entry (power-of-two) batch of independent, otherwise-valid entries.
/// Distinct wallets avoid cooldown; distinct pairs make the cross-pair replay
/// concrete.
fn base_entries(env: &Env) -> StdVec<Entry> {
    let pairs = [
        symbol_short!("XLM_USDC"),
        symbol_short!("BTC_USDC"),
        symbol_short!("ETH_USDC"),
        symbol_short!("SOL_USDC"),
    ];
    let mut v: StdVec<Entry> = StdVec::new();
    for (i, pair) in pairs.into_iter().enumerate() {
        v.push(Entry {
            wallet: Address::generate(env),
            pair,
            score: 10 + i as u32 * 5,
            timestamp: i as u64 + 1,
            confidence: 80,
            model_version: 1,
        });
    }
    v
}

fn leaves_of(env: &Env, contract: &Address, entries: &[Entry]) -> StdVec<[u8; 32]> {
    entries.iter().map(|e| merkle_leaf(env, &e.commitment(env, contract))).collect()
}

/// Submit a batch where each entry sends `(submission, proof_source_index)`.
/// A valid entry sends its own submission and its own index; a replay attempt
/// sends a mutated submission (keeping its real index) or an unchanged
/// submission with a *different* index's proof.
fn run_batch(
    env: &Env,
    client: &LedgerLensScoreContractClient<'_>,
    sends: &StdVec<(ScoreSubmission, u32)>,
    leaves: &[[u8; 32]],
    attestation: &BatchAttestation,
) -> BatchResult {
    let mut subs: Vec<ScoreSubmissionWithProof> = Vec::new(env);
    for (sub, proof_index) in sends {
        let (proof_bytes, flags) = build_merkle_proof(env, leaves, *proof_index);
        let mut proof: Vec<BytesN<32>> = Vec::new(env);
        for p in &proof_bytes {
            proof.push_back(BytesN::from_array(env, p));
        }
        subs.push_back(ScoreSubmissionWithProof {
            submission: sub.clone(),
            proof,
            proof_flags: flags,
        });
    }
    client.submit_scores_batch_attested(&Vec::new(env), &subs, attestation)
}

/// Build the canonical `(submission, own_index)` send list for a batch.
fn honest_sends(entries: &[Entry]) -> StdVec<(ScoreSubmission, u32)> {
    entries.iter().enumerate().map(|(i, e)| (e.submission(), i as u32)).collect()
}

/// Assert that exactly `target` was rejected with `InvalidAttestation` and
/// every other entry was accepted.
fn assert_only_rejected(result: &BatchResult, target: u32, total: u32) {
    assert_eq!(result.accepted_count, total - 1, "expected exactly one rejection");
    assert_eq!(result.rejected_count, 1);
    for i in 0..total {
        let r = result.results.get(i).unwrap();
        if i == target {
            assert!(!r.accepted, "target entry {i} should have been rejected");
            assert_eq!(
                r.rejection_code,
                Error::InvalidAttestation as u32,
                "replayed entry must fail as InvalidAttestation, not another code",
            );
        } else {
            assert!(r.accepted, "sibling entry {i} should still succeed");
            assert_eq!(r.rejection_code, 0);
        }
    }
}

// ── 0. Baseline — the honest batch is fully accepted ─────────────────────────

#[test]
fn test_baseline_batch_accepted() {
    let (env, client, _, _) = initialized();
    let key = signing_key(1);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key));

    let entries = base_entries(&env);
    let leaves = leaves_of(&env, &client.address, &entries);
    let root = build_merkle_root(&env, &leaves);
    let attestation = attest(&env, &key, &root);

    let result = run_batch(&env, &client, &honest_sends(&entries), &leaves, &attestation);
    assert_eq!(result.accepted_count, 4);
    assert_eq!(result.rejected_count, 0);
}

// ── 1. Pair replay — the headline isolation property ─────────────────────────

#[test]
fn test_replay_into_another_pair_rejected() {
    let (env, client, _, _) = initialized();
    let key = signing_key(1);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key));

    let entries = base_entries(&env);
    let leaves = leaves_of(&env, &client.address, &entries);
    let root = build_merkle_root(&env, &leaves);
    let attestation = attest(&env, &key, &root);

    // Entry 0 was attested for pair XLM_USDC. Replay its position's proof but
    // claim a different pair (BTC_USDC). The recomputed leaf differs, so the
    // proof no longer reproduces the signed root.
    let target = 0u32;
    let mut sends = honest_sends(&entries);
    let mut replayed = entries[target as usize].submission();
    replayed.asset_pair = symbol_short!("BTC_USDC");
    sends[target as usize] = (replayed, target);

    let result = run_batch(&env, &client, &sends, &leaves, &attestation);
    assert_only_rejected(&result, target, 4);

    // And the score was never written under the replayed pair.
    assert_eq!(
        client.try_get_score(&entries[target as usize].wallet, &symbol_short!("BTC_USDC")),
        Err(Ok(Error::ScoreNotFound)),
    );
}

// ── 2. Wallet replay ─────────────────────────────────────────────────────────

#[test]
fn test_replay_into_another_wallet_rejected() {
    let (env, client, _, _) = initialized();
    let key = signing_key(1);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key));

    let entries = base_entries(&env);
    let leaves = leaves_of(&env, &client.address, &entries);
    let root = build_merkle_root(&env, &leaves);
    let attestation = attest(&env, &key, &root);

    let target = 1u32;
    let attacker_wallet = Address::generate(&env);
    let mut sends = honest_sends(&entries);
    let mut replayed = entries[target as usize].submission();
    replayed.wallet = attacker_wallet.clone();
    sends[target as usize] = (replayed, target);

    let result = run_batch(&env, &client, &sends, &leaves, &attestation);
    assert_only_rejected(&result, target, 4);

    assert_eq!(
        client.try_get_score(&attacker_wallet, &entries[target as usize].pair),
        Err(Ok(Error::ScoreNotFound)),
    );
}

// ── 3. Index / batch-position replay ─────────────────────────────────────────

#[test]
fn test_replay_into_another_position_rejected() {
    let (env, client, _, _) = initialized();
    let key = signing_key(1);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key));

    let entries = base_entries(&env);
    let leaves = leaves_of(&env, &client.address, &entries);
    let root = build_merkle_root(&env, &leaves);
    let attestation = attest(&env, &key, &root);

    // Entry 0's leaf is unchanged, but it is presented at position 2 by
    // attaching position 2's inclusion proof. The proof no longer reproduces
    // the root for leaf 0, so the entry is rejected.
    let target = 0u32;
    let mut sends = honest_sends(&entries);
    sends[target as usize] = (entries[target as usize].submission(), 2);

    let result = run_batch(&env, &client, &sends, &leaves, &attestation);
    assert_only_rejected(&result, target, 4);
}

// ── 4. Timestamp replay ──────────────────────────────────────────────────────

#[test]
fn test_replay_with_mutated_timestamp_rejected() {
    let (env, client, _, _) = initialized();
    let key = signing_key(1);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key));

    let entries = base_entries(&env);
    let leaves = leaves_of(&env, &client.address, &entries);
    let root = build_merkle_root(&env, &leaves);
    let attestation = attest(&env, &key, &root);

    let target = 2u32;
    let mut sends = honest_sends(&entries);
    let mut replayed = entries[target as usize].submission();
    // A different, still-valid (non-zero) timestamp: the leaf binds it, so the
    // Merkle proof fails before the timestamp validation is even reached.
    replayed.timestamp = entries[target as usize].timestamp + 1_000;
    sends[target as usize] = (replayed, target);

    let result = run_batch(&env, &client, &sends, &leaves, &attestation);
    assert_only_rejected(&result, target, 4);
}

// ── 5. Model-version replay ──────────────────────────────────────────────────

#[test]
fn test_replay_with_mutated_model_version_rejected() {
    let (env, client, _, _) = initialized();
    let key = signing_key(1);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key));

    let entries = base_entries(&env);
    let leaves = leaves_of(&env, &client.address, &entries);
    let root = build_merkle_root(&env, &leaves);
    let attestation = attest(&env, &key, &root);

    let target = 3u32;
    let mut sends = honest_sends(&entries);
    let mut replayed = entries[target as usize].submission();
    replayed.model_version = entries[target as usize].model_version + 1;
    sends[target as usize] = (replayed, target);

    let result = run_batch(&env, &client, &sends, &leaves, &attestation);
    assert_only_rejected(&result, target, 4);
}

// ── 6. Cross-batch replay — a leaf from batch A can't ride batch B's root ─────

#[test]
fn test_leaf_from_other_batch_root_rejected() {
    let (env, client, _, _) = initialized();
    let key = signing_key(1);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key));

    // Two independently-signed batches.
    let entries_a = base_entries(&env);
    let leaves_a = leaves_of(&env, &client.address, &entries_a);
    let root_a = build_merkle_root(&env, &leaves_a);

    let entries_b = base_entries(&env);
    let leaves_b = leaves_of(&env, &client.address, &entries_b);
    let root_b = build_merkle_root(&env, &leaves_b);
    let attestation_b = attest(&env, &key, &root_b);

    // Submit entry from batch A, but under batch B's signed root and using
    // A's own proof. The proof reconstructs root_a, not root_b, so it fails.
    let mut subs: Vec<ScoreSubmissionWithProof> = Vec::new(&env);
    let (proof_bytes, flags) = build_merkle_proof(&env, &leaves_a, 0);
    let mut proof: Vec<BytesN<32>> = Vec::new(&env);
    for p in &proof_bytes {
        proof.push_back(BytesN::from_array(&env, p));
    }
    subs.push_back(ScoreSubmissionWithProof {
        submission: entries_a[0].submission(),
        proof,
        proof_flags: flags,
    });

    let result = client.submit_scores_batch_attested(&Vec::new(&env), &subs, &attestation_b);
    assert_eq!(result.accepted_count, 0);
    assert_eq!(result.rejected_count, 1);
    assert_eq!(result.results.get(0).unwrap().rejection_code, Error::InvalidAttestation as u32,);
}
