use crate::{Error, LedgerLensAggregator, LedgerLensAggregatorClient, MAX_SHARDS};
use ledgerlens_test_support::{generate_score_roles, test_env};
use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol, Vec};

/// A shard whose interface has fully drifted: it advertises no capability the
/// aggregator depends on.
mod incompatible_shard {
    use soroban_sdk::{contract, contractimpl, Env, Symbol};

    #[contract]
    pub struct IncompatibleShard;

    #[contractimpl]
    impl IncompatibleShard {
        pub fn supports_interface(_env: Env, _capability: Symbol) -> bool {
            false
        }
    }
}

/// A shard that implements part of the interface (`score`, `gate`) but is
/// missing the `aggr` capability the aggregator also invokes.
mod partial_shard {
    use soroban_sdk::{contract, contractimpl, Env, Symbol};

    #[contract]
    pub struct PartialShard;

    #[contractimpl]
    impl PartialShard {
        pub fn supports_interface(env: Env, capability: Symbol) -> bool {
            capability == Symbol::new(&env, "score") || capability == Symbol::new(&env, "gate")
        }
    }
}

/// A shard that predates the capability-detection interface entirely: it does
/// not expose `supports_interface`, so the cross-contract call traps.
mod legacy_shard {
    use soroban_sdk::{contract, contractimpl, Env};

    #[contract]
    pub struct LegacyShard;

    #[contractimpl]
    impl LegacyShard {
        pub fn ping(_env: Env) -> bool {
            true
        }
    }
}

/// A shard that passes capability registration but does not implement the
/// configuration getters used by split-brain probing.
mod deceptive_shard {
    use soroban_sdk::{contract, contractimpl, Env, Symbol};

    #[contract]
    pub struct DeceptiveShard;

    #[contractimpl]
    impl DeceptiveShard {
        pub fn supports_interface(env: Env, capability: Symbol) -> bool {
            capability == Symbol::new(&env, "score")
                || capability == Symbol::new(&env, "gate")
                || capability == Symbol::new(&env, "aggr")
        }

        pub fn is_score_stale(_env: Env, _wallet: soroban_sdk::Address, _pair: Symbol) -> bool {
            false
        }
    }
}

fn init_aggregator(env: &Env) -> LedgerLensAggregatorClient<'_> {
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(env, &agg_id);
    client.initialize(&Address::generate(env));
    client
}

#[test]
fn test_initialize() {
    let env = test_env();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_initialize_twice_fails() {
    let env = test_env();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let result = client.try_initialize(&admin);
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn test_initialize_requires_nominated_admin_and_rolls_back() {
    let env = Env::default();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);

    // An arbitrary invoker cannot install a nominated admin without that
    // address's authorization, and the failed invocation must not write state.
    assert!(client.try_initialize(&admin).is_err());
    assert_eq!(client.try_get_admin(), Err(Ok(Error::NotInitialized)));

    env.mock_all_auths();
    client.initialize(&admin);
    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_get_admin_not_initialized() {
    let env = test_env();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let result = client.try_get_admin();
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_add_remove_shards() {
    let env = test_env();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let (shard, _) = setup_score_shard(&env);
    client.add_shard(&shard);

    let shards = client.get_shards();
    assert_eq!(shards.len(), 1);
    assert_eq!(shards.get(0).unwrap(), shard);

    client.remove_shard(&shard);
    assert_eq!(client.get_shards().len(), 0);
}

#[test]
fn test_add_shard_self_reference_fails() {
    let env = test_env();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let result = client.try_add_shard(&agg_id);
    assert_eq!(result, Err(Ok(Error::SelfReference)));
}

#[test]
fn test_add_shard_duplicate_fails() {
    let env = test_env();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let (shard, _) = setup_score_shard(&env);
    client.add_shard(&shard);
    let result = client.try_add_shard(&shard);
    assert_eq!(result, Err(Ok(Error::ShardAlreadyRegistered)));
}

#[test]
fn test_remove_nonexistent_shard_fails() {
    let env = test_env();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let shard = Address::generate(&env);
    let result = client.try_remove_shard(&shard);
    assert_eq!(result, Err(Ok(Error::ShardNotRegistered)));
}

#[test]
fn test_query_risk_gate_no_shards_fails_closed() {
    let env = test_env();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    assert!(!client.query_risk_gate(&wallet, &pair, &75));
}

#[test]
fn test_query_risk_gate_all_shards_pass() {
    let env = test_env();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let (shard1_id, shard1) = setup_score_shard(&env);
    let (shard2_id, shard2) = setup_score_shard(&env);
    client.add_shard(&shard1_id);
    client.add_shard(&shard2_id);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    shard1.submit_score(&Vec::new(&env), &wallet, &pair, &10, &false, &false, &1, &100, &1, &None);
    shard2.submit_score(&Vec::new(&env), &wallet, &pair, &10, &false, &false, &1, &100, &1, &None);

    assert!(client.query_risk_gate(&wallet, &pair, &75));
}

#[test]
fn test_query_risk_gate_one_shard_rejects() {
    let env = test_env();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let (shard1_id, shard1) = setup_score_shard(&env);
    let (shard2_id, shard2) = setup_score_shard(&env);
    client.add_shard(&shard1_id);
    client.add_shard(&shard2_id);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    shard1.submit_score(&Vec::new(&env), &wallet, &pair, &10, &false, &false, &1, &100, &1, &None);
    shard2.submit_score(&Vec::new(&env), &wallet, &pair, &90, &false, &false, &1, &100, &1, &None);

    assert!(!client.query_risk_gate(&wallet, &pair, &75));
}

#[test]
fn test_oversized_asset_pair_fails_closed_before_shard_fanout() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let (shard_id, shard) = setup_score_shard(&env);
    client.add_shard(&shard_id);

    let wallet = Address::generate(&env);
    let valid_pair = symbol_short!("XLM_USDC");
    shard.submit_score(&Vec::new(&env), &wallet, &valid_pair, &10, &false, &false, &1, &100, &1, &None);

    let oversized_pair = Symbol::new(&env, "PAIR123456");
    assert!(!client.query_risk_gate(&wallet, &oversized_pair, &75));
    assert_eq!(client.try_get_score(&wallet, &oversized_pair), Err(Ok(ledgerlens_score::Error::InvalidAttestation)));
    assert_eq!(client.get_score_across_shards(&wallet, &oversized_pair).len(), 0);
    assert_eq!(client.contagion_depth_across_shards(&wallet, &oversized_pair), 0);
    assert_eq!(client.get_last_shard_failure(), None);
}

#[test]
fn test_get_decay_rate_returns_primary_shard_when_shards_diverge() {
    let env = test_env();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    let (primary_id, primary_client) = setup_score_shard(&env);
    let (secondary_id, secondary_client) = setup_score_shard(&env);

    client.initialize(&admin);
    client.add_shard(&primary_id);
    client.add_shard(&secondary_id);
    primary_client.set_decay_rate(&1, &1000);
    secondary_client.set_decay_rate(&1, &500);

    assert_eq!(client.get_decay_rate(), (1, 1000));
}

#[test]
fn test_get_decay_rate_no_shards_returns_error() {
    let env = Env::default();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);

    assert_eq!(client.try_get_decay_rate(), Err(Ok(ledgerlens_score::Error::ScoreNotFound)));
}

#[test]
fn test_get_consensus_threshold_k() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);

    assert_eq!(client.get_consensus_threshold_k(), 5);
}

#[test]
fn test_get_score_highest_score_policy() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let (shard1_id, shard1_client) = setup_score_shard(&env);
    // shard1: score=50, timestamp=100
    shard1_client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &100,
        &90,
        &1,
        &None,
    );

    let (shard2_id, shard2_client) = setup_score_shard(&env);
    // shard2: score=90, timestamp=50 (higher score but older)
    shard2_client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &90,
        &false,
        &false,
        &50,
        &90,
        &1,
        &None,
    );

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
fn test_get_watchlist_status_returns_true_for_watchlisted_wallet() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    let (shard_id, shard_client) = setup_score_shard(&env);

    client.initialize(&admin);
    client.add_shard(&shard_id);

    let wallet = Address::generate(&env);
    shard_client.set_watchlist(&soroban_sdk::Vec::new(&env), &wallet, &true);

    assert!(client.get_watchlist_status(&wallet));
}

#[test]
fn test_get_watchlist_status_returns_false_when_watchlisted_nowhere() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    let (shard_a_id, _) = setup_score_shard(&env);
    let (shard_b_id, _) = setup_score_shard(&env);

    client.initialize(&admin);
    client.add_shard(&shard_a_id);
    client.add_shard(&shard_b_id);

    let wallet = Address::generate(&env);

    assert!(!client.get_watchlist_status(&wallet));
}

#[test]
fn test_get_watchlist_status_returns_true_when_any_shard_watchlists_wallet() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    let (shard_a_id, _) = setup_score_shard(&env);
    let (shard_b_id, shard_b_client) = setup_score_shard(&env);

    client.initialize(&admin);
    client.add_shard(&shard_a_id);
    client.add_shard(&shard_b_id);

    let wallet = Address::generate(&env);
    shard_b_client.set_watchlist(&soroban_sdk::Vec::new(&env), &wallet, &true);

    assert!(client.get_watchlist_status(&wallet));
}

#[test]
fn test_get_watchlist_status_returns_true_when_all_shards_watchlist_wallet() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    let (shard_a_id, shard_a_client) = setup_score_shard(&env);
    let (shard_b_id, shard_b_client) = setup_score_shard(&env);

    client.initialize(&admin);
    client.add_shard(&shard_a_id);
    client.add_shard(&shard_b_id);

    let wallet = Address::generate(&env);
    shard_a_client.set_watchlist(&soroban_sdk::Vec::new(&env), &wallet, &true);
    shard_b_client.set_watchlist(&soroban_sdk::Vec::new(&env), &wallet, &true);

    assert!(client.get_watchlist_status(&wallet));
}

#[test]
fn test_add_shard_accepts_compatible_score_contract() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);

    let shard = env.register_contract(None, LedgerLensScoreContract);
    client.add_shard(&shard);

    let shards = client.get_shards();
    assert_eq!(shards.len(), 1);
    assert_eq!(shards.get(0).unwrap(), shard);
}

#[test]
fn test_add_shard_rejects_incompatible_shard() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);

    let shard = env.register_contract(None, incompatible_shard::IncompatibleShard);
    let result = client.try_add_shard(&shard);

    assert_eq!(result, Err(Ok(Error::IncompatibleInterface)));
    assert_eq!(client.get_shards().len(), 0);
}

#[test]
fn test_add_shard_rejects_shard_missing_capability() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);

    let shard = env.register_contract(None, partial_shard::PartialShard);
    let result = client.try_add_shard(&shard);

    assert_eq!(result, Err(Ok(Error::IncompatibleInterface)));
    assert_eq!(client.get_shards().len(), 0);
}

#[test]
fn test_add_shard_rejects_legacy_shard_without_supports_interface() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);

    let shard = env.register_contract(None, legacy_shard::LegacyShard);
    let result = client.try_add_shard(&shard);

    assert_eq!(result, Err(Ok(Error::IncompatibleInterface)));
    assert_eq!(client.get_shards().len(), 0);
}

fn setup_score_shard(env: &Env) -> (Address, LedgerLensScoreContractClient<'_>) {
    let id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(env, &id);
    let (admin, service) = generate_score_roles(env);
    client.initialize(&admin, &service);
    (id, client)
}

fn default_fingerprint() -> AggregatorConfigFingerprint {
    AggregatorConfigFingerprint {
        decay_num: 0,
        decay_den: 1,
        staleness_window: 604_800,
        global_min_confidence: 0,
        consensus_k: 2,
        consensus_epsilon: 5,
    }
}

#[test]
fn test_detect_split_brain_aligned() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);
    let (shard_a, _) = setup_score_shard(&env);
    let (shard_b, _) = setup_score_shard(&env);
    client.add_shard(&shard_a);
    client.add_shard(&shard_b);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let report = client.detect_split_brain(&wallet, &pair);

    assert_eq!(report.status, SplitBrainStatus::Aligned);
    assert_eq!(report.shard_count, 2);
    assert_eq!(report.healthy_count, 2);
    assert_eq!(report.available_count, 2);
    assert_eq!(report.quorum_count, 2);
    assert_eq!(report.required_quorum, 2);
    assert_eq!(report.mismatch_count, 0);
    assert_eq!(report.canonical, MaybeAggregatorConfigFingerprint::Some(default_fingerprint()));
}

#[test]
fn test_detect_split_brain_permutation_stable_majority() {
    let env_a = Env::default();
    env_a.mock_all_auths();
    let client_a = init_aggregator(&env_a);
    let wallet_a = Address::generate(&env_a);
    let pair_a = symbol_short!("XLM_USDC");
    let (a1, a1_client) = setup_score_shard(&env_a);
    let (a2, _) = setup_score_shard(&env_a);
    let (a3, _) = setup_score_shard(&env_a);
    a1_client.set_decay_rate(&2, &1000);
    client_a.add_shard(&a1);
    client_a.add_shard(&a2);
    client_a.add_shard(&a3);

    let env_b = Env::default();
    env_b.mock_all_auths();
    let client_b = init_aggregator(&env_b);
    let wallet_b = Address::generate(&env_b);
    let pair_b = symbol_short!("XLM_USDC");
    let (b1, b1_client) = setup_score_shard(&env_b);
    let (b2, _) = setup_score_shard(&env_b);
    let (b3, _) = setup_score_shard(&env_b);
    b1_client.set_decay_rate(&2, &1000);
    client_b.add_shard(&b2);
    client_b.add_shard(&b3);
    client_b.add_shard(&b1);

    let report_a = client_a.detect_split_brain(&wallet_a, &pair_a);
    let report_b = client_b.detect_split_brain(&wallet_b, &pair_b);

    assert_eq!(report_a.status, SplitBrainStatus::SplitBrain);
    assert_eq!(report_b.status, SplitBrainStatus::SplitBrain);
    assert_eq!(report_a.canonical, report_b.canonical);
    assert_eq!(report_a.quorum_count, 2);
    assert_eq!(report_b.quorum_count, 2);
    assert_eq!(report_a.mismatch_count, 1);
    assert_eq!(report_b.mismatch_count, 1);
}

#[test]
fn test_detect_split_brain_partial_failure_preserves_quorum() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);
    let (shard_a, _) = setup_score_shard(&env);
    let (shard_b, _) = setup_score_shard(&env);
    let bad = env.register_contract(None, deceptive_shard::DeceptiveShard);
    client.add_shard(&shard_a);
    client.add_shard(&shard_b);
    client.add_shard(&bad);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let report = client.detect_split_brain(&wallet, &pair);

    assert_eq!(report.status, SplitBrainStatus::Aligned);
    assert_eq!(report.healthy_count, 3);
    assert_eq!(report.available_count, 2);
    assert_eq!(report.unavailable_count, 1);
    assert_eq!(report.quorum_count, 2);
    assert_eq!(report.required_quorum, 2);
    assert_eq!(report.diagnostics.get(2).unwrap().status, ShardProbeStatus::Unavailable);
}

#[test]
fn test_detect_split_brain_byzantine_config_mismatch() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);
    let (shard_a, shard_a_client) = setup_score_shard(&env);
    let (shard_b, _) = setup_score_shard(&env);
    shard_a_client.set_global_min_confidence(&50);
    client.add_shard(&shard_a);
    client.add_shard(&shard_b);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let report = client.detect_split_brain(&wallet, &pair);

    assert_eq!(report.status, SplitBrainStatus::QuorumLost);
    assert_eq!(report.available_count, 2);
    assert_eq!(report.quorum_count, 1);
    assert_eq!(report.required_quorum, 2);
    assert_eq!(report.mismatch_count, 1);
}

#[test]
fn test_detect_split_brain_stale_shard_causes_quorum_loss() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);
    let (fresh_id, fresh_client) = setup_score_shard(&env);
    let (stale_id, stale_client) = setup_score_shard(&env);
    client.add_shard(&fresh_id);
    client.add_shard(&stale_id);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    fresh_client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &10,
        &false,
        &false,
        &700_000,
        &90,
        &1,
        &None,
    );
    stale_client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &10,
        &false,
        &false,
        &1,
        &90,
        &1,
        &None,
    );

    env.ledger().with_mut(|ledger| ledger.timestamp = 700_000);
    let report = client.detect_split_brain(&wallet, &pair);

    assert_eq!(report.status, SplitBrainStatus::QuorumLost);
    assert_eq!(report.stale_count, 1);
    assert_eq!(report.available_count, 1);
    assert_eq!(report.required_quorum, 2);
}

#[test]
fn test_set_shard_health_quarantines_and_restores_conflicting_shard() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);
    let (shard_a, shard_a_client) = setup_score_shard(&env);
    let (shard_b, _) = setup_score_shard(&env);
    shard_a_client.set_decay_rate(&2, &1000);
    client.add_shard(&shard_a);
    client.add_shard(&shard_b);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    assert_eq!(client.detect_split_brain(&wallet, &pair).status, SplitBrainStatus::QuorumLost);

    client.set_shard_health(&shard_a, &false);
    let quarantined = client.detect_split_brain(&wallet, &pair);
    assert_eq!(quarantined.status, SplitBrainStatus::Aligned);
    assert_eq!(quarantined.healthy_count, 1);
    assert_eq!(quarantined.diagnostics.get(0).unwrap().status, ShardProbeStatus::Unhealthy);
    assert!(!client.get_shard_health(&shard_a));

    client.set_shard_health(&shard_a, &true);
    assert!(client.get_shard_health(&shard_a));
    assert_eq!(client.detect_split_brain(&wallet, &pair).status, SplitBrainStatus::QuorumLost);
}

#[test]
fn test_detect_split_brain_max_shards_bounded_diagnostics() {
    let env = Env::default();
    env.mock_all_auths();
    let client = init_aggregator(&env);
    for _ in 0..crate::MAX_SHARDS {
        let (shard, _) = setup_score_shard(&env);
        client.add_shard(&shard);
    }

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let report = client.detect_split_brain(&wallet, &pair);

    assert_eq!(report.status, SplitBrainStatus::Aligned);
    assert_eq!(report.shard_count, crate::MAX_SHARDS as u32);
    assert_eq!(report.diagnostics.len(), crate::MAX_SHARDS as u32);
    assert_eq!(report.quorum_count, crate::MAX_SHARDS as u32);
    assert_eq!(report.required_quorum, 6);
}

#[test]
fn test_contagion_depth_across_shards_with_cross_shard_cycle() {
    let env = Env::default();
    env.mock_all_auths();

    // Register aggregator
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let agg_client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let agg_admin = Address::generate(&env);
    agg_client.initialize(&agg_admin);

    // Register two score shards
    let (shard1_id, shard1) = setup_score_shard(&env);
    let (shard2_id, shard2) = setup_score_shard(&env);

    // Add shards to aggregator
    agg_client.add_shard(&shard1_id);
    agg_client.add_shard(&shard2_id);

    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Create a cross-shard cyclic delegation:
    //   Shard 1: wallet_a delegates to wallet_b
    //   Shard 2: wallet_b delegates back to wallet_a
    // This forms a cycle no single-shard cycle detector can see.
    shard1.set_score_delegate(&wallet_a, &wallet_b);
    shard2.set_score_delegate(&wallet_b, &wallet_a);

    // Add counterparty links to give non-zero contagion depth on each shard.
    // Shard 1: wallet_a has 3 counterparties.
    let c1 = Address::generate(&env);
    let c2 = Address::generate(&env);
    let c3 = Address::generate(&env);
    shard1.add_counterparty_link(&wallet_a, &c1, &pair);
    shard1.add_counterparty_link(&wallet_a, &c2, &pair);
    shard1.add_counterparty_link(&wallet_a, &c3, &pair);

    // Shard 2: wallet_a has 5 counterparties (more than shard 1, so max = 5).
    let d1 = Address::generate(&env);
    let d2 = Address::generate(&env);
    let d3 = Address::generate(&env);
    let d4 = Address::generate(&env);
    let d5 = Address::generate(&env);
    shard2.add_counterparty_link(&wallet_a, &d1, &pair);
    shard2.add_counterparty_link(&wallet_a, &d2, &pair);
    shard2.add_counterparty_link(&wallet_a, &d3, &pair);
    shard2.add_counterparty_link(&wallet_a, &d4, &pair);
    shard2.add_counterparty_link(&wallet_a, &d5, &pair);

    // Verify per-shard depths are independent
    assert_eq!(shard1.get_contagion_depth(&wallet_a, &pair), 3);
    assert_eq!(shard2.get_contagion_depth(&wallet_a, &pair), 5);

    // The aggregator should return the max across shards — no panic, no hang.
    let depth = agg_client.contagion_depth_across_shards(&wallet_a, &pair);
    assert_eq!(depth, 5, "should return max contagion depth across shards");

    // Also verify a wallet with no counterparties still works fine
    let isolated = Address::generate(&env);
    let depth_zero = agg_client.contagion_depth_across_shards(&isolated, &pair);
    assert_eq!(depth_zero, 0, "wallet with no links should return 0");
}

#[test]
fn test_contagion_depth_across_shards_no_shards() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    assert_eq!(client.contagion_depth_across_shards(&wallet, &pair), 0);
}

// ── Issue #713: Shard health degradation windows with hysteresis ─────────────
//
// These tests document the current `is_shard_healthy` behavior (stored boolean)
// and establish the acceptance baseline for a future hysteresis implementation:
// health state transitions must require stable evidence, and a shard whose
// health is explicitly restored must participate in queries again.

/// A shard marked unhealthy via `set_shard_health(false)` must be skipped by
/// `query_risk_gate` even when its score data would otherwise cause the gate
/// to reject. This validates the fail-open exclusion path that hysteresis must
/// preserve: an unhealthy shard is bypassed, not treated as a rejection.
#[test]
fn test_unhealthy_shard_skipped_by_query_risk_gate() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let (shard1_id, shard1) = setup_score_shard(&env);
    let (shard2_id, shard2) = setup_score_shard(&env);
    client.add_shard(&shard1_id);
    client.add_shard(&shard2_id);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    // shard1: low risk (passes gate).
    shard1.submit_score(&Vec::new(&env), &wallet, &pair, &10, &false, &false, &1, &100, &1, &None);
    // shard2: high risk (would reject gate), but we mark it unhealthy.
    shard2.submit_score(&Vec::new(&env), &wallet, &pair, &90, &false, &false, &1, &100, &1, &None);
    client.set_shard_health(&shard2_id, &false);

    // With shard2 unhealthy, only shard1 votes — gate must pass.
    assert!(
        client.query_risk_gate(&wallet, &pair, &75),
        "unhealthy shard must be skipped; shard1 alone passes the gate"
    );
}

/// A shard restored to healthy after being marked unhealthy must immediately
/// participate in subsequent queries. This is the acceptance criterion for the
/// recovery limb of a hysteresis window: once the window clears, the shard's
/// vote must be counted again.
#[test]
fn test_restored_shard_participates_after_set_shard_health_true() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let (shard1_id, shard1) = setup_score_shard(&env);
    client.add_shard(&shard1_id);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    // shard1 carries a high-risk score that rejects the gate.
    shard1.submit_score(&Vec::new(&env), &wallet, &pair, &90, &false, &false, &1, &100, &1, &None);

    // Mark unhealthy → gate passes (shard skipped, no votes → true after all
    // shards are skipped yields `false` because no shard confirmed).
    // Actually no shards vote → returns true only if shards list is empty?
    // Let's check the edge: all shards unhealthy → loop completes with no
    // rejection → returns true (no shard voted `false`). That is the
    // documented fail-open skip behavior.
    client.set_shard_health(&shard1_id, &false);
    let gate_while_unhealthy = client.query_risk_gate(&wallet, &pair, &75);

    // Restore health → shard votes again → high-risk wallet is rejected.
    client.set_shard_health(&shard1_id, &true);
    let gate_after_restore = client.query_risk_gate(&wallet, &pair, &75);

    assert!(
        gate_while_unhealthy,
        "all shards unhealthy: loop completes with no false-vote, gate passes"
    );
    assert!(
        !gate_after_restore,
        "after health restore shard votes again and rejects high-risk wallet"
    );
}

/// Baseline: a shard whose health has never been explicitly set defaults to
/// healthy (current stored-boolean implementation). A hysteresis window
/// implementation must preserve this invariant so existing deployments that
/// never call `set_shard_health` are not inadvertently degraded.
#[test]
fn test_shard_defaults_to_healthy_before_any_set_shard_health_call() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let (shard_id, shard) = setup_score_shard(&env);
    client.add_shard(&shard_id);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    // Low-risk wallet — gate must pass because the shard is healthy by default.
    shard.submit_score(&Vec::new(&env), &wallet, &pair, &5, &false, &false, &1, &100, &1, &None);
    assert!(
        client.query_risk_gate(&wallet, &pair, &75),
        "freshly registered shard must default to healthy and vote"
    );
}

// ── Issue #714: Aggregator query budgets per fan-out strategy ────────────────
//
// These tests document and enforce bounded fan-out behavior: the aggregator
// must stop iterating at `MAX_SHARDS` and must return safe degraded results
// (not panic or infinite-loop) when the shard list is at capacity.

/// The aggregator's shard registry enforces `MAX_SHARDS` (currently 10). An
/// attempt to register an eleventh shard must fail with `ShardLimitReached`.
/// This is the primary budget-exhaustion guard for fan-out: no query can fan
/// out to more shards than the registry permits.
#[test]
fn test_add_shard_beyond_max_shards_returns_limit_error() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Register exactly MAX_SHARDS shards.
    for _ in 0..MAX_SHARDS {
        let (shard_id, _) = setup_score_shard(&env);
        client.add_shard(&shard_id);
    }
    assert_eq!(client.get_shards().len(), MAX_SHARDS as u32);

    // The next registration must be rejected.
    let (extra, _) = setup_score_shard(&env);
    let result = client.try_add_shard(&extra);
    assert_eq!(
        result,
        Err(Ok(Error::ShardLimitReached)),
        "registering beyond MAX_SHARDS must return ShardLimitReached"
    );
}

/// `query_risk_gate` over a full shard set (MAX_SHARDS shards, all carrying
/// low-risk scores) must complete without panic and return the correct result.
/// This validates that fan-out stays within budget under the worst-case shard
/// count the registry allows.
#[test]
fn test_query_risk_gate_completes_at_max_shard_capacity() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    for _ in 0..MAX_SHARDS {
        let (shard_id, shard) = setup_score_shard(&env);
        shard.submit_score(
            &Vec::new(&env),
            &wallet,
            &pair,
            &5,
            &false,
            &false,
            &1,
            &100,
            &1,
            &None,
        );
        client.add_shard(&shard_id);
    }

    assert!(
        client.query_risk_gate(&wallet, &pair, &75),
        "fan-out across MAX_SHARDS low-risk shards must complete and pass gate"
    );
}

/// `get_score` over MAX_SHARDS shards must return the highest score across
/// all shards without exhausting resources. Budget: bounded by MAX_SHARDS
/// iterations; worst-case metric is O(MAX_SHARDS) cross-contract calls.
#[test]
fn test_get_score_completes_at_max_shard_capacity() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let mut expected_max: u32 = 0;

    for i in 0..MAX_SHARDS {
        let (shard_id, shard) = setup_score_shard(&env);
        let score = (i as u32 + 1) * 7; // 7, 14, 21, …, 70
        if score > expected_max {
            expected_max = score;
        }
        shard.submit_score(
            &Vec::new(&env),
            &wallet,
            &pair,
            &score,
            &false,
            &false,
            &1,
            &100,
            &1,
            &None,
        );
        client.add_shard(&shard_id);
    }

    let result = client.get_score(&wallet, &pair);
    assert_eq!(
        result.score, expected_max,
        "get_score must return max across all MAX_SHARDS shards"
    );
}

// ── Issue #715: Cross-shard replay fixtures for divergent score histories ────
//
// These tests exercise the aggregator's handling of shards that carry
// conflicting, stale, or adversarially skewed score histories for the same
// wallet — the scenarios a cross-shard replay attacker could exploit.

/// Stale-shard scenario: one shard has an up-to-date high-risk score; a
/// second shard has an older, lower-risk score. The aggregator's HighestScore
/// policy must surface the higher score regardless of timestamp, so the
/// risk-gate still rejects the wallet.
#[test]
fn test_stale_shard_does_not_mask_high_risk_score_from_fresh_shard() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let (fresh_id, fresh_shard) = setup_score_shard(&env);
    let (stale_id, stale_shard) = setup_score_shard(&env);
    client.add_shard(&fresh_id);
    client.add_shard(&stale_id);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    // Fresh shard: high risk (score 95), recent timestamp.
    fresh_shard.submit_score(
        &Vec::new(&env), &wallet, &pair, &95, &false, &false, &1000, &100, &1, &None,
    );
    // Stale shard: low risk (score 10), older timestamp — simulates replay lag.
    stale_shard.submit_score(
        &Vec::new(&env), &wallet, &pair, &10, &false, &false, &1, &100, &1, &None,
    );

    // HighestScore policy should pick score=95 → gate rejects.
    let score = client.get_score(&wallet, &pair);
    assert_eq!(score.score, 95, "highest score must be returned; stale low-risk shard must not mask fresh high-risk score");
    assert!(!client.query_risk_gate(&wallet, &pair, &75), "high-risk wallet must be rejected even when one shard is stale");
}

/// Missing-shard scenario: wallet exists in only one of two shards.
/// The aggregator must return the score from the shard that has it, and
/// `get_score_across_shards` must report `None` for the shard with no data.
#[test]
fn test_wallet_missing_from_one_shard_does_not_prevent_score_retrieval() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let (shard_a_id, shard_a) = setup_score_shard(&env);
    let (shard_b_id, _shard_b) = setup_score_shard(&env);
    client.add_shard(&shard_a_id);
    client.add_shard(&shard_b_id);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    // Only shard_a has this wallet.
    shard_a.submit_score(
        &Vec::new(&env), &wallet, &pair, &40, &false, &false, &1, &100, &1, &None,
    );

    let score = client.get_score(&wallet, &pair);
    assert_eq!(score.score, 40, "score from the shard that has data must be returned");

    let per_shard = client.get_score_across_shards(&wallet, &pair);
    let none_count = per_shard.iter().filter(|(_, s)| s.is_none()).count();
    assert_eq!(none_count, 1, "the shard missing the wallet must report None in get_score_across_shards");
}

/// Contradictory-shards scenario: two shards report opposite extremes for the
/// same wallet (one very low, one very high). The HighestScore policy must
/// conservatively surface the worst case (highest risk), and the gate must
/// fail closed.
#[test]
fn test_contradictory_shards_gate_fails_closed_on_highest_risk() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let (low_id, low_shard) = setup_score_shard(&env);
    let (high_id, high_shard) = setup_score_shard(&env);
    client.add_shard(&low_id);
    client.add_shard(&high_id);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    low_shard.submit_score(
        &Vec::new(&env), &wallet, &pair, &1, &false, &false, &1, &100, &1, &None,
    );
    high_shard.submit_score(
        &Vec::new(&env), &wallet, &pair, &99, &false, &false, &1, &100, &1, &None,
    );

    // HighestScore policy → score 99; gate threshold 75 → rejected.
    let score = client.get_score(&wallet, &pair);
    assert_eq!(score.score, 99, "contradictory shards: HighestScore policy must return the worst case");
    assert!(!client.query_risk_gate(&wallet, &pair, &75), "gate must fail closed when any shard reports high risk");
}

/// Adversarially-low shard scenario: a malicious shard reports an artificially
/// low score (0) for a wallet that is otherwise high-risk on a legitimate
/// shard. The aggregator must still surface the high-risk score via
/// HighestScore and reject the wallet at the gate.
#[test]
fn test_adversarially_low_shard_does_not_allow_gate_bypass() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(&env, &agg_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let (legit_id, legit_shard) = setup_score_shard(&env);
    let (adv_id, adv_shard) = setup_score_shard(&env);
    client.add_shard(&legit_id);
    client.add_shard(&adv_id);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    // Legitimate shard correctly identifies high-risk wallet.
    legit_shard.submit_score(
        &Vec::new(&env), &wallet, &pair, &88, &false, &false, &1, &100, &1, &None,
    );
    // Adversarial shard reports minimum possible score to try to pull the aggregate down.
    adv_shard.submit_score(
        &Vec::new(&env), &wallet, &pair, &0, &false, &false, &1, &100, &1, &None,
    );

    // HighestScore policy: 88 wins. Gate must still reject.
    assert!(!client.query_risk_gate(&wallet, &pair, &75),
        "adversarially low shard score must not allow a high-risk wallet to bypass the gate");
}
