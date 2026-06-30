//! Tests for `bulk_reset_pair_weight`.

#![cfg(test)]

use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient};

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client)
}

#[test]
fn test_bulk_reset_removes_custom_weights() {
    let (env, client) = setup();
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("XLM_BTC");

    client.set_pair_weight(&Vec::new(&env), &pair_a, &3).unwrap();
    client.set_pair_weight(&Vec::new(&env), &pair_b, &5).unwrap();
    assert_eq!(client.get_pair_weight(&pair_a), 3);
    assert_eq!(client.get_pair_weight(&pair_b), 5);

    let mut pairs = Vec::new(&env);
    pairs.push_back(pair_a.clone());
    pairs.push_back(pair_b.clone());
    client.bulk_reset_pair_weight(&Vec::new(&env), &pairs).unwrap();

    // Both pairs fall back to the default weight of 1.
    assert_eq!(client.get_pair_weight(&pair_a), 1);
    assert_eq!(client.get_pair_weight(&pair_b), 1);
}

#[test]
fn test_bulk_reset_skips_pairs_without_custom_weight() {
    let (env, client) = setup();
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("XLM_BTC"); // never had a custom weight

    client.set_pair_weight(&Vec::new(&env), &pair_a, &7).unwrap();

    let mut pairs = Vec::new(&env);
    pairs.push_back(pair_a.clone());
    pairs.push_back(pair_b.clone());

    // Must not error even though pair_b has no custom weight.
    client.bulk_reset_pair_weight(&Vec::new(&env), &pairs).unwrap();

    assert_eq!(client.get_pair_weight(&pair_a), 1);
    assert_eq!(client.get_pair_weight(&pair_b), 1);
}

#[test]
fn test_bulk_reset_empty_list_is_noop() {
    let (env, client) = setup();
    let pair = symbol_short!("XLM_USDC");

    client.set_pair_weight(&Vec::new(&env), &pair, &4).unwrap();

    let empty: Vec<soroban_sdk::Symbol> = Vec::new(&env);
    client.bulk_reset_pair_weight(&Vec::new(&env), &empty).unwrap();

    // Custom weight must still be in place — nothing was reset.
    assert_eq!(client.get_pair_weight(&pair), 4);
}

#[test]
fn test_bulk_reset_idempotent() {
    let (env, client) = setup();
    let pair = symbol_short!("XLM_USDC");

    client.set_pair_weight(&Vec::new(&env), &pair, &2).unwrap();

    let mut pairs = Vec::new(&env);
    pairs.push_back(pair.clone());
    client.bulk_reset_pair_weight(&Vec::new(&env), &pairs).unwrap();
    // Calling again on an already-reset pair must not error.
    client.bulk_reset_pair_weight(&Vec::new(&env), &pairs).unwrap();

    assert_eq!(client.get_pair_weight(&pair), 1);
}

#[test]
fn test_bulk_reset_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let mut pairs = Vec::new(&env);
    pairs.push_back(symbol_short!("XLM_USDC"));

    let result = client.try_bulk_reset_pair_weight(&Vec::new(&env), &pairs);
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}
