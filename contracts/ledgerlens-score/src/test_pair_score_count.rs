//! Tests for `get_pair_score_count(asset_pair) -> u64`.
//!
//! The counter tracks the total number of successful score submissions for an
//! asset pair across all wallets.  It is incremented on every accepted
//! `submit_score` / `submit_scores_batch` call and is never decremented.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient, ScoreSubmission};

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

// ── Initial state ─────────────────────────────────────────────────────────────

#[test]
fn test_pair_score_count_starts_at_zero() {
    let (_env, client) = setup();
    let pair = symbol_short!("XLM_USDC");
    assert_eq!(client.get_pair_score_count(&pair), 0);
}

#[test]
fn test_pair_score_count_unknown_pair_is_zero() {
    let (_env, client) = setup();
    assert_eq!(client.get_pair_score_count(&symbol_short!("BTC_USDC")), 0);
}

// ── Single submit_score increments counter ────────────────────────────────────

#[test]
fn test_pair_score_count_increments_on_submit() {
    let (env, client) = setup();
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
    assert_eq!(client.get_pair_score_count(&pair), 1);
}

#[test]
fn test_pair_score_count_increments_per_submission() {
    let (env, client) = setup();
    let pair = symbol_short!("XLM_USDC");

    for i in 0..5u32 {
        let wallet = Address::generate(&env);
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &pair,
            &(40 + i),
            &false,
            &false,
            &START_TS,
            &90,
            &1,
            &None,
        );
    }
    assert_eq!(client.get_pair_score_count(&pair), 5);
}

// ── Counter is pair-specific ──────────────────────────────────────────────────

#[test]
fn test_pair_score_count_is_pair_specific() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("BTC_USDC");

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
    // pair_a: 1 submission; pair_b: still 0.
    assert_eq!(client.get_pair_score_count(&pair_a), 1);
    assert_eq!(client.get_pair_score_count(&pair_b), 0);
}

// ── Re-submission by the same wallet after cooldown increments the counter ────

#[test]
fn test_pair_score_count_increments_on_resubmission() {
    let (env, client) = setup();
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
    assert_eq!(client.get_pair_score_count(&pair), 1);

    // Advance past the cooldown so the second submit is accepted.
    advance(&env, crate::constants::DEFAULT_COOLDOWN_SECS);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &60,
        &false,
        &false,
        &(START_TS + crate::constants::DEFAULT_COOLDOWN_SECS),
        &90,
        &1,
        &None,
    );
    assert_eq!(client.get_pair_score_count(&pair), 2);
}

// ── Rejected submissions do NOT increment the counter ────────────────────────

#[test]
fn test_pair_score_count_not_incremented_on_rate_limit() {
    let (env, client) = setup();
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
    assert_eq!(client.get_pair_score_count(&pair), 1);

    // Second submit within cooldown is rejected.
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
    // Counter must still be 1.
    assert_eq!(client.get_pair_score_count(&pair), 1);
}

// ── Batch submission path ─────────────────────────────────────────────────────

#[test]
fn test_pair_score_count_batch_submission() {
    let (env, client) = setup();
    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

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
    // Both accepted entries must each increment the counter.
    assert_eq!(client.get_pair_score_count(&pair), 2);
}

#[test]
fn test_pair_score_count_batch_partial_rejection() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // First submission sets the cooldown.
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
    assert_eq!(client.get_pair_score_count(&pair), 1);

    // Batch: first entry is rate-limited (same wallet, same pair, within cooldown).
    // Second entry is a different wallet — accepted.
    let fresh_wallet = Address::generate(&env);
    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: wallet.clone(),
        asset_pair: pair.clone(),
        score: 60,
        benford_flag: false,
        ml_flag: false,
        timestamp: START_TS,
        confidence: 80,
        model_version: 1,
    });
    batch.push_back(ScoreSubmission {
        wallet: fresh_wallet.clone(),
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

    // Only the accepted entry should have incremented the counter.
    assert_eq!(client.get_pair_score_count(&pair), 2);
}

// ── Multiple pairs simultaneously ─────────────────────────────────────────────

#[test]
fn test_pair_score_count_multiple_pairs_independent() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("BTC_USDC");
    let pair_c = symbol_short!("ETH_USDC");

    // 3 submissions to pair_a, 2 to pair_b, 1 to pair_c.
    for _ in 0..3u32 {
        let w = Address::generate(&env);
        client.submit_score(
            &Vec::new(&env),
            &w,
            &pair_a,
            &50,
            &false,
            &false,
            &START_TS,
            &90,
            &1,
            &None,
        );
    }
    for _ in 0..2u32 {
        let w = Address::generate(&env);
        client.submit_score(
            &Vec::new(&env),
            &w,
            &pair_b,
            &60,
            &false,
            &false,
            &START_TS,
            &90,
            &1,
            &None,
        );
    }
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair_c,
        &70,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );

    assert_eq!(client.get_pair_score_count(&pair_a), 3);
    assert_eq!(client.get_pair_score_count(&pair_b), 2);
    assert_eq!(client.get_pair_score_count(&pair_c), 1);
}
