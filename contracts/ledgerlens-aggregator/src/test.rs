#![cfg(test)]

use ledgerlens_aggregator::LedgerLensAggregator;
use ledgerlens_score::LedgerLensScoreContract;
use soroban_sdk::{symbol_short, testutils::{Address as _, Ledger as _}, Address, Env, Vec};

fn setup_pair() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let shard_a = env.register_contract(None, LedgerLensScoreContract);
    let shard_b = env.register_contract(None, LedgerLensScoreContract);
    (env, agg_id, shard_a)
}

#[test]
fn test_query_risk_gate_no_shards_returns_false() {
    let env = Env::default();
    env.mock_all_auths();
    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let client = ledgerlens_aggregator::LedgerLensAggregatorClient::new(&env, &agg_id);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    assert!(!client.query_risk_gate(&wallet, &pair, &75));
}

#[test]
fn test_get_decay_rate() {
    let env = TestEnv::default();
    let contract_id = env.register_contract(None, LedgerLens);
    let client = LedgerLensClient::new(&env, &contract_id);

    let (numerator, denominator) = client.get_decay_rate();
    
    assert_eq!(numerator, 999);
    assert_eq!(denominator, 1000);
    assert!(numerator < denominator, "Decay rate should be < 1.0");
}

#[test]
fn test_get_consensus_threshold_k() {
    let env = TestEnv::default();
    let contract_id = env.register_contract(None, LedgerLens);
    let client = LedgerLensClient::new(&env, &contract_id);

    let k = client.get_consensus_threshold_k();
    
    assert_eq!(k, 5, "Should return the configured consensus threshold K");
    assert!(k >= 3, "K should be at least 3 for meaningful consensus");
}

#[test]
fn test_get_watchlist_status() {
    let env = TestEnv::default();
    let contract_id = env.register_contract(None, LedgerLens);
    let client = LedgerLensClient::new(&env, &contract_id);

    let watched_wallet = Address::generate(&env);
    let unwatched_wallet = Address::generate(&env);

    // Test unwatched (default)
    assert_eq!(client.get_watchlist_status(&unwatched_wallet), false);

    // TODO: Add logic to add to watchlist and test true case
    // For now, this verifies the function signature and basic behavior
}
