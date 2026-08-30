/// #687 — Adversarial validation tests for contradictory score signals.
///
/// The contract validates *ranges* (score 0-100, confidence 0-100,
/// timestamp != 0) but intentionally does NOT enforce internal consistency
/// between the headline score, flags, and raw sub-scores.  These tests
/// confirm:
///
/// 1. Every syntactically-valid but semantically-contradictory submission is
///    ACCEPTED — no hidden panic, no silent rejection.
/// 2. `get_score` returns the exact fields submitted (no field mutation).
/// 3. Boundary values (0 and 100) are accepted for all numeric fields.
/// 4. The batch path handles mixed-signal entries identically.
#[cfg(test)]
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger},
    Address, Env, Vec,
};

#[cfg(test)]
use crate::LedgerLensScoreContract;

/// Shared test setup: returns (env, contract_client, admin, service, wallet, pair).
#[cfg(test)]
fn setup() -> (
    Env,
    crate::LedgerLensScoreContractClient<'static>,
    Address,
    Address,
    Address,
    soroban_sdk::Symbol,
) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = crate::LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    (env, client, admin, service, wallet, pair)
}

// ── Test matrix: contradictory but syntactically-valid inputs ────────────────

/// score=0 + confidence=100 + both flags true — min risk score with max
/// confidence and both anomaly flags set simultaneously.
#[test]
fn test_score_zero_max_confidence_both_flags() {
    let (env, client, _admin, _service, wallet, pair) = setup();
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &0,    // minimum possible score
        &true, // benford anomaly flagged
        &true, // ml anomaly flagged
        &1,
        &100, // maximum confidence
        &1,
        &None,
    );
    let s = client.get_score(&wallet, &pair);
    assert_eq!(s.score, 0);
    assert!(s.benford_flag);
    assert!(s.ml_flag);
    assert_eq!(s.confidence, 100);
}

/// score=100 + confidence=0 — highest possible risk with zero model confidence.
#[test]
fn test_score_max_zero_confidence() {
    let (env, client, _admin, _service, wallet, pair) = setup();
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &100,
        &false,
        &false,
        &1,
        &0, // zero confidence
        &1,
        &None,
    );
    let s = client.get_score(&wallet, &pair);
    assert_eq!(s.score, 100);
    assert_eq!(s.confidence, 0);
    assert!(!s.benford_flag);
    assert!(!s.ml_flag);
}

/// score=100 + benford_flag=false + ml_flag=false — maximum numeric risk but
/// no flags set (flags and score are contradictory).
#[test]
fn test_max_score_no_flags() {
    let (env, client, _admin, _service, wallet, pair) = setup();
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &100,
        &false, // no benford flag despite max score
        &false, // no ml flag despite max score
        &1,
        &90,
        &1,
        &None,
    );
    let s = client.get_score(&wallet, &pair);
    assert_eq!(s.score, 100);
    assert!(!s.benford_flag);
    assert!(!s.ml_flag);
}

/// score=0 + benford_flag=true — minimum score but anomaly flag set.
#[test]
fn test_min_score_with_benford_flag() {
    let (env, client, _admin, _service, wallet, pair) = setup();
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &0,
        &true, // benford flag with zero score
        &false,
        &1,
        &50,
        &1,
        &None,
    );
    let s = client.get_score(&wallet, &pair);
    assert_eq!(s.score, 0);
    assert!(s.benford_flag);
    assert!(!s.ml_flag);
}

/// score=50 + benford_flag=true + ml_flag=false — mid-range score with
/// only one flag set.
#[test]
fn test_mid_score_mixed_flags() {
    let (env, client, _admin, _service, wallet, pair) = setup();
    client.submit_score(&Vec::new(&env), &wallet, &pair, &50, &true, &false, &1, &75, &1, &None);
    let s = client.get_score(&wallet, &pair);
    assert_eq!(s.score, 50);
    assert!(s.benford_flag);
    assert!(!s.ml_flag);
}

/// All fields at boundary minimums: score=0, confidence=0, no flags.
#[test]
fn test_all_minimums() {
    let (env, client, _admin, _service, wallet, pair) = setup();
    client.submit_score(&Vec::new(&env), &wallet, &pair, &0, &false, &false, &1, &0, &1, &None);
    let s = client.get_score(&wallet, &pair);
    assert_eq!(s.score, 0);
    assert_eq!(s.confidence, 0);
    assert!(!s.benford_flag);
    assert!(!s.ml_flag);
}

/// All fields at boundary maximums: score=100, confidence=100, both flags.
#[test]
fn test_all_maximums() {
    let (env, client, _admin, _service, wallet, pair) = setup();
    client.submit_score(&Vec::new(&env), &wallet, &pair, &100, &true, &true, &1, &100, &1, &None);
    let s = client.get_score(&wallet, &pair);
    assert_eq!(s.score, 100);
    assert_eq!(s.confidence, 100);
    assert!(s.benford_flag);
    assert!(s.ml_flag);
}

/// model_version=0 with valid score — zero model version must be accepted
/// (consensus submissions use model_version=0).
#[test]
fn test_model_version_zero_accepted() {
    let (env, client, _admin, _service, wallet, pair) = setup();
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &42,
        &false,
        &true,
        &1,
        &80,
        &0, // model_version = 0
        &None,
    );
    let s = client.get_score(&wallet, &pair);
    assert_eq!(s.score, 42);
    assert_eq!(s.model_version, 0);
}

/// High model version number with no flags and low score — tests u32 max path.
#[test]
fn test_high_model_version() {
    let (env, client, _admin, _service, wallet, pair) = setup();
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &1,
        &false,
        &false,
        &1,
        &99,
        &u32::MAX,
        &None,
    );
    let s = client.get_score(&wallet, &pair);
    assert_eq!(s.model_version, u32::MAX);
}

/// get_score returns exactly what was submitted — no field mutation anywhere.
#[test]
fn test_no_silent_field_mutation() {
    let (env, client, _admin, _service, wallet, pair) = setup();
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &37,
        &true,
        &false,
        &999_999,
        &62,
        &7,
        &None,
    );
    let s = client.get_score(&wallet, &pair);
    assert_eq!(s.score, 37, "score mutated");
    assert!(s.benford_flag, "benford_flag mutated");
    assert!(!s.ml_flag, "ml_flag mutated");
    assert_eq!(s.timestamp, 999_999, "timestamp mutated");
    assert_eq!(s.confidence, 62, "confidence mutated");
    assert_eq!(s.model_version, 7, "model_version mutated");
}

// ── Batch path: mixed-signal entries ─────────────────────────────────────────

/// Batch with five contradictory entries — all five must be accepted since
/// they are syntactically valid.
#[test]
fn test_batch_mixed_contradictory_signals() {
    use crate::ScoreSubmission;

    let (env, client, _admin, _service, _wallet, pair) = setup();

    let w1 = Address::generate(&env);
    let w2 = Address::generate(&env);
    let w3 = Address::generate(&env);
    let w4 = Address::generate(&env);
    let w5 = Address::generate(&env);

    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    // Entry 0: score=0, both flags, max confidence
    batch.push_back(ScoreSubmission {
        wallet: w1.clone(),
        asset_pair: pair.clone(),
        score: 0,
        benford_flag: true,
        ml_flag: true,
        timestamp: 1,
        confidence: 100,
        model_version: 1,
    });
    // Entry 1: score=100, no flags, zero confidence
    batch.push_back(ScoreSubmission {
        wallet: w2.clone(),
        asset_pair: pair.clone(),
        score: 100,
        benford_flag: false,
        ml_flag: false,
        timestamp: 1,
        confidence: 0,
        model_version: 1,
    });
    // Entry 2: score=50, benford flag only
    batch.push_back(ScoreSubmission {
        wallet: w3.clone(),
        asset_pair: pair.clone(),
        score: 50,
        benford_flag: true,
        ml_flag: false,
        timestamp: 1,
        confidence: 50,
        model_version: 1,
    });
    // Entry 3: score=1, ml flag only
    batch.push_back(ScoreSubmission {
        wallet: w4.clone(),
        asset_pair: pair.clone(),
        score: 1,
        benford_flag: false,
        ml_flag: true,
        timestamp: 1,
        confidence: 99,
        model_version: 1,
    });
    // Entry 4: score=99, both flags, confidence=1
    batch.push_back(ScoreSubmission {
        wallet: w5.clone(),
        asset_pair: pair.clone(),
        score: 99,
        benford_flag: true,
        ml_flag: true,
        timestamp: 1,
        confidence: 1,
        model_version: 1,
    });

    let result = client.submit_scores_batch(&batch);
    assert_eq!(result.accepted_count, 5, "all contradictory-but-valid entries must be accepted");
    assert_eq!(result.rejected_count, 0);

    // Verify stored values match what was submitted
    assert_eq!(client.get_score(&w1, &pair).score, 0);
    assert_eq!(client.get_score(&w2, &pair).score, 100);
    assert_eq!(client.get_score(&w3, &pair).score, 50);
    assert_eq!(client.get_score(&w4, &pair).score, 1);
    assert_eq!(client.get_score(&w5, &pair).score, 99);
}

/// Batch with one invalid entry (score > 100) mixed with valid contradictory
/// entries — only the invalid one should be rejected.
#[test]
fn test_batch_invalid_mixed_with_contradictory_valid() {
    use crate::ScoreSubmission;

    let (env, client, _admin, _service, _wallet, pair) = setup();

    let valid_wallet = Address::generate(&env);
    let invalid_wallet = Address::generate(&env);

    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    // Valid contradictory entry
    batch.push_back(ScoreSubmission {
        wallet: valid_wallet.clone(),
        asset_pair: pair.clone(),
        score: 0,
        benford_flag: true,
        ml_flag: true,
        timestamp: 1,
        confidence: 100,
        model_version: 1,
    });
    // Invalid: score > 100
    batch.push_back(ScoreSubmission {
        wallet: invalid_wallet.clone(),
        asset_pair: pair.clone(),
        score: 101,
        benford_flag: false,
        ml_flag: false,
        timestamp: 1,
        confidence: 50,
        model_version: 1,
    });

    let result = client.submit_scores_batch(&batch);
    assert_eq!(result.accepted_count, 1);
    assert_eq!(result.rejected_count, 1);

    let valid_res = result.results.get(0).unwrap();
    assert!(valid_res.accepted);
    assert_eq!(valid_res.rejection_code, 0);

    let invalid_res = result.results.get(1).unwrap();
    assert!(!invalid_res.accepted);
    assert_eq!(invalid_res.rejection_code, crate::Error::InvalidScore as u32);
}
