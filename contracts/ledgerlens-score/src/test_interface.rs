#![cfg(test)]

//! Interface stability suite for the `ILedgerLensScore` composability surface.
//!
//! Unlike `test.rs`, which exercises the contract's *implementation* (auth,
//! pause, batching, aggregation, …), these tests pin the *interface contract*
//! that third-party protocols depend on: the infallible gate semantics, the
//! capability registry, the `RiskScore` XDR layout, and the error
//! discriminants documented in `docs/interface-spec.md`. A failure here means
//! a breaking change to a published ABI, not merely a regression.

use soroban_sdk::{
    symbol_short, testutils::Address as _, Address, Env, IntoVal, Symbol, TryFromVal, Val,
};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient, RiskScore};

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

// ── query_risk_gate semantics ─────────────────────────────────────────────────

#[test]
fn test_query_risk_gate_safe_wallet() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Score 40 is comfortably below the gate threshold of 75 → safe.
    client.submit_score(&wallet, &pair, &40, &false, &false, &1_700_000_000, &90, &1);

    assert!(client.query_risk_gate(&wallet, &pair, &75));
}

#[test]
fn test_query_risk_gate_risky_wallet() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Score 80 is above the gate threshold of 75 → not safe.
    client.submit_score(&wallet, &pair, &80, &true, &true, &1_700_000_000, &90, &1);

    assert!(!client.query_risk_gate(&wallet, &pair, &75));
}

#[test]
fn test_query_risk_gate_at_threshold() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Boundary: score == threshold is treated as NOT safe (gate is strict `<`).
    client.submit_score(&wallet, &pair, &75, &false, &false, &1_700_000_000, &90, &1);

    assert!(!client.query_risk_gate(&wallet, &pair, &75));
}

#[test]
fn test_query_risk_gate_no_score_returns_false() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Unknown wallet: conservative default is `false` (treat as risky), never
    // a panic or error.
    assert!(!client.query_risk_gate(&wallet, &pair, &75));
}

#[test]
fn test_query_risk_gate_never_panics() {
    let (env, client, _admin, _service) = setup();

    // Lift the host CPU/mem metering for this stress loop. We are fuzzing the
    // *gate's* robustness across 1000 rounds, not measuring the cost of the
    // 1000 `submit_score` writes that feed it — without this, the cumulative
    // metering of the setup writes (not the gate) would trip the budget.
    env.budget().reset_unlimited();

    // Two wallets: one with a stored score, one that stays unknown, so the
    // fuzz covers both the Some(_) and None branches of the gate.
    let scored = Address::generate(&env);
    let unknown = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let other_pair = symbol_short!("BTC_USDC");

    // A simple deterministic LCG drives the fuzz — `Math.random` is neither
    // available nor reproducible in a contract test, and a fixed seed keeps
    // CI failures debuggable.
    let mut state: u64 = 0x1234_5678_9abc_def0;
    let mut next = || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 33) as u32
    };

    for _ in 0..1000 {
        // Refresh the scored wallet with an arbitrary in-range score so the
        // stored value (and the comparison against it) varies across rounds.
        let score = next() % 101;
        client.submit_score(&scored, &pair, &score, &false, &false, &1_700_000_000, &50, &1);

        let threshold = next() % 200; // intentionally also exceeds the 0-100 range
        let wallet = if next() % 2 == 0 { &scored } else { &unknown };
        let query_pair = if next() % 2 == 0 { &pair } else { &other_pair };

        // The contract call would trap the whole transaction if the gate ever
        // panicked; reaching the assert means it returned a clean bool.
        let result = client.query_risk_gate(wallet, query_pair, &threshold);
        let _ = result;
    }
}

// ── supports_interface registry ───────────────────────────────────────────────

#[test]
fn test_supports_interface_score() {
    let (_env, client, _admin, _service) = setup();
    assert!(client.supports_interface(&symbol_short!("score")));
}

#[test]
fn test_supports_interface_all_registered() {
    let (env, client, _admin, _service) = setup();
    for cap in ["score", "history", "batch", "gate", "aggr"] {
        let sym = Symbol::new(&env, cap);
        assert!(client.supports_interface(&sym), "capability `{cap}` should be supported");
    }
}

#[test]
fn test_supports_interface_unknown() {
    let (_env, client, _admin, _service) = setup();
    assert!(!client.supports_interface(&symbol_short!("foobar")));
}

// ── RiskScore XDR layout stability ────────────────────────────────────────────

#[test]
fn test_risk_score_xdr_stability() {
    let (env, _client, _admin, _service) = setup();

    let original = RiskScore {
        score: 87,
        benford_flag: true,
        ml_flag: false,
        timestamp: 1_700_000_000,
        confidence: 92,
        model_version: 3,
    };

    // Round-trip through the host `Val` representation. This exercises the
    // exact contracttype serialization third parties decode against; if the
    // field set or ordering changed incompatibly, the conversion back would
    // fail or yield a different struct.
    let encoded: Val = original.clone().into_val(&env);
    let decoded = RiskScore::try_from_val(&env, &encoded).expect("RiskScore must round-trip");

    assert_eq!(decoded, original);
    assert_eq!(decoded.score, 87);
    assert!(decoded.benford_flag);
    assert!(!decoded.ml_flag);
    assert_eq!(decoded.timestamp, 1_700_000_000);
    assert_eq!(decoded.confidence, 92);
    assert_eq!(decoded.model_version, 3);
}

// ── Error discriminant stability ──────────────────────────────────────────────

#[test]
fn test_error_codes_stable() {
    // These values are part of the published ABI (see docs/interface-spec.md).
    // Changing any of them silently breaks every integrator's error handling.
    assert_eq!(Error::AlreadyInitialized as u32, 1);
    assert_eq!(Error::NotInitialized as u32, 2);
    assert_eq!(Error::Unauthorized as u32, 3);
    assert_eq!(Error::InvalidScore as u32, 4);
    assert_eq!(Error::InvalidConfidence as u32, 5);
    assert_eq!(Error::ScoreNotFound as u32, 6);
    assert_eq!(Error::ContractPaused as u32, 7);
    assert_eq!(Error::NoPendingAdminTransfer as u32, 8);
    assert_eq!(Error::EmptyBatch as u32, 9);
    assert_eq!(Error::BatchTooLarge as u32, 10);
    assert_eq!(Error::ArithmeticOverflow as u32, 11);
}
