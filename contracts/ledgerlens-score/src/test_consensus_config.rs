//! Tests for `set_consensus_config`.

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};

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
fn test_set_consensus_config_happy_path() {
    let (_env, client) = setup();
    client.set_consensus_config(&3, &10).unwrap();
    assert_eq!(client.get_consensus_config(), (3, 10));
}

#[test]
fn test_set_consensus_config_updates_both_atomically() {
    let (_env, client) = setup();
    client.set_consensus_config(&2, &5).unwrap();
    assert_eq!(client.get_consensus_config(), (2, 5));
    // Override with new values — both change together.
    client.set_consensus_config(&5, &20).unwrap();
    assert_eq!(client.get_consensus_config(), (5, 20));
}

#[test]
fn test_set_consensus_config_boundary_values() {
    let (_env, client) = setup();
    // k=1 and epsilon=0 are the lowest valid values.
    client.set_consensus_config(&1, &0).unwrap();
    assert_eq!(client.get_consensus_config(), (1, 0));
    // epsilon=100 is the highest valid value.
    client.set_consensus_config(&1, &100).unwrap();
    assert_eq!(client.get_consensus_config(), (1, 100));
}

#[test]
fn test_set_consensus_config_k_zero_rejected() {
    let (_env, client) = setup();
    let result = client.try_set_consensus_config(&0, &5);
    assert_eq!(result, Err(Ok(Error::InvalidConsensusConfig)));
}

#[test]
fn test_set_consensus_config_epsilon_over_100_rejected() {
    let (_env, client) = setup();
    let result = client.try_set_consensus_config(&2, &101);
    assert_eq!(result, Err(Ok(Error::InvalidConsensusConfig)));
}

#[test]
fn test_set_consensus_config_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let result = client.try_set_consensus_config(&2, &5);
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_set_consensus_config_non_admin_rejected() {
    let env = Env::default();
    // Do NOT mock_all_auths so that require_auth() enforces real authorization.
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);

    // Initialize using a scoped mock just for the initialize call.
    env.mock_all_auths_allowing_non_root_auth();
    client.initialize(&admin, &service);

    // Without any auth context, set_consensus_config must be rejected.
    let result = client.try_set_consensus_config(&2, &5);
    assert!(result.is_err());
}
