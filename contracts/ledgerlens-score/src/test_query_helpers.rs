#![cfg(test)]

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{
    LedgerLensScoreContract, LedgerLensScoreContractClient,
};

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

// ─────────────────────────────────────────────────────────────────────────────
// Issue #100 – get_score_age
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_get_score_age_no_score_returns_zero() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // No score has ever been submitted → age is 0.
    assert_eq!(client.get_score_age(&wallet, &pair), 0);
}

#[test]
fn test_get_score_age_just_submitted() {
    // Ledger timestamp is 1_000_000 at submission, no time advances → age = 0.
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &40,
        &false,
        &false,
        &1_700_000_000,
        &90,
        &1,
        &None,
    );

    // Time has not advanced → age should be 0.
    assert_eq!(client.get_score_age(&wallet, &pair), 0);
}

#[test]
fn test_get_score_age_after_time_advance() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &40,
        &false,
        &false,
        &1_700_000_000,
        &90,
        &1,
        &None,
    );

    // Advance ledger by 7200 seconds (2 hours).
    env.ledger().with_mut(|l| l.timestamp = 1_007_200);

    assert_eq!(client.get_score_age(&wallet, &pair), 7200);
}

#[test]
fn test_get_score_age_unknown_pair_returns_zero() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let scored_pair = symbol_short!("XLM_USDC");
    let other_pair = symbol_short!("BTC_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // Submit for scored_pair only.
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &scored_pair,
        &40,
        &false,
        &false,
        &1_700_000_000,
        &90,
        &1,
        &None,
    );

    // other_pair was never scored → age is 0.
    assert_eq!(client.get_score_age(&wallet, &other_pair), 0);
}

// ─────────────────────────────────────────────────────────────────────────────
// Issue #244 – get_trend_state
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_get_trend_state_unset_returns_none() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // No submission has ever recorded a trend → None.
    assert_eq!(client.get_trend_state(&wallet, &pair), None);
}

#[test]
fn test_get_trend_state_set_returns_some() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // First submission persists the trend state (flat on the first point).
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &40,
        &false,
        &false,
        &1_700_000_000,
        &90,
        &1,
        &None,
    );

    let trend = client
        .get_trend_state(&wallet, &pair)
        .expect("trend should be recorded after a submission");
    assert_eq!(trend.trend, 0);
    assert_eq!(trend.consecutive, 0);
}
