//! SLO Burn-Rate Alert Engine — Issue #677
//!
//! # Design
//!
//! ## Problem
//! A per-(wallet, asset_pair) risk score that stays elevated over time
//! is more dangerous than a momentary spike. Classic threshold breaches
//! (`threshold_breached` event) fire once per submission; they cannot
//! distinguish a sustained high-risk state from a brief anomaly.
//!
//! ## Solution: Dual-Window Burn Rate
//! Inspired by Google SRE's error-budget burn-rate model (Beyer et al.,
//! "Site Reliability Engineering", Chapter 5).
//!
//! Two rolling time windows are maintained per `(wallet, asset_pair)`:
//! - **Short window** (default 5 min) — detects fast burns (P1 alerts)
//! - **Long window** (default 60 min) — detects slow creep (P2/P3 alerts)
//!
//! On every score write the engine:
//! 1. Accumulates `elapsed_secs` of "above-SLO-threshold" time into
//!    both windows using an exponential-decay approximation that avoids
//!    storing a full history ring.
//! 2. Computes `burn_rate = above_threshold_secs / window_secs` (×1000 for
//!    integer milli-units).
//! 3. Evaluates severity by comparing both windows to the configured
//!    P3/P2/P1 thresholds — an alert fires only when *both* windows exceed
//!    the threshold for that tier.
//! 4. Transitions the alert state machine deterministically:
//!    - `None → P3 → P2 → P1` on escalation
//!    - `P1 → P2 → P3 → None` on de-escalation (hysteresis not required
//!      because dual-window already prevents flapping)
//!
//! ## Invariants
//! - **Read-only paths never write**: `evaluate_slo` is only called from
//!   `write_score_with_rate_limit`, which is already a state-mutating path.
//! - **Bounded work**: all loops are O(1); no dynamic allocation beyond the
//!   two window structs stored per pair.
//! - **Deterministic ordering**: severity is derived solely from
//!   `env.ledger().timestamp()` and the stored window accumulators — no
//!   randomness, no external calls.
//! - **Fail-safe defaults**: if no SLO config exists, or if the feature is
//!   disabled, `evaluate_slo` returns immediately without touching storage.
//! - **Checked arithmetic**: all multiplications/additions use `saturating_*`
//!   to prevent overflow panics (Soroban contracts must not panic in prod).

use crate::{constants, events, storage, types};
use soroban_sdk::{Address, Env, Symbol};

// ── Public entry point ───────────────────────────────────────────────────────

/// Called from `write_score_with_rate_limit` (and `commit_pending_score`)
/// after a score has been committed to live storage.
///
/// Updates the burn-rate accumulators for both windows, computes the new
/// severity, and transitions the alert state machine (emitting events as
/// needed). This is a pure side-effect write — it never returns an error
/// (failures are absorbed so as not to block score writes).
pub fn evaluate_slo(env: &Env, wallet: &Address, asset_pair: &Symbol, new_score: u32) {
    let config = match storage::get_slo_config(env) {
        Some(c) => c,
        None => return, // SLO not configured
    };
    if !config.enabled {
        return;
    }

    let now = env.ledger().timestamp();

    // ── Update window accumulators ─────────────────────────────────────────
    let prev_state = storage::get_slo_window_state(env, wallet, asset_pair);
    let new_state = advance_windows(&config, prev_state, now, new_score);
    storage::set_slo_window_state(env, wallet, asset_pair, &new_state);

    // ── Compute burn rates ×1000 ──────────────────────────────────────────
    let short_burn = burn_rate_milli(&new_state.short);
    let long_burn = burn_rate_milli(&new_state.long);

    // ── Determine new severity ─────────────────────────────────────────────
    let new_severity = compute_severity(&config, short_burn, long_burn);

    // ── Transition state machine ───────────────────────────────────────────
    let prev_alert = storage::get_slo_alert_state(env, wallet, asset_pair);
    let prev_severity = prev_alert.as_ref().map(|a| a.severity).unwrap_or(types::SloSeverity::None);

    if new_severity == prev_severity {
        // No change — still update burn rates in the stored alert so that
        // operators can read the current values even when severity is stable.
        if let Some(mut alert) = prev_alert {
            alert.short_burn_rate_milli = short_burn;
            alert.long_burn_rate_milli = long_burn;
            storage::set_slo_alert_state(env, wallet, asset_pair, &alert);
        }
        return;
    }

    match (prev_severity, new_severity) {
        // ── Escalation paths ────────────────────────────────────────────────
        (types::SloSeverity::None, types::SloSeverity::P3)
        | (types::SloSeverity::None, types::SloSeverity::P2)
        | (types::SloSeverity::None, types::SloSeverity::P1)
        | (types::SloSeverity::P3, types::SloSeverity::P2)
        | (types::SloSeverity::P3, types::SloSeverity::P1)
        | (types::SloSeverity::P2, types::SloSeverity::P1) => {
            let is_new = prev_severity == types::SloSeverity::None;
            let alert = types::SloAlert {
                severity: new_severity,
                triggered_at: if is_new {
                    now
                } else {
                    prev_alert.as_ref().map(|a| a.triggered_at).unwrap_or(now)
                },
                last_changed_at: now,
                acknowledged: false,
                acknowledged_at: 0,
                short_burn_rate_milli: short_burn,
                long_burn_rate_milli: long_burn,
            };
            storage::set_slo_alert_state(env, wallet, asset_pair, &alert);
            storage::slo_index_add(env, wallet, asset_pair);

            if is_new {
                events::slo_alert(
                    env,
                    wallet,
                    asset_pair,
                    new_severity as u32,
                    short_burn,
                    long_burn,
                );
            } else {
                events::slo_escalate(
                    env,
                    wallet,
                    asset_pair,
                    prev_severity as u32,
                    new_severity as u32,
                    short_burn,
                    long_burn,
                );
            }
        }

        // ── De-escalation paths ─────────────────────────────────────────────
        (types::SloSeverity::P1, types::SloSeverity::P2)
        | (types::SloSeverity::P1, types::SloSeverity::P3)
        | (types::SloSeverity::P1, types::SloSeverity::None)
        | (types::SloSeverity::P2, types::SloSeverity::P3)
        | (types::SloSeverity::P2, types::SloSeverity::None)
        | (types::SloSeverity::P3, types::SloSeverity::None) => {
            if new_severity == types::SloSeverity::None {
                // Alert clears: remove from index and persistent state.
                storage::clear_slo_alert_state(env, wallet, asset_pair);
                storage::slo_index_remove(env, wallet, asset_pair);
            } else {
                let alert = types::SloAlert {
                    severity: new_severity,
                    triggered_at: prev_alert.as_ref().map(|a| a.triggered_at).unwrap_or(now),
                    last_changed_at: now,
                    acknowledged: false, // reset ack on severity change
                    acknowledged_at: 0,
                    short_burn_rate_milli: short_burn,
                    long_burn_rate_milli: long_burn,
                };
                storage::set_slo_alert_state(env, wallet, asset_pair, &alert);
                // still in index
            }
            events::slo_deescalate(
                env,
                wallet,
                asset_pair,
                prev_severity as u32,
                new_severity as u32,
                short_burn,
                long_burn,
            );
        }

        // This arm is unreachable but required for exhaustiveness.
        _ => {}
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Advance both window accumulators given a new score submission at `now`.
///
/// ### Algorithm (per window of duration `W` seconds):
///
/// Let `elapsed = now - last_updated` (clamped to `W`).
///
/// If the *previous* score was ≥ slo_threshold, those `elapsed` seconds
/// are added to `above_threshold_secs_scaled`. Then we apply an exponential
/// decay to evict contributions older than the window:
///
/// ```text
/// above_scaled_new = above_scaled_old × (1 - elapsed/W)  [integer approx]
///                  + (new_contribution × SLO_BURN_SCALE)
/// ```
///
/// The decay factor `(1 - elapsed/W)` is computed in integer arithmetic as
/// `(window_secs - elapsed) / window_secs`, multiplied before division to
/// preserve precision.
///
/// This produces a sliding-window estimate that:
/// - Never requires iterating over a history buffer (O(1)).
/// - Automatically drains to zero if no "above-threshold" submissions occur.
/// - Over-counts slightly at window boundaries (conservative — safer to alert
///   than to miss).
fn advance_windows(
    config: &types::SloBurnRateConfig,
    prev: Option<types::SloWindowState>,
    now: u64,
    new_score: u32,
) -> types::SloWindowState {
    let (prev_short, prev_long) = match prev {
        Some(s) => (s.short, s.long),
        None => (
            types::SloWindow {
                window_secs: config.short_window_secs,
                above_threshold_secs_scaled: 0,
                last_updated: now,
            },
            types::SloWindow {
                window_secs: config.long_window_secs,
                above_threshold_secs_scaled: 0,
                last_updated: now,
            },
        ),
    };

    types::SloWindowState {
        short: advance_window(config, prev_short, now, new_score),
        long: advance_window(config, prev_long, now, new_score),
    }
}

fn advance_window(
    config: &types::SloBurnRateConfig,
    prev: types::SloWindow,
    now: u64,
    new_score: u32,
) -> types::SloWindow {
    let window_secs = prev.window_secs;
    // Elapsed since last update, clamped to [0, window_secs].
    let elapsed = now.saturating_sub(prev.last_updated).min(window_secs);

    // Decay the accumulated "above threshold" seconds.
    // decay_factor_num / decay_factor_den = (window - elapsed) / window
    let decay_num = window_secs.saturating_sub(elapsed);
    let decayed = if window_secs == 0 {
        0u64
    } else {
        prev.above_threshold_secs_scaled
            .saturating_mul(decay_num)
            .checked_div(window_secs)
            .unwrap_or(0)
    };

    // Add new contribution: if the new score is ≥ slo_threshold, the
    // elapsed seconds count as "above threshold".
    let new_contribution = if new_score >= config.slo_threshold {
        elapsed.saturating_mul(constants::SLO_BURN_SCALE)
    } else {
        0u64
    };

    types::SloWindow {
        window_secs,
        above_threshold_secs_scaled: decayed.saturating_add(new_contribution),
        last_updated: now,
    }
}

/// Compute the burn rate (×1000) for a window.
///
/// `burn_rate_milli = (above_threshold_secs_scaled / SLO_BURN_SCALE) / window_secs × 1000`
///
/// Integer form (to avoid division by zero and float):
/// `= above_threshold_secs_scaled × 1000 / (SLO_BURN_SCALE × window_secs)`
fn burn_rate_milli(window: &types::SloWindow) -> u32 {
    if window.window_secs == 0 {
        return 0;
    }
    let denominator = constants::SLO_BURN_SCALE.saturating_mul(window.window_secs);
    if denominator == 0 {
        return 0;
    }
    let numer = window.above_threshold_secs_scaled.saturating_mul(1_000);
    // Saturate at u32::MAX (100 000× would be ~100_000_000 milli but that's
    // still within u64 — capping at u32::MAX is safe for comparison purposes).
    numer.checked_div(denominator).unwrap_or(0).min(u32::MAX as u64) as u32
}

/// Determine the SLO severity from short and long burn rates (×1000).
///
/// Both windows must independently exceed the threshold for a given tier.
/// This prevents false positives from momentary spikes (short window)
/// or from a slowly creeping window that hasn't yet affected the short window.
fn compute_severity(
    config: &types::SloBurnRateConfig,
    short_burn: u32,
    long_burn: u32,
) -> types::SloSeverity {
    if short_burn >= config.p1_burn_rate_threshold_milli
        && long_burn >= config.p1_burn_rate_threshold_milli
    {
        types::SloSeverity::P1
    } else if short_burn >= config.p2_burn_rate_threshold_milli
        && long_burn >= config.p2_burn_rate_threshold_milli
    {
        types::SloSeverity::P2
    } else if short_burn >= config.p3_burn_rate_threshold_milli
        && long_burn >= config.p3_burn_rate_threshold_milli
    {
        types::SloSeverity::P3
    } else {
        types::SloSeverity::None
    }
}

// ── Validation ────────────────────────────────────────────────────────────────

/// Validate a `SloBurnRateConfig` supplied by the admin.
///
/// Returns `Err(Error::InvalidSloConfig)` for any out-of-range combination.
pub fn validate_slo_config(config: &types::SloBurnRateConfig) -> Result<(), crate::errors::Error> {
    // slo_threshold must be 1..=100
    if config.slo_threshold == 0 || config.slo_threshold > 100 {
        return Err(crate::errors::Error::InvalidSloConfig);
    }
    // Window bounds
    if config.short_window_secs < constants::MIN_SLO_SHORT_WINDOW_SECS {
        return Err(crate::errors::Error::InvalidSloConfig);
    }
    if config.long_window_secs < constants::MIN_SLO_LONG_WINDOW_SECS {
        return Err(crate::errors::Error::InvalidSloConfig);
    }
    if config.long_window_secs > constants::MAX_SLO_LONG_WINDOW_SECS {
        return Err(crate::errors::Error::InvalidSloConfig);
    }
    // long window must be strictly greater than short window
    if config.long_window_secs <= config.short_window_secs {
        return Err(crate::errors::Error::InvalidSloConfig);
    }
    // Burn rate thresholds must be ordered: p3 < p2 < p1 and p3 >= 1000 (1×)
    if config.p3_burn_rate_threshold_milli < 1_000 {
        return Err(crate::errors::Error::InvalidSloConfig);
    }
    if config.p2_burn_rate_threshold_milli <= config.p3_burn_rate_threshold_milli {
        return Err(crate::errors::Error::InvalidSloConfig);
    }
    if config.p1_burn_rate_threshold_milli <= config.p2_burn_rate_threshold_milli {
        return Err(crate::errors::Error::InvalidSloConfig);
    }
    if config.p1_burn_rate_threshold_milli > constants::MAX_SLO_P1_BURN_RATE_MILLI {
        return Err(crate::errors::Error::InvalidSloConfig);
    }
    Ok(())
}

// ── Unit tests for pure functions (no Env dependency) ────────────────────────

#[cfg(test)]
mod unit_tests {
    extern crate std;
    use super::*;

    fn default_config() -> types::SloBurnRateConfig {
        types::SloBurnRateConfig {
            enabled: true,
            slo_threshold: 75,
            short_window_secs: constants::DEFAULT_SLO_SHORT_WINDOW_SECS,
            long_window_secs: constants::DEFAULT_SLO_LONG_WINDOW_SECS,
            p3_burn_rate_threshold_milli: constants::DEFAULT_SLO_P3_BURN_RATE_MILLI,
            p2_burn_rate_threshold_milli: constants::DEFAULT_SLO_P2_BURN_RATE_MILLI,
            p1_burn_rate_threshold_milli: constants::DEFAULT_SLO_P1_BURN_RATE_MILLI,
        }
    }

    // ── burn_rate_milli ──────────────────────────────────────────────────────

    #[test]
    fn burn_rate_zero_when_no_above_threshold() {
        let w =
            types::SloWindow { window_secs: 300, above_threshold_secs_scaled: 0, last_updated: 0 };
        assert_eq!(burn_rate_milli(&w), 0);
    }

    #[test]
    fn burn_rate_1000_when_fully_above_threshold() {
        // Fully burned window: above_threshold_secs = window_secs
        let window_secs = 300u64;
        let w = types::SloWindow {
            window_secs,
            above_threshold_secs_scaled: window_secs * constants::SLO_BURN_SCALE,
            last_updated: 0,
        };
        assert_eq!(burn_rate_milli(&w), 1_000);
    }

    #[test]
    fn burn_rate_2000_when_double_above_threshold() {
        let window_secs = 300u64;
        let w = types::SloWindow {
            window_secs,
            above_threshold_secs_scaled: 2 * window_secs * constants::SLO_BURN_SCALE,
            last_updated: 0,
        };
        assert_eq!(burn_rate_milli(&w), 2_000);
    }

    #[test]
    fn burn_rate_zero_for_zero_window() {
        let w =
            types::SloWindow { window_secs: 0, above_threshold_secs_scaled: 9999, last_updated: 0 };
        assert_eq!(burn_rate_milli(&w), 0);
    }

    // ── compute_severity ────────────────────────────────────────────────────

    #[test]
    fn severity_none_when_below_p3() {
        let cfg = default_config();
        assert_eq!(compute_severity(&cfg, 999, 999), types::SloSeverity::None);
    }

    #[test]
    fn severity_p3_when_both_windows_at_1x() {
        let cfg = default_config();
        assert_eq!(compute_severity(&cfg, 1_000, 1_000), types::SloSeverity::P3);
    }

    #[test]
    fn severity_p3_not_triggered_if_only_short_window_fires() {
        let cfg = default_config();
        // short fires at 1× but long is below
        assert_eq!(compute_severity(&cfg, 1_000, 999), types::SloSeverity::None);
    }

    #[test]
    fn severity_p2_when_both_windows_at_2x() {
        let cfg = default_config();
        assert_eq!(compute_severity(&cfg, 2_000, 2_000), types::SloSeverity::P2);
    }

    #[test]
    fn severity_p2_not_triggered_if_only_long_window_fires() {
        let cfg = default_config();
        assert_eq!(compute_severity(&cfg, 1_500, 2_000), types::SloSeverity::P3);
    }

    #[test]
    fn severity_p1_when_both_windows_at_5x() {
        let cfg = default_config();
        assert_eq!(compute_severity(&cfg, 5_000, 5_000), types::SloSeverity::P1);
    }

    #[test]
    fn severity_p1_requires_both_windows() {
        let cfg = default_config();
        assert_eq!(compute_severity(&cfg, 5_000, 4_999), types::SloSeverity::P2);
    }

    // ── validate_slo_config ─────────────────────────────────────────────────

    #[test]
    fn validate_accepts_valid_config() {
        assert!(validate_slo_config(&default_config()).is_ok());
    }

    #[test]
    fn validate_rejects_zero_slo_threshold() {
        let mut cfg = default_config();
        cfg.slo_threshold = 0;
        assert!(validate_slo_config(&cfg).is_err());
    }

    #[test]
    fn validate_rejects_slo_threshold_above_100() {
        let mut cfg = default_config();
        cfg.slo_threshold = 101;
        assert!(validate_slo_config(&cfg).is_err());
    }

    #[test]
    fn validate_rejects_short_window_too_small() {
        let mut cfg = default_config();
        cfg.short_window_secs = 59;
        assert!(validate_slo_config(&cfg).is_err());
    }

    #[test]
    fn validate_rejects_long_window_not_greater_than_short() {
        let mut cfg = default_config();
        cfg.short_window_secs = 300;
        cfg.long_window_secs = 300;
        assert!(validate_slo_config(&cfg).is_err());
    }

    #[test]
    fn validate_rejects_long_window_too_large() {
        let mut cfg = default_config();
        cfg.long_window_secs = constants::MAX_SLO_LONG_WINDOW_SECS + 1;
        assert!(validate_slo_config(&cfg).is_err());
    }

    #[test]
    fn validate_rejects_p3_below_1x() {
        let mut cfg = default_config();
        cfg.p3_burn_rate_threshold_milli = 999;
        assert!(validate_slo_config(&cfg).is_err());
    }

    #[test]
    fn validate_rejects_misordered_thresholds() {
        let mut cfg = default_config();
        cfg.p2_burn_rate_threshold_milli = cfg.p3_burn_rate_threshold_milli;
        assert!(validate_slo_config(&cfg).is_err());
    }

    #[test]
    fn validate_rejects_p1_above_max() {
        let mut cfg = default_config();
        cfg.p1_burn_rate_threshold_milli = constants::MAX_SLO_P1_BURN_RATE_MILLI + 1;
        assert!(validate_slo_config(&cfg).is_err());
    }

    // ── advance_window ───────────────────────────────────────────────────────

    #[test]
    fn advance_window_accumulates_elapsed_when_above_threshold() {
        let cfg = default_config();
        // Start with empty window at t=0.
        let initial = types::SloWindow {
            window_secs: cfg.short_window_secs,
            above_threshold_secs_scaled: 0,
            last_updated: 0,
        };
        // Submit score = 80 (≥ threshold 75) at t=100.
        let result = advance_window(&cfg, initial, 100, 80);
        // Expected: 100 seconds × SLO_BURN_SCALE (decay is ~(300-100)/300 ≈ 2/3)
        // The new_contribution = 100 × SLO_BURN_SCALE = 100_000_000.
        assert!(result.above_threshold_secs_scaled > 0);
        assert_eq!(result.last_updated, 100);
    }

    #[test]
    fn advance_window_does_not_accumulate_when_below_threshold() {
        let cfg = default_config();
        let initial = types::SloWindow {
            window_secs: cfg.short_window_secs,
            above_threshold_secs_scaled: constants::SLO_BURN_SCALE * 100, // 100 accumulated secs
            last_updated: 0,
        };
        // Submit score = 50 (< threshold 75) at t=100.
        let result = advance_window(&cfg, initial.clone(), 100, 50);
        // Should decay existing amount but add nothing new.
        let decayed =
            initial.above_threshold_secs_scaled.saturating_mul(200).checked_div(300).unwrap_or(0);
        assert_eq!(result.above_threshold_secs_scaled, decayed);
        assert_eq!(result.last_updated, 100);
    }

    #[test]
    fn advance_window_drains_fully_after_one_window_period() {
        let cfg = default_config();
        let initial = types::SloWindow {
            window_secs: cfg.short_window_secs,
            above_threshold_secs_scaled: constants::SLO_BURN_SCALE * 300, // full window
            last_updated: 0,
        };
        // Submit score = 0 (below threshold) at t = window_secs.
        let result = advance_window(&cfg, initial, cfg.short_window_secs, 0);
        // Decay factor = (300 - 300) / 300 = 0.
        assert_eq!(result.above_threshold_secs_scaled, 0);
    }

    #[test]
    fn advance_window_clamps_elapsed_to_window() {
        let cfg = default_config();
        let initial = types::SloWindow {
            window_secs: cfg.short_window_secs,
            above_threshold_secs_scaled: 0,
            last_updated: 0,
        };
        // Elapsed = 1000 > window_secs = 300: should clamp to 300.
        let result = advance_window(&cfg, initial, 1000, 80);
        // Contribution = 300 × SLO_BURN_SCALE (clamped), decay = 0.
        assert_eq!(result.above_threshold_secs_scaled, 300 * constants::SLO_BURN_SCALE);
    }

    #[test]
    fn advance_window_saturates_without_overflow() {
        let cfg = default_config();
        // Start with maximum possible accumulator.
        let initial = types::SloWindow {
            window_secs: cfg.short_window_secs,
            above_threshold_secs_scaled: u64::MAX,
            last_updated: 0,
        };
        // Should not panic.
        let _ = advance_window(&cfg, initial, 100, 80);
    }
}
