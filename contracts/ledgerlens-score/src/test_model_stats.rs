use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient, ScoreSubmission};

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

#[test]
fn test_first_submission_initializes_stats() {
    let (env, client, _, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let model_version = 1;
    let score = 50;

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &score,
        &false,
        &false,
        &1,
        &90,
        &model_version,
        &None,
    );

    let stats = client.get_model_version_stats(&model_version);
    assert_eq!(stats.model_version, model_version);
    assert_eq!(stats.submission_count, 1);
    assert_eq!(stats.score_sum, score as u64);
    assert_eq!(stats.score_max, score);
    assert_eq!(stats.score_min, score);
    assert_eq!(stats.first_seen, env.ledger().timestamp());
    assert_eq!(stats.last_seen, env.ledger().timestamp());

    let versions = client.get_all_model_versions();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions.get(0).unwrap(), model_version);
}

#[test]
fn test_subsequent_submissions_update_stats() {
    let (env, client, _, _) = setup();
    let wallet1 = Address::generate(&env);
    let wallet2 = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let model_version = 1;

    // First submission
    client.submit_score(
        &Vec::new(&env),
        &wallet1,
        &pair,
        &40,
        &false,
        &false,
        &1,
        &90,
        &model_version,
        &None,
    );

    // Advance time
    env.ledger().with_mut(|l| l.timestamp += 3600);

    // Second submission
    client.submit_score(
        &Vec::new(&env),
        &wallet2,
        &pair,
        &60,
        &false,
        &false,
        &2,
        &90,
        &model_version,
        &None,
    );

    let stats = client.get_model_version_stats(&model_version);
    assert_eq!(stats.submission_count, 2);
    assert_eq!(stats.score_sum, 100);
    assert_eq!(stats.score_max, 60);
    assert_eq!(stats.score_min, 40);
    assert_eq!(stats.last_seen, env.ledger().timestamp());
}

#[test]
fn test_batch_submissions_update_stats() {
    let (env, client, _, _) = setup();
    let wallet1 = Address::generate(&env);
    let wallet2 = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: wallet1,
        asset_pair: pair.clone(),
        score: 30,
        benford_flag: false,
        ml_flag: false,
        timestamp: 1,
        confidence: 80,
        model_version: 1,
    });
    batch.push_back(ScoreSubmission {
        wallet: wallet2,
        asset_pair: pair.clone(),
        score: 70,
        benford_flag: false,
        ml_flag: false,
        timestamp: 2,
        confidence: 80,
        model_version: 2,
    });

    client.submit_scores_batch(&batch);

    let stats1 = client.get_model_version_stats(&1);
    assert_eq!(stats1.submission_count, 1);
    assert_eq!(stats1.score_sum, 30);

    let stats2 = client.get_model_version_stats(&2);
    assert_eq!(stats2.submission_count, 1);
    assert_eq!(stats2.score_sum, 70);

    let versions = client.get_all_model_versions();
    assert_eq!(versions.len(), 2);
}

#[test]
fn test_unknown_version_returns_error() {
    let (_, client, _, _) = setup();
    let result = client.try_get_model_version_stats(&999);
    assert_eq!(result, Err(Ok(Error::NotFound)));
}

#[test]
fn test_get_all_model_versions_is_sorted() {
    let (env, client, _, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Submit in non-sorted order of versions
    client.submit_score(&Vec::new(&env), &wallet, &pair, &50, &false, &false, &1, &90, &3, &None);
    env.ledger().with_mut(|l| l.timestamp += 3600);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &50, &false, &false, &2, &90, &1, &None);
    env.ledger().with_mut(|l| l.timestamp += 3600);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &50, &false, &false, &3, &90, &2, &None);

    let versions = client.get_all_model_versions();
    assert_eq!(versions.len(), 3);
    assert_eq!(versions.get(0).unwrap(), 1);
    assert_eq!(versions.get(1).unwrap(), 2);
    assert_eq!(versions.get(2).unwrap(), 3);
}
