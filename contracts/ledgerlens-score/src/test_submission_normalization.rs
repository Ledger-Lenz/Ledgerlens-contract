//! Tests for issue #686 — canonical score-submission normalization.
//!
//! These tests verify that:
//!
//! 1. Both `submit_score` (single) and `submit_scores_batch` (batch) accept
//!    identical valid inputs and produce matching stored scores.
//! 2. Both paths reject the same malformed inputs with the same error codes
//!    and in the same order — confirming the shared `normalize_submission` →
//!    `validate_normalized_submission` path is deterministic.
//! 3. Boundary values at the edges of valid ranges (score=0, score=100,
//!    confidence=0, confidence=100) are accepted by both paths.
//! 4. Adversarial / combined-bad inputs are consistently rejected with the
//!    expected first error in validation order:
//!       score > 100  →  InvalidScore   (code 4)
//!       confidence > 100  →  InvalidConfidence  (code 5)
//!       timestamp == 0    →  InvalidTimestamp   (code 25)

use soroban_sdk::{
    symbol_short,
    testutils::Address as _,
    vec, Address, Env, Vec,
};

use crate::{
    BatchEntryResult, BatchResult, Error, LedgerLensScoreContract,
    LedgerLensScoreContractClient, ScoreSubmission,
};
use ledgerlens_test_support::{
    generate_score_roles, set_ledger_timestamp, test_env_with_unlimited_budget,
};

// ── helpers ───────────────────────────────────────────────────────────────────

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = test_env_with_unlimited_budget();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let (admin, service) = generate_score_roles(&env);
    set_ledger_timestamp(&env, 100_000);
    client.initialize(&admin, &service);
    (env, client, admin, service)
}

/// Build a valid `ScoreSubmission` entry.
fn valid_sub(env: &Env, wallet: &Address) -> ScoreSubmission {
    ScoreSubmission {
        wallet: wallet.clone(),
        asset_pair: symbol_short!("XLM_USDC"),
        score: 42,
        benford_flag: true,
        ml_flag: false,
        timestamp: 1_700_000_000,
        confidence: 90,
        model_version: 1,
    }
}

// ── 1. Success path: single and batch both accept valid inputs ────────────────

#[test]
fn normalization_single_success_path() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &symbol_short!("XLM_USDC"),
        &42,
        &true,
        &false,
        &1_700_000_000,
        &90,
        &1,
        &None,
    );

    let score = client.get_score(&wallet, &symbol_short!("XLM_USDC"));
    assert_eq!(score.score, 42);
    assert!(score.benford_flag);
    assert!(!score.ml_flag);
    assert_eq!(score.confidence, 90);
    assert_eq!(score.model_version, 1);
    assert_eq!(score.timestamp, 1_700_000_000);
}

#[test]
fn normalization_batch_success_path() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let sub = valid_sub(&env, &wallet);

    let result = client.submit_scores_batch(&vec![&env, sub]);

    assert_eq!(result.accepted_count, 1);
    assert_eq!(result.rejected_count, 0);

    let score = client.get_score(&wallet, &symbol_short!("XLM_USDC"));
    assert_eq!(score.score, 42);
    assert_eq!(score.confidence, 90);
}

#[test]
fn normalization_single_and_batch_store_identical_values() {
    // Two wallets, same payload — one via single, one via batch.
    let (env, client, _admin, _service) = setup();
    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.submit_score(
        &Vec::new(&env),
        &wallet_a,
        &pair,
        &55,
        &false,
        &true,
        &1_700_000_001,
        &75,
        &1,
        &None,
    );

    let sub = ScoreSubmission {
        wallet: wallet_b.clone(),
        asset_pair: pair.clone(),
        score: 55,
        benford_flag: false,
        ml_flag: true,
        timestamp: 1_700_000_001,
        confidence: 75,
        model_version: 1,
    };
    client.submit_scores_batch(&vec![&env, sub]);

    let a = client.get_score(&wallet_a, &pair);
    let b = client.get_score(&wallet_b, &pair);

    assert_eq!(a.score, b.score);
    assert_eq!(a.benford_flag, b.benford_flag);
    assert_eq!(a.ml_flag, b.ml_flag);
    assert_eq!(a.confidence, b.confidence);
    assert_eq!(a.model_version, b.model_version);
    assert_eq!(a.timestamp, b.timestamp);
}

// ── 2. Boundary cases ────────────────────────────────────────────────────────

#[test]
fn normalization_boundary_score_zero_accepted() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &symbol_short!("XLM_USDC"),
        &0, // minimum valid score
        &false,
        &false,
        &1_700_000_000,
        &0, // minimum valid confidence
        &1,
        &None,
    );
    let score = client.get_score(&wallet, &symbol_short!("XLM_USDC"));
    assert_eq!(score.score, 0);
    assert_eq!(score.confidence, 0);
}

#[test]
fn normalization_boundary_score_100_accepted() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &symbol_short!("XLM_USDC"),
        &100, // maximum valid score
        &false,
        &false,
        &1_700_000_000,
        &100, // maximum valid confidence
        &1,
        &None,
    );
    let score = client.get_score(&wallet, &symbol_short!("XLM_USDC"));
    assert_eq!(score.score, 100);
    assert_eq!(score.confidence, 100);
}

#[test]
fn normalization_boundary_batch_score_zero_and_100_accepted() {
    let (env, client, _admin, _service) = setup();
    let wallet_lo = Address::generate(&env);
    let wallet_hi = Address::generate(&env);

    let result = client.submit_scores_batch(&vec![
        &env,
        ScoreSubmission {
            wallet: wallet_lo.clone(),
            asset_pair: symbol_short!("XLM_USDC"),
            score: 0,
            benford_flag: false,
            ml_flag: false,
            timestamp: 1_700_000_000,
            confidence: 0,
            model_version: 1,
        },
        ScoreSubmission {
            wallet: wallet_hi.clone(),
            asset_pair: symbol_short!("XLM_USDC"),
            score: 100,
            benford_flag: false,
            ml_flag: false,
            timestamp: 1_700_000_000,
            confidence: 100,
            model_version: 1,
        },
    ]);

    assert_eq!(result.accepted_count, 2);
    assert_eq!(result.rejected_count, 0);
    assert_eq!(client.get_score(&wallet_lo, &symbol_short!("XLM_USDC")).score, 0);
    assert_eq!(client.get_score(&wallet_hi, &symbol_short!("XLM_USDC")).score, 100);
}

// ── 3. Adversarial / rejection cases — single path ──────────────────────────

#[test]
fn normalization_single_rejects_score_over_100() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &symbol_short!("XLM_USDC"),
        &101, // out of range
        &false,
        &false,
        &1_700_000_000,
        &50,
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::InvalidScore)));
}

#[test]
fn normalization_single_rejects_confidence_over_100() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &symbol_short!("XLM_USDC"),
        &50,
        &false,
        &false,
        &1_700_000_000,
        &101, // out of range
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::InvalidConfidence)));
}

#[test]
fn normalization_single_rejects_zero_timestamp() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &symbol_short!("XLM_USDC"),
        &50,
        &false,
        &false,
        &0, // zero timestamp
        &50,
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::InvalidTimestamp)));
}

// ── 4. Adversarial / rejection cases — batch path ───────────────────────────
//
// The batch path records a rejection_code per entry rather than returning an
// error.  rejection_code values match the Error discriminants:
//   InvalidScore      = 4
//   InvalidConfidence = 5
//   InvalidTimestamp  = 25

#[test]
fn normalization_batch_rejects_score_over_100() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    let result = client.submit_scores_batch(&vec![
        &env,
        ScoreSubmission {
            wallet: wallet.clone(),
            asset_pair: symbol_short!("XLM_USDC"),
            score: 101, // out of range
            benford_flag: false,
            ml_flag: false,
            timestamp: 1_700_000_000,
            confidence: 50,
            model_version: 1,
        },
    ]);

    assert_eq!(result.accepted_count, 0);
    assert_eq!(result.rejected_count, 1);
    let entry = result.results.get(0).unwrap();
    assert!(!entry.accepted);
    assert_eq!(entry.rejection_code, Error::InvalidScore as u32);
}

#[test]
fn normalization_batch_rejects_confidence_over_100() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    let result = client.submit_scores_batch(&vec![
        &env,
        ScoreSubmission {
            wallet: wallet.clone(),
            asset_pair: symbol_short!("XLM_USDC"),
            score: 50,
            benford_flag: false,
            ml_flag: false,
            timestamp: 1_700_000_000,
            confidence: 101, // out of range
            model_version: 1,
        },
    ]);

    assert_eq!(result.accepted_count, 0);
    let entry = result.results.get(0).unwrap();
    assert_eq!(entry.rejection_code, Error::InvalidConfidence as u32);
}

#[test]
fn normalization_batch_rejects_zero_timestamp() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    let result = client.submit_scores_batch(&vec![
        &env,
        ScoreSubmission {
            wallet: wallet.clone(),
            asset_pair: symbol_short!("XLM_USDC"),
            score: 50,
            benford_flag: false,
            ml_flag: false,
            timestamp: 0, // invalid
            confidence: 50,
            model_version: 1,
        },
    ]);

    assert_eq!(result.accepted_count, 0);
    let entry = result.results.get(0).unwrap();
    assert_eq!(entry.rejection_code, Error::InvalidTimestamp as u32);
}

// ── 5. Validation order is identical on both paths ───────────────────────────
//
// When multiple fields are invalid, the first error in validation order wins.
// Order: score > 100  →  confidence > 100  →  timestamp == 0.

#[test]
fn normalization_order_score_before_confidence_single() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    // Both score and confidence are bad: score error must come first.
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &symbol_short!("XLM_USDC"),
        &200, // bad score
        &false,
        &false,
        &1_700_000_000,
        &200, // bad confidence — would also fail, but score is checked first
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::InvalidScore)));
}

#[test]
fn normalization_order_score_before_confidence_batch() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    let result = client.submit_scores_batch(&vec![
        &env,
        ScoreSubmission {
            wallet: wallet.clone(),
            asset_pair: symbol_short!("XLM_USDC"),
            score: 200,   // bad score — checked first
            benford_flag: false,
            ml_flag: false,
            timestamp: 1_700_000_000,
            confidence: 200, // also bad, but score takes priority
            model_version: 1,
        },
    ]);

    let entry = result.results.get(0).unwrap();
    assert_eq!(entry.rejection_code, Error::InvalidScore as u32);
}

#[test]
fn normalization_order_confidence_before_timestamp_single() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    // Score is valid, but both confidence and timestamp are bad.
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &symbol_short!("XLM_USDC"),
        &50,
        &false,
        &false,
        &0,   // bad timestamp — checked after confidence
        &200, // bad confidence — checked second
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::InvalidConfidence)));
}

#[test]
fn normalization_order_confidence_before_timestamp_batch() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    let result = client.submit_scores_batch(&vec![
        &env,
        ScoreSubmission {
            wallet: wallet.clone(),
            asset_pair: symbol_short!("XLM_USDC"),
            score: 50,
            benford_flag: false,
            ml_flag: false,
            timestamp: 0,   // also bad, but confidence is checked first
            confidence: 200, // bad confidence
            model_version: 1,
        },
    ]);

    let entry = result.results.get(0).unwrap();
    assert_eq!(entry.rejection_code, Error::InvalidConfidence as u32);
}

// ── 6. Mixed batch: some entries valid, some invalid ─────────────────────────

#[test]
fn normalization_batch_mixed_partial_accept() {
    let (env, client, _admin, _service) = setup();
    let wallet_ok = Address::generate(&env);
    let wallet_bad_score = Address::generate(&env);
    let wallet_bad_ts = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let result = client.submit_scores_batch(&vec![
        &env,
        ScoreSubmission {
            wallet: wallet_ok.clone(),
            asset_pair: pair.clone(),
            score: 30,
            benford_flag: false,
            ml_flag: false,
            timestamp: 1_700_000_000,
            confidence: 80,
            model_version: 1,
        },
        ScoreSubmission {
            wallet: wallet_bad_score.clone(),
            asset_pair: pair.clone(),
            score: 150, // invalid
            benford_flag: false,
            ml_flag: false,
            timestamp: 1_700_000_000,
            confidence: 80,
            model_version: 1,
        },
        ScoreSubmission {
            wallet: wallet_bad_ts.clone(),
            asset_pair: pair.clone(),
            score: 30,
            benford_flag: false,
            ml_flag: false,
            timestamp: 0, // invalid
            confidence: 80,
            model_version: 1,
        },
    ]);

    assert_eq!(result.accepted_count, 1);
    assert_eq!(result.rejected_count, 2);

    let r0 = result.results.get(0).unwrap();
    let r1 = result.results.get(1).unwrap();
    let r2 = result.results.get(2).unwrap();

    assert!(r0.accepted);
    assert_eq!(r0.rejection_code, 0);

    assert!(!r1.accepted);
    assert_eq!(r1.rejection_code, Error::InvalidScore as u32);

    assert!(!r2.accepted);
    assert_eq!(r2.rejection_code, Error::InvalidTimestamp as u32);

    // The valid wallet's score was actually written.
    assert_eq!(client.get_score(&wallet_ok, &pair).score, 30);
    // The invalid wallets have no score stored.
    assert!(client.try_get_score(&wallet_bad_score, &pair).is_err());
    assert!(client.try_get_score(&wallet_bad_ts, &pair).is_err());
}
