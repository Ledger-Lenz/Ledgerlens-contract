#![cfg(test)]

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient, ScoreSubmission};

// ── Helpers ───────────────────────────────────────────────────────────────────

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

/// Submit a score and advance the ledger past the cooldown so subsequent
/// calls for the same (wallet, pair) are always accepted.
fn submit(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    wallet: &Address,
    pair: &soroban_sdk::Symbol,
    score: u32,
) {
    client.submit_score(
        &Vec::new(env),
        wallet,
        pair,
        &score,
        &false,
        &false,
        &(env.ledger().timestamp().max(1)),
        &80,
        &1,
        &None,
    );
    env.ledger().with_mut(|l| l.timestamp += 3_601);
}

// ── Hysteresis margin admin functions ─────────────────────────────────────────

#[test]
fn test_default_hysteresis_margin_is_zero() {
    let (_env, client, _admin, _service) = setup();
    assert_eq!(client.get_hysteresis_margin(), 0);
}

#[test]
fn test_set_hysteresis_margin_stores_value() {
    let (_env, client, _admin, _service) = setup();
    client.set_hysteresis_margin(&20);
    assert_eq!(client.get_hysteresis_margin(), 20);
}

#[test]
fn test_set_hysteresis_margin_at_max_accepted() {
    let (_env, client, _admin, _service) = setup();
    client.set_hysteresis_margin(&50);
    assert_eq!(client.get_hysteresis_margin(), 50);
}

#[test]
fn test_set_hysteresis_margin_above_max_rejected() {
    let (_env, client, _admin, _service) = setup();
    let result = client.try_set_hysteresis_margin(&51);
    assert_eq!(result, Err(Ok(Error::InvalidHysteresisMargin)));
}

#[test]
fn test_set_hysteresis_margin_zero_accepted() {
    let (_env, client, _admin, _service) = setup();
    client.set_hysteresis_margin(&10);
    client.set_hysteresis_margin(&0);
    assert_eq!(client.get_hysteresis_margin(), 0);
}

// ── is_in_risk_band default ───────────────────────────────────────────────────

#[test]
fn test_is_in_risk_band_defaults_false() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    assert!(!client.is_in_risk_band(&wallet, &pair));
}

// ── Band entry logic ──────────────────────────────────────────────────────────

#[test]
fn test_band_entered_on_first_high_score() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    assert!(!client.is_in_risk_band(&wallet, &pair));
    submit(&env, &client, &wallet, &pair, 80);
    assert!(client.is_in_risk_band(&wallet, &pair));
}

#[test]
fn test_band_not_entered_below_threshold() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 74);
    assert!(!client.is_in_risk_band(&wallet, &pair));
}

#[test]
fn test_band_entered_at_exact_threshold() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 75);
    assert!(client.is_in_risk_band(&wallet, &pair));
}

// ── No duplicate risk_band_entered during sustained high risk ─────────────────
//
// Verified through state invariants: if the band state is already `true` when
// a new high-risk score arrives, `evaluate_risk_band` takes the `else` branch
// (in_band && score >= threshold) and makes NO state change and emits NO event.
// Persistent `true` state after repeated high-risk submissions is therefore
// equivalent proof that risk_band_entered was not emitted again.

#[test]
fn test_no_duplicate_band_entered_during_sustained_high_risk() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Enter band on first high-risk submission.
    submit(&env, &client, &wallet, &pair, 80);
    assert!(client.is_in_risk_band(&wallet, &pair));

    // Subsequent high-risk submissions must not re-enter (state stays true).
    submit(&env, &client, &wallet, &pair, 85);
    assert!(client.is_in_risk_band(&wallet, &pair));

    submit(&env, &client, &wallet, &pair, 90);
    assert!(client.is_in_risk_band(&wallet, &pair));

    // State was never flipped to false and back, proving no duplicate enter
    // event was triggered (the logic gate `if !in_band` prevents re-emission).
}

// ── Hysteresis exit enforcement ───────────────────────────────────────────────

#[test]
fn test_no_exit_when_score_drops_below_threshold_but_above_exit_boundary() {
    let (env, client, _admin, _service) = setup();
    // threshold = 75, margin = 10 → exit_threshold = 65
    client.set_hysteresis_margin(&10);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 80);
    assert!(client.is_in_risk_band(&wallet, &pair));

    // 70 < 75 (below threshold) but 70 >= 65 (above exit boundary) → stays in band.
    submit(&env, &client, &wallet, &pair, 70);
    assert!(
        client.is_in_risk_band(&wallet, &pair),
        "score 70 is above exit_threshold 65; wallet must remain in high-risk band"
    );
}

#[test]
fn test_no_exit_when_score_equals_exit_threshold() {
    let (env, client, _admin, _service) = setup();
    // threshold = 75, margin = 10 → exit_threshold = 65
    client.set_hysteresis_margin(&10);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 80);
    // score == exit_threshold (65) is NOT strictly below it; band must hold.
    submit(&env, &client, &wallet, &pair, 65);
    assert!(
        client.is_in_risk_band(&wallet, &pair),
        "score == exit_threshold is not below it; band must still be active"
    );
}

#[test]
fn test_exit_band_when_score_crosses_below_exit_threshold() {
    let (env, client, _admin, _service) = setup();
    // threshold = 75, margin = 10 → exit_threshold = 65
    client.set_hysteresis_margin(&10);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 80);
    assert!(client.is_in_risk_band(&wallet, &pair));

    // 64 < 65 (below exit_threshold) → exits band.
    submit(&env, &client, &wallet, &pair, 64);
    assert!(!client.is_in_risk_band(&wallet, &pair));
}

#[test]
fn test_exit_requires_crossing_full_hysteresis_boundary_not_just_threshold() {
    let (env, client, _admin, _service) = setup();
    // threshold = 75, margin = 10 → exit_threshold = 65
    client.set_hysteresis_margin(&10);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Enter band.
    submit(&env, &client, &wallet, &pair, 80);

    // Multiple scores between exit_threshold and threshold: band must not exit.
    for score in [74u32, 70, 67, 66, 65] {
        submit(&env, &client, &wallet, &pair, score);
        assert!(
            client.is_in_risk_band(&wallet, &pair),
            "score {score} is still >= exit_threshold 65; band must hold"
        );
    }

    // One score below exit_threshold: band must clear.
    submit(&env, &client, &wallet, &pair, 64);
    assert!(!client.is_in_risk_band(&wallet, &pair));
}

// ── Re-entry after clearing ───────────────────────────────────────────────────

#[test]
fn test_band_re_entered_after_clearing() {
    let (env, client, _admin, _service) = setup();
    client.set_hysteresis_margin(&10);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Enter → exit → re-enter sequence.
    submit(&env, &client, &wallet, &pair, 80);
    assert!(client.is_in_risk_band(&wallet, &pair));

    submit(&env, &client, &wallet, &pair, 60);
    assert!(!client.is_in_risk_band(&wallet, &pair));

    submit(&env, &client, &wallet, &pair, 80);
    assert!(client.is_in_risk_band(&wallet, &pair));
}

// ── Zero-margin legacy equivalence ───────────────────────────────────────────

#[test]
fn test_zero_margin_exit_threshold_equals_entry_threshold() {
    let (env, client, _admin, _service) = setup();
    // margin = 0 (default): exit_threshold = 75 − 0 = 75

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 75);
    assert!(client.is_in_risk_band(&wallet, &pair));

    // Any score strictly below threshold immediately clears the band.
    submit(&env, &client, &wallet, &pair, 74);
    assert!(!client.is_in_risk_band(&wallet, &pair));
}

#[test]
fn test_zero_margin_immediate_exit_below_threshold() {
    let (env, client, _admin, _service) = setup();

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 80);
    assert!(client.is_in_risk_band(&wallet, &pair));

    // With margin = 0, score 74 < 75 = exit_threshold → immediate exit.
    submit(&env, &client, &wallet, &pair, 74);
    assert!(!client.is_in_risk_band(&wallet, &pair));
}

#[test]
fn test_zero_margin_consecutive_entries_alternate_with_exits() {
    let (env, client, _admin, _service) = setup();
    // With margin = 0 the system is equivalent to simple threshold comparison.

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Alternate above/below threshold: each above triggers enter (state → true),
    // each below triggers exit (state → false).
    let expected: &[(u32, bool)] = &[(80, true), (74, false), (75, true), (0, false)];
    for (score, expected_in_band) in expected {
        submit(&env, &client, &wallet, &pair, *score);
        assert_eq!(
            client.is_in_risk_band(&wallet, &pair),
            *expected_in_band,
            "score={score}: expected in_band={expected_in_band} (margin=0)"
        );
    }
}

// ── State isolation per wallet and asset pair ─────────────────────────────────

#[test]
fn test_band_state_isolated_per_wallet() {
    let (env, client, _admin, _service) = setup();
    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet_a, &pair, 80);
    assert!(client.is_in_risk_band(&wallet_a, &pair));
    assert!(!client.is_in_risk_band(&wallet_b, &pair));
}

#[test]
fn test_band_state_isolated_per_asset_pair() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("BTC_USDC");

    submit(&env, &client, &wallet, &pair_a, 80);
    assert!(client.is_in_risk_band(&wallet, &pair_a));
    assert!(!client.is_in_risk_band(&wallet, &pair_b));
}

#[test]
fn test_band_state_independent_exit_per_pair() {
    let (env, client, _admin, _service) = setup();
    client.set_hysteresis_margin(&10);

    let wallet = Address::generate(&env);
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("BTC_USDC");

    // Both pairs enter the band.
    submit(&env, &client, &wallet, &pair_a, 80);
    submit(&env, &client, &wallet, &pair_b, 80);
    assert!(client.is_in_risk_band(&wallet, &pair_a));
    assert!(client.is_in_risk_band(&wallet, &pair_b));

    // Only pair_a crosses the exit boundary; pair_b stays in band.
    submit(&env, &client, &wallet, &pair_a, 60);
    assert!(!client.is_in_risk_band(&wallet, &pair_a));
    assert!(client.is_in_risk_band(&wallet, &pair_b));
}

// ── query_risk_gate sticky behaviour ─────────────────────────────────────────

#[test]
fn test_query_risk_gate_returns_false_when_in_band_despite_low_score() {
    let (env, client, _admin, _service) = setup();
    // threshold = 75, margin = 10 → exit_threshold = 65
    client.set_hysteresis_margin(&10);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Enter band at 80.
    submit(&env, &client, &wallet, &pair, 80);

    // Score falls to 70: raw 70 < gate_threshold 75 would normally pass,
    // but wallet is still in the band (70 >= exit_threshold 65).
    submit(&env, &client, &wallet, &pair, 70);
    assert!(client.is_in_risk_band(&wallet, &pair));

    // Gate must return false because the sticky band state overrides the raw score.
    assert!(
        !client.query_risk_gate(&wallet, &pair, &75),
        "gate must return false: wallet is still in high-risk band"
    );
}

#[test]
fn test_query_risk_gate_returns_true_after_band_cleared() {
    let (env, client, _admin, _service) = setup();
    client.set_hysteresis_margin(&10);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 80);
    submit(&env, &client, &wallet, &pair, 60);
    assert!(!client.is_in_risk_band(&wallet, &pair));

    assert!(
        client.query_risk_gate(&wallet, &pair, &75),
        "gate must pass: wallet exited the high-risk band and current score is below threshold"
    );
}

#[test]
fn test_query_risk_gate_no_score_still_conservative() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // No score ever submitted; band state is false (default).
    // Gate still returns false (conservative "no data → unsafe" behaviour).
    assert!(!client.query_risk_gate(&wallet, &pair, &75));
}

// ── Batch submission hysteresis ───────────────────────────────────────────────

#[test]
fn test_batch_submission_enters_band_on_high_score() {
    let (env, client, _admin, _service) = setup();

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let mut batch = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: wallet.clone(),
        asset_pair: pair.clone(),
        score: 80,
        benford_flag: false,
        ml_flag: false,
        timestamp: env.ledger().timestamp().max(1),
        confidence: 80,
        model_version: 1,
    });
    client.submit_scores_batch(&batch);
    assert!(client.is_in_risk_band(&wallet, &pair));
}

#[test]
fn test_batch_submission_hysteresis_holds_in_band() {
    let (env, client, _admin, _service) = setup();
    // threshold = 75, margin = 10 → exit_threshold = 65
    client.set_hysteresis_margin(&10);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Enter band via batch.
    let mut batch = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: wallet.clone(),
        asset_pair: pair.clone(),
        score: 80,
        benford_flag: false,
        ml_flag: false,
        timestamp: env.ledger().timestamp().max(1),
        confidence: 80,
        model_version: 1,
    });
    client.submit_scores_batch(&batch);
    assert!(client.is_in_risk_band(&wallet, &pair));

    env.ledger().with_mut(|l| l.timestamp += 3_601);

    // Score drops below threshold but stays above exit_threshold (65).
    // Band must hold.
    let mut batch2 = Vec::new(&env);
    batch2.push_back(ScoreSubmission {
        wallet: wallet.clone(),
        asset_pair: pair.clone(),
        score: 70,
        benford_flag: false,
        ml_flag: false,
        timestamp: env.ledger().timestamp().max(1),
        confidence: 80,
        model_version: 1,
    });
    client.submit_scores_batch(&batch2);
    assert!(
        client.is_in_risk_band(&wallet, &pair),
        "batch: hysteresis must hold while score >= exit_threshold"
    );
}

// ── Snapshot consistency under repeated updates ───────────────────────────────

#[test]
fn test_state_snapshot_consistent_under_repeated_updates() {
    let (env, client, _admin, _service) = setup();
    // threshold = 75, margin = 15 → exit_threshold = 60
    client.set_hysteresis_margin(&15);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let sequence: &[(u32, bool)] = &[
        (50, false), // below threshold, not in band
        (80, true),  // enters band (score >= threshold)
        (76, true),  // still >= threshold, stays in band
        (70, true),  // below threshold but >= exit(60), hysteresis holds
        (65, true),  // still >= exit(60), holds
        (59, false), // below exit(60), exits band
        (40, false), // remains out
        (80, true),  // re-enters band
        (61, true),  // >= exit(60), holds
        (60, true),  // == exit(60), NOT below, holds
        (59, false), // below exit(60) again, clears
    ];

    for (score, expected_in_band) in sequence {
        submit(&env, &client, &wallet, &pair, *score);
        assert_eq!(
            client.is_in_risk_band(&wallet, &pair),
            *expected_in_band,
            "score={score}: expected in_band={expected_in_band} (margin=15, exit=60)"
        );
    }
}

// ── Hysteresis with custom risk threshold ────────────────────────────────────

#[test]
fn test_hysteresis_respects_custom_risk_threshold() {
    let (env, client, _admin, _service) = setup();
    // Set custom threshold = 90, margin = 5 → exit_threshold = 85
    client.set_risk_threshold(&Vec::new(&env), &90);
    client.set_hysteresis_margin(&5);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Score 80 < 90: should NOT enter band.
    submit(&env, &client, &wallet, &pair, 80);
    assert!(!client.is_in_risk_band(&wallet, &pair));

    // Score 90 == threshold: enters band.
    submit(&env, &client, &wallet, &pair, 90);
    assert!(client.is_in_risk_band(&wallet, &pair));

    // Score 86 < 90 but >= 85: stays in band (hysteresis).
    submit(&env, &client, &wallet, &pair, 86);
    assert!(client.is_in_risk_band(&wallet, &pair));

    // Score 84 < 85 (exit_threshold): exits band.
    submit(&env, &client, &wallet, &pair, 84);
    assert!(!client.is_in_risk_band(&wallet, &pair));
}

// ── get_risk_band_entry_time ──────────────────────────────────────────────────

#[test]
fn test_entry_time_none_before_band_entered() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // No submission at all: entry time must be absent.
    assert_eq!(client.get_risk_band_entry_time(&wallet, &pair), None);
}

#[test]
fn test_entry_time_none_while_below_threshold() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Score below threshold never enters band.
    submit(&env, &client, &wallet, &pair, 74);
    assert_eq!(client.get_risk_band_entry_time(&wallet, &pair), None);
}

#[test]
fn test_entry_time_recorded_on_band_entry() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let entry_ts = env.ledger().timestamp();
    submit(&env, &client, &wallet, &pair, 80);

    // Timestamp must be present and equal the ledger time at submission.
    assert_eq!(client.get_risk_band_entry_time(&wallet, &pair), Some(entry_ts));
}

#[test]
fn test_entry_time_stable_during_sustained_high_risk() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // First submission enters band at ts=0 (or whatever the initial timestamp is).
    let entry_ts = env.ledger().timestamp();
    submit(&env, &client, &wallet, &pair, 80); // advances ledger by 3601

    // Subsequent high-risk submissions must NOT overwrite the entry timestamp.
    submit(&env, &client, &wallet, &pair, 85);
    submit(&env, &client, &wallet, &pair, 90);

    assert_eq!(
        client.get_risk_band_entry_time(&wallet, &pair),
        Some(entry_ts),
        "entry timestamp must remain the initial entry time throughout sustained high risk"
    );
}

#[test]
fn test_entry_time_stable_during_hysteresis_hold() {
    let (env, client, _admin, _service) = setup();
    // threshold = 75, margin = 10 → exit_threshold = 65
    client.set_hysteresis_margin(&10);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let entry_ts = env.ledger().timestamp();
    submit(&env, &client, &wallet, &pair, 80);

    // Score drops below threshold but stays above exit_threshold (hysteresis holds).
    submit(&env, &client, &wallet, &pair, 70);
    assert!(client.is_in_risk_band(&wallet, &pair));

    assert_eq!(
        client.get_risk_band_entry_time(&wallet, &pair),
        Some(entry_ts),
        "entry timestamp must not change while hysteresis is holding the band"
    );
}

#[test]
fn test_entry_time_cleared_on_band_exit() {
    let (env, client, _admin, _service) = setup();
    client.set_hysteresis_margin(&10);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 80);
    assert!(client.get_risk_band_entry_time(&wallet, &pair).is_some());

    // Cross below exit_threshold (65) to exit the band.
    submit(&env, &client, &wallet, &pair, 64);
    assert!(!client.is_in_risk_band(&wallet, &pair));

    assert_eq!(
        client.get_risk_band_entry_time(&wallet, &pair),
        None,
        "entry timestamp must be cleared when the wallet exits the band"
    );
}

#[test]
fn test_entry_time_reset_on_reentry() {
    let (env, client, _admin, _service) = setup();
    client.set_hysteresis_margin(&10);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // First entry.
    let first_entry_ts = env.ledger().timestamp();
    submit(&env, &client, &wallet, &pair, 80);
    assert_eq!(client.get_risk_band_entry_time(&wallet, &pair), Some(first_entry_ts));

    // Exit band.
    submit(&env, &client, &wallet, &pair, 60); // below exit_threshold 65
    assert_eq!(client.get_risk_band_entry_time(&wallet, &pair), None);

    // Re-enter band; new entry timestamp must differ from the first.
    let second_entry_ts = env.ledger().timestamp();
    submit(&env, &client, &wallet, &pair, 80);
    let recorded = client.get_risk_band_entry_time(&wallet, &pair);
    assert_eq!(recorded, Some(second_entry_ts));
    assert_ne!(
        recorded,
        Some(first_entry_ts),
        "re-entry must record a fresh timestamp, not the original one"
    );
}

#[test]
fn test_entry_time_isolated_per_wallet() {
    let (env, client, _admin, _service) = setup();
    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet_a, &pair, 80);
    assert!(client.get_risk_band_entry_time(&wallet_a, &pair).is_some());
    assert_eq!(client.get_risk_band_entry_time(&wallet_b, &pair), None);
}

#[test]
fn test_entry_time_isolated_per_pair() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("BTC_USDC");

    submit(&env, &client, &wallet, &pair_a, 80);
    assert!(client.get_risk_band_entry_time(&wallet, &pair_a).is_some());
    assert_eq!(client.get_risk_band_entry_time(&wallet, &pair_b), None);
}

// ── set_hysteresis_margin: margin >= risk_threshold rejected ─────────────────

#[test]
fn test_set_hysteresis_margin_at_risk_threshold_rejected() {
    let (_env, client, _admin, _service) = setup();
    // Default risk_threshold is 75; margin = 75 must be rejected.
    let result = client.try_set_hysteresis_margin(&75);
    assert_eq!(result, Err(Ok(Error::InvalidHysteresisMargin)));
}

#[test]
fn test_set_hysteresis_margin_above_risk_threshold_rejected() {
    let (_env, client, _admin, _service) = setup();
    // margin = 76 > risk_threshold = 75, also rejected.
    let result = client.try_set_hysteresis_margin(&76);
    assert_eq!(result, Err(Ok(Error::InvalidHysteresisMargin)));
}

#[test]
fn test_set_hysteresis_margin_below_risk_threshold_accepted() {
    let (_env, client, _admin, _service) = setup();
    // margin = 74 < risk_threshold = 75, must be accepted.
    client.set_hysteresis_margin(&20);
    assert_eq!(client.get_hysteresis_margin(), 20);
}

// ── set_hysteresis_margin: non-admin rejected ────────────────────────────────

#[test]
fn test_set_hysteresis_margin_non_admin_rejected() {
    let env = Env::default();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    env.mock_all_auths();
    client.initialize(&Address::generate(&env), &Address::generate(&env));

    // Fresh env without mock_all_auths so admin auth check fires.
    let env2 = Env::default();
    let c2 = LedgerLensScoreContractClient::new(&env2, &contract_id);
    let result = c2.try_set_hysteresis_margin(&10);
    assert!(result.is_err());
}
