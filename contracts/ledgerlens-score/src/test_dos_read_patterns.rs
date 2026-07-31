//! Resource denial-of-service tests for hostile read patterns.
//!
//! These tests exercise scenarios that a hostile caller could use to probe for
//! panics, unbounded work, or unexpected state mutations in the read-only
//! functions of `LedgerLensScoreContract`.  Each test proves that the contract
//! either:
//! - Returns a documented error (safe failure), or
//! - Returns a bounded result (bounded work).
//!
//! None of these tests should panic or hang.
//!
//! # Patterns covered
//!
//! | Test | Hostile pattern |
//! |------|----------------|
//! | `repeated_query_same_wallet` | 200 identical `get_score` calls |
//! | `query_unscored_wallets` | 100 distinct unscored wallets |
//! | `gate_boundary_thresholds` | threshold=0, threshold=u32::MAX, threshold=101 |
//! | `get_score_history_missing_wallet` | history for unscored wallet |
//! | `get_aggregate_score_no_pairs` | aggregate for wallet with no pairs |
//! | `get_expiring_entries_over_cap` | max_entries=200 (> 100 cap) |
//! | `get_score_count_never_scored` | score count for unknown wallet |
//! | `batch_read_mixed_wallets` | 20-entry batch with scored and unscored |
//! | `supports_interface_unknown_cap` | arbitrary unknown capability symbol |
//! | `get_pending_upgrade_when_none` | get_pending_upgrade with nothing pending |
//! | `gate_with_confidence_boundary` | min_confidence=u32::MAX, gate_threshold=0 |
//! | `get_global_min_confidence_default` | read before any admin sets it |
//! | `get_history_max_depth_default` | read before any admin changes it |
//! | `get_cooldown_default` | read before any admin changes it |
//! | `is_paused_default` | read without pausing |
//! | `get_paused_pairs_empty` | no pairs paused |

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
};

use crate::{
    constants::{BATCH_READ_MAX, MAX_EXPIRING_ENTRIES_PER_CALL},
    Error, LedgerLensScoreContract, LedgerLensScoreContractClient, ScoreQuery,
};

const START_TS: u64 = 1_700_000_000;

fn setup<'a>(env: &'a Env) -> LedgerLensScoreContractClient<'a> {
    env.mock_all_auths();
    env.budget().reset_unlimited();
    env.ledger().with_mut(|l| l.timestamp = START_TS);
    let id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(env, &id);
    let admin = Address::generate(env);
    let service = Address::generate(env);
    client.initialize(&admin, &service);
    client
}

fn submit(env: &Env, client: &LedgerLensScoreContractClient, wallet: &Address, score: u32) {
    client.submit_score(
        &Vec::new(env),
        wallet,
        &symbol_short!("XLM_USDC"),
        &score,
        &false,
        &false,
        &env.ledger().timestamp(),
        &90,
        &1,
        &None,
    );
}

// ── Pattern 1: Repeated queries on the same wallet ───────────────────────────

/// 200 consecutive `get_score` calls on the same (wallet, pair) must not
/// panic and must each return the same score value.  Proves that repeated
/// reads are idempotent and do not accumulate state.
#[test]
fn repeated_query_same_wallet_does_not_panic() {
    let env = Env::default();
    let client = setup(&env);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, 42);

    for _ in 0..200 {
        let score = client.get_score(&wallet, &pair);
        assert_eq!(score.score, 42);
    }
}

// ── Pattern 2: Missing scores for many distinct wallets ──────────────────────

/// 100 distinct wallets that have never been scored each return
/// `ScoreNotFound` from `try_get_score` and `false` from `query_risk_gate`.
/// No panic, no unexpected error.
#[test]
fn query_unscored_wallets_returns_not_found_for_all() {
    let env = Env::default();
    let client = setup(&env);
    let pair = symbol_short!("XLM_USDC");

    for _ in 0..100 {
        let wallet = Address::generate(&env);
        let score_result = client.try_get_score(&wallet, &pair);
        assert_eq!(score_result, Err(Ok(Error::ScoreNotFound)));

        let gate_result = client.query_risk_gate(&wallet, &pair, &75);
        assert!(!gate_result);
    }
}

// ── Pattern 3: Gate boundary thresholds ─────────────────────────────────────

/// Boundary thresholds that a hostile caller might try to exploit.
///
/// - threshold=0: impossible to pass (score must be < 0).
/// - threshold=u32::MAX: any scored wallet passes (score is bounded ≤ 100).
/// - threshold=101: any wallet with score ≤ 100 passes (scores are bounded).
#[test]
fn gate_boundary_thresholds_are_safe() {
    let env = Env::default();
    let client = setup(&env);
    let pair = symbol_short!("XLM_USDC");

    let unscored = Address::generate(&env);
    let scored = Address::generate(&env);
    submit(&env, &client, &scored, 0); // Lowest possible score

    // threshold=0: even score=0 cannot pass (need score < 0)
    assert!(!client.query_risk_gate(&scored, &pair, &0));
    assert!(!client.query_risk_gate(&unscored, &pair, &0));

    // threshold=u32::MAX: score=0 is well below MAX → passes; unscored fails closed
    assert!(client.query_risk_gate(&scored, &pair, &u32::MAX));
    assert!(!client.query_risk_gate(&unscored, &pair, &u32::MAX));

    // threshold=101: score=0 < 101 → passes; unscored still fails closed
    assert!(client.query_risk_gate(&scored, &pair, &101));
    assert!(!client.query_risk_gate(&unscored, &pair, &101));
}

// ── Pattern 4: History for missing wallet ────────────────────────────────────

/// `get_score_history` for a wallet that has never been scored returns an
/// empty `Vec` — it never panics.
#[test]
fn get_score_history_missing_wallet_returns_empty() {
    let env = Env::default();
    let client = setup(&env);
    let unknown = Address::generate(&env);

    let history = client.get_score_history(&unknown, &symbol_short!("XLM_USDC"));
    assert_eq!(history.len(), 0);
}

// ── Pattern 5: Aggregate score with no pairs ─────────────────────────────────

/// `get_aggregate_score` for a wallet with no scored pairs returns
/// `ScoreNotFound` — it never panics.
#[test]
fn get_aggregate_score_no_pairs_returns_not_found() {
    let env = Env::default();
    let client = setup(&env);
    let unknown = Address::generate(&env);

    let result = client.try_get_aggregate_score(&unknown);
    assert_eq!(result, Err(Ok(Error::ScoreNotFound)));
}

// ── Pattern 6: get_expiring_entries over cap ─────────────────────────────────

/// Requesting 200 expiring entries when the cap is 100 must not panic.
/// The result is capped at `MAX_EXPIRING_ENTRIES_PER_CALL` (100).
#[test]
fn get_expiring_entries_is_bounded_at_cap() {
    let env = Env::default();
    let client = setup(&env);

    // Request well above the cap — should be silently capped, not panic.
    let entries = client.get_expiring_entries(&200);
    assert!(
        entries.len() <= MAX_EXPIRING_ENTRIES_PER_CALL,
        "get_expiring_entries must return at most {} entries, got {}",
        MAX_EXPIRING_ENTRIES_PER_CALL,
        entries.len()
    );
}

// ── Pattern 7: Score count for never-scored wallet ───────────────────────────

/// `get_score_count` for a wallet that has never been scored returns 0 — it
/// never panics or returns an error.
#[test]
fn get_score_count_never_scored_returns_zero() {
    let env = Env::default();
    let client = setup(&env);
    let unknown = Address::generate(&env);

    assert_eq!(client.get_score_count(&unknown, &symbol_short!("XLM_USDC")), 0);
}

// ── Pattern 8: Batch read with mixed scored/unscored wallets ─────────────────

/// A batch of up to `BATCH_READ_MAX` (50) queries — half scored, half not —
/// must return results for every entry: `found=true` for scored wallets and
/// `found=false` for unscored ones.  No panic.
#[test]
fn batch_read_mixed_wallets_returns_per_entry_results() {
    let env = Env::default();
    let client = setup(&env);
    let pair = symbol_short!("XLM_USDC");

    let mut queries: Vec<ScoreQuery> = Vec::new(&env);
    let half = 10u32;

    // Half scored
    for i in 0..half {
        let wallet = Address::generate(&env);
        submit(&env, &client, &wallet, 30 + i);
        queries.push_back(ScoreQuery { wallet, asset_pair: pair.clone() });
        // Advance past cooldown for next submission
        env.ledger().with_mut(|l| l.timestamp += 3_601);
    }
    // Half unscored
    for _ in 0..half {
        let wallet = Address::generate(&env);
        queries.push_back(ScoreQuery { wallet, asset_pair: pair.clone() });
    }

    let results = client.get_scores_batch(&queries).unwrap();
    assert_eq!(results.len(), half * 2);

    let found_count = results.iter().filter(|r| r.found).count();
    let not_found_count = results.iter().filter(|r| !r.found).count();
    assert_eq!(found_count, half as usize);
    assert_eq!(not_found_count, half as usize);
}

/// `get_scores_batch` with more than `BATCH_READ_MAX` entries returns
/// `BatchTooLarge` — it never panics.
#[test]
fn batch_read_over_limit_returns_batch_too_large() {
    let env = Env::default();
    let client = setup(&env);
    let pair = symbol_short!("XLM_USDC");

    let mut queries: Vec<ScoreQuery> = Vec::new(&env);
    for _ in 0..(BATCH_READ_MAX + 1) {
        queries.push_back(ScoreQuery {
            wallet: Address::generate(&env),
            asset_pair: pair.clone(),
        });
    }

    let result = client.try_get_scores_batch(&queries);
    assert_eq!(result, Err(Ok(Error::BatchTooLarge)));
}

// ── Pattern 9: Unknown capability symbol ─────────────────────────────────────

/// `supports_interface` with an unknown symbol returns `false` — it never
/// panics or returns an error.
#[test]
fn supports_interface_unknown_capability_returns_false() {
    let env = Env::default();
    let client = setup(&env);

    let unknown_caps = ["doesntexist", "xyzzy", "NULL", "gate_v99"];
    for cap_str in unknown_caps {
        let cap = Symbol::new(&env, cap_str);
        assert!(
            !client.supports_interface(&cap),
            "unknown capability '{}' should return false",
            cap_str
        );
    }
}

// ── Pattern 10: get_pending_upgrade when none ────────────────────────────────

/// `get_pending_upgrade` when no upgrade is in flight returns
/// `NoPendingUpgrade` — not a panic.
#[test]
fn get_pending_upgrade_when_none_returns_error() {
    let env = Env::default();
    let client = setup(&env);

    let result = client.try_get_pending_upgrade();
    assert_eq!(result, Err(Ok(Error::NoPendingUpgrade)));
}

// ── Pattern 11: Confidence gate with extreme thresholds ─────────────────────

/// `query_risk_gate_with_confidence` with `min_confidence=u32::MAX` can never
/// be satisfied (confidence is bounded 0–100) — returns `false`, never panics.
#[test]
fn confidence_gate_min_confidence_u32_max_returns_false() {
    let env = Env::default();
    let client = setup(&env);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, 10); // Low-risk score, high confidence (90)

    // min_confidence=u32::MAX: no wallet can pass (confidence ≤ 100)
    assert!(!client.query_risk_gate_with_confidence(&wallet, &pair, &75, &u32::MAX));
}

/// `query_risk_gate_with_confidence` with `gate_threshold=0` never passes
/// regardless of confidence.
#[test]
fn confidence_gate_threshold_zero_never_passes() {
    let env = Env::default();
    let client = setup(&env);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, 0); // Lowest possible score

    // Even score=0 cannot pass threshold=0 (need score < 0)
    assert!(!client.query_risk_gate_with_confidence(&wallet, &pair, &0, &0));
}

// ── Pattern 12: Default read-only state before any admin action ──────────────

/// `get_global_min_confidence` returns 0 by default (no global floor).
#[test]
fn get_global_min_confidence_default_is_zero() {
    let env = Env::default();
    let client = setup(&env);
    assert_eq!(client.get_global_min_confidence(), 0);
}

/// `get_history_max_depth` returns the default (10) before the admin changes it.
#[test]
fn get_history_max_depth_default_is_ten() {
    let env = Env::default();
    let client = setup(&env);
    assert_eq!(client.get_history_max_depth(), 10);
}

/// `get_cooldown` returns the default (3600 seconds) before the admin changes it.
#[test]
fn get_cooldown_default_is_one_hour() {
    let env = Env::default();
    let client = setup(&env);
    assert_eq!(client.get_cooldown(), 3_600);
}

/// `is_paused` returns `false` before any `pause()` call.
#[test]
fn is_paused_default_is_false() {
    let env = Env::default();
    let client = setup(&env);
    assert!(!client.is_paused());
}

/// `get_paused_pairs` returns an empty vec before any pair is paused.
#[test]
fn get_paused_pairs_empty_by_default() {
    let env = Env::default();
    let client = setup(&env);
    let pairs = client.get_paused_pairs();
    assert_eq!(pairs.len(), 0);
}

/// `is_pair_paused` returns `false` for any unpaused pair.
#[test]
fn is_pair_paused_returns_false_for_any_unpaused_pair() {
    let env = Env::default();
    let client = setup(&env);
    assert!(!client.is_pair_paused(&symbol_short!("XLM_USDC")));
    assert!(!client.is_pair_paused(&symbol_short!("XLM_BTC")));
    assert!(!client.is_pair_paused(&symbol_short!("XLM_ETH")));
}
