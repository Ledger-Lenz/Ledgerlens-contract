//! Tests for `get_cooldown_period() -> u64`.
//!
//! `get_cooldown_period` returns the currently configured score-submission
//! cooldown in seconds — the minimum time that must elapse between accepted
//! submissions for the same `(wallet, asset_pair)`.  It is a named alias of
//! `get_cooldown` targeted at off-chain scheduling services.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{
    constants::{DEFAULT_COOLDOWN_SECS, MAX_COOLDOWN_SECS, MIN_COOLDOWN_SECS},
    Error, LedgerLensScoreContract, LedgerLensScoreContractClient,
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

// ── Default value ─────────────────────────────────────────────────────────────

#[test]
fn test_cooldown_period_default() {
    let (_env, client) = setup();
    assert_eq!(client.get_cooldown_period(), DEFAULT_COOLDOWN_SECS);
    assert_eq!(client.get_cooldown_period(), 3_600);
}

// ── Agrees with get_cooldown ──────────────────────────────────────────────────

#[test]
fn test_cooldown_period_matches_get_cooldown() {
    let (_env, client) = setup();
    assert_eq!(client.get_cooldown_period(), client.get_cooldown());
}

// ── Reflects admin-configured cooldown ───────────────────────────────────────

#[test]
fn test_cooldown_period_reflects_set_cooldown_min() {
    let (env, client) = setup();
    client.set_cooldown(&Vec::new(&env), &MIN_COOLDOWN_SECS);
    assert_eq!(client.get_cooldown_period(), MIN_COOLDOWN_SECS);
    assert_eq!(client.get_cooldown_period(), client.get_cooldown());
}

#[test]
fn test_cooldown_period_reflects_set_cooldown_max() {
    let (env, client) = setup();
    client.set_cooldown(&Vec::new(&env), &MAX_COOLDOWN_SECS);
    assert_eq!(client.get_cooldown_period(), MAX_COOLDOWN_SECS);
    assert_eq!(client.get_cooldown_period(), client.get_cooldown());
}

#[test]
fn test_cooldown_period_reflects_arbitrary_value() {
    let (env, client) = setup();
    let custom = 7_200u64; // 2 hours
    client.set_cooldown(&Vec::new(&env), &custom);
    assert_eq!(client.get_cooldown_period(), custom);
}

// ── Scheduling use-case: cooldown enforced as expected ───────────────────────

#[test]
fn test_cooldown_period_matches_actual_enforcement() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Record the cooldown before submitting.
    let cooldown = client.get_cooldown_period();

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

    let next_allowed = client.get_last_submit_time(&wallet, &pair).unwrap() + cooldown;

    // One second before the window: rejected.
    env.ledger().with_mut(|l| l.timestamp = next_allowed - 1);
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

    // Exactly at the window boundary: accepted.
    env.ledger().with_mut(|l| l.timestamp = next_allowed);
    assert!(client
        .try_submit_score(
            &Vec::new(&env),
            &wallet,
            &pair,
            &60,
            &false,
            &false,
            &next_allowed,
            &90,
            &1,
            &None,
        )
        .is_ok());
}
