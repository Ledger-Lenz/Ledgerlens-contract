//! Bounded drift checks for consecutive score updates (#724).
//!
//! A score "drifts" when consecutive updates for the same (wallet, pair) move
//! the score by more than a configurable threshold without matching evidence
//! fields or explicit override authorization.  The contract uses the
//! `jump_threshold` to define the maximum permitted absolute delta between
//! consecutive scores; updates that exceed it trigger a `ScoreJumpAnomalyEvent`
//! and — depending on policy — may be soft-rejected.
//!
//! Tests in this file document and verify:
//!
//! D1. A score update within the drift threshold is accepted silently (no
//!     jump anomaly event).
//! D2. A score update that exceeds the drift threshold triggers the
//!     `ScoreJumpAnomalyEvent`.
//! D3. A score update exactly at the drift threshold boundary is accepted
//!     (boundary is inclusive).
//! D4. A score update one above the threshold triggers the anomaly event.
//! D5. The drift threshold is configurable; changing it takes effect on
//!     subsequent submissions.
//! D6. Legitimate large score changes accompanied by `is_flagged=true` are
//!     still accepted and still emit the anomaly event.
//! D7. The first submission for a (wallet, pair) never triggers a drift
//!     anomaly because there is no previous score to compare against.
//! D8. `get_jump_stats` correctly increments the anomaly counter for the
//!     wallet/pair after each threshold-exceeding submission.
//! D9. Decreasing scores (drops) are also checked: a suspicious drop exceeding
//!     the threshold triggers the anomaly event.

#![cfg(test)]

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger as _},
    Address, Env, IntoVal, Symbol, Vec,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient};

const START_TS: u64 = 1_700_000_000;
const COOLDOWN: u64 = 3_601;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>) {
    let env = Env::default();
    env.mock_all_auths();
    env.budget().reset_unlimited();
    env.ledger().with_mut(|l| l.timestamp = START_TS);
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client)
}

fn submit(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    wallet: &Address,
    score: u32,
    is_flagged: bool,
) {
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
    client.submit_score(
        &Vec::new(env),
        wallet,
        &symbol_short!("XLM_USDC"),
        &score,
        &is_flagged,
        &false,
        &(env.ledger().timestamp()),
        &90,
        &1,
        &None,
    );
}

/// Returns `true` if a `jmp_ano` (ScoreJumpAnomalyEvent) was emitted for
/// this wallet/pair since the last call to `env.events().all()`.
fn jump_anomaly_emitted(
    env: &Env,
    contract_id: &Address,
    wallet: &Address,
    pair: &Symbol,
) -> bool {
    let topic = (symbol_short!("jmp_ano"), 1u32, wallet.clone(), pair.clone());
    env.events().all().iter().any(|(addr, topics, _)| {
        &addr == contract_id && topics == topic.into_val(env)
    })
}

fn contract_id(client: &LedgerLensScoreContractClient) -> Address {
    client.address.clone()
}

// ── D1: Update within threshold — no anomaly event ────────────────────────────

#[test]
fn drift_within_threshold_no_anomaly() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let cid = contract_id(&client);

    // Set threshold to 20.
    client.set_jump_threshold(&Vec::new(&env), &20);

    // First submission (no previous score, so never triggers anomaly).
    submit(&env, &client, &wallet, 50, false);

    // Second submission: delta = |60 - 50| = 10 ≤ 20, within threshold.
    submit(&env, &client, &wallet, 60, false);

    assert!(
        !jump_anomaly_emitted(&env, &cid, &wallet, &pair),
        "D1: update within threshold should not emit jump anomaly"
    );
}

// ── D2: Update exceeding threshold — anomaly event emitted ────────────────────

#[test]
fn drift_exceeds_threshold_emits_anomaly() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let cid = contract_id(&client);

    // Set threshold to 20.
    client.set_jump_threshold(&Vec::new(&env), &20);

    // First submission.
    submit(&env, &client, &wallet, 30, false);

    // Clear events so we only look at those from the second submission.
    // (env.events() always returns all events; we look for the pattern after)

    // Second submission: delta = |80 - 30| = 50 > 20, exceeds threshold.
    submit(&env, &client, &wallet, 80, false);

    assert!(
        jump_anomaly_emitted(&env, &cid, &wallet, &pair),
        "D2: update exceeding threshold should emit jump anomaly"
    );
}

// ── D3: Update exactly at threshold — no anomaly (inclusive boundary) ─────────

#[test]
fn drift_exactly_at_threshold_no_anomaly() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let cid = contract_id(&client);

    // Set threshold to 30.
    client.set_jump_threshold(&Vec::new(&env), &30);

    // First submission.
    submit(&env, &client, &wallet, 40, false);

    // Second submission: delta = |70 - 40| = 30 == threshold (boundary).
    submit(&env, &client, &wallet, 70, false);

    assert!(
        !jump_anomaly_emitted(&env, &cid, &wallet, &pair),
        "D3: update exactly at threshold should not emit anomaly (inclusive)"
    );
}

// ── D4: Update one above threshold — anomaly triggered ───────────────────────

#[test]
fn drift_one_above_threshold_emits_anomaly() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let cid = contract_id(&client);

    client.set_jump_threshold(&Vec::new(&env), &30);

    // First submission.
    submit(&env, &client, &wallet, 40, false);

    // Second submission: delta = |71 - 40| = 31 > 30.
    submit(&env, &client, &wallet, 71, false);

    assert!(
        jump_anomaly_emitted(&env, &cid, &wallet, &pair),
        "D4: update one above threshold should emit anomaly"
    );
}

// ── D5: Threshold is configurable and takes effect on next submission ─────────

#[test]
fn drift_threshold_configurable() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let cid = contract_id(&client);

    // Start with a high threshold (100) — no anomaly for delta=50.
    client.set_jump_threshold(&Vec::new(&env), &100);
    submit(&env, &client, &wallet, 20, false);
    submit(&env, &client, &wallet, 70, false); // delta=50 ≤ 100
    assert!(
        !jump_anomaly_emitted(&env, &cid, &wallet, &pair),
        "D5: delta=50 with threshold=100 should not trigger anomaly"
    );

    // Lower threshold to 10 — same delta=50 would now trigger.
    client.set_jump_threshold(&Vec::new(&env), &10);
    let wallet2 = Address::generate(&env);
    submit(&env, &client, &wallet2, 20, false);
    submit(&env, &client, &wallet2, 70, false); // delta=50 > 10
    assert!(
        jump_anomaly_emitted(&env, &cid, &wallet2, &pair),
        "D5: delta=50 with threshold=10 should trigger anomaly"
    );

    // Verify threshold stored correctly.
    assert_eq!(client.get_jump_threshold(), 10, "D5: threshold should be 10");
}

// ── D6: Flagged submission still accepted and still triggers anomaly ──────────

#[test]
fn drift_flagged_submission_triggers_anomaly_and_is_stored() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let cid = contract_id(&client);

    client.set_jump_threshold(&Vec::new(&env), &20);

    // First valid submission.
    submit(&env, &client, &wallet, 30, false);

    // Large flagged submission: delta = |90 - 30| = 60 > 20.
    submit(&env, &client, &wallet, 90, true);

    let stored = client.get_score(&wallet, &pair).score;
    assert_eq!(stored, 90, "D6: flagged submission should be stored despite anomaly");
    assert!(
        jump_anomaly_emitted(&env, &cid, &wallet, &pair),
        "D6: flagged submission should still emit jump anomaly"
    );
}

// ── D7: First submission never triggers anomaly ───────────────────────────────

#[test]
fn drift_first_submission_no_anomaly() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let cid = contract_id(&client);

    // Set a very low threshold.
    client.set_jump_threshold(&Vec::new(&env), &0);

    // First and only submission.
    submit(&env, &client, &wallet, 100, false);

    assert!(
        !jump_anomaly_emitted(&env, &cid, &wallet, &pair),
        "D7: first submission should never trigger drift anomaly"
    );
}

// ── D8: get_jump_stats increments counter on each anomaly ────────────────────

#[test]
fn drift_jump_stats_counter_increments() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_jump_threshold(&Vec::new(&env), &10);

    // No anomaly yet.
    let (count0, _) = client.get_jump_stats(&wallet, &pair);
    assert_eq!(count0, 0, "D8: initial jump count should be 0");

    // First submission (no previous score — no anomaly).
    submit(&env, &client, &wallet, 50, false);
    let (count1, _) = client.get_jump_stats(&wallet, &pair);
    assert_eq!(count1, 0, "D8: first submission should not increment jump count");

    // Second submission: delta=40 > 10 → anomaly.
    submit(&env, &client, &wallet, 90, false);
    let (count2, _) = client.get_jump_stats(&wallet, &pair);
    assert_eq!(count2, 1, "D8: one anomaly expected after threshold-exceeding update");

    // Third submission: delta=|90-20|=70 > 10 → another anomaly.
    submit(&env, &client, &wallet, 20, false);
    let (count3, _) = client.get_jump_stats(&wallet, &pair);
    assert_eq!(count3, 2, "D8: two anomalies expected");
}

// ── D9: Score drops are also checked ─────────────────────────────────────────

#[test]
fn drift_suspicious_drop_triggers_anomaly() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let cid = contract_id(&client);

    client.set_jump_threshold(&Vec::new(&env), &20);

    // Start at a high score.
    submit(&env, &client, &wallet, 90, false);

    // Sudden drop: delta = |90 - 30| = 60 > 20.
    submit(&env, &client, &wallet, 30, false);

    assert!(
        jump_anomaly_emitted(&env, &cid, &wallet, &pair),
        "D9: suspicious score drop should trigger jump anomaly"
    );
}
