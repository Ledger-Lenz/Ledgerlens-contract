//! Compatibility tests that pin the behaviour of the `ILedgerLensScore`
//! composability surface across interface versions.
//!
//! # Purpose
//!
//! Unlike `test.rs` (which exercises implementation correctness) and
//! `test_interface.rs` (which verifies gate semantics), **this file pins
//! deprecated-but-still-live interface compatibility**. A failing test here
//! means a symbol that was promised to remain callable during its deprecation
//! window was broken before its removal date — that is a regression, not a
//! known breakage.
//!
//! # How to add a test
//!
//! When you deprecate a symbol:
//! 1. Add a `#[test]` here that calls the deprecated function and asserts the
//!    documented return value.
//! 2. Tag the test with a comment:
//!    `// DEPRECATED_COMPAT: pinned for interface vN compatibility`
//! 3. Do **not** delete the test until the sunset checklist in
//!    `docs/deprecation-policy.md §6` is complete.
//!
//! # Current deprecation window
//!
//! Interface version 3 is the current stable version. Functions deprecated
//! in v3 may not be removed before v5.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient};

fn setup(env: &Env) -> LedgerLensScoreContractClient<'_> {
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_700_000_000);
    let id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(env, &id);
    let admin = Address::generate(env);
    let service = Address::generate(env);
    client.initialize(&admin, &service);
    client
}

fn submit_score(env: &Env, client: &LedgerLensScoreContractClient, wallet: &Address, score: u32) {
    client.submit_score(
        &Vec::new(env),
        wallet,
        &symbol_short!("XLM_USDC"),
        &score,
        &false,
        &false,
        &env.ledger().timestamp(),
        &90,
        &1,
        &None,
    );
}

// ── Capability registry stability ─────────────────────────────────────────────

/// `supports_interface` must return `true` for every currently-live capability
/// symbol.  This test fails if a capability is silently removed without going
/// through the sunset checklist.
///
/// Current live capabilities (interface v3):
/// score, gate, history, batch, aggr, count, cgate, pr_rd
// DEPRECATED_COMPAT: pinned for interface v3 compatibility
#[test]
fn all_v3_capabilities_are_registered() {
    let env = Env::default();
    let client = setup(&env);

    let caps = ["score", "gate", "history", "batch", "aggr", "count", "cgate", "pr_rd"];
    for cap_str in caps {
        let cap = Symbol::new(&env, cap_str);
        assert!(
            client.supports_interface(&cap),
            "capability '{}' must be registered in interface v3",
            cap_str
        );
    }
}

/// An unknown capability symbol returns `false` — never panics.
// DEPRECATED_COMPAT: pinned for interface v3 compatibility
#[test]
fn unknown_capability_returns_false_without_panic() {
    let env = Env::default();
    let client = setup(&env);

    let unknown = Symbol::new(&env, "unknown99");
    assert!(!client.supports_interface(&unknown));
}

// ── query_risk_gate boundary semantics (pinned, must not regress) ─────────────

/// `gate_threshold = 0` means no score can pass (score must be strictly below
/// 0, which is impossible for a u32).  Returns `false` for any wallet.
// DEPRECATED_COMPAT: pinned for interface v3 compatibility
#[test]
fn gate_threshold_zero_always_returns_false() {
    let env = Env::default();
    let client = setup(&env);
    let wallet = Address::generate(&env);
    submit_score(&env, &client, &wallet, 0); // Even score=0 cannot pass threshold=0

    assert!(!client.query_risk_gate(&wallet, &symbol_short!("XLM_USDC"), &0));
}

/// An unscored wallet returns `false` (fail-closed) for any threshold,
/// including `u32::MAX`.
// DEPRECATED_COMPAT: pinned for interface v3 compatibility
#[test]
fn gate_fails_closed_for_unscored_wallet_regardless_of_threshold() {
    let env = Env::default();
    let client = setup(&env);
    let unscored = Address::generate(&env);

    assert!(!client.query_risk_gate(&unscored, &symbol_short!("XLM_USDC"), &u32::MAX));
    assert!(!client.query_risk_gate(&unscored, &symbol_short!("XLM_USDC"), &50));
    assert!(!client.query_risk_gate(&unscored, &symbol_short!("XLM_USDC"), &0));
}

// ── Error discriminant stability ──────────────────────────────────────────────

/// `get_score` returns `ScoreNotFound` (not a panic) for an unscored wallet.
/// This error code must not change — integrators that match on it numerically
/// would break.
// DEPRECATED_COMPAT: pinned for interface v3 compatibility
#[test]
fn get_score_returns_score_not_found_for_unscored_wallet() {
    let env = Env::default();
    let client = setup(&env);
    let unknown = Address::generate(&env);

    let result = client.try_get_score(&unknown, &symbol_short!("XLM_USDC"));
    assert_eq!(result, Err(Ok(Error::ScoreNotFound)));
}

/// `get_aggregate_score` returns `ScoreNotFound` for a wallet with no scored
/// pairs — same error as `get_score`.
// DEPRECATED_COMPAT: pinned for interface v3 compatibility
#[test]
fn get_aggregate_score_returns_score_not_found_with_no_pairs() {
    let env = Env::default();
    let client = setup(&env);
    let unknown = Address::generate(&env);

    let result = client.try_get_aggregate_score(&unknown);
    assert_eq!(result, Err(Ok(Error::ScoreNotFound)));
}

/// `get_pending_upgrade` returns `NoPendingUpgrade` when no upgrade is in
/// flight — not a panic.
// DEPRECATED_COMPAT: pinned for interface v3 compatibility
#[test]
fn get_pending_upgrade_returns_no_pending_when_none_exists() {
    let env = Env::default();
    let client = setup(&env);

    let result = client.try_get_pending_upgrade();
    assert_eq!(result, Err(Ok(Error::NoPendingUpgrade)));
}

// ── Score-count baseline ──────────────────────────────────────────────────────

/// `get_score_count` returns 0 for a wallet that has never been scored.
/// This is the documented initial value and must not change.
// DEPRECATED_COMPAT: pinned for interface v3 compatibility
#[test]
fn score_count_is_zero_for_unscored_wallet() {
    let env = Env::default();
    let client = setup(&env);
    let unknown = Address::generate(&env);

    assert_eq!(client.get_score_count(&unknown, &symbol_short!("XLM_USDC")), 0);
}

/// `get_score_history` returns an empty `Vec` for a wallet with no history.
/// It must not return an error or panic.
// DEPRECATED_COMPAT: pinned for interface v3 compatibility
#[test]
fn score_history_is_empty_for_unscored_wallet() {
    let env = Env::default();
    let client = setup(&env);
    let unknown = Address::generate(&env);

    let history = client.get_score_history(&unknown, &symbol_short!("XLM_USDC"));
    assert_eq!(history.len(), 0);
}
