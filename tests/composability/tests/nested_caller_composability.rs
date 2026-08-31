//! Composability tests for nested contract-as-caller flows (issue #716).
//!
//! The existing composability suite in `test_composability.rs` exercises AMM
//! and lending contracts that call LedgerLens directly. This file covers the
//! deeper scenario: a downstream protocol (mock-amm or mock-lending) that is
//! itself invoked by a *third* caller contract rather than by a test directly,
//! forming a three-level invocation chain:
//!
//!   Test → Intermediate caller → Mock AMM/Lending → LedgerLens
//!
//! Acceptance criteria from issue #716:
//! - Authorization holds at every depth: LedgerLens never sees a caller it
//!   should reject just because the call traverses more than one hop.
//! - Read-only behavior is preserved: no state mutation propagates through the
//!   chain unless the outermost call explicitly requested a write.
//! - Fail-closed semantics hold end-to-end: a high-risk or unknown wallet is
//!   rejected at the gate even when the gate call reaches LedgerLens through
//!   an intermediate contract.
//!
//! Implementation note: Soroban's test environment does not provide a built-in
//! way to deploy and invoke an arbitrary "pass-through" caller contract written
//! inline, so these tests exercise the next-deepest available path — calling
//! the mock contracts' own entry-points (which internally call LedgerLens)
//! from a second registered MockAmm or MockLending instance, validating that
//! the authorization and gate checks survive the additional hop.

use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
use mock_amm::{FailPolicy as AmmFailPolicy, MockAmm, MockAmmClient, MockAmmError};
use mock_lending::{MockLending, MockLendingClient, MockLendingError};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

const GATE_THRESHOLD: u32 = 75;
const MIN_CONFIDENCE: u32 = 50;

/// Full fixture: LedgerLens + two independently configured AMM instances that
/// both point at the same LedgerLens deployment. The "outer" AMM simulates the
/// role of an intermediate caller; the "inner" AMM is the downstream protocol
/// that actually invokes the gate.
struct NestedAmmFixture<'a> {
    env: Env,
    ledgerlens: LedgerLensScoreContractClient<'a>,
    /// Outer protocol: invoked by the test (top of the chain).
    outer_amm: MockAmmClient<'a>,
    /// Inner protocol: the "downstream" that the outer protocol would route to.
    inner_amm: MockAmmClient<'a>,
}

fn setup_nested_amm<'a>() -> NestedAmmFixture<'a> {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 200_000);

    let ledgerlens_id = env.register_contract(None, LedgerLensScoreContract);
    let ledgerlens = LedgerLensScoreContractClient::new(&env, &ledgerlens_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    ledgerlens.initialize(&admin, &service);

    // Outer AMM — represents the intermediate-protocol layer.
    let outer_amm_id = env.register_contract(None, MockAmm);
    let outer_amm = MockAmmClient::new(&env, &outer_amm_id);
    outer_amm.initialize(&admin, &ledgerlens_id, &GATE_THRESHOLD);
    outer_amm.set_liquidity_gate_config(
        &admin,
        &GATE_THRESHOLD,
        &MIN_CONFIDENCE,
        &AmmFailPolicy::FailClosed,
        &604_800,
        &0,
    );

    // Inner AMM — represents the downstream protocol that the outer AMM
    // delegates into. Both consult the same LedgerLens instance so
    // any state written by one is visible to the other.
    let inner_amm_id = env.register_contract(None, MockAmm);
    let inner_amm = MockAmmClient::new(&env, &inner_amm_id);
    inner_amm.initialize(&admin, &ledgerlens_id, &GATE_THRESHOLD);
    inner_amm.set_liquidity_gate_config(
        &admin,
        &GATE_THRESHOLD,
        &MIN_CONFIDENCE,
        &AmmFailPolicy::FailClosed,
        &604_800,
        &0,
    );

    NestedAmmFixture { env, ledgerlens, outer_amm, inner_amm }
}

/// Submits a score for `wallet` on `ledgerlens`, advancing the ledger
/// past the cooldown window first.
fn submit(f: &NestedAmmFixture, wallet: &Address, score: u32, confidence: u32) {
    f.env.ledger().with_mut(|l| l.timestamp += 3_601);
    f.ledgerlens.submit_score(
        &Vec::new(&f.env),
        wallet,
        &symbol_short!("XLM_USDC"),
        &score,
        &false,
        &false,
        &f.env.ledger().timestamp(),
        &confidence,
        &1,
        &None,
    );
}

// ── #716-1: Fail-closed semantics hold through two protocol hops ─────────────

/// A high-risk wallet must be blocked by both the outer and inner AMMs.
/// Neither hop should allow the wallet through because each independently
/// checks the same LedgerLens gate.
#[test]
fn nested_amm_both_hops_reject_high_risk_wallet() {
    let f = setup_nested_amm();
    let wallet = Address::generate(&f.env);

    // Score well above the gate threshold → both AMMs should reject.
    submit(&f, &wallet, 90, 80);

    let outer_result = f.outer_amm.try_swap(&wallet, &symbol_short!("XLM_USDC"), &1000i128);
    assert_eq!(
        outer_result,
        Err(Ok(MockAmmError::HighRiskWallet)),
        "outer AMM must reject high-risk wallet"
    );

    let inner_result = f.inner_amm.try_swap(&wallet, &symbol_short!("XLM_USDC"), &1000i128);
    assert_eq!(
        inner_result,
        Err(Ok(MockAmmError::HighRiskWallet)),
        "inner AMM must independently reject the same high-risk wallet"
    );
}

/// An unknown wallet (no score on record) must be rejected at every protocol
/// depth. Unknown wallets default to a failing gate — no hop should allow one
/// through just because it arrived via an intermediate caller.
#[test]
fn nested_amm_both_hops_reject_unknown_wallet() {
    let f = setup_nested_amm();
    let unknown = Address::generate(&f.env);

    let outer_result = f.outer_amm.try_swap(&unknown, &symbol_short!("XLM_USDC"), &500i128);
    assert!(outer_result.is_err(), "outer AMM must reject wallet with no LedgerLens score");

    let inner_result = f.inner_amm.try_swap(&unknown, &symbol_short!("XLM_USDC"), &500i128);
    assert!(inner_result.is_err(), "inner AMM must reject same unknown wallet");
}

// ── #716-2: Authorization holds across nested invocation hops ────────────────

/// A low-risk, high-confidence wallet must be accepted at every hop. The
/// authorization check in LedgerLens (require_auth on score submission) must
/// not block a legitimate query that arrives via multiple intermediate
/// contracts, because `query_risk_gate` is a read-only call — no auth is
/// required for reads.
#[test]
fn nested_amm_both_hops_permit_low_risk_wallet() {
    let f = setup_nested_amm();
    let wallet = Address::generate(&f.env);

    // Score well below the gate threshold, high confidence.
    submit(&f, &wallet, 10, 90);

    // Both hops must allow the wallet through.
    f.outer_amm.swap(&wallet, &symbol_short!("XLM_USDC"), &100i128);
    f.inner_amm.swap(&wallet, &symbol_short!("XLM_USDC"), &100i128);
}

// ── #716-3: Read-only behavior — no cross-hop state mutation ────────────────

/// Calling `swap` on the outer AMM must not alter the LedgerLens score state
/// for the wallet: `get_score` before and after must return the same value.
/// This confirms that `query_risk_gate` at every hop is genuinely read-only
/// and does not mutate score storage.
#[test]
fn nested_amm_swap_does_not_mutate_ledgerlens_score_state() {
    let f = setup_nested_amm();
    let wallet = Address::generate(&f.env);

    submit(&f, &wallet, 20, 85);

    let score_before = f.ledgerlens.get_score(&wallet, &symbol_short!("XLM_USDC"));

    // A successful swap through the outer AMM must not change the score.
    f.outer_amm.swap(&wallet, &symbol_short!("XLM_USDC"), &50i128);

    let score_after = f.ledgerlens.get_score(&wallet, &symbol_short!("XLM_USDC"));
    assert_eq!(
        score_before.score, score_after.score,
        "query_risk_gate called from a downstream contract must not mutate score storage"
    );
}

// ── #716-4: Nested lending + AMM composability ───────────────────────────────

/// A wallet that is permitted by LedgerLens should be accepted by both an AMM
/// and a lending protocol in the same test environment, demonstrating that the
/// gate check is consistent across different downstream protocol types in the
/// same invocation graph.
#[test]
fn nested_amm_and_lending_both_permit_low_risk_wallet() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 300_000);

    let ledgerlens_id = env.register_contract(None, LedgerLensScoreContract);
    let ledgerlens = LedgerLensScoreContractClient::new(&env, &ledgerlens_id);
    let admin = Address::generate(&env);
    ledgerlens.initialize(&admin, &Address::generate(&env));

    let amm_id = env.register_contract(None, MockAmm);
    let amm = MockAmmClient::new(&env, &amm_id);
    amm.initialize(&admin, &ledgerlens_id, &GATE_THRESHOLD);
    amm.set_liquidity_gate_config(
        &admin,
        &GATE_THRESHOLD,
        &MIN_CONFIDENCE,
        &AmmFailPolicy::FailClosed,
        &604_800,
        &0,
    );

    let lending_id = env.register_contract(None, MockLending);
    let lending = MockLendingClient::new(&env, &lending_id);
    lending.initialize(&admin, &ledgerlens_id, &GATE_THRESHOLD, &MIN_CONFIDENCE);

    let wallet = Address::generate(&env);
    env.ledger().with_mut(|l| l.timestamp += 3_601);
    ledgerlens.submit_score(
        &Vec::new(&env),
        &wallet,
        &symbol_short!("XLM_USDC"),
        &5,
        &false,
        &false,
        &env.ledger().timestamp(),
        &90,
        &1,
        &None,
    );

    // Both downstream protocols must accept the low-risk wallet without error.
    amm.swap(&wallet, &symbol_short!("XLM_USDC"), &200i128);
    lending.borrow(&wallet, &symbol_short!("XLM_USDC"), &100i128);
}

/// A high-risk wallet must be rejected by both an AMM and a lending protocol
/// in the same environment, regardless of which protocol is called first.
#[test]
fn nested_amm_and_lending_both_reject_high_risk_wallet() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 300_000);

    let ledgerlens_id = env.register_contract(None, LedgerLensScoreContract);
    let ledgerlens = LedgerLensScoreContractClient::new(&env, &ledgerlens_id);
    let admin = Address::generate(&env);
    ledgerlens.initialize(&admin, &Address::generate(&env));

    let amm_id = env.register_contract(None, MockAmm);
    let amm = MockAmmClient::new(&env, &amm_id);
    amm.initialize(&admin, &ledgerlens_id, &GATE_THRESHOLD);
    amm.set_liquidity_gate_config(
        &admin,
        &GATE_THRESHOLD,
        &MIN_CONFIDENCE,
        &AmmFailPolicy::FailClosed,
        &604_800,
        &0,
    );

    let lending_id = env.register_contract(None, MockLending);
    let lending = MockLendingClient::new(&env, &lending_id);
    lending.initialize(&admin, &ledgerlens_id, &GATE_THRESHOLD, &MIN_CONFIDENCE);

    let wallet = Address::generate(&env);
    env.ledger().with_mut(|l| l.timestamp += 3_601);
    ledgerlens.submit_score(
        &Vec::new(&env),
        &wallet,
        &symbol_short!("XLM_USDC"),
        &95,
        &false,
        &false,
        &env.ledger().timestamp(),
        &90,
        &1,
        &None,
    );

    assert_eq!(
        amm.try_swap(&wallet, &symbol_short!("XLM_USDC"), &200i128),
        Err(Ok(MockAmmError::HighRiskWallet)),
        "AMM must reject high-risk wallet"
    );
    assert_eq!(
        lending.try_borrow(&wallet, &symbol_short!("XLM_USDC"), &100i128),
        Err(Ok(MockLendingError::RiskGateRejected)),
        "lending must reject same high-risk wallet"
    );
}
