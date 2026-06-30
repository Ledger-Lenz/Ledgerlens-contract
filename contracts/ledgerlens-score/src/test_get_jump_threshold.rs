//! Unit tests for `get_jump_threshold() -> u32` — issue #228.

use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient};

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
fn test_get_jump_threshold_default() {
    let (_env, client) = setup();
    assert_eq!(client.get_jump_threshold(), 30);
}

#[test]
fn test_get_jump_threshold_after_set() {
    let (env, client) = setup();
    client.set_jump_threshold(&Vec::new(&env), &50);
    assert_eq!(client.get_jump_threshold(), 50);
}

#[test]
fn test_get_jump_threshold_minimum_valid() {
    let (env, client) = setup();
    client.set_jump_threshold(&Vec::new(&env), &1);
    assert_eq!(client.get_jump_threshold(), 1);
}

#[test]
fn test_get_jump_threshold_maximum_valid() {
    let (env, client) = setup();
    client.set_jump_threshold(&Vec::new(&env), &99);
    assert_eq!(client.get_jump_threshold(), 99);
}
