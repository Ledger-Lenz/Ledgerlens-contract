#![cfg(test)]
//! Dedicated tests for `is_score_stale(wallet, asset_pair) -> bool`.
//!
//! Exercises the three acceptance-criteria cases: missing score, fresh score,
//! and stale score.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient};

const BASE_TS: u64 = 1_700_000_000;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = BASE_TS);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    (env, client)
}

// ── Missing score ─────────────────────────────────────────────────────────────

#[test]
fn test_is_score_stale_returns_true_when_no_score_exists() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    // No submission has ever been made — must report stale.
    assert!(client.is_score_stale(&wallet, &pair));
}

// ── Fresh score ───────────────────────────────────────────────────────────────

#[test]
fn test_is_score_stale_returns_false_for_fresh_score() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = BASE_TS);
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &BASE_TS,
        &80,
        &1,
        &None,
    );

    // Ledger timestamp equals the score timestamp — age is 0, not stale.
    assert!(!client.is_score_stale(&wallet, &pair));
}

// ── Stale score ───────────────────────────────────────────────────────────────

#[test]
fn test_is_score_stale_returns_true_when_score_older_than_window() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = BASE_TS);
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &BASE_TS,
        &80,
        &1,
        &None,
    );

    let window = client.get_staleness_window();
    // Advance the ledger one second past the staleness window.
    env.ledger().with_mut(|l| l.timestamp = BASE_TS + window + 1);
    assert!(client.is_score_stale(&wallet, &pair));
}

// ── Boundary condition ────────────────────────────────────────────────────────

#[test]
fn test_is_score_stale_returns_false_exactly_at_window_boundary() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = BASE_TS);
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &BASE_TS,
        &80,
        &1,
        &None,
    );

    let window = client.get_staleness_window();
    // age == window exactly: the condition is age > window, so still fresh.
    env.ledger().with_mut(|l| l.timestamp = BASE_TS + window);
    assert!(!client.is_score_stale(&wallet, &pair));
}
