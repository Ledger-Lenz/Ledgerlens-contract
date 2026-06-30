//! Tests for score jump anomaly detection (issue #74).
//!
//! When the absolute delta between consecutive scores exceeds the configured
//! `JumpThreshold`, a `ScoreJumpAnomalyEvent` is emitted in addition to the
//! normal `ScoreDeltaEvent`.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger as _},
    Address, Env, IntoVal, Symbol, Vec,
};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient};

const START_TS: u64 = 1_700_000_000;
const COOLDOWN: u64 = 3_601;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>) {
    let env = Env::default();
    env.mock_all_auths();
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
    pair: &Symbol,
    score: u32,
) {
    client.submit_score(
        &Vec::new(env),
        wallet,
        pair,
        &score,
        &false,
        &false,
        &env.ledger().timestamp(),
        &90,
        &1,
        &None,
    );
}

/// Returns the last `jmp_ano` event data for `(wallet, pair)`, or `None` if
/// no jump anomaly event was emitted.
fn last_jump_event(
    env: &Env,
    contract_id: &Address,
    wallet: &Address,
    pair: &Symbol,
) -> Option<(u32, u32, i64, u32, u64)> {
    let topic = (symbol_short!("jmp_ano"), wallet.clone(), pair.clone());
    for (addr, topics, data) in env.events().all().iter().rev() {
        if &addr == contract_id && topics == topic.into_val(env) {
            let (prev, new, delta, model, ts): (u32, u32, i64, u32, u64) = data.into_val(env);
            return Some((prev, new, delta, model, ts));
        }
    }
    None
}

// ── Default threshold ─────────────────────────────────────────────────────────

#[test]
fn test_default_jump_threshold() {
    let (env, client) = setup();
    assert_eq!(client.get_jump_threshold(), 30);
    let _ = env;
}

// ── First submission emits no jump event (no previous score) ──────────────────

#[test]
fn test_first_submission_no_jump_event() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let c2 = LedgerLensScoreContractClient::new(&env, &contract_id);
    c2.initialize(&Address::generate(&env), &Address::generate(&env));

    submit(&env, &c2, &wallet, &pair, 50);

    // No jump event on first submission because there's no previous score.
    assert!(last_jump_event(&env, &contract_id, &wallet, &pair).is_none());
    let _ = client;
}

// ── Jump at exactly threshold: no event ───────────────────────────────────────

#[test]
fn test_jump_at_threshold_no_event() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let c2 = LedgerLensScoreContractClient::new(&env, &contract_id);
    c2.initialize(&Address::generate(&env), &Address::generate(&env));

    // First: score = 10
    submit(&env, &c2, &wallet, &pair, 10);
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);

    // Second: score = 40 (delta = 30, exactly at default threshold — no event)
    submit(&env, &c2, &wallet, &pair, 40);

    assert!(last_jump_event(&env, &contract_id, &wallet, &pair).is_none());
    let _ = client;
}

// ── Jump one above threshold: event emitted ──────────────────────────────────

#[test]
fn test_jump_one_above_threshold_emits_event() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let c2 = LedgerLensScoreContractClient::new(&env, &contract_id);
    c2.initialize(&Address::generate(&env), &Address::generate(&env));

    // First: score = 10
    submit(&env, &c2, &wallet, &pair, 10);
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);

    // Second: score = 41 (delta = 31 > 30, triggers event)
    submit(&env, &c2, &wallet, &pair, 41);

    let event = last_jump_event(&env, &contract_id, &wallet, &pair)
        .expect("jump anomaly event should have been emitted");
    assert_eq!(event.0, 10, "previous_score");
    assert_eq!(event.1, 41, "new_score");
    assert_eq!(event.2, 31, "delta should be positive (rose)");
    assert_eq!(event.3, 1, "model_version");
    assert_eq!(event.4, START_TS + COOLDOWN, "timestamp");
    let _ = client;
}

// ── Negative jump (fall): event emitted with negative delta ───────────────────

#[test]
fn test_negative_jump_emits_event() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let c2 = LedgerLensScoreContractClient::new(&env, &contract_id);
    c2.initialize(&Address::generate(&env), &Address::generate(&env));

    // First: score = 80
    submit(&env, &c2, &wallet, &pair, 80);
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);

    // Second: score = 30 (delta = 50 > 30, triggers event; delta is negative)
    submit(&env, &c2, &wallet, &pair, 30);

    let event = last_jump_event(&env, &contract_id, &wallet, &pair)
        .expect("jump anomaly event should have been emitted");
    assert_eq!(event.0, 80, "previous_score");
    assert_eq!(event.1, 30, "new_score");
    assert_eq!(event.2, -50, "delta should be negative (fell)");
    assert_eq!(event.3, 1, "model_version");
    let _ = client;
}

// ── Batch with mixed jumps ────────────────────────────────────────────────────

#[test]
fn test_batch_mixed_jumps() {
    let (env, client) = setup();
    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);
    let wallet_c = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let c2 = LedgerLensScoreContractClient::new(&env, &contract_id);
    c2.initialize(&Address::generate(&env), &Address::generate(&env));

    // Pre-populate scores so second submissions have a previous to diff.
    submit(&env, &c2, &wallet_a, &pair, 10);
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
    submit(&env, &c2, &wallet_b, &pair, 10);
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
    submit(&env, &c2, &wallet_c, &pair, 10);
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);

    // Batch with:
    //   wallet_a: 10 -> 35 (delta = 25 < 30, no jump event)
    //   wallet_b: 10 -> 45 (delta = 35 > 30, jump event)
    //   wallet_c: 10 -> 25 (delta = 15 < 30, no jump event)
    let mut batch: Vec<crate::ScoreSubmission> = Vec::new(&env);
    let ts = env.ledger().timestamp();
    batch.push_back(crate::ScoreSubmission {
        wallet: wallet_a.clone(),
        asset_pair: pair.clone(),
        score: 35,
        benford_flag: false,
        ml_flag: false,
        timestamp: ts,
        confidence: 90,
        model_version: 2,
    });
    batch.push_back(crate::ScoreSubmission {
        wallet: wallet_b.clone(),
        asset_pair: pair.clone(),
        score: 45,
        benford_flag: false,
        ml_flag: false,
        timestamp: ts,
        confidence: 90,
        model_version: 2,
    });
    batch.push_back(crate::ScoreSubmission {
        wallet: wallet_c.clone(),
        asset_pair: pair.clone(),
        score: 25,
        benford_flag: false,
        ml_flag: false,
        timestamp: ts,
        confidence: 90,
        model_version: 2,
    });

    let result = c2.submit_scores_batch(&batch);
    assert_eq!(result.accepted_count, 3);
    assert_eq!(result.rejected_count, 0);

    // wallet_a: delta = 25, no jump event
    assert!(
        last_jump_event(&env, &contract_id, &wallet_a, &pair).is_none(),
        "wallet_a jump of 25 should not trigger"
    );

    // wallet_b: delta = 35, should have jump event
    let event_b = last_jump_event(&env, &contract_id, &wallet_b, &pair)
        .expect("wallet_b jump of 35 should trigger");
    assert_eq!(event_b.0, 10, "wallet_b previous_score");
    assert_eq!(event_b.1, 45, "wallet_b new_score");
    assert_eq!(event_b.2, 35, "wallet_b delta");
    assert_eq!(event_b.3, 2, "wallet_b model_version");

    // wallet_c: delta = 15, no jump event
    assert!(
        last_jump_event(&env, &contract_id, &wallet_c, &pair).is_none(),
        "wallet_c jump of 15 should not trigger"
    );

    let _ = client;
}

// ── Custom threshold via set_jump_threshold ───────────────────────────────────

#[test]
fn test_custom_threshold_emits_event() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let c2 = LedgerLensScoreContractClient::new(&env, &contract_id);
    c2.initialize(&Address::generate(&env), &Address::generate(&env));
    c2.set_jump_threshold(&Vec::new(&env), &10);

    // First: score = 20
    submit(&env, &c2, &wallet, &pair, 20);
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);

    // Second: score = 31 (delta = 11 > 10, triggers)
    submit(&env, &c2, &wallet, &pair, 31);

    let event = last_jump_event(&env, &contract_id, &wallet, &pair)
        .expect("jump anomaly event should have been emitted");
    assert_eq!(event.2, 11, "delta with custom threshold");

    let _ = client;
}

// ── set_jump_threshold validation ─────────────────────────────────────────────

#[test]
fn test_set_jump_threshold_zero_rejected() {
    let (env, client) = setup();
    let result = client.try_set_jump_threshold(&Vec::new(&env), &0);
    assert_eq!(result, Err(Ok(Error::InvalidThreshold)));
}

#[test]
fn test_set_jump_threshold_over_99_rejected() {
    let (env, client) = setup();
    let result = client.try_set_jump_threshold(&Vec::new(&env), &100);
    assert_eq!(result, Err(Ok(Error::InvalidThreshold)));
}

#[test]
fn test_set_jump_threshold_99_accepted() {
    let (env, client) = setup();
    client.set_jump_threshold(&Vec::new(&env), &99);
    assert_eq!(client.get_jump_threshold(), 99);
}
