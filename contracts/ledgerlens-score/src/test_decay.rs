use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient};

const START_TS: u64 = 1_700_000_000;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client, admin, service)
}

fn submit_pair(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    wallet: &Address,
    pair: &soroban_sdk::Symbol,
    score: u32,
    ts: u64,
) {
    client.submit_score(&Vec::new(env), wallet, pair, &score, &false, &false, &ts, &90, &1, &None);
}

#[test]
fn test_get_decay_rate_defaults_to_zero() {
    let (_env, client, _, _) = setup();
    let (num, den) = client.get_decay_rate();
    assert_eq!(num, 0);
    assert_eq!(den, 1);
}

#[test]
fn test_set_decay_rate_valid_accepted() {
    let (_env, client, _, _) = setup();
    client.set_decay_rate(&1, &1000);
    let (num, den) = client.get_decay_rate();
    assert_eq!(num, 1);
    assert_eq!(den, 1000);
}

#[test]
fn test_set_decay_rate_above_max_rejected() {
    let (_env, client, _, _) = setup();
    // λ = 2/1 = 2.0 exceeds MAX_DECAY_LAMBDA (numerator > denominator)
    assert_eq!(client.try_set_decay_rate(&2, &1), Err(Ok(Error::InvalidDecayRate)));
}

#[test]
fn test_set_decay_rate_zero_denominator_rejected() {
    let (_env, client, _, _) = setup();
    assert_eq!(client.try_set_decay_rate(&1, &0), Err(Ok(Error::InvalidDecayRate)));
}

#[test]
fn test_set_decay_rate_blocked_when_paused() {
    let (env, client, _, _) = setup();
    client.pause(&Vec::new(&env));
    assert_eq!(client.try_set_decay_rate(&1, &1000), Err(Ok(Error::ContractPaused)));
}

#[test]
fn test_set_decay_rate_max_boundary_accepted() {
    let (_env, client, _, _) = setup();
    // MAX_DECAY_LAMBDA = 0.01 = 1/100
    client.set_decay_rate(&1, &100);
    let (num, den) = client.get_decay_rate();
    assert_eq!(num, 1);
    assert_eq!(den, 100);
}

#[test]
fn test_set_decay_rate_below_max_boundary_accepted() {
    let (_env, client, _, _) = setup();
    // λ = 0.001 < 0.01
    client.set_decay_rate(&1, &1000);
    let (num, den) = client.get_decay_rate();
    assert_eq!(num, 1);
    assert_eq!(den, 1000);
}

#[test]
fn test_decay_rate_zero_reproduces_static_average() {
    let (env, client, _, _) = setup();
    let wallet = Address::generate(&env);
    let pair1 = symbol_short!("XLMU");
    let pair2 = symbol_short!("XLMB");

    let (num, _) = client.get_decay_rate();
    assert_eq!(num, 0);

    client.set_pair_weight(&Vec::new(&env), &pair1, &1);
    client.set_pair_weight(&Vec::new(&env), &pair2, &2);

    submit_pair(&env, &client, &wallet, &pair1, 30, 100);
    submit_pair(&env, &client, &wallet, &pair2, 60, 200);

    let aggregate = client.get_aggregate_score(&wallet);
    // Expected: (30*1 + 60*2) / (1 + 2) = 150 / 3 = 50
    assert_eq!(aggregate.aggregate_score, 50);
    assert!(!aggregate.decay_lambda_applied);
}

#[test]
fn test_decay_rate_nonzero_reduces_old_scores() {
    let (env, client, _, _) = setup();
    let wallet = Address::generate(&env);
    let pair_a = symbol_short!("XLMA");
    let pair_b = symbol_short!("XLMB");

    client.set_pair_weight(&Vec::new(&env), &pair_a, &1);
    client.set_pair_weight(&Vec::new(&env), &pair_b, &1);
    client.set_decay_rate(&1, &1000);

    let current_ts = env.ledger().timestamp();
    let score_a_ts = current_ts.saturating_sub(100);

    submit_pair(&env, &client, &wallet, &pair_a, 100, score_a_ts);

    env.ledger().with_mut(|l| l.timestamp = current_ts + 50);

    let score_b_ts = env.ledger().timestamp();
    submit_pair(&env, &client, &wallet, &pair_b, 50, score_b_ts);

    let aggregate = client.get_aggregate_score(&wallet);
    // With decay, older score A should be weighted less → aggregate closer to 50 than 75
    assert!(aggregate.aggregate_score < 75);
    assert!(aggregate.decay_lambda_applied);
}

#[test]
fn test_aggregate_decay_lambda_applied_false_when_zero() {
    let (env, client, _, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLMU");

    let (num, _) = client.get_decay_rate();
    assert_eq!(num, 0);

    submit_pair(&env, &client, &wallet, &pair, 50, 100);

    let aggregate = client.get_aggregate_score(&wallet);
    assert!(!aggregate.decay_lambda_applied);
}

#[test]
fn test_aggregate_decay_lambda_applied_true_when_nonzero() {
    let (env, client, _, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLMU");

    client.set_decay_rate(&1, &1000);
    submit_pair(&env, &client, &wallet, &pair, 50, START_TS);

    let aggregate = client.get_aggregate_score(&wallet);
    assert!(aggregate.decay_lambda_applied);
}

#[test]
fn test_decay_applies_consistently_across_pairs() {
    let (env, client, _, _) = setup();
    let wallet = Address::generate(&env);
    let pairs = [symbol_short!("PXL1"), symbol_short!("PXL2"), symbol_short!("PXL3")];

    client.set_decay_rate(&1, &500);

    let ts = env.ledger().timestamp();
    for pair in &pairs {
        client.set_pair_weight(&Vec::new(&env), pair, &1);
        submit_pair(&env, &client, &wallet, pair, 60, ts);
    }

    let aggregate = client.get_aggregate_score(&wallet);
    // All pairs same age → same decay factor → aggregate = 60
    assert_eq!(aggregate.aggregate_score, 60);
    assert_eq!(aggregate.pair_count, 3);
}
