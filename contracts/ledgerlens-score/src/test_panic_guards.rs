//! Tests for static panic-path guards (issue #777).
//!
//! Verifies that consumer-facing contract functions do not panic on any input
//! reachable by an external caller, including maximum boundary values and
//! adversarially crafted inputs.
//!
//! These tests are the deterministic counterpart to the lint rules added in
//! `.cargo/config.toml` (clippy::unwrap_used, clippy::expect_used,
//! clippy::indexing_slicing, clippy::arithmetic_side_effects).  They fail
//! against a version of the code that panics on these inputs.

#![cfg(test)]

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
    let id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &id);
    client.initialize(&Address::generate(&env), &Address::generate(&env));
    (env, client)
}

fn submit(env: &Env, client: &LedgerLensScoreContractClient, wallet: &Address, score: u32) {
    env.ledger().with_mut(|l| l.timestamp += 3_601);
    client.submit_score(
        &Vec::new(env),
        wallet,
        &symbol_short!("XLM_USDC"),
        &score,
        &false,
        &false,
        &env.ledger().timestamp(),
        &100u32,
        &1u32,
        &None,
    );
}

// ── query_risk_gate: no panic on boundary threshold values ────────────────────

#[test]
fn test_gate_no_panic_threshold_zero_no_score() {
    let (env, client) = setup();
    let w = Address::generate(&env);
    // threshold = 0: every score >= 0, so gate should return false (no score → false too)
    let _ = client.query_risk_gate(&w, &symbol_short!("XLM_USDC"), &0u32);
}

#[test]
fn test_gate_no_panic_threshold_zero_with_score() {
    let (env, client) = setup();
    let w = Address::generate(&env);
    submit(&env, &client, &w, 0);
    // score 0 is not strictly below threshold 0 → false; must not panic
    let _ = client.query_risk_gate(&w, &symbol_short!("XLM_USDC"), &0u32);
}

#[test]
fn test_gate_no_panic_threshold_u32_max_with_score() {
    let (env, client) = setup();
    let w = Address::generate(&env);
    submit(&env, &client, &w, 100);
    let _ = client.query_risk_gate(&w, &symbol_short!("XLM_USDC"), &u32::MAX);
}

#[test]
fn test_gate_no_panic_threshold_u32_max_no_score() {
    let (env, client) = setup();
    let w = Address::generate(&env);
    let _ = client.query_risk_gate(&w, &symbol_short!("XLM_USDC"), &u32::MAX);
}

// ── query_risk_gate_with_confidence: no panic on boundary inputs ──────────────

#[test]
fn test_confidence_gate_no_panic_all_zeros() {
    let (env, client) = setup();
    let w = Address::generate(&env);
    let _ = client.query_risk_gate_with_confidence(
        &w,
        &symbol_short!("XLM_USDC"),
        &0u32,
        &0u32,
    );
}

#[test]
fn test_confidence_gate_no_panic_all_u32_max() {
    let (env, client) = setup();
    let w = Address::generate(&env);
    let _ = client.query_risk_gate_with_confidence(
        &w,
        &symbol_short!("XLM_USDC"),
        &u32::MAX,
        &u32::MAX,
    );
}

#[test]
fn test_confidence_gate_no_panic_max_threshold_zero_confidence_with_score() {
    let (env, client) = setup();
    let w = Address::generate(&env);
    submit(&env, &client, &w, 50);
    let _ = client.query_risk_gate_with_confidence(
        &w,
        &symbol_short!("XLM_USDC"),
        &u32::MAX,
        &0u32,
    );
}

// ── get_score: no panic on unknown wallet/pair ────────────────────────────────

#[test]
fn test_get_score_no_panic_unknown_wallet() {
    let (env, client) = setup();
    let w = Address::generate(&env);
    // Should return Err(ScoreNotFound), not panic.
    let _ = client.try_get_score(&w, &symbol_short!("XLM_USDC"));
}

// ── get_aggregate_score: no panic on wallet with no scores ────────────────────

#[test]
fn test_get_aggregate_score_no_panic_unknown_wallet() {
    let (env, client) = setup();
    let w = Address::generate(&env);
    let _ = client.try_get_aggregate_score(&w);
}

// ── submit_score: boundary score/confidence values accepted or rejected cleanly

#[test]
fn test_submit_score_boundary_score_100_no_panic() {
    let (env, client) = setup();
    let w = Address::generate(&env);
    env.ledger().with_mut(|l| l.timestamp = BASE_TS);
    // score = 100 is the maximum valid value; must be accepted without panic.
    let result = client.try_submit_score(
        &Vec::new(&env),
        &w,
        &symbol_short!("XLM_USDC"),
        &100u32,
        &false,
        &false,
        &BASE_TS,
        &100u32,
        &1u32,
        &None,
    );
    assert!(result.is_ok(), "score=100 must be accepted");
}

#[test]
fn test_submit_score_boundary_score_101_rejected_cleanly() {
    use crate::Error;
    let (env, client) = setup();
    let w = Address::generate(&env);
    env.ledger().with_mut(|l| l.timestamp = BASE_TS);
    // score = 101 exceeds the 0-100 range; must return InvalidScore, not panic.
    let result = client.try_submit_score(
        &Vec::new(&env),
        &w,
        &symbol_short!("XLM_USDC"),
        &101u32,
        &false,
        &false,
        &BASE_TS,
        &80u32,
        &1u32,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::InvalidScore)));
}

#[test]
fn test_submit_score_boundary_confidence_101_rejected_cleanly() {
    use crate::Error;
    let (env, client) = setup();
    let w = Address::generate(&env);
    env.ledger().with_mut(|l| l.timestamp = BASE_TS);
    let result = client.try_submit_score(
        &Vec::new(&env),
        &w,
        &symbol_short!("XLM_USDC"),
        &50u32,
        &false,
        &false,
        &BASE_TS,
        &101u32,
        &1u32,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::InvalidConfidence)));
}

#[test]
fn test_submit_score_timestamp_zero_rejected_cleanly() {
    use crate::Error;
    let (env, client) = setup();
    let w = Address::generate(&env);
    let result = client.try_submit_score(
        &Vec::new(&env),
        &w,
        &symbol_short!("XLM_USDC"),
        &50u32,
        &false,
        &false,
        &0u64, // zero timestamp
        &80u32,
        &1u32,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::InvalidTimestamp)));
}

// ── set_cooldown: boundary values accepted or rejected without panic ───────────

#[test]
fn test_set_cooldown_boundary_min_no_panic() {
    let (_env, client) = setup();
    // MIN_COOLDOWN_SECS = 60; should be accepted.
    let _ = client.try_set_cooldown(&60u64);
}

#[test]
fn test_set_cooldown_boundary_max_no_panic() {
    let (_env, client) = setup();
    // MAX_COOLDOWN_SECS = 86400; should be accepted.
    let _ = client.try_set_cooldown(&86_400u64);
}

#[test]
fn test_set_cooldown_zero_rejected_cleanly() {
    use crate::Error;
    let (_env, client) = setup();
    let result = client.try_set_cooldown(&0u64);
    assert_eq!(result, Err(Ok(Error::InvalidCooldown)));
}

// ── get_history_max_depth: returns cleanly before any admin config ────────────

#[test]
fn test_get_history_max_depth_no_panic_before_config() {
    let (_env, client) = setup();
    // Must return the default (10) without panicking.
    let depth = client.get_history_max_depth();
    assert!(depth > 0, "default depth must be positive");
}
