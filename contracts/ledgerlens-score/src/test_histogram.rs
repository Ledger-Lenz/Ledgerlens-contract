#![cfg(test)]

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient};

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

fn submit(env: &Env, client: &LedgerLensScoreContractClient, wallet: &Address, score: u32) {
    env.ledger().with_mut(|l| l.timestamp += 3_601);
    client.submit_score(
        &Vec::new(env),
        wallet,
        &symbol_short!("XLM_USDC"),
        &score,
        &false,
        &false,
        &env.ledger().timestamp(),
        &90,
        &1,
        &None,
    );
}

#[test]
fn test_histogram_empty_contract() {
    let (_env, client, _admin, _service) = setup();
    let hist = client.get_score_histogram();
    assert_eq!(hist.buckets.len(), 10);
    for i in 0..10 {
        assert_eq!(hist.buckets.get(i).unwrap(), 0);
    }
    assert_eq!(hist.total, 0);
}

#[test]
fn test_histogram_single_submission_correct_bucket() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    submit(&env, &client, &wallet, 42);
    let hist = client.get_score_histogram();
    assert_eq!(hist.buckets.get(4).unwrap(), 1);
    assert_eq!(hist.total, 1);
}

#[test]
fn test_histogram_spread_distribution() {
    let (env, client, _admin, _service) = setup();

    let w0 = Address::generate(&env);
    let w5 = Address::generate(&env);
    let w9 = Address::generate(&env);

    submit(&env, &client, &w0, 3);
    submit(&env, &client, &w5, 55);
    submit(&env, &client, &w9, 100);

    let hist = client.get_score_histogram();
    assert_eq!(hist.buckets.get(0).unwrap(), 1);
    assert_eq!(hist.buckets.get(5).unwrap(), 1);
    assert_eq!(hist.buckets.get(9).unwrap(), 1);
    assert_eq!(hist.total, 3);
}

#[test]
fn test_histogram_multiple_scores_same_bucket() {
    let (env, client, _admin, _service) = setup();

    for _ in 0..5 {
        let w = Address::generate(&env);
        submit(&env, &client, &w, 95);
    }

    let hist = client.get_score_histogram();
    assert_eq!(hist.buckets.get(9).unwrap(), 5);
    assert_eq!(hist.total, 5);
}

#[test]
fn test_histogram_clearing_score_decrements() {
    let (env, client, _admin, _service) = setup();

    let w1 = Address::generate(&env);
    let w2 = Address::generate(&env);

    submit(&env, &client, &w1, 10);
    submit(&env, &client, &w2, 20);

    let mut hist = client.get_score_histogram();
    assert_eq!(hist.total, 2);

    let empty_signers: Vec<Address> = Vec::new(&env);
    client.clear_score(&empty_signers, &w1, &symbol_short!("XLM_USDC"));

    hist = client.get_score_histogram();
    assert_eq!(hist.total, 1);
    assert_eq!(hist.buckets.get(2).unwrap(), 1);
}

#[test]
fn test_histogram_clearing_score_decrements_correct_bucket() {
    let (env, client, _admin, _service) = setup();

    let w = Address::generate(&env);
    submit(&env, &client, &w, 33);

    let hist_before = client.get_score_histogram();
    assert_eq!(hist_before.buckets.get(3).unwrap(), 1);

    let empty_signers: Vec<Address> = Vec::new(&env);
    client.clear_score(&empty_signers, &w, &symbol_short!("XLM_USDC"));

    let hist_after = client.get_score_histogram();
    assert_eq!(hist_after.buckets.get(3).unwrap(), 0);
    assert_eq!(hist_after.total, 0);
}

#[test]
fn test_histogram_multiple_pairs_sum_to_total() {
    let (env, client, _admin, _service) = setup();

    let wallet = Address::generate(&env);

    env.ledger().with_mut(|l| l.timestamp += 3_601);
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &symbol_short!("XLM_USDC"),
        &30,
        &false,
        &false,
        &env.ledger().timestamp(),
        &90,
        &1,
        &None,
    );

    env.ledger().with_mut(|l| l.timestamp += 3_601);
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &symbol_short!("ETH_USDC"),
        &70,
        &false,
        &false,
        &env.ledger().timestamp(),
        &90,
        &1,
        &None,
    );

    let hist = client.get_score_histogram();
    assert_eq!(hist.total, 2);
    assert_eq!(hist.buckets.get(3).unwrap(), 1);
    assert_eq!(hist.buckets.get(7).unwrap(), 1);
}

#[test]
fn test_percentile_empty_contract() {
    let (_env, client, _admin, _service) = setup();
    assert_eq!(client.get_score_percentile(&50), 0);
}

#[test]
fn test_percentile_all_scores_below() {
    let (env, client, _admin, _service) = setup();
    for i in 0..10 {
        let w = Address::generate(&env);
        submit(&env, &client, &w, i * 10);
    }
    assert_eq!(client.get_score_percentile(&100), 90);
}

#[test]
fn test_percentile_half_below() {
    let (env, client, _admin, _service) = setup();
    for i in 0..10 {
        let w = Address::generate(&env);
        submit(&env, &client, &w, i * 10 + 5);
    }
    assert_eq!(client.get_score_percentile(&55), 50);
}

#[test]
fn test_query_risk_gate_relative_no_score() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let result = client.query_risk_gate_relative(&wallet, &symbol_short!("XLM_USDC"), &50);
    assert!(!result);
}

#[test]
fn test_query_risk_gate_relative_below_percentile() {
    let (env, client, _admin, _service) = setup();
    for i in 0..10 {
        let w = Address::generate(&env);
        submit(&env, &client, &w, i * 10 + 5);
    }
    let low_wallet = Address::generate(&env);
    submit(&env, &client, &low_wallet, 5);
    let result = client.query_risk_gate_relative(&low_wallet, &symbol_short!("XLM_USDC"), &50);
    assert!(result);
}

#[test]
fn test_query_risk_gate_relative_above_percentile() {
    let (env, client, _admin, _service) = setup();
    for i in 0..10 {
        let w = Address::generate(&env);
        submit(&env, &client, &w, i * 10 + 5);
    }
    let high_wallet = Address::generate(&env);
    submit(&env, &client, &high_wallet, 95);
    let result = client.query_risk_gate_relative(&high_wallet, &symbol_short!("XLM_USDC"), &50);
    assert!(!result);
}
