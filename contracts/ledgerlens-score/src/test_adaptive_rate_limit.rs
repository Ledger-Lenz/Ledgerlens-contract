//! Unit tests for the adaptive rate-limit feature (#275).
//!
//! The effective cooldown formula is:
//!   effective = base * (1 + variance_scale * normalised_variance / 1000)
//!
//! normalised_variance is derived from the global score histogram
//! (10 buckets, midpoints 5,15,…,95), normalised to [0, 1000] against the
//! theoretical maximum variance of 2500.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{
    constants::DEFAULT_COOLDOWN_SECS, AdaptiveRateLimit, Error, LedgerLensScoreContract,
    LedgerLensScoreContractClient,
};

const START_TS: u64 = 1_700_000_000;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    (env, client, admin)
}

fn advance_to(env: &Env, ts: u64) {
    env.ledger().with_mut(|l| l.timestamp = ts);
}

/// Submit a score at the current ledger timestamp.
fn submit(env: &Env, client: &LedgerLensScoreContractClient, wallet: &Address, pair: &soroban_sdk::Symbol, score: u32) -> Result<(), crate::Error> {
    let ts = env.ledger().timestamp();
    client.try_submit_score(
        &Vec::new(env),
        wallet,
        pair,
        &score,
        &false,
        &false,
        &ts,
        &90,
        &1,
        &None,
    )
}

// ── Defaults ─────────────────────────────────────────────────────────────────

#[test]
fn test_default_adaptive_rate_limit_disabled() {
    let (_env, client, _admin) = setup();
    let config = client.get_adaptive_rate_limit();
    assert!(!config.enabled);
    assert_eq!(config.variance_scale, 0);
}

#[test]
fn test_get_effective_cooldown_equals_base_when_disabled() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    // With adaptive disabled, effective cooldown == global cooldown.
    assert_eq!(client.get_effective_cooldown(&wallet, &pair), DEFAULT_COOLDOWN_SECS);
}

// ── set / get ─────────────────────────────────────────────────────────────────

#[test]
fn test_set_and_get_adaptive_rate_limit() {
    let (env, client, _admin) = setup();
    client.set_adaptive_rate_limit(&Vec::new(&env), &true, &500);
    let config = client.get_adaptive_rate_limit();
    assert!(config.enabled);
    assert_eq!(config.variance_scale, 500);
}

#[test]
fn test_set_adaptive_rate_limit_requires_init() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let result = client.try_set_adaptive_rate_limit(&Vec::new(&env), &true, &100);
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

// ── Zero variance (all scores identical) ─────────────────────────────────────

#[test]
fn test_effective_cooldown_zero_variance_equals_base() {
    // With all scores in one bucket, variance ≈ 0 and the effective cooldown
    // should equal the base cooldown regardless of variance_scale.
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_adaptive_rate_limit(&Vec::new(&env), &true, &1000);

    // Submit ten scores all at 50 (bucket 5, midpoint 55) to build histogram.
    for i in 0..10u64 {
        advance_to(&env, START_TS + i * DEFAULT_COOLDOWN_SECS);
        submit(&env, &client, &wallet, &pair, 50).unwrap();
    }

    // All scores are in one bucket → near-zero variance → effective cooldown ≈ base.
    // Allow a small tolerance for bucket-midpoint approximation.
    let effective = client.get_effective_cooldown(&wallet, &pair);
    // normalised_variance should be tiny; effective must be >= base and not
    // astronomically larger than base.
    assert!(effective >= DEFAULT_COOLDOWN_SECS);
    // With zero true variance the scale factor is ≤ 1 % off base.
    assert!(effective < DEFAULT_COOLDOWN_SECS * 2);
}

// ── High variance (bimodal extremes) ─────────────────────────────────────────

#[test]
fn test_effective_cooldown_increases_with_high_variance() {
    // With scores spread across extremes (0 and 100), variance ≈ max.
    // With variance_scale = 1000 the cooldown should roughly double.
    let (env, client, _admin) = setup();
    let pair = symbol_short!("XLM_USDC");

    client.set_adaptive_rate_limit(&Vec::new(&env), &true, &1000);

    // Submit alternating 0 and 100 scores for many distinct wallets so the
    // histogram is populated without cooldown interference.
    let n = 20u64;
    for i in 0..n {
        advance_to(&env, START_TS + i * DEFAULT_COOLDOWN_SECS);
        let w = Address::generate(&env);
        let score = if i % 2 == 0 { 0 } else { 100 };
        submit(&env, &client, &w, &pair, score).unwrap();
    }

    let wallet = Address::generate(&env);
    let effective = client.get_effective_cooldown(&wallet, &pair);
    // With maximum variance and scale=1000, effective ≈ base * 2.
    // Allow 20 % tolerance around the expected value.
    let expected = DEFAULT_COOLDOWN_SECS * 2;
    assert!(effective > DEFAULT_COOLDOWN_SECS, "effective should exceed base");
    assert!(
        effective >= expected * 8 / 10 && effective <= expected * 12 / 10,
        "expected ~{expected} but got {effective}"
    );
}

// ── variance_scale = 0 behaves like disabled ──────────────────────────────────

#[test]
fn test_variance_scale_zero_equals_base_cooldown() {
    let (env, client, _admin) = setup();
    let pair = symbol_short!("XLM_USDC");

    // Enable but with scale = 0.
    client.set_adaptive_rate_limit(&Vec::new(&env), &true, &0);

    // Populate histogram with mixed scores.
    for i in 0..10u64 {
        let w = Address::generate(&env);
        submit(&env, &client, &w, &pair, (i * 10) as u32).unwrap();
    }

    let wallet = Address::generate(&env);
    assert_eq!(client.get_effective_cooldown(&wallet, &pair), DEFAULT_COOLDOWN_SECS);
}

// ── Rate limit enforcement respects effective cooldown ────────────────────────

#[test]
fn test_submit_blocked_by_adaptive_cooldown() {
    // With high variance and scale=1000 the effective cooldown ≈ 2×base.
    // A second submission after exactly 1×base should be rejected.
    let (env, client, _admin) = setup();
    let pair = symbol_short!("XLM_USDC");

    client.set_adaptive_rate_limit(&Vec::new(&env), &true, &1000);

    // Populate histogram with bimodal distribution.
    for i in 0..20u64 {
        let w = Address::generate(&env);
        advance_to(&env, START_TS + i * DEFAULT_COOLDOWN_SECS);
        let score = if i % 2 == 0 { 0 } else { 100 };
        submit(&env, &client, &w, &pair, score).unwrap();
    }

    let wallet = Address::generate(&env);
    let t0 = START_TS + 20 * DEFAULT_COOLDOWN_SECS;
    advance_to(&env, t0);
    submit(&env, &client, &wallet, &pair, 50).unwrap();

    // Advance exactly 1× base cooldown — should still be blocked by the
    // adaptive (≈2×) cooldown.
    advance_to(&env, t0 + DEFAULT_COOLDOWN_SECS);
    let result = submit(&env, &client, &wallet, &pair, 60);
    assert_eq!(result, Err(Ok(Error::RateLimitExceeded)));

    // Advance past 2× base — should succeed.
    advance_to(&env, t0 + DEFAULT_COOLDOWN_SECS * 2 + 1);
    let result = submit(&env, &client, &wallet, &pair, 60);
    assert!(result.is_ok());
}

// ── Disabling adaptive mode restores base cooldown ────────────────────────────

#[test]
fn test_disable_adaptive_restores_base_cooldown() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_adaptive_rate_limit(&Vec::new(&env), &true, &1000);

    // Populate histogram with extreme scores.
    for i in 0..20u64 {
        let w = Address::generate(&env);
        advance_to(&env, START_TS + i * DEFAULT_COOLDOWN_SECS);
        let score = if i % 2 == 0 { 0 } else { 100 };
        submit(&env, &client, &w, &pair, score).unwrap();
    }

    // Confirm effective cooldown is elevated.
    assert!(client.get_effective_cooldown(&wallet, &pair) > DEFAULT_COOLDOWN_SECS);

    // Disable adaptive mode.
    client.set_adaptive_rate_limit(&Vec::new(&env), &false, &1000);
    assert_eq!(client.get_effective_cooldown(&wallet, &pair), DEFAULT_COOLDOWN_SECS);
}

// ── Partial variance levels ────────────────────────────────────────────────────

#[test]
fn test_moderate_variance_scale_gives_intermediate_cooldown() {
    // With moderate variance and scale=500, the effective cooldown should be
    // between 1× and 2× base.
    let (env, client, _admin) = setup();
    let pair = symbol_short!("XLM_USDC");

    client.set_adaptive_rate_limit(&Vec::new(&env), &true, &500);

    // Bimodal distribution to maximise variance.
    for i in 0..20u64 {
        let w = Address::generate(&env);
        advance_to(&env, START_TS + i * DEFAULT_COOLDOWN_SECS);
        let score = if i % 2 == 0 { 0 } else { 100 };
        submit(&env, &client, &w, &pair, score).unwrap();
    }

    let wallet = Address::generate(&env);
    let effective = client.get_effective_cooldown(&wallet, &pair);
    assert!(
        effective > DEFAULT_COOLDOWN_SECS && effective < DEFAULT_COOLDOWN_SECS * 2,
        "expected cooldown between base and 2×base, got {effective}"
    );
}
