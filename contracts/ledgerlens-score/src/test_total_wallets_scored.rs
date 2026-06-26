//! Tests for `get_total_wallets_scored() -> u64`.
//!
//! The counter tracks the number of unique `(wallet, asset_pair)` combinations
//! ever successfully scored.  It is incremented exactly once per combination
//! — on the first accepted submission — and never decremented.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{
    constants::DEFAULT_COOLDOWN_SECS, LedgerLensScoreContract, LedgerLensScoreContractClient,
    ScoreSubmission,
};

const START_TS: u64 = 1_700_000_000;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    (env, client)
}

fn advance(env: &Env, delta: u64) {
    env.ledger().with_mut(|l| l.timestamp += delta);
}

fn submit(
    env: &Env,
    client: &LedgerLensScoreContractClient<'_>,
    wallet: &Address,
    pair: &soroban_sdk::Symbol,
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

// ── Initial state ─────────────────────────────────────────────────────────────

#[test]
fn test_total_wallets_scored_starts_at_zero() {
    let (_env, client) = setup();
    assert_eq!(client.get_total_wallets_scored(), 0);
}

// ── Single submission increments the counter once ────────────────────────────

#[test]
fn test_total_wallets_scored_first_submission() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    assert_eq!(client.get_total_wallets_scored(), 0);
    submit(&env, &client, &wallet, &pair, 50);
    assert_eq!(client.get_total_wallets_scored(), 1);
}

// ── Re-submission for same combination does NOT increment ─────────────────────

#[test]
fn test_total_wallets_scored_resubmission_does_not_increment() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 50);
    assert_eq!(client.get_total_wallets_scored(), 1);

    // Advance past cooldown and resubmit the same (wallet, pair).
    advance(&env, DEFAULT_COOLDOWN_SECS);
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &60,
        &false,
        &false,
        &(START_TS + DEFAULT_COOLDOWN_SECS),
        &90,
        &1,
        &None,
    );
    // Must still be 1 — same combination.
    assert_eq!(client.get_total_wallets_scored(), 1);
}

// ── Different wallets are counted independently ───────────────────────────────

#[test]
fn test_total_wallets_scored_multiple_wallets() {
    let (env, client) = setup();
    let pair = symbol_short!("XLM_USDC");

    for expected in 1u64..=5 {
        let w = Address::generate(&env);
        submit(&env, &client, &w, &pair, 50);
        assert_eq!(client.get_total_wallets_scored(), expected);
    }
}

// ── Different asset pairs count as separate combinations ─────────────────────

#[test]
fn test_total_wallets_scored_different_pairs() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("BTC_USDC");

    submit(&env, &client, &wallet, &pair_a, 50);
    assert_eq!(client.get_total_wallets_scored(), 1);

    // Same wallet, different pair → new combination.
    submit(&env, &client, &wallet, &pair_b, 60);
    assert_eq!(client.get_total_wallets_scored(), 2);
}

// ── Rate-limited rejections do NOT increment the counter ─────────────────────

#[test]
fn test_total_wallets_scored_rejected_does_not_increment() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 50);
    assert_eq!(client.get_total_wallets_scored(), 1);

    // Immediate re-submit — rejected by cooldown.
    let _ = client.try_submit_score(
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
    assert_eq!(client.get_total_wallets_scored(), 1);
}

// ── Batch submission path ─────────────────────────────────────────────────────

#[test]
fn test_total_wallets_scored_batch_new_combinations() {
    let (env, client) = setup();
    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    assert_eq!(client.get_total_wallets_scored(), 0);

    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: wallet_a.clone(),
        asset_pair: pair.clone(),
        score: 45,
        benford_flag: false,
        ml_flag: false,
        timestamp: START_TS,
        confidence: 80,
        model_version: 1,
    });
    batch.push_back(ScoreSubmission {
        wallet: wallet_b.clone(),
        asset_pair: pair.clone(),
        score: 70,
        benford_flag: false,
        ml_flag: false,
        timestamp: START_TS,
        confidence: 80,
        model_version: 1,
    });

    let result = client.submit_scores_batch(&batch);
    assert_eq!(result.accepted_count, 2);
    assert_eq!(client.get_total_wallets_scored(), 2);
}

#[test]
fn test_total_wallets_scored_batch_existing_combinations_not_double_counted() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Prime with a first submission outside of batch.
    submit(&env, &client, &wallet, &pair, 50);
    assert_eq!(client.get_total_wallets_scored(), 1);

    // Batch with a new wallet (increments) and a re-submission for the same
    // wallet that is now rate-limited (rejected → no increment).
    let fresh_wallet = Address::generate(&env);
    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: wallet.clone(),   // rate-limited: rejected
        asset_pair: pair.clone(),
        score: 60,
        benford_flag: false,
        ml_flag: false,
        timestamp: START_TS,
        confidence: 80,
        model_version: 1,
    });
    batch.push_back(ScoreSubmission {
        wallet: fresh_wallet.clone(), // new combination: accepted
        asset_pair: pair.clone(),
        score: 40,
        benford_flag: false,
        ml_flag: false,
        timestamp: START_TS,
        confidence: 80,
        model_version: 1,
    });

    let result = client.submit_scores_batch(&batch);
    assert_eq!(result.accepted_count, 1);
    assert_eq!(result.rejected_count, 1);
    // Counter: 1 (primed) + 1 (fresh_wallet) = 2.
    assert_eq!(client.get_total_wallets_scored(), 2);
}

// ── Mixed pairs and wallets ───────────────────────────────────────────────────

#[test]
fn test_total_wallets_scored_cross_pair_accuracy() {
    let (env, client) = setup();
    let w1 = Address::generate(&env);
    let w2 = Address::generate(&env);
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("BTC_USDC");

    // 4 unique combinations: (w1,a), (w1,b), (w2,a), (w2,b).
    submit(&env, &client, &w1, &pair_a, 10);
    submit(&env, &client, &w1, &pair_b, 20);
    submit(&env, &client, &w2, &pair_a, 30);
    submit(&env, &client, &w2, &pair_b, 40);

    assert_eq!(client.get_total_wallets_scored(), 4);
}
