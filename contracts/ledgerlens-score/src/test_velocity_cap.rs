#![cfg(test)]
#![allow(unused_imports)]

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{
    test::initialized, test::setup, Error, LedgerLensScoreContract, LedgerLensScoreContractClient,
    ScoreSubmission,
};

#[test]
fn test_default_velocity_cap_disabled() {
    let (_env, client, _admin, _service) = initialized();
    let cap = client.get_score_velocity_cap();
    assert!(!cap.enabled);
    assert_eq!(cap.points_per_hour, 0);
}

#[test]
fn test_set_velocity_cap() {
    let (env, client, admin, _service) = initialized();

    client.set_score_velocity_cap(&Vec::from_array(&env, [admin.clone()]), &true, &10);

    let cap = client.get_score_velocity_cap();
    assert!(cap.enabled);
    assert_eq!(cap.points_per_hour, 10);
}

#[test]
fn test_velocity_cap_disabled_allows_large_delta() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    // First submission (baseline)
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &10,
        &false,
        &false,
        &1,
        &90,
        &1,
        &None,
    );

    // Advance time past cooldown (1 hour)
    env.ledger().with_mut(|l| l.timestamp += 3601);

    // Submitting with delta 80 -> should succeed since cap is disabled
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &90,
        &false,
        &false,
        &2,
        &90,
        &1,
        &None,
    );

    assert_eq!(client.get_score(&wallet, &asset_pair).score, 90);
}

#[test]
fn test_velocity_cap_first_submission_unaffected() {
    let (env, client, admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    // Enable cap
    client.set_score_velocity_cap(&Vec::from_array(&env, [admin.clone()]), &true, &10);

    // First submission can be large since there's no baseline
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &80,
        &false,
        &false,
        &1,
        &90,
        &1,
        &None,
    );

    assert_eq!(client.get_score(&wallet, &asset_pair).score, 80);
}

#[test]
fn test_velocity_cap_rejects_large_delta() {
    let (env, client, admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    // Enable cap (10 points per hour)
    client.set_score_velocity_cap(&Vec::from_array(&env, [admin.clone()]), &true, &10);

    // Baseline
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &10,
        &false,
        &false,
        &1,
        &90,
        &1,
        &None,
    );

    // Advance time by 1 hour + 1 second
    env.ledger().with_mut(|l| l.timestamp += 3601);

    // Delta 10 should be allowed (10 * 3601 / 3600 = 10)
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &20,
        &false,
        &false,
        &2,
        &90,
        &1,
        &None,
    );

    // Advance time by 1 hour
    env.ledger().with_mut(|l| l.timestamp += 3600);

    // Delta 12 should be rejected (limit is 10)
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &32,
        &false,
        &false,
        &3,
        &90,
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::ScoreVelocityExceeded)));

    // Ensure previous score remains
    assert_eq!(client.get_score(&wallet, &asset_pair).score, 20);
}

#[test]
fn test_velocity_cap_allows_large_delta_over_long_time() {
    let (env, client, admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    // Enable cap (10 points per hour)
    client.set_score_velocity_cap(&Vec::from_array(&env, [admin.clone()]), &true, &10);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &10,
        &false,
        &false,
        &1,
        &90,
        &1,
        &None,
    );

    // Advance time by 5 hours
    env.ledger().with_mut(|l| l.timestamp += 5 * 3600);

    // Delta 50 should be allowed (10 points * 5 hours)
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &60,
        &false,
        &false,
        &2,
        &90,
        &1,
        &None,
    );

    assert_eq!(client.get_score(&wallet, &asset_pair).score, 60);
}

#[test]
fn test_velocity_cap_override_bypass() {
    let (env, client, admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    // Enable cap (10 points per hour)
    client.set_score_velocity_cap(&Vec::from_array(&env, [admin.clone()]), &true, &10);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &10,
        &false,
        &false,
        &1,
        &90,
        &1,
        &None,
    );

    // Advance time by 1 hour
    env.ledger().with_mut(|l| l.timestamp += 3600);

    // Trying delta 50 fails normally
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &60,
        &false,
        &false,
        &2,
        &90,
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::ScoreVelocityExceeded)));

    // Admin sets override
    client.override_score_velocity_cap(
        &Vec::from_array(&env, [admin.clone()]),
        &wallet,
        &asset_pair,
    );

    // Retrying delta 50 succeeds because of override
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &60,
        &false,
        &false,
        &3,
        &90,
        &1,
        &None,
    );
    assert_eq!(client.get_score(&wallet, &asset_pair).score, 60);

    // Advance time by another hour
    env.ledger().with_mut(|l| l.timestamp += 3600);

    // Override is consumed, subsequent large delta fails
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &90,
        &false,
        &false,
        &4,
        &90,
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::ScoreVelocityExceeded)));
}

#[test]
fn test_batch_submission_velocity_cap() {
    let (env, client, admin, _service) = initialized();

    // Enable cap (10 points per hour)
    client.set_score_velocity_cap(&Vec::from_array(&env, [admin.clone()]), &true, &10);

    let wallet1 = Address::generate(&env);
    let wallet2 = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    // Establish baselines
    let mut batch1: Vec<ScoreSubmission> = Vec::new(&env);
    batch1.push_back(ScoreSubmission {
        wallet: wallet1.clone(),
        asset_pair: asset_pair.clone(),
        score: 20,
        benford_flag: false,
        ml_flag: false,
        timestamp: 100,
        confidence: 90,
        model_version: 1,
    });
    batch1.push_back(ScoreSubmission {
        wallet: wallet2.clone(),
        asset_pair: asset_pair.clone(),
        score: 20,
        benford_flag: false,
        ml_flag: false,
        timestamp: 100,
        confidence: 90,
        model_version: 1,
    });
    client.submit_scores_batch(&batch1);

    // Advance 1 hour
    env.ledger().with_mut(|l| l.timestamp += 3600);

    // Next batch: Wallet 1 stays within cap (+10), Wallet 2 exceeds cap (+20)
    let mut batch2: Vec<ScoreSubmission> = Vec::new(&env);
    batch2.push_back(ScoreSubmission {
        wallet: wallet1.clone(),
        asset_pair: asset_pair.clone(),
        score: 30,
        benford_flag: false,
        ml_flag: false,
        timestamp: 200,
        confidence: 90,
        model_version: 1,
    });
    batch2.push_back(ScoreSubmission {
        wallet: wallet2.clone(),
        asset_pair: asset_pair.clone(),
        score: 40,
        benford_flag: false,
        ml_flag: false,
        timestamp: 200,
        confidence: 90,
        model_version: 1,
    });

    let result = client.submit_scores_batch(&batch2);

    assert_eq!(result.accepted_count, 1);
    assert_eq!(result.rejected_count, 1);

    assert_eq!(result.results.get(0).unwrap().accepted, true);
    assert_eq!(result.results.get(1).unwrap().accepted, false);
    assert_eq!(result.results.get(1).unwrap().rejection_code, Error::ScoreVelocityExceeded as u32);

    assert_eq!(client.get_score(&wallet1, &asset_pair).score, 30);
    assert_eq!(client.get_score(&wallet2, &asset_pair).score, 20); // Unchanged
}
