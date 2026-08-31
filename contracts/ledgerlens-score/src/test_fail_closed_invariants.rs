//! Invariant tests for fail-closed behavior across all consumer-facing reads
//! (issue #776).
//!
//! Every consumer-facing read that may be called mid-transaction by an
//! external protocol MUST fail closed: when data is unavailable, stale,
//! malformed, or below a confidence floor the gate returns `false` (deny),
//! never `true` (allow).
//!
//! Tests here:
//!   1. `query_risk_gate` returns `false` when no score exists.
//!   2. `query_risk_gate` returns `false` when score >= threshold.
//!   3. `query_risk_gate` returns `true` only when score < threshold.
//!   4. `query_risk_gate_with_confidence` returns `false` when confidence < floor.
//!   5. `query_risk_gate_with_confidence` returns `false` when no score exists.
//!   6. `query_risk_gate_with_confidence` returns `false` when score >= threshold
//!      regardless of confidence.
//!   7. `query_risk_gate` / `_with_confidence` never panic for u32::MAX inputs.
//!   8. Global min-confidence floor is applied when caller's param is lower.
//!   9. `get_score` returns `ScoreNotFound` for unknown wallet/pair.
//!  10. `get_aggregate_score` returns `ScoreNotFound` for a wallet with no scores.
//!  11. A score below global confidence floor is treated as "no data" by the gate.
//!  12. Nested consumer call pattern: gate is safe to call from within a batch.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient};

const BASE_TS: u64 = 1_700_000_000;
const COOLDOWN: u64 = 3_601;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = BASE_TS);
    let id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &id);
    client.initialize(&Address::generate(&env), &Address::generate(&env));
    (env, client)
}

fn submit(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    wallet: &Address,
    score: u32,
    confidence: u32,
) {
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
    client.submit_score(
        &Vec::new(env),
        wallet,
        &symbol_short!("XLM_USDC"),
        &score,
        &false,
        &false,
        &env.ledger().timestamp(),
        &confidence,
        &1,
        &None,
    );
}

// ── 1. Gate returns false when no score exists ────────────────────────────────

#[test]
fn test_gate_false_when_no_score_exists() {
    let (env, client) = setup();
    let unknown = Address::generate(&env);
    let result = client.query_risk_gate(&unknown, &symbol_short!("XLM_USDC"), &75u32);
    assert!(!result, "gate must fail closed for unknown wallet");
}

// ── 2. Gate returns false when score >= threshold ─────────────────────────────

#[test]
fn test_gate_false_when_score_at_threshold() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    submit(&env, &client, &wallet, 75, 90);
    // score == threshold → not strictly below → false
    assert!(!client.query_risk_gate(&wallet, &symbol_short!("XLM_USDC"), &75u32));
}

#[test]
fn test_gate_false_when_score_above_threshold() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    submit(&env, &client, &wallet, 90, 90);
    assert!(!client.query_risk_gate(&wallet, &symbol_short!("XLM_USDC"), &75u32));
}

// ── 3. Gate returns true only when score strictly below threshold ─────────────

#[test]
fn test_gate_true_when_score_below_threshold() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    submit(&env, &client, &wallet, 74, 90);
    assert!(client.query_risk_gate(&wallet, &symbol_short!("XLM_USDC"), &75u32));
}

// ── 4. Confidence gate returns false when confidence < floor ──────────────────

#[test]
fn test_confidence_gate_false_when_confidence_below_floor() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    submit(&env, &client, &wallet, 50, 30); // low confidence
                                            // score < threshold but confidence < min_confidence → false
    let result = client.query_risk_gate_with_confidence(
        &wallet,
        &symbol_short!("XLM_USDC"),
        &75u32,
        &50u32, // min_confidence = 50, actual = 30
    );
    assert!(!result, "low confidence must fail closed");
}

// ── 5. Confidence gate returns false when no score exists ─────────────────────

#[test]
fn test_confidence_gate_false_when_no_score_exists() {
    let (env, client) = setup();
    let unknown = Address::generate(&env);
    let result =
        client.query_risk_gate_with_confidence(&unknown, &symbol_short!("XLM_USDC"), &75u32, &0u32);
    assert!(!result, "missing score must fail closed for confidence gate");
}

// ── 6. Confidence gate returns false when score >= threshold ──────────────────

#[test]
fn test_confidence_gate_false_when_score_at_or_above_threshold() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    submit(&env, &client, &wallet, 80, 95);
    assert!(!client.query_risk_gate_with_confidence(
        &wallet,
        &symbol_short!("XLM_USDC"),
        &75u32,
        &50u32,
    ));
}

// ── 7. Neither gate function panics for u32::MAX inputs ──────────────────────

#[test]
fn test_gate_no_panic_on_u32_max_threshold() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    submit(&env, &client, &wallet, 50, 80);
    // Must not panic; result value is not the concern here.
    let _ = client.query_risk_gate(&wallet, &symbol_short!("XLM_USDC"), &u32::MAX);
    let _ = client.query_risk_gate_with_confidence(
        &wallet,
        &symbol_short!("XLM_USDC"),
        &u32::MAX,
        &u32::MAX,
    );
}

#[test]
fn test_gate_no_panic_on_u32_max_no_score() {
    let (env, client) = setup();
    let unknown = Address::generate(&env);
    let _ = client.query_risk_gate(&unknown, &symbol_short!("XLM_USDC"), &u32::MAX);
    let _ = client.query_risk_gate_with_confidence(
        &unknown,
        &symbol_short!("XLM_USDC"),
        &u32::MAX,
        &u32::MAX,
    );
}

// ── 8. Global min-confidence floor is applied when caller param is lower ──────

#[test]
fn test_global_confidence_floor_overrides_caller_param() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    // Score low enough to pass the threshold, confidence = 40.
    submit(&env, &client, &wallet, 50, 40);

    // Set global floor to 60 — higher than the submitted confidence.
    client.set_global_min_confidence(&60u32);

    // Caller passes min_confidence=0 but global floor (60) applies.
    let result =
        client.query_risk_gate_with_confidence(&wallet, &symbol_short!("XLM_USDC"), &75u32, &0u32);
    assert!(!result, "global confidence floor must override caller's zero floor");
}

// ── 9. get_score returns ScoreNotFound for unknown wallet/pair ────────────────

#[test]
fn test_get_score_returns_not_found_for_unknown() {
    let (env, client) = setup();
    let unknown = Address::generate(&env);
    let result = client.try_get_score(&unknown, &symbol_short!("XLM_USDC"));
    assert_eq!(result, Err(Ok(Error::ScoreNotFound)));
}

// ── 10. get_aggregate_score returns ScoreNotFound for wallet with no scores ───

#[test]
fn test_get_aggregate_score_returns_not_found_for_no_scores() {
    let (env, client) = setup();
    let unknown = Address::generate(&env);
    let result = client.try_get_aggregate_score(&unknown);
    assert_eq!(result, Err(Ok(Error::ScoreNotFound)));
}

// ── 11. Score below global confidence floor treated as "no data" ──────────────

#[test]
fn test_score_below_global_floor_treated_as_no_data() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    // Submit with confidence 20.
    submit(&env, &client, &wallet, 50, 20);
    // Global floor set to 50 — the submitted confidence (20) is below.
    client.set_global_min_confidence(&50u32);

    // Gate must return false (treated same as no data).
    assert!(
        !client
            .query_risk_gate_with_confidence(&wallet, &symbol_short!("XLM_USDC"), &75u32, &0u32,),
        "score with confidence below global floor must fail closed"
    );
}

// ── 12. Gate is safe to call sequentially for multiple wallets ────────────────

#[test]
fn test_gate_safe_for_multiple_wallet_calls() {
    let (env, client) = setup();
    let safe_wallet = Address::generate(&env);
    let risky_wallet = Address::generate(&env);

    submit(&env, &client, &safe_wallet, 30, 90);
    submit(&env, &client, &risky_wallet, 85, 90);

    // safe wallet passes
    assert!(client.query_risk_gate(&safe_wallet, &symbol_short!("XLM_USDC"), &75u32));
    // risky wallet fails closed
    assert!(!client.query_risk_gate(&risky_wallet, &symbol_short!("XLM_USDC"), &75u32));
    // unknown wallet fails closed
    let unknown = Address::generate(&env);
    assert!(!client.query_risk_gate(&unknown, &symbol_short!("XLM_USDC"), &75u32));
}
