//! Tests for the escrow hold window: submitted scores are held in escrow and
//! auto-committed after the window elapses; admin can cancel during the window.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{
    constants::MAX_ESCROW_HOLD_WINDOW_SECS, Error, LedgerLensScoreContract,
    LedgerLensScoreContractClient,
};

const START_TS: u64 = 1_700_000_000;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    (env, client, admin, service)
}

fn submit(
    env: &Env,
    client: &LedgerLensScoreContractClient,
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
        &env.ledger().timestamp(),
        &80u32,
        &1u32,
        &None,
    );
}

fn advance_to(env: &Env, ts: u64) {
    env.ledger().with_mut(|l| l.timestamp = ts);
}

// ── Configuration ─────────────────────────────────────────────────────────────

#[test]
fn test_escrow_hold_window_default_zero() {
    let (_env, client, _admin, _service) = setup();
    assert_eq!(client.get_escrow_hold_window(), 0);
}

#[test]
fn test_set_and_get_escrow_hold_window() {
    let (env, client, _admin, _service) = setup();
    client.set_escrow_hold_window(&Vec::new(&env), &600u64);
    assert_eq!(client.get_escrow_hold_window(), 600);
}

#[test]
fn test_set_escrow_hold_window_above_max_rejected() {
    let (env, client, _admin, _service) = setup();
    let result =
        client.try_set_escrow_hold_window(&Vec::new(&env), &(MAX_ESCROW_HOLD_WINDOW_SECS + 1));
    assert_eq!(result, Err(Ok(Error::InvalidFinalityBuffer)));
}

#[test]
fn test_set_escrow_hold_window_zero_disables() {
    let (env, client, _admin, _service) = setup();
    client.set_escrow_hold_window(&Vec::new(&env), &300u64);
    client.set_escrow_hold_window(&Vec::new(&env), &0u64);
    assert_eq!(client.get_escrow_hold_window(), 0);
}

// ── Escrow path active ────────────────────────────────────────────────────────

#[test]
fn test_submit_with_escrow_writes_escrow_not_live() {
    let (env, client, _admin, _service) = setup();
    client.set_escrow_hold_window(&Vec::new(&env), &300u64);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 75);

    // Score not yet visible on live path.
    assert!(client.try_get_score(&wallet, &pair).is_err());
    assert!(!client.query_risk_gate(&wallet, &pair, &100u32));
    assert_eq!(client.get_score_count(&wallet, &pair), 0);

    // But the escrow entry exists.
    let entry = client.get_escrow_score(&wallet, &pair).unwrap();
    assert_eq!(entry.score, 75);
    assert_eq!(entry.submitted_at, START_TS);
    assert_eq!(entry.commit_after, START_TS + 300);
}

// ── auto_commit_score eligibility ─────────────────────────────────────────────

#[test]
fn test_auto_commit_before_window_fails() {
    let (env, client, _admin, _service) = setup();
    client.set_escrow_hold_window(&Vec::new(&env), &300u64);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 60);

    advance_to(&env, START_TS + 299);
    let result = client.try_auto_commit_score(&wallet, &pair);
    assert_eq!(result, Err(Ok(Error::FinalityWindowNotElapsed)));
    assert!(client.try_get_score(&wallet, &pair).is_err());
}

#[test]
fn test_auto_commit_at_exact_window_succeeds() {
    let (env, client, _admin, _service) = setup();
    client.set_escrow_hold_window(&Vec::new(&env), &300u64);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 60);

    advance_to(&env, START_TS + 300);
    client.auto_commit_score(&wallet, &pair);

    let score = client.get_score(&wallet, &pair);
    assert_eq!(score.score, 60);
    assert!(client.get_escrow_score(&wallet, &pair).is_none());
    assert_eq!(client.get_score_count(&wallet, &pair), 1);
    assert_eq!(client.get_score_history(&wallet, &pair).len(), 1);
}

#[test]
fn test_auto_commit_after_window_also_succeeds() {
    let (env, client, _admin, _service) = setup();
    client.set_escrow_hold_window(&Vec::new(&env), &300u64);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 80);

    advance_to(&env, START_TS + 1_000);
    client.auto_commit_score(&wallet, &pair);
    assert_eq!(client.get_score(&wallet, &pair).score, 80);
}

#[test]
fn test_auto_commit_is_permissionless() {
    let (env, client, _admin, _service) = setup();
    client.set_escrow_hold_window(&Vec::new(&env), &60u64);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 30);

    advance_to(&env, START_TS + 60);
    // Any address (not just admin/service) can trigger the commit.
    let _anyone = Address::generate(&env);
    client.auto_commit_score(&wallet, &pair);
    assert_eq!(client.get_score(&wallet, &pair).score, 30);
}

#[test]
fn test_auto_commit_no_escrow_fails() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let result = client.try_auto_commit_score(&wallet, &pair);
    assert_eq!(result, Err(Ok(Error::NoPendingScore)));
}

// ── Admin cancel ──────────────────────────────────────────────────────────────

#[test]
fn test_admin_cancel_during_window_removes_escrow() {
    let (env, client, _admin, _service) = setup();
    client.set_escrow_hold_window(&Vec::new(&env), &300u64);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 90);

    advance_to(&env, START_TS + 100); // inside window
    client.cancel_escrow_score(&Vec::new(&env), &wallet, &pair);

    assert!(client.get_escrow_score(&wallet, &pair).is_none());

    // Even after window elapses, there is nothing to commit.
    advance_to(&env, START_TS + 300);
    let result = client.try_auto_commit_score(&wallet, &pair);
    assert_eq!(result, Err(Ok(Error::NoPendingScore)));
    assert!(client.try_get_score(&wallet, &pair).is_err());
}

#[test]
fn test_admin_cancel_no_escrow_fails() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let result = client.try_cancel_escrow_score(&Vec::new(&env), &wallet, &pair);
    assert_eq!(result, Err(Ok(Error::NoPendingScore)));
}

// ── Re-submission during escrow window ───────────────────────────────────────

#[test]
fn test_resubmission_after_cooldown_replaces_escrow() {
    let (env, client, _admin, _service) = setup();
    client.set_escrow_hold_window(&Vec::new(&env), &300u64);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 40);

    // Advance past cooldown (default 1 hour) but still within the escrow window.
    advance_to(&env, START_TS + 3_700);
    submit(&env, &client, &wallet, &pair, 85);

    let entry = client.get_escrow_score(&wallet, &pair).unwrap();
    assert_eq!(entry.score, 85, "second submission should replace the first in escrow");
    assert_eq!(entry.submitted_at, START_TS + 3_700);
    assert_eq!(entry.commit_after, START_TS + 3_700 + 300);
}

#[test]
fn test_resubmission_before_cooldown_rate_limited() {
    let (env, client, _admin, _service) = setup();
    client.set_escrow_hold_window(&Vec::new(&env), &300u64);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 40);

    // Try resubmitting immediately (well within the 1-hour cooldown).
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &70u32,
        &false,
        &false,
        &env.ledger().timestamp(),
        &80u32,
        &1u32,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::RateLimitExceeded)));
}

// ── Escrow disabled — direct commit path ─────────────────────────────────────

#[test]
fn test_escrow_disabled_commits_immediately() {
    let (env, client, _admin, _service) = setup();
    // escrow_hold_window defaults to 0 (disabled)
    assert_eq!(client.get_escrow_hold_window(), 0);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 55);

    // With escrow disabled, score should be live immediately.
    assert_eq!(client.get_score(&wallet, &pair).score, 55);
    assert!(client.get_escrow_score(&wallet, &pair).is_none());
}
