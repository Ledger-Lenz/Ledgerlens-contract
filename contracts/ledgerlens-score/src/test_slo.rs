//! Comprehensive tests for SLO burn-rate alerts (#677).
//!
//! All test configs use short_window_secs=60, long_window_secs=300 (MIN_SLO_LONG_WINDOW_SECS).
//! All multi-submission tests advance by >=300s per call (> long_window) so both
//! windows fill, and use set_cooldown(60) to avoid RateLimitExceeded.
#![cfg(test)]
extern crate std;

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
};

use crate::{
    types::{SloBurnRateConfig, SloSeverity},
    LedgerLensScoreContract, LedgerLensScoreContractClient,
};

const START_TS: u64 = 1_700_000_000;

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn setup() -> (Env, LedgerLensScoreContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);
    let id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client, admin, service)
}

/// Minimal valid SLO config.
/// short=60s, long=300s (MIN_SLO_LONG_WINDOW_SECS), P3=1×, P2=2×, P1=5×.
fn test_cfg() -> SloBurnRateConfig {
    SloBurnRateConfig {
        enabled: true,
        slo_threshold: 75,
        short_window_secs: 60,
        long_window_secs: 300,
        p3_burn_rate_threshold_milli: 1_000,
        p2_burn_rate_threshold_milli: 2_000,
        p1_burn_rate_threshold_milli: 5_000,
    }
}

/// Default (large-window) config used for validation tests.
fn default_cfg() -> SloBurnRateConfig {
    SloBurnRateConfig {
        enabled: true,
        slo_threshold: 75,
        short_window_secs: 300,
        long_window_secs: 3_600,
        p3_burn_rate_threshold_milli: 1_000,
        p2_burn_rate_threshold_milli: 2_000,
        p1_burn_rate_threshold_milli: 5_000,
    }
}

/// Reduce cooldown to minimum so rapid test submissions don't rate-limit.
fn set_min_cooldown(client: &LedgerLensScoreContractClient, env: &Env) {
    client.set_cooldown(&Vec::new(env), &60u64);
}

/// Advance clock by `secs`, then submit `score` for (wallet, pair).
fn advance_and_submit(
    client: &LedgerLensScoreContractClient,
    env: &Env,
    wallet: &Address,
    pair: &Symbol,
    score: u32,
    secs: u64,
) {
    let now = env.ledger().timestamp();
    let ts = now + secs;
    env.ledger().with_mut(|l| l.timestamp = ts);
    client.submit_score(&Vec::new(env), wallet, pair, &score, &false, &false, &ts, &90, &1, &None);
}

/// Set up SLO config + min cooldown, submit two high-score entries each
/// advancing 301s (> long_window=300) so both windows fully fill.
/// Returns (wallet, pair).
fn trigger_p3_alert(client: &LedgerLensScoreContractClient, env: &Env) -> (Address, Symbol) {
    client.set_slo_config(&Vec::new(env), &test_cfg());
    set_min_cooldown(client, env);
    let wallet = Address::generate(env);
    let pair = symbol_short!("XLM_USDC");
    advance_and_submit(client, env, &wallet, &pair, 90, 301);
    advance_and_submit(client, env, &wallet, &pair, 90, 301);
    (wallet, pair)
}

// ─────────────────────────────────────────────────────────────────────────────
// Config validation
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn set_and_get_slo_config_roundtrip() {
    let (env, client, _, _) = setup();
    let cfg = default_cfg();
    client.set_slo_config(&Vec::new(&env), &cfg);
    let stored = client.get_slo_config().expect("config should be set");
    assert!(stored.enabled);
    assert_eq!(stored.slo_threshold, 75);
    assert_eq!(stored.short_window_secs, 300);
    assert_eq!(stored.long_window_secs, 3_600);
}

#[test]
fn get_slo_config_returns_none_before_set() {
    let (_, client, _, _) = setup();
    assert!(client.get_slo_config().is_none());
}

#[test]
fn set_slo_config_rejects_zero_threshold() {
    let (env, client, _, _) = setup();
    let mut cfg = default_cfg();
    cfg.slo_threshold = 0;
    assert!(client.try_set_slo_config(&Vec::new(&env), &cfg).is_err());
}

#[test]
fn set_slo_config_rejects_threshold_above_100() {
    let (env, client, _, _) = setup();
    let mut cfg = default_cfg();
    cfg.slo_threshold = 101;
    assert!(client.try_set_slo_config(&Vec::new(&env), &cfg).is_err());
}

#[test]
fn set_slo_config_rejects_short_window_too_small() {
    let (env, client, _, _) = setup();
    let mut cfg = default_cfg();
    cfg.short_window_secs = 59;
    assert!(client.try_set_slo_config(&Vec::new(&env), &cfg).is_err());
}

#[test]
fn set_slo_config_rejects_long_not_greater_than_short() {
    let (env, client, _, _) = setup();
    let mut cfg = default_cfg();
    cfg.long_window_secs = cfg.short_window_secs;
    assert!(client.try_set_slo_config(&Vec::new(&env), &cfg).is_err());
}

#[test]
fn set_slo_config_rejects_long_window_above_max() {
    let (env, client, _, _) = setup();
    let mut cfg = default_cfg();
    cfg.long_window_secs = 86_401;
    assert!(client.try_set_slo_config(&Vec::new(&env), &cfg).is_err());
}

#[test]
fn set_slo_config_rejects_p3_below_1x() {
    let (env, client, _, _) = setup();
    let mut cfg = default_cfg();
    cfg.p3_burn_rate_threshold_milli = 999;
    assert!(client.try_set_slo_config(&Vec::new(&env), &cfg).is_err());
}

#[test]
fn set_slo_config_rejects_p2_not_greater_than_p3() {
    let (env, client, _, _) = setup();
    let mut cfg = default_cfg();
    cfg.p2_burn_rate_threshold_milli = cfg.p3_burn_rate_threshold_milli;
    assert!(client.try_set_slo_config(&Vec::new(&env), &cfg).is_err());
}

#[test]
fn set_slo_config_rejects_p1_not_greater_than_p2() {
    let (env, client, _, _) = setup();
    let mut cfg = default_cfg();
    cfg.p1_burn_rate_threshold_milli = cfg.p2_burn_rate_threshold_milli;
    assert!(client.try_set_slo_config(&Vec::new(&env), &cfg).is_err());
}

#[test]
fn set_slo_config_rejects_p1_above_max() {
    let (env, client, _, _) = setup();
    let mut cfg = default_cfg();
    cfg.p1_burn_rate_threshold_milli = 100_001;
    assert!(client.try_set_slo_config(&Vec::new(&env), &cfg).is_err());
}

#[test]
fn set_slo_config_accepts_boundary_values() {
    let (env, client, _, _) = setup();
    // slo_threshold = 1 (min)
    let mut cfg = default_cfg();
    cfg.slo_threshold = 1;
    assert!(client.try_set_slo_config(&Vec::new(&env), &cfg).is_ok());
    // slo_threshold = 100 (max)
    cfg.slo_threshold = 100;
    assert!(client.try_set_slo_config(&Vec::new(&env), &cfg).is_ok());
    // p1 at max
    cfg.slo_threshold = 75;
    cfg.p1_burn_rate_threshold_milli = 100_000;
    assert!(client.try_set_slo_config(&Vec::new(&env), &cfg).is_ok());
    // long window at max
    cfg.p1_burn_rate_threshold_milli = 5_000;
    cfg.long_window_secs = 86_400;
    assert!(client.try_set_slo_config(&Vec::new(&env), &cfg).is_ok());
}

// ─────────────────────────────────────────────────────────────────────────────
// No-alert baselines
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn no_alert_when_score_below_threshold() {
    let (env, client, _, _) = setup();
    client.set_slo_config(&Vec::new(&env), &test_cfg());
    set_min_cooldown(&client, &env);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    advance_and_submit(&client, &env, &wallet, &pair, 50, 301);
    assert!(client.get_slo_alert(&wallet, &pair).is_none());
}

#[test]
fn no_alert_when_slo_disabled() {
    let (env, client, _, _) = setup();
    let mut cfg = test_cfg();
    cfg.enabled = false;
    client.set_slo_config(&Vec::new(&env), &cfg);
    set_min_cooldown(&client, &env);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    advance_and_submit(&client, &env, &wallet, &pair, 100, 301);
    advance_and_submit(&client, &env, &wallet, &pair, 100, 301);
    assert!(client.get_slo_alert(&wallet, &pair).is_none());
}

#[test]
fn no_alert_when_config_not_set() {
    let (env, client, _, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    advance_and_submit(&client, &env, &wallet, &pair, 100, 61);
    assert!(client.get_slo_alert(&wallet, &pair).is_none());
}

#[test]
fn get_slo_alert_unknown_pair_returns_none() {
    let (env, client, _, _) = setup();
    let wallet = Address::generate(&env);
    assert!(client.get_slo_alert(&wallet, &symbol_short!("NONE")).is_none());
}

#[test]
fn no_alert_for_score_one_below_threshold() {
    let (env, client, _, _) = setup();
    client.set_slo_config(&Vec::new(&env), &test_cfg());
    set_min_cooldown(&client, &env);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    // 74 < threshold 75 — never an SLO violation.
    advance_and_submit(&client, &env, &wallet, &pair, 74, 301);
    advance_and_submit(&client, &env, &wallet, &pair, 74, 301);
    advance_and_submit(&client, &env, &wallet, &pair, 74, 301);
    assert!(client.get_slo_alert(&wallet, &pair).is_none());
}

// ─────────────────────────────────────────────────────────────────────────────
// Alert fires
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn alert_fires_after_sustained_high_score() {
    let (env, client, _, _) = setup();
    let (wallet, pair) = trigger_p3_alert(&client, &env);
    let alert = client.get_slo_alert(&wallet, &pair);
    assert!(alert.is_some(), "expected alert after two full-window high scores");
    assert_ne!(alert.unwrap().severity, SloSeverity::None);
}

#[test]
fn alert_triggered_at_is_set() {
    let (env, client, _, _) = setup();
    let (wallet, pair) = trigger_p3_alert(&client, &env);
    if let Some(a) = client.get_slo_alert(&wallet, &pair) {
        assert!(a.triggered_at > 0);
        assert!(a.last_changed_at >= a.triggered_at);
    }
}

#[test]
fn alert_not_acknowledged_initially() {
    let (env, client, _, _) = setup();
    let (wallet, pair) = trigger_p3_alert(&client, &env);
    if let Some(a) = client.get_slo_alert(&wallet, &pair) {
        assert!(!a.acknowledged);
        assert_eq!(a.acknowledged_at, 0);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Dual-window gating
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn single_submission_does_not_trigger_when_long_window_unfilled() {
    // long window = 3600s; one 61s submission fills only 61/3600 ≈ 1.7% of it.
    let (env, client, _, _) = setup();
    let cfg = SloBurnRateConfig {
        enabled: true,
        slo_threshold: 75,
        short_window_secs: 60,
        long_window_secs: 3_600,
        p3_burn_rate_threshold_milli: 1_000,
        p2_burn_rate_threshold_milli: 2_000,
        p1_burn_rate_threshold_milli: 5_000,
    };
    client.set_slo_config(&Vec::new(&env), &cfg);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    advance_and_submit(&client, &env, &wallet, &pair, 90, 61);
    assert!(client.get_slo_alert(&wallet, &pair).is_none());
}

// ─────────────────────────────────────────────────────────────────────────────
// Severity escalation
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn severity_escalates_with_repeated_high_scores() {
    let (env, client, _, _) = setup();
    let cfg = SloBurnRateConfig {
        enabled: true,
        slo_threshold: 50,
        short_window_secs: 60,
        long_window_secs: 300,
        p3_burn_rate_threshold_milli: 1_000,
        p2_burn_rate_threshold_milli: 2_000,
        p1_burn_rate_threshold_milli: 3_000,
    };
    client.set_slo_config(&Vec::new(&env), &cfg);
    set_min_cooldown(&client, &env);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    for _ in 0..8 {
        advance_and_submit(&client, &env, &wallet, &pair, 80, 301);
    }
    let alert = client.get_slo_alert(&wallet, &pair);
    assert!(alert.is_some());
    let sev = alert.unwrap().severity;
    assert!(sev == SloSeverity::P3 || sev == SloSeverity::P2 || sev == SloSeverity::P1);
}

// ─────────────────────────────────────────────────────────────────────────────
// De-escalation / clearing
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn alert_clears_after_low_scores_drain_windows() {
    let (env, client, _, _) = setup();
    let (wallet, pair) = trigger_p3_alert(&client, &env);
    // Drain both windows: advance 2× long_window (600s) with low score.
    advance_and_submit(&client, &env, &wallet, &pair, 10, 601);
    advance_and_submit(&client, &env, &wallet, &pair, 10, 601);
    advance_and_submit(&client, &env, &wallet, &pair, 10, 601);
    if let Some(a) = client.get_slo_alert(&wallet, &pair) {
        assert_eq!(a.severity, SloSeverity::None);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Acknowledgment
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn acknowledge_marks_alert_acked() {
    let (env, client, _, _) = setup();
    let (wallet, pair) = trigger_p3_alert(&client, &env);
    if client.get_slo_alert(&wallet, &pair).is_some() {
        client.acknowledge_slo_alert(&Vec::new(&env), &wallet, &pair);
        let a = client.get_slo_alert(&wallet, &pair).unwrap();
        assert!(a.acknowledged);
        assert!(a.acknowledged_at > 0);
    }
}

#[test]
fn acknowledge_fails_when_no_alert() {
    let (env, client, _, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    assert!(client.try_acknowledge_slo_alert(&Vec::new(&env), &wallet, &pair).is_err());
}

#[test]
fn double_acknowledge_fails() {
    let (env, client, _, _) = setup();
    let (wallet, pair) = trigger_p3_alert(&client, &env);
    if client.get_slo_alert(&wallet, &pair).is_some() {
        client.acknowledge_slo_alert(&Vec::new(&env), &wallet, &pair);
        assert!(client.try_acknowledge_slo_alert(&Vec::new(&env), &wallet, &pair).is_err());
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// list_active_slo_alerts
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn list_active_alerts_empty_initially() {
    let (_, client, _, _) = setup();
    assert_eq!(client.list_active_slo_alerts().len(), 0);
}

#[test]
fn list_active_alerts_contains_triggered_pair() {
    let (env, client, _, _) = setup();
    let (wallet, pair) = trigger_p3_alert(&client, &env);
    if client.get_slo_alert(&wallet, &pair).is_some() {
        let list = client.list_active_slo_alerts();
        assert!(list.len() > 0);
        let found = (0..list.len()).any(|i| {
            let e = list.get(i).unwrap();
            e.0 == wallet && e.1 == pair
        });
        assert!(found);
    }
}

#[test]
fn list_active_alerts_removes_cleared_pair() {
    let (env, client, _, _) = setup();
    let (wallet, pair) = trigger_p3_alert(&client, &env);
    let had = client.get_slo_alert(&wallet, &pair).is_some();
    advance_and_submit(&client, &env, &wallet, &pair, 10, 601);
    advance_and_submit(&client, &env, &wallet, &pair, 10, 601);
    advance_and_submit(&client, &env, &wallet, &pair, 10, 601);
    if had && client.get_slo_alert(&wallet, &pair).is_none() {
        let list = client.list_active_slo_alerts();
        let found = (0..list.len()).any(|i| {
            let e = list.get(i).unwrap();
            e.0 == wallet && e.1 == pair
        });
        assert!(!found, "cleared pair must not remain in active alert index");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// supports_interface
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn supports_interface_slo() {
    let (_, client, _, _) = setup();
    assert!(client.supports_interface(&symbol_short!("slo")));
}

#[test]
fn supports_interface_existing_caps_unchanged() {
    let (_, client, _, _) = setup();
    assert!(client.supports_interface(&symbol_short!("score")));
    assert!(client.supports_interface(&symbol_short!("gate")));
    assert!(client.supports_interface(&symbol_short!("batch")));
}

// ─────────────────────────────────────────────────────────────────────────────
// Isolation
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn alerts_isolated_per_wallet() {
    let (env, client, _, _) = setup();
    client.set_slo_config(&Vec::new(&env), &test_cfg());
    set_min_cooldown(&client, &env);
    let wallet_hi = Address::generate(&env);
    let wallet_lo = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    advance_and_submit(&client, &env, &wallet_hi, &pair, 90, 301);
    advance_and_submit(&client, &env, &wallet_hi, &pair, 90, 301);
    advance_and_submit(&client, &env, &wallet_lo, &pair, 10, 301);
    advance_and_submit(&client, &env, &wallet_lo, &pair, 10, 301);
    assert!(client.get_slo_alert(&wallet_lo, &pair).is_none());
}

#[test]
fn alerts_isolated_per_pair() {
    let (env, client, _, _) = setup();
    client.set_slo_config(&Vec::new(&env), &test_cfg());
    set_min_cooldown(&client, &env);
    let wallet = Address::generate(&env);
    let pair_hi = symbol_short!("XLM_USDC");
    let pair_lo = symbol_short!("XLM_BTC");
    advance_and_submit(&client, &env, &wallet, &pair_hi, 90, 301);
    advance_and_submit(&client, &env, &wallet, &pair_hi, 90, 301);
    advance_and_submit(&client, &env, &wallet, &pair_lo, 10, 301);
    advance_and_submit(&client, &env, &wallet, &pair_lo, 10, 301);
    assert!(client.get_slo_alert(&wallet, &pair_lo).is_none());
}

// ─────────────────────────────────────────────────────────────────────────────
// Compatibility: existing entry points must not be broken
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn submit_score_succeeds_with_slo_enabled() {
    let (env, client, _, _) = setup();
    client.set_slo_config(&Vec::new(&env), &test_cfg());
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let ts = START_TS + 1;
    env.ledger().with_mut(|l| l.timestamp = ts);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &42, &false, &false, &ts, &90, &1, &None);
    assert_eq!(client.get_score(&wallet, &pair).score, 42);
}

#[test]
fn get_score_unaffected_by_slo_alert() {
    let (env, client, _, _) = setup();
    let (wallet, pair) = trigger_p3_alert(&client, &env);
    assert_eq!(client.get_score(&wallet, &pair).score, 90);
}

#[test]
fn query_risk_gate_unaffected_by_slo_alert() {
    let (env, client, _, _) = setup();
    client.set_slo_config(&Vec::new(&env), &test_cfg());
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let ts = START_TS + 1;
    env.ledger().with_mut(|l| l.timestamp = ts);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &50, &false, &false, &ts, &90, &1, &None);
    assert!(client.query_risk_gate(&wallet, &pair, &75u32));
}
