use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol, Vec};

use crate::{constants, Error, LedgerLensScoreContract};

mod setup {
    use super::*;
    
    pub fn initialized(env: &Env) -> (Address, Address) {
        env.mock_all_auths();
        let admin = Address::generate(env);
        let service = Address::generate(env);
        LedgerLensScoreContract::initialize(env, admin.clone(), service.clone());
        (admin, service)
    }
}

/// Test that CONTRACT_VERSION has been bumped to 3
#[test]
fn test_contract_version_bumped_to_3() {
    let env = Env::default();
    let _setup = setup::initialized(&env);
    let version = LedgerLensScoreContract::get_version(env);
    assert_eq!(version, 3);
}

/// Test that get_decay_rate defaults to (0, 1) before any configuration
#[test]
fn test_get_decay_rate_defaults_to_zero() {
    let env = Env::default();
    let _setup = setup::initialized(&env);
    let (num, den) = LedgerLensScoreContract::get_decay_rate(env.clone());
    assert_eq!(num, 0);
    assert_eq!(den, 1);
}

/// Test that set_decay_rate accepts valid configurations
#[test]
fn test_set_decay_rate_valid_accepted() {
    let env = Env::default();
    let _setup = setup::initialized(&env);
    
    // Set to 0.001 (valid)
    let result = LedgerLensScoreContract::try_set_decay_rate(env.clone(), 1, 1000);
    assert_eq!(result.ok(), Some(()));
    let (num, den) = LedgerLensScoreContract::get_decay_rate(env.clone());
    assert_eq!(num, 1);
    assert_eq!(den, 1000);
}

/// Test that set_decay_rate rejects rates above the maximum
#[test]
fn test_set_decay_rate_above_max_rejected() {
    let env = Env::default();
    let _setup = setup::initialized(&env);
    
    // Try to set λ = 0.02 (exceeds MAX_DECAY_LAMBDA = 0.01)
    let result = LedgerLensScoreContract::try_set_decay_rate(env.clone(), 2, 100);
    assert_eq!(result, Err(Ok(Error::InvalidDecayRate)));
}

/// Test that set_decay_rate with denominator 0 is rejected
#[test]
fn test_set_decay_rate_zero_denominator_rejected() {
    let env = Env::default();
    let _setup = setup::initialized(&env);
    
    let result = LedgerLensScoreContract::try_set_decay_rate(env.clone(), 1, 0);
    assert_eq!(result, Err(Ok(Error::InvalidDecayRate)));
}

/// Test that set_decay_rate is blocked when contract is paused
#[test]
fn test_set_decay_rate_blocked_when_paused() {
    let env = Env::default();
    let _setup = setup::initialized(&env);
    
    // Pause the contract
    let _pause_result = LedgerLensScoreContract::pause(env.clone());
    
    // Try to set decay rate
    let result = LedgerLensScoreContract::try_set_decay_rate(env.clone(), 1, 1000);
    assert_eq!(result, Err(Ok(Error::ContractPaused)));
}

/// Test that max-allowed decay rate is accepted at boundary
#[test]
fn test_set_decay_rate_max_boundary_accepted() {
    let env = Env::default();
    let _setup = setup::initialized(&env);
    
    // MAX_DECAY_LAMBDA = 0.01 = 1/100
    assert_eq!(
        LedgerLensScoreContract::try_set_decay_rate(env.clone(), 1, 100).ok(),
        Some(())
    );
    let (num, den) = LedgerLensScoreContract::get_decay_rate(env.clone());
    assert_eq!(num, 1);
    assert_eq!(den, 100);
}

/// Test that just-below max decay rate is accepted
#[test]
fn test_set_decay_rate_below_max_boundary_accepted() {
    let env = Env::default();
    let _setup = setup::initialized(&env);
    
    // λ = 0.001 < 0.01 (should be accepted)
    assert_eq!(
        LedgerLensScoreContract::try_set_decay_rate(env.clone(), 1, 1000).ok(),
        Some(())
    );
    let (num, den) = LedgerLensScoreContract::get_decay_rate(env.clone());
    assert_eq!(num, 1);
    assert_eq!(den, 1000);
}

/// Test that decay_rate = (0, 1) produces identical results to the pre-decay
/// static average computation
#[test]
fn test_decay_rate_zero_reproduces_static_average() {
    let env = Env::default();
    let _setup = setup::initialized(&env);
    
    let wallet = Address::generate(&env);
    let pair1 = symbol_short!("XLMU");
    let pair2 = symbol_short!("XLMB");
    
    // Ensure decay is disabled
    let (num, den) = LedgerLensScoreContract::get_decay_rate(env.clone());
    assert_eq!(num, 0); // Default is no decay
    
    // Submit two scores with different weights and scores
    LedgerLensScoreContract::set_pair_weight(env.clone(), pair1, 1)
        .expect("set pair weight failed");
    LedgerLensScoreContract::set_pair_weight(env.clone(), pair2, 2)
        .expect("set pair weight failed");
    
    LedgerLensScoreContract::submit_score(
        env.clone(),
        Vec::new(&env),
        wallet.clone(),
        pair1,
        30,
        false,
        false,
        100,
        90,
        1,
        None,
    )
    .expect("submit_score failed");
    
    LedgerLensScoreContract::submit_score(
        env.clone(),
        Vec::new(&env),
        wallet.clone(),
        pair2,
        60,
        false,
        false,
        200,
        85,
        1,
        None,
    )
    .expect("submit_score failed");
    
    let aggregate = LedgerLensScoreContract::get_aggregate_score(env.clone(), wallet)
        .expect("get_aggregate_score failed");
    
    // Expected: (30*1 + 60*2) / (1 + 2) = 150 / 3 = 50
    assert_eq!(aggregate.aggregate_score, 50);
    assert!(!aggregate.decay_lambda_applied);
}

/// Test that non-zero decay rate weights older scores less heavily
#[test]
fn test_decay_rate_nonzero_reduces_old_scores() {
    let env = Env::default();
    let _setup = setup::initialized(&env);
    
    let wallet = Address::generate(&env);
    let pair_a = symbol_short!("XLMA");
    let pair_b = symbol_short!("XLMB");
    
    // Set equal weights
    LedgerLensScoreContract::set_pair_weight(env.clone(), pair_a, 1)
        .expect("set pair weight failed");
    LedgerLensScoreContract::set_pair_weight(env.clone(), pair_b, 1)
        .expect("set pair weight failed");
    
    // Set decay rate to λ = 0.001 per second
    LedgerLensScoreContract::set_decay_rate(env.clone(), 1, 1000)
        .expect("set_decay_rate failed");
    
    let current_ts = env.ledger().timestamp();
    
    // Score A: old (100 seconds in the past), high score
    let score_a_ts = current_ts.saturating_sub(100);
    LedgerLensScoreContract::submit_score(
        env.clone(),
        Vec::new(&env),
        wallet.clone(),
        pair_a,
        100,
        false,
        false,
        score_a_ts,
        90,
        1,
        None,
    )
    .expect("submit_score failed");
    
    // Advance time
    env.ledger().with_mut(|l| l.timestamp = current_ts + 50);
    
    // Score B: fresh, lower score
    let score_b_ts = env.ledger().timestamp();
    LedgerLensScoreContract::submit_score(
        env.clone(),
        Vec::new(&env),
        wallet.clone(),
        pair_b,
        50,
        false,
        false,
        score_b_ts,
        85,
        1,
        None,
    )
    .expect("submit_score failed");
    
    let aggregate = LedgerLensScoreContract::get_aggregate_score(env.clone(), wallet)
        .expect("get_aggregate_score failed");
    
    // With decay, older score A should be weighted less
    // Aggregate should be closer to 50 than to 75 (the no-decay average)
    assert!(aggregate.aggregate_score < 75);
    assert!(aggregate.decay_lambda_applied);
}

/// Test that AggregateRiskScore.decay_lambda_applied is false when decay is 0
#[test]
fn test_aggregate_decay_lambda_applied_false_when_zero() {
    let env = Env::default();
    let _setup = setup::initialized(&env);
    
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLMU");
    
    // Ensure decay is off (default)
    let (num, _den) = LedgerLensScoreContract::get_decay_rate(env.clone());
    assert_eq!(num, 0);
    
    LedgerLensScoreContract::submit_score(
        env.clone(),
        Vec::new(&env),
        wallet.clone(),
        pair,
        50,
        false,
        false,
        100,
        90,
        1,
        None,
    )
    .expect("submit_score failed");
    
    let aggregate = LedgerLensScoreContract::get_aggregate_score(env.clone(), wallet)
        .expect("get_aggregate_score failed");
    assert!(!aggregate.decay_lambda_applied);
}

/// Test that AggregateRiskScore.decay_lambda_applied is true when decay is non-zero
#[test]
fn test_aggregate_decay_lambda_applied_true_when_nonzero() {
    let env = Env::default();
    let _setup = setup::initialized(&env);
    
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLMU");
    
    LedgerLensScoreContract::set_decay_rate(env.clone(), 1, 1000)
        .expect("set_decay_rate failed");
    
    LedgerLensScoreContract::submit_score(
        env.clone(),
        Vec::new(&env),
        wallet.clone(),
        pair,
        50,
        false,
        false,
        100,
        90,
        1,
        None,
    )
    .expect("submit_score failed");
    
    let aggregate = LedgerLensScoreContract::get_aggregate_score(env.clone(), wallet)
        .expect("get_aggregate_score failed");
    assert!(aggregate.decay_lambda_applied);
}

/// Test that decay applies consistently across pairs
#[test]
fn test_decay_applies_consistently_across_pairs() {
    let env = Env::default();
    let _setup = setup::initialized(&env);
    
    let wallet = Address::generate(&env);
    let pairs = vec![symbol_short!("PXL1"), symbol_short!("PXL2"), symbol_short!("PXL3")];
    
    LedgerLensScoreContract::set_decay_rate(env.clone(), 1, 500)
        .expect("set_decay_rate failed");
    
    // Submit equal scores for all pairs with same age
    let ts = env.ledger().timestamp();
    for pair in &pairs {
        LedgerLensScoreContract::set_pair_weight(env.clone(), *pair, 1)
            .expect("set_pair_weight failed");
        LedgerLensScoreContract::submit_score(
            env.clone(),
            Vec::new(&env),
            wallet.clone(),
            *pair,
            60,
            false,
            false,
            ts,
            90,
            1,
            None,
        )
        .expect("submit_score failed");
    }
    
    let aggregate = LedgerLensScoreContract::get_aggregate_score(env.clone(), wallet)
        .expect("get_aggregate_score failed");
    
    // All pairs have same age → same decay factor → aggregate should be 60
    assert_eq!(aggregate.aggregate_score, 60);
    assert_eq!(aggregate.pair_count, 3);
}
