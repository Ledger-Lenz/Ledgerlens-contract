//! Tests for `get_rate_limit_window() -> u64`.
//!
//! `get_rate_limit_window` returns the configured rate-limit window in seconds —
//! the same underlying value as `get_cooldown`, exposed under an integrator-
//! friendly name.  Both functions must always agree.

use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

use crate::{
    constants::{DEFAULT_COOLDOWN_SECS, MAX_COOLDOWN_SECS, MIN_COOLDOWN_SECS},
    LedgerLensScoreContract, LedgerLensScoreContractClient,
};

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    (env, client)
}

// ── Default value ─────────────────────────────────────────────────────────────

#[test]
fn test_rate_limit_window_default() {
    let (_env, client) = setup();
    assert_eq!(client.get_rate_limit_window(), DEFAULT_COOLDOWN_SECS);
    assert_eq!(client.get_rate_limit_window(), 3_600);
}

// ── Agrees with get_cooldown ──────────────────────────────────────────────────

#[test]
fn test_rate_limit_window_matches_cooldown() {
    let (_env, client) = setup();
    assert_eq!(client.get_rate_limit_window(), client.get_cooldown());
}

#[test]
fn test_rate_limit_window_reflects_set_cooldown() {
    let (env, client) = setup();

    client.set_cooldown(&Vec::new(&env), &MIN_COOLDOWN_SECS);
    assert_eq!(client.get_rate_limit_window(), MIN_COOLDOWN_SECS);
    assert_eq!(client.get_rate_limit_window(), client.get_cooldown());

    client.set_cooldown(&Vec::new(&env), &MAX_COOLDOWN_SECS);
    assert_eq!(client.get_rate_limit_window(), MAX_COOLDOWN_SECS);
    assert_eq!(client.get_rate_limit_window(), client.get_cooldown());
}

// ── Boundary values ───────────────────────────────────────────────────────────

#[test]
fn test_rate_limit_window_min_bound() {
    let (env, client) = setup();
    client.set_cooldown(&Vec::new(&env), &MIN_COOLDOWN_SECS);
    assert_eq!(client.get_rate_limit_window(), MIN_COOLDOWN_SECS);
}

#[test]
fn test_rate_limit_window_max_bound() {
    let (env, client) = setup();
    client.set_cooldown(&Vec::new(&env), &MAX_COOLDOWN_SECS);
    assert_eq!(client.get_rate_limit_window(), MAX_COOLDOWN_SECS);
}
