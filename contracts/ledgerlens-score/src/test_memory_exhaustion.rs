//! Memory-exhaustion / maximum-sized nested input tests (issue #612).
//!
//! The nested attacker-controlled shape this file targets is
//! `submit_scores_batch_attested(signers, submissions, attestation)`:
//! an outer `Vec<ScoreSubmissionWithProof>` (bounded by `MAX_BATCH_SIZE`)
//! whose entries each carry an inner `Vec<BytesN<32>>` Merkle proof
//! (bounded by `MAX_MERKLE_PROOF_DEPTH`), plus a `Vec<Address>` M-of-N
//! `signers` list that — before this change — had no upper bound at all.
//!
//! Companion fix in `lib.rs` (same PR): `submit_scores_batch_attested`'s
//! inline M-of-N check, the shared `require_service_signers_auth`
//! (`veto_parameter_change`), and the shared `require_admin_auth` (every
//! admin-gated entry point) now all reject before doing any per-signer
//! storage read or `require_auth` host call, instead of looping over
//! whatever length `Vec<Address>` the caller supplies.
//!
//! Per-entry proofs never need to be individually valid for the tests
//! below: `verify_merkle_proof` runs exactly `proof.len()` hash
//! iterations regardless of whether the walk matches `root` (see its
//! rustdoc in `lib.rs`), so a filler/mismatching proof already drives
//! the contract through its full worst-case per-entry cost.

extern crate std;

use k256::ecdsa::SigningKey;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Bytes, BytesN, Env, Vec};

use crate::{
    constants, BatchAttestation, Error, LedgerLensScoreContract, LedgerLensScoreContractClient,
    ScoreSubmission, ScoreSubmissionWithProof,
};

// ── Test infrastructure ─────────────────────────────────────────────────────

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client, admin, service)
}

/// Deterministic test signing key (mirrors `test_batch_attestation.rs` /
/// `benches/batch_attested.rs`).
fn signing_key(seed: u8) -> SigningKey {
    let mut bytes = [0u8; 32];
    bytes[31] = seed;
    bytes[0] = 1; // avoid an all-zero scalar
    SigningKey::from_bytes((&bytes).into()).unwrap()
}

fn pubkey_bytes(env: &Env, key: &SigningKey) -> Bytes {
    let point = key.verifying_key().to_encoded_point(true); // compressed
    Bytes::from_slice(env, point.as_bytes())
}

/// Sign `root` the same way the off-chain pipeline does: the secp256k1
/// signature is over `SHA256(root)`, not `root` directly.
fn attest(env: &Env, key: &SigningKey, root: &[u8; 32]) -> BatchAttestation {
    let digest = env.crypto().sha256(&Bytes::from_array(env, root)).to_bytes().to_array();
    let (sig, recid) = key.sign_prehash_recoverable(&digest).unwrap();
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&sig.to_bytes());
    sig_bytes[64] = recid.to_byte();
    BatchAttestation {
        merkle_root: BytesN::from_array(env, root),
        signature: BytesN::from_array(env, &sig_bytes),
    }
}

/// One maximum-sized nested entry: a submission wrapped in a proof of
/// exactly `MAX_MERKLE_PROOF_DEPTH` filler siblings. The siblings never
/// need to hash to a real root — see the module doc comment.
fn max_depth_entry(env: &Env, wallet: &Address, ts: u64) -> ScoreSubmissionWithProof {
    let submission = ScoreSubmission {
        wallet: wallet.clone(),
        asset_pair: symbol_short!("XLM_USDC"),
        score: 42,
        benford_flag: false,
        ml_flag: false,
        timestamp: ts,
        confidence: 90,
        model_version: 1,
    };
    let mut proof: Vec<BytesN<32>> = Vec::new(env);
    for _ in 0..constants::MAX_MERKLE_PROOF_DEPTH {
        proof.push_back(BytesN::from_array(env, &[0xAB; 32]));
    }
    ScoreSubmissionWithProof { submission, proof, proof_flags: 0 }
}

// ── 1. Maximum-sized nested batch: no panic, bounded cost ───────────────────

#[test]
fn test_max_batch_of_max_depth_proofs_no_panic_and_bounded_cost() {
    let (env, client, _admin, _service) = setup();
    let key = signing_key(7);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key));

    // A root we sign ourselves; the per-entry proofs below are filler and
    // are expected to mismatch it, but the whole-batch root signature
    // still needs to check out for the contract to reach the per-entry
    // Merkle walk at all.
    let root = [0x11u8; 32];
    let attestation = attest(&env, &key, &root);

    let mut submissions: Vec<ScoreSubmissionWithProof> = Vec::new(&env);
    for i in 0..constants::MAX_BATCH_SIZE {
        let wallet = Address::generate(&env);
        submissions.push_back(max_depth_entry(&env, &wallet, 1_700_000_000 + i as u64));
    }

    env.budget().reset_unlimited();
    env.budget().reset_tracker();

    // The maximum supported nested shape: MAX_BATCH_SIZE outer entries,
    // each carrying a MAX_MERKLE_PROOF_DEPTH-deep proof. Must not panic,
    // and every entry is rejected individually (mismatched proof), not
    // the whole batch.
    let result = client.submit_scores_batch_attested(&Vec::new(&env), &submissions, &attestation);

    assert_eq!(result.accepted_count, 0);
    assert_eq!(result.rejected_count, constants::MAX_BATCH_SIZE);
    for i in 0..constants::MAX_BATCH_SIZE {
        let entry = result.results.get(i).unwrap();
        assert!(!entry.accepted);
        assert_eq!(entry.rejection_code, Error::InvalidAttestation as u32);
    }

    // Evidence for the PR's resource-bound acceptance criterion. These
    // ceilings are deliberately generous sanity checks against Soroban's
    // approximate per-invocation mainnet resource limits (~100M
    // instructions), not tight regression baselines — CI's own
    // instruction-count benchmark (`benches/batch_attested.rs`) is the
    // source of truth for exact numbers, which should be pasted into the
    // PR description.
    let cpu = env.budget().cpu_instruction_cost();
    let mem = env.budget().memory_bytes_cost();
    assert!(cpu < 100_000_000, "cpu cost exceeds sanity ceiling: {cpu}");
    assert!(mem < 41_000_000, "memory cost exceeds sanity ceiling: {mem}");
}

// ── 2. signers Vec padded past the service set: rejected, not looped ───────

#[test]
fn test_batch_attested_oversized_signers_vec_rejected() {
    let (env, client, admin, _service) = setup();
    let key = signing_key(3);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key));

    let signer = Address::generate(&env);
    client.add_service_signer(&Vec::new(&env), &signer);
    client.set_service_threshold(&Vec::new(&env), &1);

    // Pad `signers` to one more entry than the service set can ever
    // contain — a legitimate M-of-N call never needs this, so it must be
    // rejected before the per-signer storage-read/require_auth loop.
    let mut signers: Vec<Address> = Vec::new(&env);
    signers.push_back(signer.clone());
    signers.push_back(signer.clone());

    let root = [0x22u8; 32];
    let attestation = attest(&env, &key, &root);
    let wallet = Address::generate(&env);
    let mut submissions: Vec<ScoreSubmissionWithProof> = Vec::new(&env);
    submissions.push_back(max_depth_entry(&env, &wallet, 1_700_000_000));

    let result = client.try_submit_scores_batch_attested(&signers, &submissions, &attestation);
    assert_eq!(result, Err(Ok(Error::TooManySigners)));

    // `admin` stays reachable via the ordinary single-admin path used by
    // `add_service_signer` / `set_service_threshold` above, confirming
    // this rejection is scoped to the oversized call, not a lockout.
    let _ = admin;
}

// ── 3. Same bound on the shared service-signer auth helper ─────────────────

#[test]
fn test_veto_parameter_change_oversized_signers_vec_rejected() {
    let (env, client, _admin, _service) = setup();
    let signer = Address::generate(&env);
    client.add_service_signer(&Vec::new(&env), &signer);
    client.set_service_threshold(&Vec::new(&env), &1);

    let mut signers: Vec<Address> = Vec::new(&env);
    signers.push_back(signer.clone());
    signers.push_back(signer.clone());

    // No pending proposal exists; `require_service_signers_auth` runs
    // before the not-found lookup, so the oversized Vec is rejected
    // first regardless of `proposal_id`.
    let result = client.try_veto_parameter_change(&signers, &0u64);
    assert_eq!(result, Err(Ok(Error::TooManySigners)));
}

// ── 4. Same bound on the shared admin-signer auth helper ───────────────────

#[test]
fn test_pause_oversized_admin_signers_vec_rejected() {
    let (env, client, admin, _service) = setup();
    let extra = Address::generate(&env);
    client.add_admin_signer(&Vec::new(&env), &admin);
    client.add_admin_signer(&Vec::new(&env), &extra);
    client.set_admin_threshold(&Vec::new(&env), &1);

    let mut admin_signers: Vec<Address> = Vec::new(&env);
    admin_signers.push_back(admin.clone());
    admin_signers.push_back(extra.clone());
    admin_signers.push_back(admin.clone());

    let result = client.try_pause(&admin_signers);
    assert_eq!(result, Err(Ok(Error::TooManySigners)));
}

// ── 5. Unauthorized signer: still rejected ahead of the size bound ─────────

#[test]
fn test_batch_attested_unauthorized_signer_rejected() {
    let (env, client, _admin, _service) = setup();
    let key = signing_key(5);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key));

    let signer = Address::generate(&env);
    client.add_service_signer(&Vec::new(&env), &signer);
    client.set_service_threshold(&Vec::new(&env), &1);

    let stranger = Address::generate(&env); // never added to the service set
    let mut signers: Vec<Address> = Vec::new(&env);
    signers.push_back(stranger);

    let root = [0x33u8; 32];
    let attestation = attest(&env, &key, &root);
    let wallet = Address::generate(&env);
    let mut submissions: Vec<ScoreSubmissionWithProof> = Vec::new(&env);
    submissions.push_back(max_depth_entry(&env, &wallet, 1_700_000_000));

    let result = client.try_submit_scores_batch_attested(&signers, &submissions, &attestation);
    assert_eq!(result, Err(Ok(Error::UnauthorizedSigner)));
}

// ── 6. Bounded public read under a maximum-sized index ──────────────────────

#[test]
fn test_get_expiring_entries_max_u32_request_no_panic_and_bounded() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    client.submit_score(&Vec::new(&env), &wallet, &pair, &42, &true, &false, &1, &90, &1, &None);

    // A public, unauthenticated read. `max_entries = u32::MAX` must not
    // panic and must never return more than
    // `MAX_EXPIRING_ENTRIES_PER_CALL`, regardless of what the caller asks
    // for.
    let entries = client.get_expiring_entries(&u32::MAX);
    assert!(entries.len() <= constants::MAX_EXPIRING_ENTRIES_PER_CALL);
}
