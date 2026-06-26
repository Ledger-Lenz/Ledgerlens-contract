//! Cross-contract composability integration suite (issue #121).
//!
//! `query_risk_gate` and `query_risk_gate_with_confidence` are documented in
//! `docs/interface-spec.md` as the primary integration primitives for AMMs
//! and lending protocols, but until now nothing actually deployed a second
//! contract that calls them. These tests deploy `mock-amm` and
//! `mock-lending` alongside `LedgerLensScoreContract` in the same Soroban
//! test environment and exercise the real cross-contract call path —
//! `client.swap(...)` / `client.borrow(...)` invoking the mocks, which in
//! turn invoke LedgerLens — rather than calling the gate functions directly.

use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
use mock_amm::{MockAmm, MockAmmClient, MockAmmError};
use mock_lending::{MockLending, MockLendingClient, MockLendingError};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

const GATE_THRESHOLD: u32 = 75;
const MIN_CONFIDENCE: u32 = 50;

struct Fixture<'a> {
    env: Env,
    ledgerlens: LedgerLensScoreContractClient<'a>,
    amm: MockAmmClient<'a>,
    lending: MockLendingClient<'a>,
}

/// Deploys LedgerLens plus both mock contracts in one shared `Env`, wires
/// each mock at the configured gate threshold / confidence floor, and
/// returns ready-to-use clients.
fn setup<'a>() -> Fixture<'a> {
    let env = Env::default();
    env.mock_all_auths();

    let ledgerlens_id = env.register_contract(None, LedgerLensScoreContract);
    let ledgerlens = LedgerLensScoreContractClient::new(&env, &ledgerlens_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    ledgerlens.initialize(&admin, &service);

    let amm_id = env.register_contract(None, MockAmm);
    let amm = MockAmmClient::new(&env, &amm_id);
    amm.initialize(&ledgerlens_id, &GATE_THRESHOLD);
    amm.set_liquidity_gate_config(&GATE_THRESHOLD, &MIN_CONFIDENCE);

    let lending_id = env.register_contract(None, MockLending);
    let lending = MockLendingClient::new(&env, &lending_id);
    lending.initialize(&ledgerlens_id, &GATE_THRESHOLD, &MIN_CONFIDENCE);

    Fixture { env, ledgerlens, amm, lending }
}

/// Submits a score for `wallet`, advancing the ledger past the 1-hour
/// cooldown first so repeated submissions in the same test never collide.
fn submit_score(fixture: &Fixture, wallet: &Address, score: u32, confidence: u32) {
    fixture.env.ledger().with_mut(|l| l.timestamp += 3_601);
    fixture
        .ledgerlens
        .submit_score(
            &Vec::new(&fixture.env),
            wallet,
            &symbol_short!("XLM_USDC"),
            &score,
            &false,
            &false,
            &fixture.env.ledger().timestamp(),
            &confidence,
            &1,
            &None,
        );
}

// ── Acceptance criterion: both mock contracts compile and deploy ───────────

#[test]
fn both_mock_contracts_deploy_alongside_ledgerlens() {
    // `setup()` deploying without panicking *is* the assertion: it proves
    // mock-amm and mock-lending compile, link against ledgerlens-score, and
    // register in the same Env as a real LedgerLens deployment.
    let fixture = setup();
    assert_eq!(fixture.ledgerlens.get_version(), 3);
}

// ── Acceptance criterion: AMM swap rejected/accepted by risk score ─────────

#[test]
fn amm_swap_rejected_for_high_risk_score() {
    let fixture = setup();
    let wallet = Address::generate(&fixture.env);
    submit_score(&fixture, &wallet, 90, 95); // 90 >= GATE_THRESHOLD(75)

    let result = fixture.amm.try_swap(&wallet, &symbol_short!("XLM_USDC"), &1_000);
    assert_eq!(result, Err(Ok(MockAmmError::HighRiskWallet)));
}

#[test]
fn amm_swap_accepted_for_low_risk_score() {
    let fixture = setup();
    let wallet = Address::generate(&fixture.env);
    submit_score(&fixture, &wallet, 10, 95); // 10 < GATE_THRESHOLD(75)

    let result = fixture.amm.try_swap(&wallet, &symbol_short!("XLM_USDC"), &1_000);
    assert_eq!(result, Ok(Ok(())));
}

#[test]
fn amm_swap_rejected_for_unknown_wallet() {
    let fixture = setup();
    let wallet = Address::generate(&fixture.env); // never scored

    let result = fixture.amm.try_swap(&wallet, &symbol_short!("XLM_USDC"), &1_000);
    assert_eq!(result, Err(Ok(MockAmmError::HighRiskWallet)));
}

// ── AMM gated liquidity provision (issue #214) ───────────────────────────────

#[test]
fn amm_provide_liquidity_allowed_for_low_risk_high_confidence() {
    let fixture = setup();
    let provider = Address::generate(&fixture.env);
    submit_score(&fixture, &provider, 10, 90);

    assert_eq!(fixture.amm.try_provide_liquidity_gated(&provider, &1_000), Ok(Ok(())));
}

#[test]
fn amm_provide_liquidity_blocked_for_high_risk_provider() {
    let fixture = setup();
    let provider = Address::generate(&fixture.env);
    submit_score(&fixture, &provider, 90, 95);

    let result = fixture.amm.try_provide_liquidity_gated(&provider, &1_000);
    assert_eq!(result, Err(Ok(MockAmmError::HighRiskWallet)));
}

#[test]
fn amm_provide_liquidity_blocked_for_low_confidence() {
    let fixture = setup();
    let provider = Address::generate(&fixture.env);
    submit_score(&fixture, &provider, 10, 20);

    let result = fixture.amm.try_provide_liquidity_gated(&provider, &1_000);
    assert_eq!(result, Err(Ok(MockAmmError::LowConfidence)));
}

#[test]
fn amm_provide_liquidity_uses_set_risk_oracle() {
    let fixture = setup();
    let alt_oracle_id = fixture.env.register_contract(None, LedgerLensScoreContract);
    let alt_oracle = LedgerLensScoreContractClient::new(&fixture.env, &alt_oracle_id);
    let admin = Address::generate(&fixture.env);
    let service = Address::generate(&fixture.env);
    alt_oracle.initialize(&admin, &service);

    let provider = Address::generate(&fixture.env);
    fixture.env.ledger().with_mut(|l| l.timestamp += 3_601);
    alt_oracle.submit_score(
        &Vec::new(&fixture.env),
        &provider,
        &symbol_short!("XLM_USDC"),
        &10,
        &false,
        &false,
        &fixture.env.ledger().timestamp(),
        &90,
        &1,
        &None,
    );

    fixture.amm.set_risk_oracle(&alt_oracle_id);
    assert_eq!(fixture.amm.try_provide_liquidity_gated(&provider, &500), Ok(Ok(())));
}

// ── Acceptance criterion: lending gate fails on low confidence ─────────────

#[test]
fn lending_borrow_rejected_for_low_confidence_despite_low_score() {
    let fixture = setup();
    let wallet = Address::generate(&fixture.env);
    // Score itself is safe (10 < 75) but confidence(20) is below the
    // market's floor(50) — must be treated as "no data", not "safe".
    submit_score(&fixture, &wallet, 10, 20);

    let result = fixture.lending.try_borrow(&wallet, &symbol_short!("XLM_USDC"), &1_000);
    assert_eq!(result, Err(Ok(MockLendingError::RiskGateRejected)));
}

#[test]
fn lending_borrow_accepted_for_low_risk_high_confidence_score() {
    let fixture = setup();
    let wallet = Address::generate(&fixture.env);
    submit_score(&fixture, &wallet, 10, 90);

    let result = fixture.lending.try_borrow(&wallet, &symbol_short!("XLM_USDC"), &1_000);
    assert_eq!(result, Ok(Ok(())));
}

#[test]
fn lending_borrow_rejected_for_high_risk_score_even_with_high_confidence() {
    let fixture = setup();
    let wallet = Address::generate(&fixture.env);
    submit_score(&fixture, &wallet, 90, 90); // score fails despite confidence passing

    let result = fixture.lending.try_borrow(&wallet, &symbol_short!("XLM_USDC"), &1_000);
    assert_eq!(result, Err(Ok(MockLendingError::RiskGateRejected)));
}

// ── Acceptance criterion: embargoed wallet → gate false regardless of score ─

#[test]
fn amm_swap_rejected_for_embargoed_wallet_with_otherwise_safe_score() {
    let fixture = setup();
    let wallet = Address::generate(&fixture.env);
    submit_score(&fixture, &wallet, 5, 99); // would otherwise easily pass

    fixture.ledgerlens.set_score_embargo(&wallet, &None);
    assert!(fixture.ledgerlens.is_embargoed(&wallet));

    let result = fixture.amm.try_swap(&wallet, &symbol_short!("XLM_USDC"), &1_000);
    assert_eq!(result, Err(Ok(MockAmmError::HighRiskWallet)));
}

#[test]
fn lending_borrow_rejected_for_embargoed_wallet_with_otherwise_safe_score() {
    let fixture = setup();
    let wallet = Address::generate(&fixture.env);
    submit_score(&fixture, &wallet, 5, 99);

    fixture.ledgerlens.set_score_embargo(&wallet, &None);

    let result = fixture.lending.try_borrow(&wallet, &symbol_short!("XLM_USDC"), &1_000);
    assert_eq!(result, Err(Ok(MockLendingError::RiskGateRejected)));
}

#[test]
fn amm_swap_resumes_after_embargo_lifted() {
    let fixture = setup();
    let wallet = Address::generate(&fixture.env);
    submit_score(&fixture, &wallet, 5, 99);

    fixture.ledgerlens.set_score_embargo(&wallet, &None);
    assert_eq!(
        fixture.amm.try_swap(&wallet, &symbol_short!("XLM_USDC"), &1_000),
        Err(Ok(MockAmmError::HighRiskWallet))
    );

    fixture.ledgerlens.lift_score_embargo(&wallet);
    assert_eq!(fixture.amm.try_swap(&wallet, &symbol_short!("XLM_USDC"), &1_000), Ok(Ok(())));
}

// ── Mock-contract input validation (not LedgerLens-specific, but part of
//    proving the mocks are well-formed integrators) ─────────────────────────

#[test]
fn amm_swap_rejects_non_positive_amount_before_consulting_ledgerlens() {
    let fixture = setup();
    let wallet = Address::generate(&fixture.env);
    submit_score(&fixture, &wallet, 10, 90); // would pass the gate

    let result = fixture.amm.try_swap(&wallet, &symbol_short!("XLM_USDC"), &0);
    assert_eq!(result, Err(Ok(MockAmmError::InvalidAmount)));
}

#[test]
fn lending_borrow_rejects_non_positive_amount_before_consulting_ledgerlens() {
    let fixture = setup();
    let wallet = Address::generate(&fixture.env);
    submit_score(&fixture, &wallet, 10, 90);

    let result = fixture.lending.try_borrow(&wallet, &symbol_short!("XLM_USDC"), &-5);
    assert_eq!(result, Err(Ok(MockLendingError::InvalidAmount)));
}
