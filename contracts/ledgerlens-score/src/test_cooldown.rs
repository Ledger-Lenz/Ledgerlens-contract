//! Dedicated cooldown / rate-limit edge-case tests.
//!
//! Complements `test_rate_limit.rs` with scenarios that exercise
//! `override_rate_limit` reset semantics, cross-path cooldown enforcement
//! (consensus, batch), mid-flight cooldown reconfiguration, and boundary
//! conditions on `set_cooldown`.

use k256::ecdsa::SigningKey;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Bytes, BytesN, Env, Symbol, Vec,
};

use crate::{
    constants::{DEFAULT_COOLDOWN_SECS, MAX_COOLDOWN_SECS, MIN_COOLDOWN_SECS},
    Error, LedgerLensScoreContract, LedgerLensScoreContractClient, ModelSubmission,
    ScoreAttestation, ScoreSubmission,
};

const START_TS: u64 = 1_700_000_000;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    (env, client, admin)
}

fn advance_to(env: &Env, ts: u64) {
    env.ledger().with_mut(|l| l.timestamp = ts);
}

fn submit(
    env: &Env,
    client: &LedgerLensScoreContractClient<'_>,
    wallet: &Address,
    pair: &Symbol,
    score: u32,
) {
    client.submit_score(
        &Vec::new(env),
        wallet,
        pair,
        &score,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );
}

fn try_submit(
    env: &Env,
    client: &LedgerLensScoreContractClient<'_>,
    wallet: &Address,
    pair: &Symbol,
    score: u32,
) -> Result<(), Result<Error, soroban_sdk::InvokeError>> {
    client.try_submit_score(
        &Vec::new(env),
        wallet,
        pair,
        &score,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    )
}

// ── Consensus submission helpers ──────────────────────────────────────────────

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

fn commitment(
    env: &Env,
    contract_id: &Address,
    wallet: &Address,
    pair: &Symbol,
    score: u32,
    timestamp: u64,
    confidence: u32,
    model_version: u32,
) -> [u8; 32] {
    env.as_contract(contract_id, || {
        LedgerLensScoreContract::compute_commitment(
            env, wallet, pair, score, false, false, timestamp, confidence, model_version,
        )
        .unwrap()
        .to_bytes()
        .to_array()
    })
}

fn attest(env: &Env, key: &SigningKey, digest: [u8; 32]) -> ScoreAttestation {
    let (sig, recid) = key.sign_prehash_recoverable(&digest).unwrap();
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&sig.to_bytes());
    sig_bytes[64] = recid.to_byte();
    ScoreAttestation {
        commitment: BytesN::from_array(env, &digest),
        signature: BytesN::from_array(env, &sig_bytes),
    }
}

fn consensus_pair(
    env: &Env,
    client: &LedgerLensScoreContractClient<'_>,
    key: &SigningKey,
    wallet: &Address,
    pair: &Symbol,
    scores: &[u32],
    timestamp: u64,
) -> Vec<ModelSubmission> {
    let mut subs = Vec::new(env);
    for (i, &score) in scores.iter().enumerate() {
        let mv = (i + 1) as u32;
        let digest = commitment(env, &client.address, wallet, pair, score, timestamp, 90, mv);
        subs.push_back(ModelSubmission {
            model_version: mv,
            score,
            confidence: 90,
            benford_flag: false,
            ml_flag: false,
            attestation: attest(env, key, digest),
        });
    }
    subs
}

// ── override_rate_limit: cooldown restarts after override ─────────────────────

#[test]
fn test_override_then_submit_restarts_cooldown() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 50);

    client.override_rate_limit(&Vec::new(&env), &wallet, &pair);
    submit(&env, &client, &wallet, &pair, 70);

    // A third submit immediately after must be rejected — the override only
    // cleared the *previous* cooldown; the second submit started a new one.
    let result = try_submit(&env, &client, &wallet, &pair, 80);
    assert_eq!(result, Err(Ok(Error::RateLimitExceeded)));
    assert_eq!(client.get_score(&wallet, &pair).score, 70);
}

#[test]
fn test_override_then_submit_then_wait_full_cooldown() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 50);
    client.override_rate_limit(&Vec::new(&env), &wallet, &pair);
    submit(&env, &client, &wallet, &pair, 70);

    advance_to(&env, START_TS + DEFAULT_COOLDOWN_SECS);
    submit(&env, &client, &wallet, &pair, 90);
    assert_eq!(client.get_score(&wallet, &pair).score, 90);
}

// ── override_rate_limit: scope isolation ──────────────────────────────────────

#[test]
fn test_override_does_not_affect_other_pairs() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("XLM_BTC");

    submit(&env, &client, &wallet, &pair_a, 50);
    submit(&env, &client, &wallet, &pair_b, 60);

    client.override_rate_limit(&Vec::new(&env), &wallet, &pair_a);

    // pair_a is cleared — immediate re-submit works.
    submit(&env, &client, &wallet, &pair_a, 70);

    // pair_b was NOT overridden — still rate-limited.
    let result = try_submit(&env, &client, &wallet, &pair_b, 80);
    assert_eq!(result, Err(Ok(Error::RateLimitExceeded)));
    assert_eq!(client.get_score(&wallet, &pair_b).score, 60);
}

#[test]
fn test_override_does_not_affect_other_wallets() {
    let (env, client, _admin) = setup();
    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet_a, &pair, 50);
    submit(&env, &client, &wallet_b, &pair, 60);

    client.override_rate_limit(&Vec::new(&env), &wallet_a, &pair);

    submit(&env, &client, &wallet_a, &pair, 70);

    let result = try_submit(&env, &client, &wallet_b, &pair, 80);
    assert_eq!(result, Err(Ok(Error::RateLimitExceeded)));
    assert_eq!(client.get_score(&wallet_b, &pair).score, 60);
}

// ── override_rate_limit: idempotent / no-op cases ─────────────────────────────

#[test]
fn test_override_on_never_submitted_pair_is_noop() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    assert_eq!(client.get_last_submit_time(&wallet, &pair), 0);
    client.override_rate_limit(&Vec::new(&env), &wallet, &pair);
    assert_eq!(client.get_last_submit_time(&wallet, &pair), 0);

    // First submit after a no-op override still works.
    submit(&env, &client, &wallet, &pair, 42);
    assert_eq!(client.get_score(&wallet, &pair).score, 42);
}

#[test]
fn test_double_override_is_idempotent() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 50);

    client.override_rate_limit(&Vec::new(&env), &wallet, &pair);
    client.override_rate_limit(&Vec::new(&env), &wallet, &pair);

    assert_eq!(client.get_last_submit_time(&wallet, &pair), 0);
    submit(&env, &client, &wallet, &pair, 70);
    assert_eq!(client.get_score(&wallet, &pair).score, 70);
}

// ── override_rate_limit + batch submission ────────────────────────────────────

#[test]
fn test_batch_respects_override_for_single_entry() {
    let (env, client, _admin) = setup();
    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet_a, &pair, 10);
    submit(&env, &client, &wallet_b, &pair, 20);

    // Override only wallet_a.
    client.override_rate_limit(&Vec::new(&env), &wallet_a, &pair);

    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: wallet_a.clone(),
        asset_pair: pair.clone(),
        score: 55,
        benford_flag: false,
        ml_flag: false,
        timestamp: START_TS,
        confidence: 90,
        model_version: 1,
    });
    batch.push_back(ScoreSubmission {
        wallet: wallet_b.clone(),
        asset_pair: pair.clone(),
        score: 65,
        benford_flag: false,
        ml_flag: false,
        timestamp: START_TS,
        confidence: 90,
        model_version: 1,
    });

    let result = client.submit_scores_batch(&batch);
    assert_eq!(result.accepted_count, 1);
    assert_eq!(result.rejected_count, 1);
    assert!(result.results.get(0).unwrap().accepted);
    assert!(!result.results.get(1).unwrap().accepted);
    assert_eq!(result.results.get(1).unwrap().rejection_code, Error::RateLimitExceeded as u32);

    assert_eq!(client.get_score(&wallet_a, &pair).score, 55);
    assert_eq!(client.get_score(&wallet_b, &pair).score, 20);
}

// ── Consensus path cooldown ──────────────────────────────────────────────────

#[test]
fn test_consensus_within_cooldown_rejected() {
    let (env, client, _admin) = setup();
    let key = signing_key(1);
    client.set_service_pubkey(&pubkey_bytes(&env, &key));

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Prime with a regular submission.
    submit(&env, &client, &wallet, &pair, 50);

    // Consensus submission within cooldown must fail.
    let subs = consensus_pair(&env, &client, &key, &wallet, &pair, &[50, 52], START_TS);
    let result =
        client.try_submit_consensus_score(&Vec::new(&env), &wallet, &pair, &subs, &START_TS);
    assert_eq!(result, Err(Ok(Error::RateLimitExceeded)));
    assert_eq!(client.get_score(&wallet, &pair).score, 50);
}

#[test]
fn test_consensus_after_cooldown_accepted() {
    let (env, client, _admin) = setup();
    let key = signing_key(2);
    client.set_service_pubkey(&pubkey_bytes(&env, &key));

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 50);

    advance_to(&env, START_TS + DEFAULT_COOLDOWN_SECS);
    let subs = consensus_pair(
        &env,
        &client,
        &key,
        &wallet,
        &pair,
        &[70, 72],
        START_TS + DEFAULT_COOLDOWN_SECS,
    );
    client.submit_consensus_score(&Vec::new(&env), &wallet, &pair, &subs, &(START_TS + DEFAULT_COOLDOWN_SECS));
    assert_eq!(client.get_score(&wallet, &pair).score, 70);
}

#[test]
fn test_consensus_after_override_accepted() {
    let (env, client, _admin) = setup();
    let key = signing_key(3);
    client.set_service_pubkey(&pubkey_bytes(&env, &key));

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 50);
    client.override_rate_limit(&Vec::new(&env), &wallet, &pair);

    let subs = consensus_pair(&env, &client, &key, &wallet, &pair, &[80, 82], START_TS);
    client.submit_consensus_score(&Vec::new(&env), &wallet, &pair, &subs, &START_TS);
    assert_eq!(client.get_score(&wallet, &pair).score, 80);
}

// ── set_cooldown: mid-flight reconfiguration ─────────────────────────────────

#[test]
fn test_shortening_cooldown_unlocks_earlier() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 50);

    // Reduce cooldown from 3600 s to the minimum (60 s).
    client.set_cooldown(&Vec::new(&env), &MIN_COOLDOWN_SECS);

    advance_to(&env, START_TS + MIN_COOLDOWN_SECS);
    submit(&env, &client, &wallet, &pair, 60);
    assert_eq!(client.get_score(&wallet, &pair).score, 60);
}

#[test]
fn test_lengthening_cooldown_blocks_previously_valid_time() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 50);

    // Extend cooldown to 24 hours.
    client.set_cooldown(&Vec::new(&env), &MAX_COOLDOWN_SECS);

    // At DEFAULT_COOLDOWN (1 h) past the original submit — would have been fine
    // under the old cooldown, but 24 h is now in effect.
    advance_to(&env, START_TS + DEFAULT_COOLDOWN_SECS);
    let result = try_submit(&env, &client, &wallet, &pair, 60);
    assert_eq!(result, Err(Ok(Error::RateLimitExceeded)));

    // After the full 24 h cooldown, it succeeds.
    advance_to(&env, START_TS + MAX_COOLDOWN_SECS);
    submit(&env, &client, &wallet, &pair, 60);
    assert_eq!(client.get_score(&wallet, &pair).score, 60);
}

// ── set_cooldown: boundary values ────────────────────────────────────────────

#[test]
fn test_set_cooldown_to_min_then_enforce() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_cooldown(&Vec::new(&env), &MIN_COOLDOWN_SECS);
    assert_eq!(client.get_cooldown(), MIN_COOLDOWN_SECS);

    submit(&env, &client, &wallet, &pair, 50);

    // One second before the min cooldown expires — rejected.
    advance_to(&env, START_TS + MIN_COOLDOWN_SECS - 1);
    let result = try_submit(&env, &client, &wallet, &pair, 60);
    assert_eq!(result, Err(Ok(Error::RateLimitExceeded)));

    // Exactly at the min cooldown boundary — accepted.
    advance_to(&env, START_TS + MIN_COOLDOWN_SECS);
    submit(&env, &client, &wallet, &pair, 60);
    assert_eq!(client.get_score(&wallet, &pair).score, 60);
}

#[test]
fn test_set_cooldown_to_max_then_enforce() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_cooldown(&Vec::new(&env), &MAX_COOLDOWN_SECS);
    assert_eq!(client.get_cooldown(), MAX_COOLDOWN_SECS);

    submit(&env, &client, &wallet, &pair, 50);

    // One second before the max cooldown expires — rejected.
    advance_to(&env, START_TS + MAX_COOLDOWN_SECS - 1);
    let result = try_submit(&env, &client, &wallet, &pair, 60);
    assert_eq!(result, Err(Ok(Error::RateLimitExceeded)));

    // Exactly at the max cooldown boundary — accepted.
    advance_to(&env, START_TS + MAX_COOLDOWN_SECS);
    submit(&env, &client, &wallet, &pair, 60);
    assert_eq!(client.get_score(&wallet, &pair).score, 60);
}

// ── Batch: per-entry cooldown independence ───────────────────────────────────

#[test]
fn test_batch_mixed_cooldown_states_per_wallet_pair() {
    let (env, client, _admin) = setup();
    let w1 = Address::generate(&env);
    let w2 = Address::generate(&env);
    let w3 = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // w1: already submitted, still within cooldown.
    submit(&env, &client, &w1, &pair, 10);
    // w2: submitted long ago — cooldown expired.
    submit(&env, &client, &w2, &pair, 20);
    // w3: never submitted.

    advance_to(&env, START_TS + DEFAULT_COOLDOWN_SECS + 1);

    // Re-submit w1 so its cooldown is fresh at the new timestamp.
    submit(&env, &client, &w1, &pair, 15);

    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: w1.clone(),
        asset_pair: pair.clone(),
        score: 99,
        benford_flag: false,
        ml_flag: false,
        timestamp: START_TS,
        confidence: 90,
        model_version: 1,
    });
    batch.push_back(ScoreSubmission {
        wallet: w2.clone(),
        asset_pair: pair.clone(),
        score: 88,
        benford_flag: false,
        ml_flag: false,
        timestamp: START_TS,
        confidence: 90,
        model_version: 1,
    });
    batch.push_back(ScoreSubmission {
        wallet: w3.clone(),
        asset_pair: pair.clone(),
        score: 77,
        benford_flag: false,
        ml_flag: false,
        timestamp: START_TS,
        confidence: 90,
        model_version: 1,
    });

    let result = client.submit_scores_batch(&batch);
    // w1 rejected (fresh cooldown), w2 accepted (cooldown expired), w3 accepted (never submitted).
    assert_eq!(result.accepted_count, 2);
    assert_eq!(result.rejected_count, 1);

    assert!(!result.results.get(0).unwrap().accepted);
    assert_eq!(result.results.get(0).unwrap().rejection_code, Error::RateLimitExceeded as u32);
    assert!(result.results.get(1).unwrap().accepted);
    assert!(result.results.get(2).unwrap().accepted);

    assert_eq!(client.get_score(&w1, &pair).score, 15);
    assert_eq!(client.get_score(&w2, &pair).score, 88);
    assert_eq!(client.get_score(&w3, &pair).score, 77);
}

#[test]
fn test_batch_different_pairs_same_wallet_independent_cooldown() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("XLM_BTC");

    // Submit only for pair_a.
    submit(&env, &client, &wallet, &pair_a, 10);

    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: wallet.clone(),
        asset_pair: pair_a.clone(),
        score: 50,
        benford_flag: false,
        ml_flag: false,
        timestamp: START_TS,
        confidence: 90,
        model_version: 1,
    });
    batch.push_back(ScoreSubmission {
        wallet: wallet.clone(),
        asset_pair: pair_b.clone(),
        score: 60,
        benford_flag: false,
        ml_flag: false,
        timestamp: START_TS,
        confidence: 90,
        model_version: 1,
    });

    let result = client.submit_scores_batch(&batch);
    // pair_a: rate-limited. pair_b: first submission, accepted.
    assert_eq!(result.accepted_count, 1);
    assert_eq!(result.rejected_count, 1);
    assert!(!result.results.get(0).unwrap().accepted);
    assert!(result.results.get(1).unwrap().accepted);

    assert_eq!(client.get_score(&wallet, &pair_a).score, 10);
    assert_eq!(client.get_score(&wallet, &pair_b).score, 60);
}

// ── Cross-path cooldown interaction ──────────────────────────────────────────

#[test]
fn test_single_submit_then_consensus_within_cooldown_rejected() {
    let (env, client, _admin) = setup();
    let key = signing_key(4);
    client.set_service_pubkey(&pubkey_bytes(&env, &key));

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 50);

    // Consensus uses write_score_with_rate_limit, same cooldown applies.
    let subs = consensus_pair(&env, &client, &key, &wallet, &pair, &[70, 72], START_TS);
    let result =
        client.try_submit_consensus_score(&Vec::new(&env), &wallet, &pair, &subs, &START_TS);
    assert_eq!(result, Err(Ok(Error::RateLimitExceeded)));
}

#[test]
fn test_consensus_then_single_submit_within_cooldown_rejected() {
    let (env, client, _admin) = setup();
    let key = signing_key(5);
    client.set_service_pubkey(&pubkey_bytes(&env, &key));

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let subs = consensus_pair(&env, &client, &key, &wallet, &pair, &[50, 52], START_TS);
    client.submit_consensus_score(&Vec::new(&env), &wallet, &pair, &subs, &START_TS);

    // Single submit immediately after — same cooldown timer.
    let result = try_submit(&env, &client, &wallet, &pair, 80);
    assert_eq!(result, Err(Ok(Error::RateLimitExceeded)));
}
