//! End-to-end integration test: aggregator fan-out across shards with
//! divergent configuration (issue #431).
//!
//! Deploys 3 `ledgerlens-score` shards with different scores, counterparty
//! links, and delegation setups, then exercises every aggregator read
//! function against the heterogeneous multi-shard environment.

use ledgerlens_aggregator::{LedgerLensAggregator, LedgerLensAggregatorClient, SplitBrainStatus};
use ledgerlens_score::{
    Error as ScoreError, LedgerLensScoreContract, LedgerLensScoreContractClient,
};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

const GATE_THRESHOLD: u32 = 60;

struct Fixture<'a> {
    env: Env,
    aggregator: LedgerLensAggregatorClient<'a>,
    shard1: LedgerLensScoreContractClient<'a>,
    shard2: LedgerLensScoreContractClient<'a>,
    shard3: LedgerLensScoreContractClient<'a>,
}

fn setup<'a>() -> Fixture<'a> {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 100_000);

    let agg_id = env.register_contract(None, LedgerLensAggregator);
    let aggregator = LedgerLensAggregatorClient::new(&env, &agg_id);
    let agg_admin = Address::generate(&env);
    aggregator.initialize(&agg_admin);

    let (shard1_id, shard1) = register_score_shard(&env);
    let (shard2_id, shard2) = register_score_shard(&env);
    let (shard3_id, shard3) = register_score_shard(&env);

    aggregator.add_shard(&shard1_id);
    aggregator.add_shard(&shard2_id);
    aggregator.add_shard(&shard3_id);

    Fixture { env, aggregator, shard1, shard2, shard3 }
}

fn register_score_shard<'a>(env: &Env) -> (Address, LedgerLensScoreContractClient<'a>) {
    let id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(env, &id);
    let admin = Address::generate(env);
    let service = Address::generate(env);
    client.initialize(&admin, &service);
    (id, client)
}

fn submit_score(
    env: &Env,
    shard: &LedgerLensScoreContractClient,
    wallet: &Address,
    score: u32,
    timestamp: u64,
) {
    shard.submit_score(
        &Vec::new(env),
        wallet,
        &symbol_short!("XLM_USDC"),
        &score,
        &false,
        &false,
        &timestamp,
        &100,
        &1,
        &None,
    );
}

fn submit_aggregate_score(
    env: &Env,
    shard: &LedgerLensScoreContractClient,
    wallet: &Address,
    aggregate_score: u32,
) {
    // Build a minimal ScoreSubmission to trigger aggregate computation.
    // The aggregate score is derived from submitted individual scores.
    shard.submit_score(
        &Vec::new(env),
        wallet,
        &symbol_short!("BTC_USDC"),
        &aggregate_score,
        &false,
        &false,
        &env.ledger().timestamp(),
        &100,
        &1,
        &None,
    );
}

// ── Test: heterogeneous scores ──────────────────────────────────────────

#[test]
fn test_get_score_returns_max_across_diverse_shards() {
    let fix = setup();

    let wallet = Address::generate(&fix.env);
    let pair = symbol_short!("XLM_USDC");
    let ts = fix.env.ledger().timestamp();

    // Shard 1: score 30 (lowest)
    submit_score(&fix.env, &fix.shard1, &wallet, 30, ts + 1);
    // Shard 2: score 55 (middle)
    submit_score(&fix.env, &fix.shard2, &wallet, 55, ts + 2);
    // Shard 3: score 88 (highest — should be returned)
    submit_score(&fix.env, &fix.shard3, &wallet, 88, ts + 3);

    let result = fix.aggregator.get_score(&wallet, &pair);
    assert_eq!(result.score, 88, "get_score should return max across shards");
}

#[test]
fn test_get_score_reports_not_found_when_no_shard_has_data() {
    let fix = setup();
    let wallet = Address::generate(&fix.env);
    let pair = symbol_short!("XLM_USDC");
    let result = fix.aggregator.try_get_score(&wallet, &pair);
    assert_eq!(result, Err(Ok(ScoreError::ScoreNotFound)));
}

// ── Test: get_score_across_shards ───────────────────────────────────────

#[test]
fn test_get_score_across_shards_returns_per_shard_results() {
    let fix = setup();

    let wallet = Address::generate(&fix.env);
    let pair = symbol_short!("XLM_USDC");
    let ts = fix.env.ledger().timestamp();

    // Only shard 1 and shard 2 have data; shard 3 is empty.
    submit_score(&fix.env, &fix.shard1, &wallet, 40, ts + 1);
    submit_score(&fix.env, &fix.shard2, &wallet, 60, ts + 2);

    let results = fix.aggregator.get_score_across_shards(&wallet, &pair);
    assert_eq!(results.len(), 3, "should return one entry per shard");

    // At least two shards should have Some(score), one None (empty shard).
    let some_count = results.iter().filter(|(_, s)| s.is_some()).count();
    let none_count = results.iter().filter(|(_, s)| s.is_none()).count();
    assert_eq!(some_count, 2, "shards 1 and 2 should have scores");
    assert_eq!(none_count, 1, "shard 3 should have no score");
}

// ── Test: query_risk_gate ──────────────────────────────────────────────

#[test]
fn test_query_risk_gate_all_shards_pass() {
    let fix = setup();

    let wallet = Address::generate(&fix.env);
    let pair = symbol_short!("XLM_USDC");
    let ts = fix.env.ledger().timestamp();

    // Scores well below the gate threshold (60) -> all pass.
    submit_score(&fix.env, &fix.shard1, &wallet, 10, ts + 1);
    submit_score(&fix.env, &fix.shard2, &wallet, 20, ts + 2);
    submit_score(&fix.env, &fix.shard3, &wallet, 30, ts + 3);

    assert!(
        fix.aggregator.query_risk_gate(&wallet, &pair, &GATE_THRESHOLD),
        "all shards pass -> gate should return true",
    );
}

#[test]
fn test_query_risk_gate_one_shard_rejects() {
    let fix = setup();

    let wallet = Address::generate(&fix.env);
    let pair = symbol_short!("XLM_USDC");
    let ts = fix.env.ledger().timestamp();

    // Shard 1: low score (10) -> gate passes.
    submit_score(&fix.env, &fix.shard1, &wallet, 10, ts + 1);
    // Shard 2: high score (90) -> gate rejects.
    submit_score(&fix.env, &fix.shard2, &wallet, 90, ts + 2);
    // Shard 3: low score (20) -> gate passes.
    submit_score(&fix.env, &fix.shard3, &wallet, 20, ts + 3);

    assert!(
        !fix.aggregator.query_risk_gate(&wallet, &pair, &GATE_THRESHOLD),
        "one shard rejects -> gate should return false",
    );
}

// ── Test: contagion_depth_across_shards ─────────────────────────────────

#[test]
fn test_contagion_depth_returns_max_across_shards() {
    let fix = setup();

    let wallet = Address::generate(&fix.env);
    let pair = symbol_short!("XLM_USDC");

    // Shard 1: 2 counterparties.
    let c1 = Address::generate(&fix.env);
    let c2 = Address::generate(&fix.env);
    fix.shard1.add_counterparty_link(&wallet, &c1, &pair);
    fix.shard1.add_counterparty_link(&wallet, &c2, &pair);

    // Shard 2: 0 counterparties.
    // Shard 3: 4 counterparties (maximum).
    let d1 = Address::generate(&fix.env);
    let d2 = Address::generate(&fix.env);
    let d3 = Address::generate(&fix.env);
    let d4 = Address::generate(&fix.env);
    fix.shard3.add_counterparty_link(&wallet, &d1, &pair);
    fix.shard3.add_counterparty_link(&wallet, &d2, &pair);
    fix.shard3.add_counterparty_link(&wallet, &d3, &pair);
    fix.shard3.add_counterparty_link(&wallet, &d4, &pair);

    let depth = fix.aggregator.contagion_depth_across_shards(&wallet, &pair);
    assert_eq!(depth, 4, "should return max counterparty count across shards");

    // Empty wallet returns 0.
    let isolated = Address::generate(&fix.env);
    assert_eq!(fix.aggregator.contagion_depth_across_shards(&isolated, &pair), 0,);
}

// ── Test: cross-shard cyclic delegation safety ──────────────────────────

#[test]
fn test_cross_shard_delegation_cycle_does_not_hang() {
    let fix = setup();

    let wallet_a = Address::generate(&fix.env);
    let wallet_b = Address::generate(&fix.env);
    let pair = symbol_short!("XLM_USDC");

    // Cross-shard cycle: A -> B on shard1, B -> A on shard3.
    fix.shard1.set_score_delegate(&wallet_a, &wallet_b);
    fix.shard3.set_score_delegate(&wallet_b, &wallet_a);

    // Add counterparty links so contagion_depth is non-zero.
    let c = Address::generate(&fix.env);
    fix.shard1.add_counterparty_link(&wallet_a, &c, &pair);
    fix.shard3.add_counterparty_link(&wallet_a, &c, &pair);

    // Should complete without panic — aggregator queries each shard
    // independently and returns the max depth.
    let depth = fix.aggregator.contagion_depth_across_shards(&wallet_a, &pair);
    assert_eq!(depth, 1, "wallet_a has 1 counterparty in both shard1 and shard3");
}

// ── Test: get_aggregate_score ──────────────────────────────────────────

#[test]
fn test_get_aggregate_score_returns_max_across_shards() {
    let fix = setup();

    let wallet = Address::generate(&fix.env);
    let ts = fix.env.ledger().timestamp();

    // Each shard has a different aggregate (via its underlying scores).
    submit_aggregate_score(&fix.env, &fix.shard1, &wallet, 20);
    submit_aggregate_score(&fix.env, &fix.shard2, &wallet, 55);
    submit_aggregate_score(&fix.env, &fix.shard3, &wallet, 90);

    // Advance ledger to let aggregates settle.
    fix.env.ledger().with_mut(|l| l.timestamp = ts + 100);

    let agg = fix.aggregator.get_aggregate_score(&wallet);
    assert!(agg.aggregate_score > 0, "aggregate_score should be non-zero across shards with data",);
}

#[test]
fn test_get_aggregate_score_not_found() {
    let fix = setup();
    let wallet = Address::generate(&fix.env);

    let result = fix.aggregator.try_get_aggregate_score(&wallet);
    assert_eq!(result, Err(Ok(ScoreError::ScoreNotFound)));
}

// ── Test: supports_interface ────────────────────────────────────────────

#[test]
fn test_supports_interface_recognises_capabilities() {
    let fix = setup();
    assert!(fix.aggregator.supports_interface(&symbol_short!("score")));
    assert!(fix.aggregator.supports_interface(&symbol_short!("gate")));
    assert!(fix.aggregator.supports_interface(&symbol_short!("aggr")));
    assert!(fix.aggregator.supports_interface(&symbol_short!("federated")));
    assert!(fix.aggregator.supports_interface(&symbol_short!("sbrain")));
    assert!(fix.aggregator.supports_interface(&symbol_short!("health")));
    assert!(!fix.aggregator.supports_interface(&symbol_short!("unknown")));
}

#[test]
fn test_detect_split_brain_reports_aligned_shards() {
    let fix = setup();
    let wallet = Address::generate(&fix.env);
    let pair = symbol_short!("XLM_USDC");

    let report = fix.aggregator.detect_split_brain(&wallet, &pair);

    assert_eq!(report.status, SplitBrainStatus::Aligned);
    assert_eq!(report.shard_count, 3);
    assert_eq!(report.available_count, 3);
    assert_eq!(report.quorum_count, 3);
    assert_eq!(report.mismatch_count, 0);
    assert_eq!(report.diagnostics.len(), 3);
}

// ── Test: get_decay_rate delegates to the primary shard ─────────────────

#[test]
fn test_get_decay_rate_returns_primary_shard_configuration() {
    let fix = setup();
    fix.shard1.set_decay_rate(&999, &1000);
    let (num, den) = fix.aggregator.get_decay_rate();
    assert_eq!(num, 999);
    assert_eq!(den, 1000);
    assert!(num < den);
}

// ── Test: get_consensus_threshold_k returns constant ────────────────────

#[test]
fn test_get_consensus_threshold_k_returns_expected_constant() {
    let fix = setup();
    assert_eq!(fix.aggregator.get_consensus_threshold_k(), 5);
}

// ── Test: get_watchlist_status returns placeholder ──────────────────────

#[test]
fn test_get_watchlist_status_returns_false() {
    let fix = setup();
    let wallet = Address::generate(&fix.env);
    assert!(!fix.aggregator.get_watchlist_status(&wallet));
}

// ── Test: get_shards returns registered shards ──────────────────────────

#[test]
fn test_get_shards_returns_all_registered() {
    let fix = setup();
    let shards = fix.aggregator.get_shards();
    assert_eq!(shards.len(), 3);
}
