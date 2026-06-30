#![cfg(test)]

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient};

const START_TS: u64 = 1_700_000_000;
/// Fixed-point scale: 1_000_000 == 1.0
const SCALE: u64 = 1_000_000;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);
    let id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client)
}

/// Submit a score with a specific model_version, advancing time past cooldown.
fn submit(
    env: &Env,
    client: &LedgerLensScoreContractClient<'_>,
    wallet: &Address,
    pair: &soroban_sdk::Symbol,
    score: u32,
    model_version: u32,
) {
    env.ledger().with_mut(|l| l.timestamp += 3_601);
    client.submit_score(
        &Vec::new(env),
        wallet,
        pair,
        &score,
        &false,
        &false,
        &env.ledger().timestamp(),
        &80,
        &model_version,
        &None,
    );
}

/// Two models with equal default priors (both 1.0) → simple average.
#[test]
fn test_bayesian_two_models_equal_priors() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 60, 1);
    submit(&env, &client, &wallet, &pair, 80, 2);

    // Both priors default to SCALE, so aggregate = (1*60 + 1*80) / 2 = 70.
    let agg = client.get_bayesian_aggregate(&wallet, &pair).unwrap();
    assert_eq!(agg, 70);
}

/// Two models with different prior weights.
#[test]
fn test_bayesian_two_models_custom_priors() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // version 1 weight = 1.0, version 2 weight = 3.0
    client.set_model_prior_weight(&1, &SCALE);
    client.set_model_prior_weight(&2, &(3 * SCALE));

    submit(&env, &client, &wallet, &pair, 60, 1);
    submit(&env, &client, &wallet, &pair, 80, 2);

    // aggregate = (1*60 + 3*80) / 4 = 300 / 4 = 75
    let agg = client.get_bayesian_aggregate(&wallet, &pair).unwrap();
    assert_eq!(agg, 75);
}

/// Three models with different prior weights.
#[test]
fn test_bayesian_three_models_custom_priors() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // weights: v1=1, v2=2, v3=3
    client.set_model_prior_weight(&1, &SCALE);
    client.set_model_prior_weight(&2, &(2 * SCALE));
    client.set_model_prior_weight(&3, &(3 * SCALE));

    submit(&env, &client, &wallet, &pair, 50, 1);
    submit(&env, &client, &wallet, &pair, 60, 2);
    submit(&env, &client, &wallet, &pair, 70, 3);

    // aggregate = (1*50 + 2*60 + 3*70) / 6 = 380 / 6 = 63
    let agg = client.get_bayesian_aggregate(&wallet, &pair).unwrap();
    assert_eq!(agg, 63);
}

/// get_model_prior_weight returns SCALE for unconfigured versions.
#[test]
fn test_get_model_prior_weight_default() {
    let (_env, client) = setup();
    assert_eq!(client.get_model_prior_weight(&99), SCALE);
}

/// set_model_prior_weight persists the value.
#[test]
fn test_set_model_prior_weight_persisted() {
    let (_env, client) = setup();
    client.set_model_prior_weight(&5, &(2 * SCALE));
    assert_eq!(client.get_model_prior_weight(&5), 2 * SCALE);
}

/// set_model_prior_weight rejects weight == 0.
#[test]
fn test_set_model_prior_weight_zero_rejected() {
    let (_env, client) = setup();
    let result = client.try_set_model_prior_weight(&1, &0);
    assert_eq!(result, Err(Ok(Error::InvalidModelPriorWeight)));
}

/// get_bayesian_aggregate returns ScoreNotFound when no history exists.
#[test]
fn test_bayesian_aggregate_no_score() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let result = client.try_get_bayesian_aggregate(&wallet, &pair);
    assert_eq!(result, Err(Ok(Error::ScoreNotFound)));
}
