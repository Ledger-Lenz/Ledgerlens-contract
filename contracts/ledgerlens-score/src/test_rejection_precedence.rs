/// #689 — Deterministic rejection-precedence tests.
///
/// Verifies that when a single batch entry violates multiple validation rules
/// simultaneously, exactly the highest-priority rule's rejection code is
/// returned, and that codes are stable and distinct across all combinations.
///
/// Precedence order (1 = highest):
/// 1. PairPaused      (code 7)   — pair frozen
/// 2. InvalidScore    (code 4)   — score > 100
/// 3. InvalidConfidence (code 5) — confidence > 100
/// 4. InvalidTimestamp (code 25) — timestamp == 0
/// 5. ModelVersion*   (varies)   — version checks
/// 6. RateLimitExceeded (code 23)
/// 7. BelowScoreFloor  (code 43)  — DISTINCT from InvalidScore (4)
#[cfg(test)]
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger},
    Address, Env, Vec,
};

#[cfg(test)]
use crate::{Error, LedgerLensScoreContract, ScoreSubmission};

#[cfg(test)]
fn setup(
) -> (Env, crate::LedgerLensScoreContractClient<'static>, Address, Address, soroban_sdk::Symbol) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = crate::LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    env.ledger().with_mut(|l| l.timestamp = 5_000_000);
    let pair = symbol_short!("XLM_USDC");
    (env, client, admin, service, pair)
}

// ── Priority 1: PairPaused wins over everything ───────────────────────────────

/// PairPaused (7) beats InvalidScore (4) when score > 100 AND pair is paused.
#[test]
fn test_pair_paused_beats_invalid_score() {
    let (env, client, _admin, _service, pair) = setup();
    client.set_pair_paused(&pair, &true);

    let wallet = Address::generate(&env);
    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: wallet.clone(),
        asset_pair: pair.clone(),
        score: 200, // also invalid
        benford_flag: false,
        ml_flag: false,
        timestamp: 1,
        confidence: 50,
        model_version: 1,
    });

    let result = client.submit_scores_batch(&batch);
    let entry = result.results.get(0).unwrap();
    assert!(!entry.accepted);
    // PairPaused = ContractPaused = 7, must beat InvalidScore = 4
    assert_eq!(
        entry.rejection_code,
        Error::ContractPaused as u32,
        "PairPaused (code 7) must take priority over InvalidScore (code 4)"
    );
}

/// PairPaused beats invalid confidence.
#[test]
fn test_pair_paused_beats_invalid_confidence() {
    let (env, client, _admin, _service, pair) = setup();
    client.set_pair_paused(&pair, &true);

    let wallet = Address::generate(&env);
    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet,
        asset_pair: pair.clone(),
        score: 50,
        benford_flag: false,
        ml_flag: false,
        timestamp: 1,
        confidence: 200, // also invalid
        model_version: 1,
    });

    let result = client.submit_scores_batch(&batch);
    let entry = result.results.get(0).unwrap();
    assert!(!entry.accepted);
    assert_eq!(
        entry.rejection_code,
        Error::ContractPaused as u32,
        "PairPaused must beat InvalidConfidence"
    );
}

// ── Priority 2: InvalidScore (4) beats lower priorities ──────────────────────

/// InvalidScore (4) beats InvalidConfidence (5) when both score > 100 and confidence > 100.
#[test]
fn test_invalid_score_beats_invalid_confidence() {
    let (env, client, _admin, _service, pair) = setup();

    let wallet = Address::generate(&env);
    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet,
        asset_pair: pair.clone(),
        score: 101, // invalid score
        benford_flag: false,
        ml_flag: false,
        timestamp: 1,
        confidence: 101, // also invalid confidence
        model_version: 1,
    });

    let result = client.submit_scores_batch(&batch);
    let entry = result.results.get(0).unwrap();
    assert!(!entry.accepted);
    assert_eq!(
        entry.rejection_code,
        Error::InvalidScore as u32, // code 4, checked before confidence
        "InvalidScore (4) must beat InvalidConfidence (5)"
    );
}

/// InvalidScore (4) beats InvalidTimestamp (25) when score > 100 and timestamp == 0.
#[test]
fn test_invalid_score_beats_invalid_timestamp() {
    let (env, client, _admin, _service, pair) = setup();

    let wallet = Address::generate(&env);
    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet,
        asset_pair: pair.clone(),
        score: 999, // invalid
        benford_flag: false,
        ml_flag: false,
        timestamp: 0, // also invalid
        confidence: 50,
        model_version: 1,
    });

    let result = client.submit_scores_batch(&batch);
    let entry = result.results.get(0).unwrap();
    assert!(!entry.accepted);
    assert_eq!(
        entry.rejection_code,
        Error::InvalidScore as u32,
        "InvalidScore (4) must beat InvalidTimestamp (25)"
    );
}

// ── Priority 3: InvalidConfidence beats lower priorities ─────────────────────

/// InvalidConfidence (5) beats InvalidTimestamp (25).
#[test]
fn test_invalid_confidence_beats_invalid_timestamp() {
    let (env, client, _admin, _service, pair) = setup();

    let wallet = Address::generate(&env);
    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet,
        asset_pair: pair.clone(),
        score: 50, // valid score
        benford_flag: false,
        ml_flag: false,
        timestamp: 0,    // invalid timestamp
        confidence: 200, // invalid confidence — checked before timestamp
        model_version: 1,
    });

    let result = client.submit_scores_batch(&batch);
    let entry = result.results.get(0).unwrap();
    assert!(!entry.accepted);
    assert_eq!(
        entry.rejection_code,
        Error::InvalidConfidence as u32,
        "InvalidConfidence (5) must beat InvalidTimestamp (25)"
    );
}

// ── Priority 7: BelowScoreFloor uses code 43, not code 4 ─────────────────────

/// BelowScoreFloor rejection_code must be 43, NOT 4 (InvalidScore).
/// This is the core fix for #689: the two rejection reasons are
/// distinguishable by their numeric codes.
#[test]
fn test_below_score_floor_uses_code_43_not_4() {
    let (env, client, _admin, _service, pair) = setup();

    // Enable floor: HWM=80, floor=20
    client.set_score_floor_policy(&Vec::new(&env), &true, &80, &20);

    // Raise the wallet's historical max to 85 (above HWM of 80)
    let wallet = Address::generate(&env);
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &85, // above HWM
        &false,
        &false,
        &1,
        &90,
        &1,
        &None,
    );

    // Advance past cooldown
    env.ledger().with_mut(|l| l.timestamp += 3_601);

    // Now try to submit score=5 (below floor of 20) via batch
    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: wallet.clone(),
        asset_pair: pair.clone(),
        score: 5, // below floor
        benford_flag: false,
        ml_flag: false,
        timestamp: 1,
        confidence: 80,
        model_version: 1,
    });

    let result = client.submit_scores_batch(&batch);
    let entry = result.results.get(0).unwrap();
    assert!(!entry.accepted);

    // Must be 43 (BelowScoreFloor), NOT 4 (InvalidScore)
    assert_eq!(
        entry.rejection_code, 43u32,
        "BelowScoreFloor must emit rejection_code=43, not InvalidScore=4"
    );
    assert_ne!(
        entry.rejection_code,
        Error::InvalidScore as u32,
        "rejection_code 43 must be distinct from InvalidScore (4)"
    );
}

/// Score > 100 (code 4) and BelowScoreFloor (code 43) have different codes —
/// confirms the two are numerically distinguishable even though they share
/// an alias in the Error enum.
#[test]
fn test_invalid_score_code_distinct_from_floor_code() {
    assert_ne!(
        Error::InvalidScore as u32,
        43u32,
        "InvalidScore (code 4) must be numerically distinct from BelowScoreFloor (code 43)"
    );
    assert_eq!(Error::InvalidScore as u32, 4u32);
}

// ── RateLimitExceeded comes after score/confidence/timestamp checks ──────────

/// RateLimitExceeded (23) is NOT returned for score > 100; score check wins.
#[test]
fn test_rate_limit_does_not_override_invalid_score() {
    let (env, client, _admin, _service, pair) = setup();

    // Submit once to trigger cooldown
    let wallet = Address::generate(&env);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &50, &false, &false, &1, &80, &1, &None);

    // Immediately (cooldown not elapsed) try to submit with score > 100
    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: wallet.clone(),
        asset_pair: pair.clone(),
        score: 200, // invalid — AND within cooldown
        benford_flag: false,
        ml_flag: false,
        timestamp: 1,
        confidence: 50,
        model_version: 1,
    });

    let result = client.submit_scores_batch(&batch);
    let entry = result.results.get(0).unwrap();
    assert!(!entry.accepted);
    assert_eq!(
        entry.rejection_code,
        Error::InvalidScore as u32,
        "InvalidScore (4) must win over RateLimitExceeded (23)"
    );
}

// ── Valid sibling in batch is accepted despite invalid neighbours ─────────────

/// A valid entry surrounded by invalid neighbours must still be accepted.
#[test]
fn test_valid_entry_accepted_among_invalid_siblings() {
    let (env, client, _admin, _service, pair) = setup();

    let bad1 = Address::generate(&env);
    let good = Address::generate(&env);
    let bad2 = Address::generate(&env);

    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: bad1,
        asset_pair: pair.clone(),
        score: 200, // invalid
        benford_flag: false,
        ml_flag: false,
        timestamp: 1,
        confidence: 50,
        model_version: 1,
    });
    batch.push_back(ScoreSubmission {
        wallet: good.clone(),
        asset_pair: pair.clone(),
        score: 75, // valid
        benford_flag: true,
        ml_flag: false,
        timestamp: 1,
        confidence: 85,
        model_version: 1,
    });
    batch.push_back(ScoreSubmission {
        wallet: bad2,
        asset_pair: pair.clone(),
        score: 50,
        benford_flag: false,
        ml_flag: false,
        timestamp: 0, // invalid timestamp
        confidence: 60,
        model_version: 1,
    });

    let result = client.submit_scores_batch(&batch);
    assert_eq!(result.accepted_count, 1);
    assert_eq!(result.rejected_count, 2);

    let r0 = result.results.get(0).unwrap();
    let r1 = result.results.get(1).unwrap();
    let r2 = result.results.get(2).unwrap();

    assert!(!r0.accepted);
    assert_eq!(r0.rejection_code, Error::InvalidScore as u32);

    assert!(r1.accepted, "valid middle entry must be accepted");
    assert_eq!(r1.rejection_code, 0);

    assert!(!r2.accepted);
    assert_eq!(r2.rejection_code, Error::InvalidTimestamp as u32);

    // Confirm the good entry was actually stored
    assert_eq!(client.get_score(&good, &pair).score, 75);
}
