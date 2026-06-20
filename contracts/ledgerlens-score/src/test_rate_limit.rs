//! Tests for the per-wallet/pair submission rate limiting (cooldown) mechanism.
//!
//! Time is simulated with `env.ledger().with_mut(|l| l.timestamp = ...)`; the
//! contract derives the cooldown deadline from `env.ledger().timestamp()`,
//! which is deterministic and cannot be set by the caller on-chain.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{
    constants::{DEFAULT_COOLDOWN_SECS, MAX_COOLDOWN_SECS, MIN_COOLDOWN_SECS},
    BatchResult, Error, LedgerLensScoreContract, LedgerLensScoreContractClient, ScoreSubmission,
};

/// Ledger timestamp the tests start from (an arbitrary fixed instant).
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

// ── Defaults ───────────────────────────────────────────────────────────────────

#[test]
fn test_default_cooldown_is_one_hour() {
    let (_env, client, _admin) = setup();
    assert_eq!(client.get_cooldown(), DEFAULT_COOLDOWN_SECS);
    assert_eq!(client.get_cooldown(), 3_600);
}

#[test]
fn test_get_last_submit_time_initial_zero() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    assert_eq!(client.get_last_submit_time(&wallet, &pair), 0);
}

// ── Core cooldown enforcement ─────────────────────────────────────────────────

#[test]
fn test_first_submit_always_accepted() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );
    assert!(result.is_ok());
    assert_eq!(client.get_last_submit_time(&wallet, &pair), START_TS);
}

#[test]
fn test_second_submit_within_cooldown_rejected() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );

    advance_to(&env, START_TS + DEFAULT_COOLDOWN_SECS - 1);
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &60,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::RateLimitExceeded)));

    // The rejected submission must not have overwritten the stored score.
    assert_eq!(client.get_score(&wallet, &pair).score, 50);
}

#[test]
fn test_second_submit_after_cooldown_accepted() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );

    advance_to(&env, START_TS + DEFAULT_COOLDOWN_SECS + 1);
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &60,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );
    assert_eq!(client.get_score(&wallet, &pair).score, 60);
}

#[test]
fn test_cooldown_exactly_at_boundary() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );

    // now == last_submit + cooldown exactly — must be accepted (strict `<` rejects).
    advance_to(&env, START_TS + DEFAULT_COOLDOWN_SECS);
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &60,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );
    assert!(result.is_ok());
    assert_eq!(client.get_score(&wallet, &pair).score, 60);
}

// ── Batch submission ──────────────────────────────────────────────────────────

#[test]
fn test_batch_rate_limited_entry_skipped() {
    let (env, client, _admin) = setup();
    let limited_wallet = Address::generate(&env);
    let fresh_wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.submit_score(
        &Vec::new(&env),
        &limited_wallet,
        &pair,
        &10,
        &false,
        &false,
        &START_TS,
        &50,
        &1,
        &None,
    );

    advance_to(&env, START_TS + 10); // still well within the default cooldown

    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: limited_wallet.clone(),
        asset_pair: pair.clone(),
        score: 99,
        benford_flag: false,
        ml_flag: false,
        timestamp: START_TS,
        confidence: 90,
        model_version: 1,
    });
    batch.push_back(ScoreSubmission {
        wallet: fresh_wallet.clone(),
        asset_pair: pair.clone(),
        score: 40,
        benford_flag: false,
        ml_flag: false,
        timestamp: START_TS,
        confidence: 90,
        model_version: 1,
    });

    let result: BatchResult = client.submit_scores_batch(&batch);
    assert_eq!(result.accepted_count, 1);
    assert_eq!(result.rejected_count, 1);
    // First entry (rate-limited) — rejected with code 23 (RateLimitExceeded).
    assert!(!result.results.get(0).unwrap().accepted);
    assert_eq!(result.results.get(0).unwrap().rejection_code, 23);
    // Second entry — accepted.
    assert!(result.results.get(1).unwrap().accepted);
    assert_eq!(result.results.get(1).unwrap().rejection_code, 0);

    // limited_wallet's entry was skipped — its score is unchanged.
    assert_eq!(client.get_score(&limited_wallet, &pair).score, 10);
    // fresh_wallet's entry, sharing no prior submission, was processed.
    assert_eq!(client.get_score(&fresh_wallet, &pair).score, 40);
}

#[test]
fn test_batch_second_entry_for_same_pair_rate_limited() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: wallet.clone(),
        asset_pair: pair.clone(),
        score: 10,
        benford_flag: false,
        ml_flag: false,
        timestamp: START_TS,
        confidence: 50,
        model_version: 1,
    });
    batch.push_back(ScoreSubmission {
        wallet: wallet.clone(),
        asset_pair: pair.clone(),
        score: 20,
        benford_flag: false,
        ml_flag: false,
        timestamp: START_TS,
        confidence: 50,
        model_version: 1,
    });

    // Both entries share the same ledger timestamp, so the second is rejected
    // by the cooldown the first entry just set.
    let result: BatchResult = client.submit_scores_batch(&batch);
    assert_eq!(result.accepted_count, 1);
    assert_eq!(result.rejected_count, 1);
    // First entry — accepted.
    assert!(result.results.get(0).unwrap().accepted);
    // Second entry — rate-limited.
    assert!(!result.results.get(1).unwrap().accepted);
    assert_eq!(result.results.get(1).unwrap().rejection_code, 23);
    assert_eq!(client.get_score(&wallet, &pair).score, 10);
}

// ── Admin override ────────────────────────────────────────────────────────────

#[test]
fn test_admin_override_clears_cooldown() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );

    client.override_rate_limit(&Vec::new(&env), &wallet, &pair);
    assert_eq!(client.get_last_submit_time(&wallet, &pair), 0);

    // Still at START_TS, but immediately accepted since the cooldown was cleared.
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &70,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );
    assert_eq!(client.get_score(&wallet, &pair).score, 70);
}

#[test]
fn test_override_rate_limit_before_init_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let result = client.try_override_rate_limit(&Vec::new(&env), &wallet, &pair);
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

// ── Cooldown configuration ─────────────────────────────────────────────────────

#[test]
fn test_set_cooldown_below_min_rejected() {
    let (env, client, _admin) = setup();
    let result = client.try_set_cooldown(&Vec::new(&env), &(MIN_COOLDOWN_SECS - 1));
    assert_eq!(result, Err(Ok(Error::InvalidCooldown)));
}

#[test]
fn test_set_cooldown_above_max_rejected() {
    let (env, client, _admin) = setup();
    let result = client.try_set_cooldown(&Vec::new(&env), &(MAX_COOLDOWN_SECS + 1));
    assert_eq!(result, Err(Ok(Error::InvalidCooldown)));
}

#[test]
fn test_set_cooldown_within_bounds_applied() {
    let (env, client, _admin) = setup();
    client.set_cooldown(&Vec::new(&env), &MIN_COOLDOWN_SECS);
    assert_eq!(client.get_cooldown(), MIN_COOLDOWN_SECS);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );

    advance_to(&env, START_TS + MIN_COOLDOWN_SECS);
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &60,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );
    assert_eq!(client.get_score(&wallet, &pair).score, 60);
}

#[test]
fn test_set_cooldown_boundary_values_accepted() {
    let (env, client, _admin) = setup();

    client.set_cooldown(&Vec::new(&env), &MIN_COOLDOWN_SECS);
    assert_eq!(client.get_cooldown(), MIN_COOLDOWN_SECS);

    client.set_cooldown(&Vec::new(&env), &MAX_COOLDOWN_SECS);
    assert_eq!(client.get_cooldown(), MAX_COOLDOWN_SECS);
}

// ── Independence across pairs and wallets ───────────────────────────────────────

#[test]
fn test_cooldown_is_per_pair() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("XLM_BTC");

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair_a,
        &50,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );

    // Still within pair_a's cooldown, but pair_b has never been submitted.
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair_b,
        &60,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );
    assert!(result.is_ok());
}

#[test]
fn test_cooldown_is_per_wallet() {
    let (env, client, _admin) = setup();
    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.submit_score(
        &Vec::new(&env),
        &wallet_a,
        &pair,
        &50,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );

    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet_b,
        &pair,
        &60,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );
    assert!(result.is_ok());
}
