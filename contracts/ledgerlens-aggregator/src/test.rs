#![cfg(test)]

use crate::{ConflictPolicy, Error, LedgerLensAggregator, LedgerLensAggregatorClient};
use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
use soroban_sdk::{
    symbol_short,
    testutils::Address as _,
    Address, Env, Vec,
};

fn setup_score<'a>(env: &'a Env, admin: &Address, service: &Address) -> (Address, LedgerLensScoreContractClient<'a>) {
    let id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(env, &id);
    client.initialize(admin, service);
    (id, client)
}

fn submit_score(client: &LedgerLensScoreContractClient, env: &Env, wallet: &Address, asset_pair: &soroban_sdk::Symbol, score: u32, timestamp: u64) {
    client.submit_score(
        &Vec::new(env),
        wallet,
        asset_pair,
        &score,
        &false,
        &false,
        &timestamp,
        &90,
        &1,
        &None,
    );
}

#[test]
fn test_default_conflict_policy() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);

    let policy = client.get_conflict_resolution_policy();
    assert_eq!(policy, ConflictPolicy::HighestScore);
}

#[test]
fn test_set_and_get_conflict_resolution_policy() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    client.initialize(&admin);

    assert_eq!(client.get_conflict_resolution_policy(), ConflictPolicy::HighestScore);

    client.set_conflict_resolution_policy(&ConflictPolicy::MostRecent);
    assert_eq!(client.get_conflict_resolution_policy(), ConflictPolicy::MostRecent);

    client.set_conflict_resolution_policy(&ConflictPolicy::HighestScore);
    assert_eq!(client.get_conflict_resolution_policy(), ConflictPolicy::HighestScore);
}

#[test]
fn test_set_conflict_policy_uninitialized_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);

    let result = client.try_set_conflict_resolution_policy(&ConflictPolicy::MostRecent);
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_get_score_highest_score_policy() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let (shard1_id, shard1_client) = setup_score(&env, &admin, &service);
    // shard1: score=50, timestamp=100
    submit_score(&shard1_client, &env, &wallet, &pair, 50, 100);

    let (shard2_id, shard2_client) = setup_score(&env, &admin, &service);
    // shard2: score=90, timestamp=50 (higher score but older)
    submit_score(&shard2_client, &env, &wallet, &pair, 90, 50);

    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let agg_client = LedgerLensAggregatorClient::new(&env, &agg_id);
    agg_client.initialize(&admin);
    agg_client.add_shard(&shard1_id);
    agg_client.add_shard(&shard2_id);

    // Default policy is HighestScore — should return score 90 from shard2
    let score = agg_client.get_score(&wallet, &pair);
    assert_eq!(score.score, 90);
    assert_eq!(score.timestamp, 50);
}

#[test]
fn test_get_score_most_recent_policy() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let (shard1_id, shard1_client) = setup_score(&env, &admin, &service);
    // shard1: score=50, timestamp=100 (lower score but newer)
    submit_score(&shard1_client, &env, &wallet, &pair, 50, 100);

    let (shard2_id, shard2_client) = setup_score(&env, &admin, &service);
    // shard2: score=90, timestamp=50 (higher score but older)
    submit_score(&shard2_client, &env, &wallet, &pair, 90, 50);

    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let agg_client = LedgerLensAggregatorClient::new(&env, &agg_id);
    agg_client.initialize(&admin);
    agg_client.add_shard(&shard1_id);
    agg_client.add_shard(&shard2_id);

    // Switch to MostRecent policy — should return score 50 with timestamp 100
    agg_client.set_conflict_resolution_policy(&ConflictPolicy::MostRecent);
    let score = agg_client.get_score(&wallet, &pair);
    assert_eq!(score.score, 50);
    assert_eq!(score.timestamp, 100);
}

// ── Shard health (circuit-breaker) tests ─────────────────────────────────

#[test]
fn test_shard_health_default_true() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    let (shard_id, _) = setup_score(&env, &admin, &service);

    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let agg_client = LedgerLensAggregatorClient::new(&env, &agg_id);
    agg_client.initialize(&admin);
    agg_client.add_shard(&shard_id);

    // Default is healthy
    assert!(agg_client.get_shard_health(&shard_id));
}

#[test]
fn test_set_shard_health_mark_unhealthy() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    let (shard_id, _) = setup_score(&env, &admin, &service);

    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let agg_client = LedgerLensAggregatorClient::new(&env, &agg_id);
    agg_client.initialize(&admin);
    agg_client.add_shard(&shard_id);

    agg_client.set_shard_health(&shard_id, &false);
    assert!(!agg_client.get_shard_health(&shard_id));
}

#[test]
fn test_set_shard_health_restore() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    let (shard_id, _) = setup_score(&env, &admin, &service);

    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let agg_client = LedgerLensAggregatorClient::new(&env, &agg_id);
    agg_client.initialize(&admin);
    agg_client.add_shard(&shard_id);

    agg_client.set_shard_health(&shard_id, &false);
    assert!(!agg_client.get_shard_health(&shard_id));

    agg_client.set_shard_health(&shard_id, &true);
    assert!(agg_client.get_shard_health(&shard_id));
}

#[test]
fn test_set_shard_health_unregistered_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let unknown = Address::generate(&env);

    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let agg_client = LedgerLensAggregatorClient::new(&env, &agg_id);
    agg_client.initialize(&admin);

    let result = agg_client.try_set_shard_health(&unknown, &false);
    assert_eq!(result, Err(Ok(Error::ShardNotFound)));
}

#[test]
fn test_set_shard_health_uninitialized_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let shard = Address::generate(&env);

    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let agg_client = LedgerLensAggregatorClient::new(&env, &agg_id);

    let result = agg_client.try_set_shard_health(&shard, &false);
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_get_score_excludes_unhealthy_shard() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // shard1: score=90 (would win on HighestScore)
    let (shard1_id, shard1_client) = setup_score(&env, &admin, &service);
    submit_score(&shard1_client, &env, &wallet, &pair, 90, 100);

    // shard2: score=50 (would lose, but shard1 is unhealthy so this should win)
    let (shard2_id, shard2_client) = setup_score(&env, &admin, &service);
    submit_score(&shard2_client, &env, &wallet, &pair, 50, 200);

    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let agg_client = LedgerLensAggregatorClient::new(&env, &agg_id);
    agg_client.initialize(&admin);
    agg_client.add_shard(&shard1_id);
    agg_client.add_shard(&shard2_id);

    // Mark shard1 unhealthy — should be excluded, so shard2's score wins
    agg_client.set_shard_health(&shard1_id, &false);

    let score = agg_client.get_score(&wallet, &pair);
    assert_eq!(score.score, 50);
    assert_eq!(score.timestamp, 200);
}

#[test]
fn test_get_score_all_shards_unhealthy_returns_not_found() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let (shard_id, shard_client) = setup_score(&env, &admin, &service);
    submit_score(&shard_client, &env, &wallet, &pair, 90, 100);

    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let agg_client = LedgerLensAggregatorClient::new(&env, &agg_id);
    agg_client.initialize(&admin);
    agg_client.add_shard(&shard_id);
    agg_client.set_shard_health(&shard_id, &false);

    let result = agg_client.try_get_score(&wallet, &pair);
    assert_eq!(result, Err(Ok(Error::ScoreNotFound)));
}

#[test]
fn test_get_score_excludes_unhealthy_shard_across_policies() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // shard1: score=90, timestamp=2000 (higher score, but unhealthy)
    let (shard1_id, shard1_client) = setup_score(&env, &admin, &service);
    submit_score(&shard1_client, &env, &wallet, &pair, 90, 2000);

    // shard2: score=50, timestamp=3000 (newer, lower score, healthy)
    let (shard2_id, shard2_client) = setup_score(&env, &admin, &service);
    submit_score(&shard2_client, &env, &wallet, &pair, 50, 3000);

    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let agg_client = LedgerLensAggregatorClient::new(&env, &agg_id);
    agg_client.initialize(&admin);
    agg_client.add_shard(&shard1_id);
    agg_client.add_shard(&shard2_id);

    // Mark shard1 unhealthy
    agg_client.set_shard_health(&shard1_id, &false);

    // With HighestScore policy, only healthy shard2 is considered — score=50
    let score = agg_client.get_score(&wallet, &pair);
    assert_eq!(score.score, 50);
    assert_eq!(score.timestamp, 3000);

    // With MostRecent policy, still only healthy shard2 — same result
    agg_client.set_conflict_resolution_policy(&ConflictPolicy::MostRecent);
    let score = agg_client.get_score(&wallet, &pair);
    assert_eq!(score.score, 50);
    assert_eq!(score.timestamp, 3000);
}

#[test]
fn test_get_score_not_found_returns_error() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let agg_client = LedgerLensAggregatorClient::new(&env, &agg_id);
    agg_client.initialize(&admin);

    let result = agg_client.try_get_score(&wallet, &pair);
    assert_eq!(result, Err(Ok(Error::ScoreNotFound)));
}
