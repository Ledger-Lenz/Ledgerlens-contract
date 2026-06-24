//! Integration tests for the failover protocol.
//!
//! Tests register two `LedgerLensScoreContract` instances in the same Soroban
//! `Env` and verify that the primary transparently delegates to the secondary
//! when paused, subject to the `FAILOVER_STALENESS_WINDOW` guard.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{
    constants::FAILOVER_STALENESS_WINDOW, LedgerLensScoreContract, LedgerLensScoreContractClient,
};

const START_TS: u64 = 1_700_000_000;

/// Set up two independent contract instances in the same `Env`.
/// Returns `(env, primary_client, secondary_client, admin, service)`.
fn setup_two<'a>() -> (
    Env,
    LedgerLensScoreContractClient<'a>,
    LedgerLensScoreContractClient<'a>,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);

    let primary_id = env.register_contract(None, LedgerLensScoreContract);
    let secondary_id = env.register_contract(None, LedgerLensScoreContract);

    let primary = LedgerLensScoreContractClient::new(&env, &primary_id);
    let secondary = LedgerLensScoreContractClient::new(&env, &secondary_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);

    primary.initialize(&admin, &service);
    secondary.initialize(&admin, &service);

    (env, primary, secondary, admin, service)
}

// ── Registration ──────────────────────────────────────────────────────────────

#[test]
fn test_set_and_get_failover_contract() {
    let (env, primary, secondary, _admin, _service) = setup_two();

    assert!(primary.get_failover_contract().is_none());

    primary.set_failover_contract(&Vec::new(&env), &secondary.address);
    assert_eq!(primary.get_failover_contract(), Some(secondary.address.clone()));
}

// ── Normal (unpaused) operation — no failover ─────────────────────────────────

#[test]
fn test_gate_uses_primary_when_not_paused() {
    let (env, primary, secondary, _admin, _service) = setup_two();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Primary has a passing score; secondary has none.
    primary.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &30,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );

    primary.set_failover_contract(&Vec::new(&env), &secondary.address.clone());

    // Gate should pass (score 30 < threshold 75) using primary's score.
    assert!(primary.query_risk_gate(&wallet, &pair, &75));
}

// ── Failover triggered when primary is paused ─────────────────────────────────

#[test]
fn test_gate_falls_back_to_secondary_when_paused() {
    let (env, primary, secondary, _admin, _service) = setup_two();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Secondary has a fresh, passing score.
    secondary.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &20,
        &false,
        &false,
        &START_TS,
        &85,
        &1,
        &None,
    );

    primary.set_failover_contract(&Vec::new(&env), &secondary.address.clone());

    // Pause the primary.
    primary.pause(&Vec::new(&env));
    assert!(primary.is_paused());

    // Gate should pass via secondary (score 20 < threshold 75).
    assert!(primary.query_risk_gate(&wallet, &pair, &75));
}

#[test]
fn test_gate_returns_false_when_paused_with_no_secondary() {
    let (env, primary, _secondary, _admin, _service) = setup_two();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    primary.pause(&Vec::new(&env));

    // No secondary registered — must fail closed.
    assert!(!primary.query_risk_gate(&wallet, &pair, &75));
}

#[test]
fn test_gate_returns_false_when_secondary_has_no_score() {
    let (env, primary, secondary, _admin, _service) = setup_two();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Secondary has no score for this wallet.
    primary.set_failover_contract(&Vec::new(&env), &secondary.address.clone());
    primary.pause(&Vec::new(&env));

    // No score on secondary → fail closed.
    assert!(!primary.query_risk_gate(&wallet, &pair, &75));
}

// ── Staleness guard ───────────────────────────────────────────────────────────

#[test]
fn test_stale_secondary_score_fails_closed() {
    let (env, primary, secondary, _admin, _service) = setup_two();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Secondary score timestamp is exactly at START_TS.
    secondary.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &10,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );

    primary.set_failover_contract(&Vec::new(&env), &secondary.address.clone());
    primary.pause(&Vec::new(&env));

    // Advance time beyond the staleness window.
    env.ledger().with_mut(|l| l.timestamp = START_TS + FAILOVER_STALENESS_WINDOW + 1);

    // Score is stale — fail closed even though score is low.
    assert!(!primary.query_risk_gate(&wallet, &pair, &75));
}

#[test]
fn test_fresh_secondary_score_passes() {
    let (env, primary, secondary, _admin, _service) = setup_two();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    secondary.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &10,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );

    primary.set_failover_contract(&Vec::new(&env), &secondary.address.clone());
    primary.pause(&Vec::new(&env));

    // Advance time within the staleness window.
    env.ledger()
        .with_mut(|l| l.timestamp = START_TS + FAILOVER_STALENESS_WINDOW);

    // Score is fresh — gate passes.
    assert!(primary.query_risk_gate(&wallet, &pair, &75));
}

// ── Fail-closed when secondary score too high ─────────────────────────────────

#[test]
fn test_high_secondary_score_blocks_gate() {
    let (env, primary, secondary, _admin, _service) = setup_two();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Secondary score is above the threshold.
    secondary.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &80,
        &true,
        &true,
        &START_TS,
        &90,
        &1,
        &None,
    );

    primary.set_failover_contract(&Vec::new(&env), &secondary.address.clone());
    primary.pause(&Vec::new(&env));

    // Score 80 >= threshold 75 → gate returns false.
    assert!(!primary.query_risk_gate(&wallet, &pair, &75));
}

// ── Confidence floor applied during failover ──────────────────────────────────

#[test]
fn test_failover_confidence_floor_applied() {
    let (env, primary, secondary, _admin, _service) = setup_two();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Low-confidence score on secondary.
    secondary.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &10,
        &false,
        &false,
        &START_TS,
        &40, // confidence = 40
        &1,
        &None,
    );

    primary.set_failover_contract(&Vec::new(&env), &secondary.address.clone());
    primary.pause(&Vec::new(&env));

    // Require confidence >= 60 — secondary confidence is 40, so gate fails.
    assert!(!primary.query_risk_gate_with_confidence(&wallet, &pair, &75, &60));

    // Require confidence >= 30 — secondary confidence is 40, so gate passes.
    assert!(primary.query_risk_gate_with_confidence(&wallet, &pair, &75, &30));
}

// ── get_score_opt ─────────────────────────────────────────────────────────────

#[test]
fn test_get_score_opt_returns_none_when_missing() {
    let (env, primary, _secondary, _admin, _service) = setup_two();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    assert!(primary.get_score_opt(&wallet, &pair).is_none());
}

#[test]
fn test_get_score_opt_returns_score_when_present() {
    let (env, primary, _secondary, _admin, _service) = setup_two();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    primary.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &55,
        &false,
        &false,
        &START_TS,
        &80,
        &1,
        &None,
    );

    let result = primary.get_score_opt(&wallet, &pair);
    assert!(result.is_some());
    assert_eq!(result.unwrap().score, 55);
}
