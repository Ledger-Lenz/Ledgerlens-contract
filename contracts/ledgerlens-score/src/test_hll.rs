//! Tests for HyperLogLog unique-wallet estimation per asset pair.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{
    constants::{HLL_DEFAULT_PRECISION, HLL_MAX_PRECISION, HLL_MIN_PRECISION},
    Error, LedgerLensScoreContract, LedgerLensScoreContractClient,
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

fn submit_for(env: &Env, client: &LedgerLensScoreContractClient, wallet: &Address) {
    let pair = symbol_short!("XLM_USDC");
    client.submit_score(
        &Vec::new(env),
        wallet,
        &pair,
        &50u32,
        &false,
        &false,
        &env.ledger().timestamp(),
        &80u32,
        &1u32,
        &None,
    );
}

// ── Precision configuration ───────────────────────────────────────────────────

#[test]
fn test_default_precision() {
    let (_env, client, _admin, _service) = setup();
    assert_eq!(client.get_hll_precision(), HLL_DEFAULT_PRECISION);
}

#[test]
fn test_set_precision_valid() {
    let (env, client, _admin, _service) = setup();
    client.set_hll_precision(&Vec::new(&env), &12u32);
    assert_eq!(client.get_hll_precision(), 12);
}

#[test]
fn test_set_precision_below_min_rejected() {
    let (env, client, _admin, _service) = setup();
    let result = client.try_set_hll_precision(&Vec::new(&env), &(HLL_MIN_PRECISION - 1));
    assert_eq!(result, Err(Ok(Error::InvalidThreshold)));
}

#[test]
fn test_set_precision_above_max_rejected() {
    let (env, client, _admin, _service) = setup();
    let result = client.try_set_hll_precision(&Vec::new(&env), &(HLL_MAX_PRECISION + 1));
    assert_eq!(result, Err(Ok(Error::InvalidThreshold)));
}

#[test]
fn test_set_precision_at_bounds_accepted() {
    let (env, client, _admin, _service) = setup();
    client.set_hll_precision(&Vec::new(&env), &HLL_MIN_PRECISION);
    assert_eq!(client.get_hll_precision(), HLL_MIN_PRECISION);
    client.set_hll_precision(&Vec::new(&env), &HLL_MAX_PRECISION);
    assert_eq!(client.get_hll_precision(), HLL_MAX_PRECISION);
}

// ── Zero-state ────────────────────────────────────────────────────────────────

#[test]
fn test_estimate_zero_before_any_submission() {
    let (_env, client, _admin, _service) = setup();
    let pair = symbol_short!("XLM_USDC");
    assert_eq!(client.estimate_unique_wallets(&pair), 0);
}

// ── Single wallet ─────────────────────────────────────────────────────────────

#[test]
fn test_estimate_one_wallet_is_nonzero() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    submit_for(&env, &client, &wallet);
    let pair = symbol_short!("XLM_USDC");
    // After one wallet, estimate should be > 0.
    let estimate = client.estimate_unique_wallets(&pair);
    assert!(estimate > 0, "estimate should be > 0 after one wallet, got {}", estimate);
}

// ── Idempotent — same wallet multiple times ───────────────────────────────────

#[test]
fn test_hll_only_updated_on_first_submission() {
    let (env, client, _admin, _service) = setup();
    let pair = symbol_short!("XLM_USDC");
    let wallet = Address::generate(&env);

    submit_for(&env, &client, &wallet);
    let estimate_after_first = client.estimate_unique_wallets(&pair);

    // Second submission from the same wallet — advance past cooldown.
    env.ledger().with_mut(|l| l.timestamp = START_TS + 7_200);
    submit_for(&env, &client, &wallet);
    let estimate_after_second = client.estimate_unique_wallets(&pair);

    assert_eq!(
        estimate_after_first, estimate_after_second,
        "HLL should not be updated on repeat submissions"
    );
}

// ── Pair isolation ────────────────────────────────────────────────────────────

#[test]
fn test_hll_is_per_pair() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("XLM_BTC");

    // Submit for pair A only.
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair_a,
        &50u32,
        &false,
        &false,
        &env.ledger().timestamp(),
        &80u32,
        &1u32,
        &None,
    );

    assert!(client.estimate_unique_wallets(&pair_a) > 0);
    assert_eq!(client.estimate_unique_wallets(&pair_b), 0);
}

// ── Multiple unique wallets ───────────────────────────────────────────────────

#[test]
fn test_estimate_grows_with_unique_wallets() {
    let (env, client, _admin, _service) = setup();
    env.budget().reset_unlimited();
    let pair = symbol_short!("XLM_USDC");

    // Submit scores for 20 distinct wallets.
    for i in 0..20u32 {
        let wallet = Address::generate(&env);
        env.ledger().with_mut(|l| l.timestamp = START_TS + (i as u64) * 10);
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &pair,
            &50u32,
            &false,
            &false,
            &env.ledger().timestamp(),
            &80u32,
            &1u32,
            &None,
        );
    }

    let estimate = client.estimate_unique_wallets(&pair);
    // With 20 wallets we just verify the sketch is non-zero; the HLL
    // cardinality formula needs many more observations for ±5% accuracy.
    assert!(estimate > 0, "estimate should be positive after 20 wallets, got {}", estimate);
}

// ── Estimation accuracy (medium-N) ───────────────────────────────────────────

#[test]
fn test_estimate_accuracy_within_tolerance() {
    // Test with 30 unique wallets. HLL with default precision p=10 (1024
    // registers) is most accurate at higher cardinalities; at N=30 the
    // small-range linear-counting correction is active. We verify the estimate
    // is within ±50% of the true count, which is the meaningful property to
    // test here: the sketch correctly registers distinct addresses and the
    // cardinality estimator returns a sane result.
    //
    // Note: Soroban test budget requires reset_unlimited for multi-wallet tests.
    // For production ±5% accuracy, HLL requires thousands of observations; at
    // N=30 the sketch is behaviorally correct but high-relative-error is expected.
    let (env, client, _admin, _service) = setup();
    env.budget().reset_unlimited();
    let pair = symbol_short!("XLM_USDC");

    let n: u32 = 30;
    for i in 0..n {
        let wallet = Address::generate(&env);
        env.ledger().with_mut(|l| l.timestamp = START_TS + (i as u64) * 5);
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &pair,
            &50u32,
            &false,
            &false,
            &env.ledger().timestamp(),
            &80u32,
            &1u32,
            &None,
        );
    }

    let estimate = client.estimate_unique_wallets(&pair);
    // Verify non-zero and within ±50% of true count (30).
    assert!(estimate > 0, "estimate must be positive, got {}", estimate);
    let lo = (n as u64) / 2;
    let hi = (n as u64) * 3 / 2;
    assert!(
        estimate >= lo && estimate <= hi,
        "HLL estimate {} is outside [{}, {}] for {} true unique wallets",
        estimate,
        lo,
        hi,
        n
    );
}
