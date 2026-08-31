//! Consumer integration fixtures for stale-oracle fallback modes (issue #717).
//!
//! Documents and asserts the concrete behavior of `query_risk_gate` and
//! `query_risk_gate_with_confidence` when LedgerLens is in one of four
//! degraded states that a real downstream protocol must handle:
//!
//! 1. **Paused, no failover** — primary circuit-breaker tripped, no secondary
//!    configured: gate returns `false` (fail closed).
//! 2. **Paused, failover present but stale** — secondary score older than
//!    `FAILOVER_STALENESS_WINDOW`: gate returns `false` (fail closed).
//! 3. **Paused, healthy failover** — secondary has a recent, low-risk score:
//!    gate delegates to secondary and returns `true`.
//! 4. **Active, score missing** — primary is live but wallet has never been
//!    scored: gate returns `false` (fail closed, unknown risk is not safe).
//! 5. **Active, score present but confidence below floor** — score exists but
//!    the confidence level is beneath the consumer's minimum: gate returns
//!    `false` even when the risk score itself would pass.
//!
//! Each fixture below is a deterministic, self-contained test that fails
//! against any implementation that treats unknown risk as safe (AC from
//! issue #717). The `setup_*` helpers document the exact deploy/config steps
//! a real AMM or lending contract must replicate.

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
/// LedgerLens hard-coded failover staleness window (seconds).  Mirrors the
/// constant in `contracts/ledgerlens-score/src/constants.rs` so fixtures here
/// stay honest without importing a private module.
const FAILOVER_STALENESS_WINDOW: u64 = 3_600;

// ── Shared helpers ────────────────────────────────────────────────────────────

/// Deploy a fresh LedgerLens instance with `admin` and `service` already set.
fn deploy_ledgerlens(env: &Env) -> (LedgerLensScoreContractClient, Address) {
    let id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(env, &id);
    let admin = Address::generate(env);
    let service = Address::generate(env);
    client.initialize(&admin, &service);
    (client, id)
}

/// Submit a score, advancing ledger time past the 1-hour submission cooldown.
fn submit_score(
    env: &Env,
    ledgerlens: &LedgerLensScoreContractClient,
    wallet: &Address,
    score: u32,
    confidence: u32,
) {
    env.ledger().with_mut(|l| l.timestamp += 3_601);
    ledgerlens.submit_score(
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

/// Deploy mock-amm wired to the given LedgerLens instance.
fn deploy_amm<'a>(env: &'a Env, ledgerlens_id: &Address) -> MockAmmClient<'a> {
    let amm_id = env.register_contract(None, MockAmm);
    let amm = MockAmmClient::new(env, &amm_id);
    let admin = Address::generate(env);
    amm.initialize(&admin, ledgerlens_id, &GATE_THRESHOLD);
    amm.set_liquidity_gate_config(
        &admin,
        &GATE_THRESHOLD,
        &MIN_CONFIDENCE,
        &AmmFailPolicy::FailClosed,
        &604_800,
        &0,
    );
    amm
}

/// Deploy mock-lending wired to the given LedgerLens instance.
fn deploy_lending<'a>(env: &'a Env, ledgerlens_id: &Address) -> MockLendingClient<'a> {
    let lending_id = env.register_contract(None, MockLending);
    let lending = MockLendingClient::new(env, &lending_id);
    lending.initialize(&Address::generate(env), ledgerlens_id, &GATE_THRESHOLD, &MIN_CONFIDENCE);
    lending
}

// ── Fixture 1: Paused primary, no failover configured ─────────────────────────
//
// Accept/reject path: primary paused → no secondary → fail closed.
// This is the safest path: the consumer gets `false` unconditionally and must
// reject the operation.  An integrator that treats a paused oracle as "no
// opinion" (i.e. defaults to `true`) introduces a circuit-breaker bypass.

#[test]
fn amm_swap_blocked_when_primary_paused_and_no_failover() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 100_000);

    let (primary, primary_id) = deploy_ledgerlens(&env);
    let amm = deploy_amm(&env, &primary_id);

    let wallet = Address::generate(&env);
    // Score a wallet that would otherwise pass the gate.
    submit_score(&env, &primary, &wallet, 10, 90);

    // Operator pauses the primary — no failover is set.
    primary.pause(&Vec::new(&env));
    assert!(primary.is_paused());

    // Gate must reject even though the score on the paused primary would pass.
    let result = amm.try_swap(&wallet, &symbol_short!("XLM_USDC"), &1_000);
    assert_eq!(result, Err(Ok(MockAmmError::HighRiskWallet)));
}

#[test]
fn lending_borrow_blocked_when_primary_paused_and_no_failover() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 100_000);

    let (primary, primary_id) = deploy_ledgerlens(&env);
    let lending = deploy_lending(&env, &primary_id);

    let wallet = Address::generate(&env);
    submit_score(&env, &primary, &wallet, 10, 90);

    primary.pause(&Vec::new(&env));

    let result = lending.try_borrow(&wallet, &symbol_short!("XLM_USDC"), &1_000);
    assert_eq!(result, Err(Ok(MockLendingError::RiskGateRejected)));
}

// ── Fixture 2: Paused primary, failover present but score is stale ────────────
//
// Accept/reject path: primary paused → secondary consulted → secondary score
// age exceeds FAILOVER_STALENESS_WINDOW → fail closed.
// A score on the secondary that is older than the window is treated as
// missing, not as "safe".  Integrators must not assume a secondary always has
// a usable score.

#[test]
fn amm_swap_blocked_when_failover_score_is_stale() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 100_000);

    let (primary, primary_id) = deploy_ledgerlens(&env);
    let (secondary, _secondary_id) = deploy_ledgerlens(&env);
    let secondary_id = secondary.address.clone();

    let amm = deploy_amm(&env, &primary_id);
    let wallet = Address::generate(&env);

    // Submit a score on the secondary when ledger time is at 100_000 + 3_601.
    submit_score(&env, &secondary, &wallet, 10, 90);
    let score_time = env.ledger().timestamp();

    // Register the secondary as failover.
    primary.set_failover_contract(&Vec::new(&env), &secondary_id);

    // Advance time beyond the staleness window so the secondary score is stale.
    env.ledger().with_mut(|l| {
        l.timestamp = score_time + FAILOVER_STALENESS_WINDOW + 1;
    });

    primary.pause(&Vec::new(&env));

    // Gate must fail closed: stale failover score = no usable signal.
    let result = amm.try_swap(&wallet, &symbol_short!("XLM_USDC"), &1_000);
    assert_eq!(result, Err(Ok(MockAmmError::HighRiskWallet)));
}

// ── Fixture 3: Paused primary, healthy failover ───────────────────────────────
//
// Accept/reject path: primary paused → secondary consulted → secondary has a
// recent, low-risk, high-confidence score → gate passes.
// This proves the failover path is reachable and functional.  Without this
// fixture, an integrator cannot distinguish "failover works" from "always
// fails closed regardless".

#[test]
fn amm_swap_allowed_via_healthy_failover_when_primary_paused() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 100_000);

    let (primary, primary_id) = deploy_ledgerlens(&env);
    let (secondary, _secondary_id) = deploy_ledgerlens(&env);
    let secondary_id = secondary.address.clone();

    let amm = deploy_amm(&env, &primary_id);
    let wallet = Address::generate(&env);

    // Submit a fresh, low-risk score on the secondary.
    submit_score(&env, &secondary, &wallet, 10, 90);

    primary.set_failover_contract(&Vec::new(&env), &secondary_id);
    primary.pause(&Vec::new(&env));

    // Time has not advanced past the staleness window — score is fresh.
    let result = amm.try_swap(&wallet, &symbol_short!("XLM_USDC"), &1_000);
    assert_eq!(result, Ok(Ok(())));
}

// ── Fixture 4: Active primary, wallet never scored ────────────────────────────
//
// Accept/reject path: oracle active → no score record for wallet → fail closed.
// A missing score is not equivalent to a score of zero.  Integrators must
// never allow wallets with no risk signal through the gate.

#[test]
fn amm_swap_blocked_for_wallet_with_no_score_on_active_oracle() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 100_000);

    let (_primary, primary_id) = deploy_ledgerlens(&env);
    let amm = deploy_amm(&env, &primary_id);

    let wallet = Address::generate(&env); // never scored

    let result = amm.try_swap(&wallet, &symbol_short!("XLM_USDC"), &1_000);
    assert_eq!(result, Err(Ok(MockAmmError::HighRiskWallet)));
}

#[test]
fn lending_borrow_blocked_for_wallet_with_no_score_on_active_oracle() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 100_000);

    let (_primary, primary_id) = deploy_ledgerlens(&env);
    let lending = deploy_lending(&env, &primary_id);

    let wallet = Address::generate(&env);

    let result = lending.try_borrow(&wallet, &symbol_short!("XLM_USDC"), &1_000);
    assert_eq!(result, Err(Ok(MockLendingError::RiskGateRejected)));
}

// ── Fixture 5: Active primary, score present but confidence below floor ────────
//
// Accept/reject path: oracle active → score exists, passes risk threshold →
// confidence < min_confidence → fail closed.
// This is the subtlest failure mode: the raw score would allow the operation
// but the confidence gate rejects it.  Integrators who skip confidence checks
// allow low-quality risk signals to pass through as if they were conclusive.

#[test]
fn amm_provide_liquidity_blocked_when_score_passes_but_confidence_insufficient() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 100_000);

    let (primary, primary_id) = deploy_ledgerlens(&env);
    let amm = deploy_amm(&env, &primary_id);

    let provider = Address::generate(&env);
    // Score is below gate threshold (safe) but confidence is below MIN_CONFIDENCE.
    submit_score(&env, &primary, &provider, 10, MIN_CONFIDENCE - 1);

    let result = amm.try_provide_liquidity_gated(&provider, &1_000);
    assert_eq!(result, Err(Ok(MockAmmError::LowConfidence)));
}

#[test]
fn lending_borrow_blocked_when_score_passes_but_confidence_insufficient() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 100_000);

    let (primary, primary_id) = deploy_ledgerlens(&env);
    let lending = deploy_lending(&env, &primary_id);

    let wallet = Address::generate(&env);
    submit_score(&env, &primary, &wallet, 10, MIN_CONFIDENCE - 1);

    let result = lending.try_borrow(&wallet, &symbol_short!("XLM_USDC"), &1_000);
    assert_eq!(result, Err(Ok(MockLendingError::RiskGateRejected)));
}

// ── Fixture: Gate resumes after unpause ───────────────────────────────────────
//
// Validates the round-trip: pause → fail closed → unpause → previous signal
// honoured again.  Downstream protocols that cache "is LedgerLens paused"
// must refresh on unpause; this fixture provides the canonical test for that
// behavior.

#[test]
fn amm_swap_resumes_after_primary_unpaused() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 100_000);

    let (primary, primary_id) = deploy_ledgerlens(&env);
    let amm = deploy_amm(&env, &primary_id);

    let wallet = Address::generate(&env);
    submit_score(&env, &primary, &wallet, 10, 90);

    primary.pause(&Vec::new(&env));
    assert_eq!(
        amm.try_swap(&wallet, &symbol_short!("XLM_USDC"), &1_000),
        Err(Ok(MockAmmError::HighRiskWallet))
    );

    primary.unpause(&Vec::new(&env));
    assert_eq!(amm.try_swap(&wallet, &symbol_short!("XLM_USDC"), &1_000), Ok(Ok(())));
}
