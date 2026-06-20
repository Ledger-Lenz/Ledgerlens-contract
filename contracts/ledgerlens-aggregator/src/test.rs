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
