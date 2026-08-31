//! Abuse-case tests for repeated emergency overrides (issue #778).
//!
//! Verifies that an attacker or compromised admin cannot silently launder a
//! wallet's risk history by repeating `override_rate_limit` and/or
//! `override_score_floor` in rapid succession.  Each test asserts one of:
//!   - The audit log records every override, making abuse visible.
//!   - The score-floor protection resumes after a new high-risk score is
//!     recorded, even if the floor was cleared multiple times.
//!   - The rate-limit ring buffer caps how many override entries are retained,
//!     bounding storage cost while preserving the most recent N entries.
//!   - A repeated rate-limit override on the same pair only allows one
//!     accepted submission per override (cooldown is re-armed after each write).
//!   - Alternating override / submit cycles cannot produce a permanently-zero
//!     score for a wallet whose history has crossed the high-water mark.

#![cfg(test)]

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Bytes, Env, Vec,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient, ScoreSubmission};

const BASE_TS: u64 = 1_700_000_000;
const COOLDOWN: u64 = 3_601;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = BASE_TS);
    let id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &id);
    client.initialize(&Address::generate(&env), &Address::generate(&env));
    (env, client)
}

fn just(env: &Env, msg: &[u8]) -> Bytes {
    Bytes::from_slice(env, msg)
}

fn submit(env: &Env, client: &LedgerLensScoreContractClient, wallet: &Address, score: u32) {
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
    client.submit_score(
        &Vec::new(env),
        wallet,
        &symbol_short!("XLM_USDC"),
        &score,
        &false,
        &false,
        &env.ledger().timestamp(),
        &90u32,
        &1u32,
        &None,
    );
}

// ── Every override is recorded in the audit log ───────────────────────────────

#[test]
fn test_repeated_rate_limit_overrides_all_logged() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    for i in 0u32..5 {
        let msg = format!("override {}", i);
        client.override_rate_limit(
            &Vec::new(&env),
            &wallet,
            &pair,
            &just(&env, msg.as_bytes()),
        );
        env.ledger().with_mut(|l| l.timestamp += 1);
    }

    let log = client.get_rate_limit_override_log();
    assert_eq!(log.len(), 5, "all 5 overrides must appear in the audit log");
}

// ── Repeated score-floor overrides do not permanently disable the floor ────────

#[test]
fn test_repeated_score_floor_overrides_floor_resumes_after_high_score() {
    use crate::Error;
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_score_floor_policy(&Vec::new(&env), &true, &80u32, &20u32);

    // First cycle: score peaks at 90, override clears it, submit 0 succeeds.
    submit(&env, &client, &wallet, 90);
    client.override_score_floor(&Vec::new(&env), &wallet, &pair);
    submit(&env, &client, &wallet, 0);
    assert_eq!(client.get_score(&wallet, &pair).score, 0);

    // Second high score re-arms the floor.
    submit(&env, &client, &wallet, 90);

    // Second cycle: another sub-floor attempt is blocked (floor is active again).
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &5u32,
        &false,
        &false,
        &env.ledger().timestamp(),
        &90u32,
        &1u32,
        &None,
    );
    assert_eq!(
        result,
        Err(Ok(Error::BelowScoreFloor)),
        "floor must be re-armed after a new high score, even after previous override"
    );
}

// ── Alternating override + submit cannot permanently zero-out a high-risk wallet

#[test]
fn test_alternating_override_submit_cannot_launder_score() {
    use crate::Error;
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_score_floor_policy(&Vec::new(&env), &true, &80u32, &20u32);

    // The attacker repeats: override → submit 0 → submit high → override → ...
    for _ in 0..3 {
        submit(&env, &client, &wallet, 90); // push past HWM
        client.override_score_floor(&Vec::new(&env), &wallet, &pair);
        submit(&env, &client, &wallet, 0); // allowed by the override
        // high score re-arms the floor immediately on the next submission
    }

    // After the last cycle, score is 0 but a new high score re-arms the floor.
    submit(&env, &client, &wallet, 90);

    // The floor is now active; a sub-floor submission must be rejected.
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &1u32,
        &false,
        &false,
        &env.ledger().timestamp(),
        &90u32,
        &1u32,
        &None,
    );
    assert_eq!(
        result,
        Err(Ok(Error::BelowScoreFloor)),
        "floor must engage after any high score, regardless of how many overrides preceded it"
    );
}

// ── Rate-limit ring buffer caps at MAX_RATE_LIMIT_OVERRIDE_LOG ───────────────

#[test]
fn test_repeated_overrides_ring_buffer_capped() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let cap = crate::constants::MAX_RATE_LIMIT_OVERRIDE_LOG;

    // Fill past the cap.
    for i in 0..(cap + 5) {
        let msg = format!("o{}", i);
        client.override_rate_limit(
            &Vec::new(&env),
            &wallet,
            &pair,
            &just(&env, msg.as_bytes()),
        );
        env.ledger().with_mut(|l| l.timestamp += 1);
    }

    let log = client.get_rate_limit_override_log();
    assert_eq!(
        log.len(),
        cap,
        "override log must be capped at MAX_RATE_LIMIT_OVERRIDE_LOG"
    );
}

// ── Each rate-limit override allows exactly one submission before re-cooling ───

#[test]
fn test_rate_limit_override_allows_exactly_one_submission() {
    use crate::Error;
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Initial submission to arm the cooldown.
    submit(&env, &client, &wallet, 50);

    // Override clears the cooldown; one immediate submission is accepted.
    client.override_rate_limit(&Vec::new(&env), &wallet, &pair, &just(&env, b"fix"));
    env.ledger().with_mut(|l| l.timestamp += 1);
    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: wallet.clone(),
        asset_pair: pair.clone(),
        score: 60,
        benford_flag: false,
        ml_flag: false,
        timestamp: env.ledger().timestamp(),
        confidence: 90,
        model_version: 1,
    });
    let r1 = client.submit_scores_batch(&batch);
    assert_eq!(r1.accepted_count, 1, "first submission after override must succeed");

    // Immediate second submission (within cooldown) must be rejected.
    env.ledger().with_mut(|l| l.timestamp += 1);
    let mut batch2: Vec<ScoreSubmission> = Vec::new(&env);
    batch2.push_back(ScoreSubmission {
        wallet: wallet.clone(),
        asset_pair: pair.clone(),
        score: 70,
        benford_flag: false,
        ml_flag: false,
        timestamp: env.ledger().timestamp(),
        confidence: 90,
        model_version: 1,
    });
    let r2 = client.submit_scores_batch(&batch2);
    assert_eq!(
        r2.rejected_count, 1,
        "second submission within cooldown must be rejected even after override"
    );
    assert_eq!(
        r2.results.get(0).unwrap().rejection_code,
        Error::RateLimitExceeded as u32
    );
}

// ── Repeated override+submit pattern across multiple pairs stays per-pair ─────

#[test]
fn test_repeated_overrides_are_per_pair_isolated() {
    use crate::Error;
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("XLM_BTC");

    client.set_score_floor_policy(&Vec::new(&env), &true, &80u32, &20u32);

    // Push pair_a past HWM.
    submit(&env, &client, &wallet, 90);

    // Override only pair_a's floor.
    client.override_score_floor(&Vec::new(&env), &wallet, &pair_a);

    // pair_b has no history; low score is accepted regardless of floor policy.
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
    let result_b = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair_b,
        &0u32,
        &false,
        &false,
        &env.ledger().timestamp(),
        &90u32,
        &1u32,
        &None,
    );
    assert!(result_b.is_ok(), "pair_b has no HWM; low score must be accepted");

    // pair_a: floor cleared by override, so low score is also accepted here.
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
    let result_a = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair_a,
        &0u32,
        &false,
        &false,
        &env.ledger().timestamp(),
        &90u32,
        &1u32,
        &None,
    );
    assert!(result_a.is_ok(), "pair_a floor was cleared; low score must be accepted");

    // Re-arm pair_a's floor by submitting a high score.
    submit(&env, &client, &wallet, 90); // this uses pair XLM_USDC via the submit helper

    // Now a sub-floor submission to pair_a is rejected again.
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
    let result_a2 = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair_a,
        &5u32,
        &false,
        &false,
        &env.ledger().timestamp(),
        &90u32,
        &1u32,
        &None,
    );
    assert_eq!(result_a2, Err(Ok(Error::BelowScoreFloor)));
}
