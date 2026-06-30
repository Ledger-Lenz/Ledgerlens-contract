use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient, ScoreHistogram};

fn initialized<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client, admin, service)
}

fn all_zero(h: &ScoreHistogram) -> bool {
    if h.total != 0 {
        return false;
    }
    for i in 0..10 {
        if h.buckets.get(i).unwrap() != 0 {
            return false;
        }
    }
    true
}

#[test]
fn test_empty_histogram() {
    let (_env, client, _admin, _service) = initialized();
    let h = client.get_score_histogram();
    assert!(all_zero(&h));
}

#[test]
fn test_single_submission_updates_histogram() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    client.submit_score(&Vec::new(&env), &wallet, &pair, &42, &false, &false, &1, &90, &1, &None);
    let h = client.get_score_histogram();
    assert_eq!(h.total, 1);
    assert_eq!(h.buckets.get(4).unwrap(), 1);
    for i in 0..10 {
        if i != 4 {
            assert_eq!(h.buckets.get(i).unwrap(), 0, "bucket {i} must be 0");
        }
    }
}

#[test]
fn test_score_update_moves_buckets() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.submit_score(&Vec::new(&env), &wallet, &pair, &15, &false, &false, &1, &90, &1, &None);
    let h = client.get_score_histogram();
    assert_eq!(h.total, 1);
    assert_eq!(h.buckets.get(1).unwrap(), 1);
    assert_eq!(h.buckets.get(4).unwrap(), 0);

    env.ledger().with_mut(|l| l.timestamp += 3_601);

    client.submit_score(&Vec::new(&env), &wallet, &pair, &42, &false, &false, &2, &95, &1, &None);
    let h = client.get_score_histogram();
    assert_eq!(h.total, 1);
    assert_eq!(h.buckets.get(1).unwrap(), 0);
    assert_eq!(h.buckets.get(4).unwrap(), 1);
}

#[test]
fn test_multiple_wallets_histogram() {
    let (env, client, _admin, _service) = initialized();
    let pair = symbol_short!("XLM_USDC");

    let w1 = Address::generate(&env);
    let w2 = Address::generate(&env);
    let w3 = Address::generate(&env);

    client.submit_score(&Vec::new(&env), &w1, &pair, &5, &false, &false, &1, &90, &1, &None);
    client.submit_score(&Vec::new(&env), &w2, &pair, &55, &false, &false, &1, &90, &1, &None);
    client.submit_score(&Vec::new(&env), &w3, &pair, &95, &false, &false, &1, &90, &1, &None);

    let h = client.get_score_histogram();
    assert_eq!(h.total, 3);
    assert_eq!(h.buckets.get(0).unwrap(), 1);
    assert_eq!(h.buckets.get(5).unwrap(), 1);
    assert_eq!(h.buckets.get(9).unwrap(), 1);
}

#[test]
fn test_percentile_single_wallet() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.submit_score(&Vec::new(&env), &wallet, &pair, &42, &false, &false, &1, &90, &1, &None);
    assert_eq!(client.get_score_percentile(&wallet, &pair), 0);
}

#[test]
fn test_percentile_ranking() {
    let (env, client, _admin, _service) = initialized();
    let pair = symbol_short!("XLM_USDC");

    let wa = Address::generate(&env);
    client.submit_score(&Vec::new(&env), &wa, &pair, &5, &false, &false, &1, &90, &1, &None);
    let wb = Address::generate(&env);
    client.submit_score(&Vec::new(&env), &wb, &pair, &55, &false, &false, &1, &90, &1, &None);
    let wc = Address::generate(&env);
    client.submit_score(&Vec::new(&env), &wc, &pair, &95, &false, &false, &1, &90, &1, &None);

    assert_eq!(client.get_score_percentile(&wa, &pair), 0);
    assert_eq!(client.get_score_percentile(&wb, &pair), 33);
    assert_eq!(client.get_score_percentile(&wc, &pair), 66);
}

#[test]
fn test_relative_gate_top_20() {
    let (env, client, _admin, _service) = initialized();
    let pair = symbol_short!("XLM_USDC");

    let wallets = [
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    ];
    for (i, w) in wallets.iter().enumerate() {
        client.submit_score(
            &Vec::new(&env),
            w,
            &pair,
            &(10 + i as u32 * 20),
            &false,
            &false,
            &1,
            &90,
            &1,
            &None,
        );
    }
    assert!(client.query_risk_gate_relative(&wallets[4], &pair, &20));
    assert!(!client.query_risk_gate_relative(&wallets[3], &pair, &20));
}

#[test]
fn test_relative_gate_all_risky_at_100() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    client.submit_score(&Vec::new(&env), &wallet, &pair, &50, &false, &false, &1, &90, &1, &None);
    assert!(client.query_risk_gate_relative(&wallet, &pair, &100));
}

#[test]
fn test_relative_gate_invalid_percentile() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    assert_eq!(
        client.try_query_risk_gate_relative(&wallet, &pair, &0),
        Err(Ok(Error::InvalidThreshold))
    );
    assert_eq!(
        client.try_query_risk_gate_relative(&wallet, &pair, &101),
        Err(Ok(Error::InvalidThreshold))
    );
}

#[test]
fn test_relative_gate_score_not_found() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    assert_eq!(
        client.try_query_risk_gate_relative(&wallet, &pair, &10),
        Err(Ok(Error::ScoreNotFound))
    );
}

#[test]
fn test_batch_submission_updates_histogram() {
    let (env, client, _admin, _service) = initialized();
    let pair = symbol_short!("XLM_USDC");

    let mut batch: Vec<crate::ScoreSubmission> = Vec::new(&env);
    for i in 0u32..5 {
        let w = Address::generate(&env);
        batch.push_back(crate::ScoreSubmission {
            wallet: w,
            asset_pair: pair.clone(),
            score: i * 20 + 10,
            benford_flag: false,
            ml_flag: false,
            timestamp: 1000 + i as u64,
            confidence: 80,
            model_version: 1,
        });
    }
    let result = client.submit_scores_batch(&batch);
    assert_eq!(result.accepted_count, 5);

    let h = client.get_score_histogram();
    assert_eq!(h.total, 5);
    assert_eq!(h.buckets.get(1).unwrap(), 1);
    assert_eq!(h.buckets.get(3).unwrap(), 1);
    assert_eq!(h.buckets.get(5).unwrap(), 1);
    assert_eq!(h.buckets.get(7).unwrap(), 1);
    assert_eq!(h.buckets.get(9).unwrap(), 1);
}

#[test]
fn test_score_100_maps_to_bucket_9() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    client.submit_score(&Vec::new(&env), &wallet, &pair, &100, &false, &false, &1, &90, &1, &None);
    let h = client.get_score_histogram();
    assert_eq!(h.buckets.get(9).unwrap(), 1);
}

#[test]
fn test_score_0_maps_to_bucket_0() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    client.submit_score(&Vec::new(&env), &wallet, &pair, &0, &false, &false, &1, &90, &1, &None);
    let h = client.get_score_histogram();
    assert_eq!(h.buckets.get(0).unwrap(), 1);
}

#[test]
fn test_score_99_maps_to_bucket_9() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    client.submit_score(&Vec::new(&env), &wallet, &pair, &99, &false, &false, &1, &90, &1, &None);
    let h = client.get_score_histogram();
    assert_eq!(h.buckets.get(9).unwrap(), 1);
}

#[test]
fn test_clear_score_decrements_histogram() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    client.submit_score(&Vec::new(&env), &wallet, &pair, &42, &false, &false, &1, &90, &1, &None);
    assert_eq!(client.get_score_histogram().total, 1);

    client.clear_score(&Vec::new(&env), &wallet, &pair);
    let h = client.get_score_histogram();
    assert_eq!(h.total, 0);
    assert_eq!(h.buckets.get(4).unwrap(), 0);
}

#[test]
fn test_clear_score_history_decrements_histogram() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    client.submit_score(&Vec::new(&env), &wallet, &pair, &42, &false, &false, &1, &90, &1, &None);
    assert_eq!(client.get_score_histogram().total, 1);

    client.clear_score_history(&Vec::new(&env), &wallet, &pair);
    let h = client.get_score_histogram();
    assert_eq!(h.total, 0);
    assert_eq!(h.buckets.get(4).unwrap(), 0);
}

#[test]
fn test_percentile_score_not_found() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    assert_eq!(client.try_get_score_percentile(&wallet, &pair), Err(Ok(Error::ScoreNotFound)));
}
