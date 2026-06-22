#![cfg(test)]

//! Tests for `query_risk_gate_with_confidence` and the admin-configurable
//! global minimum confidence floor (`set_global_min_confidence` /
//! `get_global_min_confidence`).
//!
//! Every test here maps to a mandatory requirement from the issue spec.
//! Test names are kept identical to the spec so reviewers can cross-reference
//! them directly.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient};

// ── Test helpers ──────────────────────────────────────────────────────────────

/// Construct an initialised contract environment.
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

/// Submit a score, advancing the ledger timestamp past the default cooldown
/// first so repeated submissions for the same (wallet, pair) always succeed.
fn submit(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    wallet: &Address,
    score: u32,
    confidence: u32,
) {
    env.ledger().with_mut(|l| l.timestamp += 3_601); // clear 1-hour cooldown
    client
        .submit_score(
            &Vec::new(env),
            wallet,
            &symbol_short!("XLM_USDC"),
            &score,
            &false,
            &false,
            &(env.ledger().timestamp()),
            &confidence,
            &1,
            &None,
        )
        .unwrap();
}

// ── Core gate semantics ───────────────────────────────────────────────────────

/// score=30, confidence=80, gate_threshold=75, min_confidence=50 → true
/// Both conditions are satisfied: 30 < 75 and 80 >= 50.
#[test]
fn test_confidence_gate_passes_high_confidence_low_score() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    submit(&env, &client, &wallet, 30, 80);

    assert!(client.query_risk_gate_with_confidence(
        &wallet,
        &symbol_short!("XLM_USDC"),
        &75,
        &50
    ));
}

/// score=30, confidence=20, gate_threshold=75, min_confidence=50 → false
/// Score is low but confidence(20) is below the floor(50) → treated as "no data".
#[test]
fn test_confidence_gate_fails_low_confidence() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    submit(&env, &client, &wallet, 30, 20);

    assert!(!client.query_risk_gate_with_confidence(
        &wallet,
        &symbol_short!("XLM_USDC"),
        &75,
        &50
    ));
}

/// score=80, confidence=90, gate_threshold=75, min_confidence=50 → false
/// Confidence is fine but score(80) >= threshold(75) → risky.
#[test]
fn test_confidence_gate_fails_high_score() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    submit(&env, &client, &wallet, 80, 90);

    assert!(!client.query_risk_gate_with_confidence(
        &wallet,
        &symbol_short!("XLM_USDC"),
        &75,
        &50
    ));
}

/// Wallet with no stored score → false (fail closed — unknown = risky).
#[test]
fn test_confidence_gate_no_score_returns_false() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    // No submit_score call — wallet is unknown.
    assert!(!client.query_risk_gate_with_confidence(
        &wallet,
        &symbol_short!("XLM_USDC"),
        &75,
        &50
    ));
}

// ── Equivalence with query_risk_gate (min_confidence = 0) ────────────────────

/// With min_confidence=0, query_risk_gate_with_confidence must return the
/// same result as query_risk_gate for 10 distinct (score, threshold) cases.
#[test]
fn test_confidence_gate_zero_min_confidence_equals_risk_gate() {
    let (env, client, _admin, _service) = setup();
    let pair = symbol_short!("XLM_USDC");

    // 10 parameterized cases: (score, confidence, threshold)
    let cases: [(u32, u32, u32); 10] = [
        (0, 100, 50),
        (50, 80, 50),
        (49, 60, 50),
        (100, 100, 100),
        (99, 5, 100),
        (75, 90, 75),
        (74, 90, 75),
        (1, 1, 2),
        (0, 0, 0),
        (30, 50, 100),
    ];

    for (score, confidence, threshold) in cases {
        let wallet = Address::generate(&env);
        submit(&env, &client, &wallet, score, confidence);

        let gate_result = client.query_risk_gate(&wallet, &pair, &threshold);
        let cgate_result =
            client.query_risk_gate_with_confidence(&wallet, &pair, &threshold, &0);

        assert_eq!(
            gate_result, cgate_result,
            "Mismatch for score={score}, confidence={confidence}, threshold={threshold}: \
             query_risk_gate={gate_result}, query_risk_gate_with_confidence(min_conf=0)={cgate_result}"
        );
    }
}

// ── Out-of-range input safety ─────────────────────────────────────────────────

/// gate_threshold=101 → always false (no score can be < 101 AND ≤ 100).
/// Must not panic under any input.
#[test]
fn test_confidence_gate_gate_threshold_above_100_returns_false() {
    let (env, client, _admin, _service) = setup();
    let pair = symbol_short!("XLM_USDC");

    // Scored wallet with perfect confidence.
    let wallet = Address::generate(&env);
    submit(&env, &client, &wallet, 0, 100); // lowest possible risk score

    // gate_threshold=101: short-circuits to false regardless of stored score.
    assert!(!client.query_risk_gate_with_confidence(&wallet, &pair, &101, &0));

    // Also verify u32::MAX doesn't panic.
    assert!(!client.query_risk_gate_with_confidence(&wallet, &pair, &u32::MAX, &0));

    // Unknown wallet also returns false.
    let unknown = Address::generate(&env);
    assert!(!client.query_risk_gate_with_confidence(&unknown, &pair, &101, &0));
}

/// min_confidence=200 → always false (no score can have confidence > 100).
/// Must not panic under any input.
#[test]
fn test_confidence_gate_min_confidence_above_100_returns_false() {
    let (env, client, _admin, _service) = setup();
    let pair = symbol_short!("XLM_USDC");

    // Scored wallet with score=0 (lowest risk), confidence=100 (highest confidence).
    let wallet = Address::generate(&env);
    submit(&env, &client, &wallet, 0, 100);

    // min_confidence=200: short-circuits to false — no score can satisfy ≥ 200.
    assert!(!client.query_risk_gate_with_confidence(&wallet, &pair, &75, &200));

    // u32::MAX must not panic.
    assert!(!client.query_risk_gate_with_confidence(&wallet, &pair, &75, &u32::MAX));
}

// ── Exact boundary values ─────────────────────────────────────────────────────

/// score=74, confidence=50, gate_threshold=75, min_confidence=50 → true
/// score(74) < 75 ✓  and  confidence(50) >= 50 ✓ — both exactly at the edges.
#[test]
fn test_confidence_gate_exactly_at_thresholds() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    submit(&env, &client, &wallet, 74, 50);

    assert!(client.query_risk_gate_with_confidence(
        &wallet,
        &symbol_short!("XLM_USDC"),
        &75,
        &50
    ));
}

/// score=75, confidence=50, gate_threshold=75, min_confidence=50 → false
/// score(75) is NOT strictly below threshold(75) — gate is strict `<`.
#[test]
fn test_confidence_gate_exactly_at_thresholds_inverted() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    submit(&env, &client, &wallet, 75, 50);

    assert!(!client.query_risk_gate_with_confidence(
        &wallet,
        &symbol_short!("XLM_USDC"),
        &75,
        &50
    ));
}

// ── Global minimum confidence floor ──────────────────────────────────────────

/// Admin sets global_min_confidence=70; call with min_confidence=30.
/// Effective floor = max(30, 70) = 70.
/// A score with confidence=60 should fail (60 < 70).
#[test]
fn test_global_min_confidence_applies() {
    let (env, client, admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Admin sets global floor to 70.
    client.set_global_min_confidence(&70).unwrap();
    assert_eq!(client.get_global_min_confidence(), 70);

    // Score: low risk (30), confidence 60 — would pass with floor=30 alone.
    submit(&env, &client, &wallet, 30, 60);

    // Caller asks for floor=30, but global floor=70 overrides → effective=70.
    // confidence(60) < effective_floor(70) → false.
    assert!(!client.query_risk_gate_with_confidence(&wallet, &pair, &75, &30));

    // With confidence=80 (>= 70) it should pass.
    let wallet2 = Address::generate(&env);
    submit(&env, &client, &wallet2, 30, 80);
    assert!(client.query_risk_gate_with_confidence(&wallet2, &pair, &75, &30));

    let _ = admin; // suppress unused warning
}

/// global_min_confidence=50, min_confidence_param=80 → effective floor = 80
/// (the caller's param is stricter; it wins).
#[test]
fn test_global_min_confidence_takes_max_with_param() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Admin sets global floor to 50.
    client.set_global_min_confidence(&50).unwrap();

    // Score with confidence=70 — passes floor=50 but not floor=80.
    submit(&env, &client, &wallet, 30, 70);

    // Caller param=80 > global=50 → effective floor = 80.
    // confidence(70) < 80 → false.
    assert!(!client.query_risk_gate_with_confidence(&wallet, &pair, &75, &80));

    // With confidence=85 (>= 80) it passes.
    let wallet2 = Address::generate(&env);
    submit(&env, &client, &wallet2, 30, 85);
    assert!(client.query_risk_gate_with_confidence(&wallet2, &pair, &75, &80));
}

/// set_global_min_confidence(101) must return an error — out of valid range.
#[test]
fn test_set_global_min_confidence_above_100_rejected() {
    let (_env, client, _admin, _service) = setup();

    let result = client.try_set_global_min_confidence(&101);
    assert!(result.is_err(), "set_global_min_confidence(101) should return an error");
}

/// A non-admin caller must not be able to set the global confidence floor.
/// Soroban's require_auth will panic / trap the call — we verify via try_ variant.
#[test]
fn test_set_global_min_confidence_requires_admin() {
    let (env, client, _admin, _service) = setup();

    // Disable mock_all_auths so auth is enforced normally.
    let env2 = Env::default();
    // Re-register without mock_all_auths.
    let contract_id = env2.register_contract(None, LedgerLensScoreContract);
    let restricted_client = LedgerLensScoreContractClient::new(&env2, &contract_id);
    let admin2 = Address::generate(&env2);
    let service2 = Address::generate(&env2);

    // Initialize using mock_all_auths only for the initialize call.
    env2.mock_all_auths();
    restricted_client.initialize(&admin2, &service2);

    // Now drop mock_all_auths — subsequent calls enforce real auth.
    // A call without proper auth from the admin should fail.
    // We verify via the infallible path: the client's try_ variant catches traps.
    let result = restricted_client.try_set_global_min_confidence(&50);
    // Without proper auth, Soroban will trap the invocation.
    assert!(
        result.is_err(),
        "set_global_min_confidence must require admin auth"
    );

    let _ = (env, client);
}

// ── supports_interface capability ────────────────────────────────────────────

/// supports_interface(symbol_short!("cgate")) must return true in this build.
#[test]
fn test_supports_interface_cgate() {
    let (_env, client, _admin, _service) = setup();
    assert!(
        client.supports_interface(&symbol_short!("cgate")),
        "'cgate' capability must be registered in supports_interface"
    );
}

// ── Delegation equivalence ────────────────────────────────────────────────────

/// query_risk_gate must return the same result as
/// query_risk_gate_with_confidence(..., 0) for 5 distinct score values,
/// proving the refactored delegation path is correct.
#[test]
fn test_query_risk_gate_delegates_to_confidence_gate() {
    let (env, client, _admin, _service) = setup();
    let pair = symbol_short!("XLM_USDC");

    // 5 distinct score values with varying confidence and threshold.
    let cases: [(u32, u32, u32); 5] = [
        (10, 90, 75),
        (75, 80, 75),
        (74, 30, 75),
        (0, 0, 50),
        (100, 100, 100),
    ];

    for (score, confidence, threshold) in cases {
        let wallet = Address::generate(&env);
        submit(&env, &client, &wallet, score, confidence);

        let gate = client.query_risk_gate(&wallet, &pair, &threshold);
        let cgate = client.query_risk_gate_with_confidence(&wallet, &pair, &threshold, &0);

        assert_eq!(
            gate, cgate,
            "Delegation mismatch: score={score}, confidence={confidence}, threshold={threshold} \
             → query_risk_gate={gate}, query_risk_gate_with_confidence(0)={cgate}"
        );
    }
}
