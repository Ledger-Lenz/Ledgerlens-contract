//! Tests for `bulk_deregister_model_version`.

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

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
fn test_bulk_deregister_deprecates_all_versions() {
    let (env, client) = setup();
    client.register_model_version(&Vec::new(&env), &1);
    client.register_model_version(&Vec::new(&env), &2);
    client.register_model_version(&Vec::new(&env), &3);

    let mut versions = Vec::new(&env);
    versions.push_back(1u32);
    versions.push_back(2u32);
    versions.push_back(3u32);

    client.bulk_deregister_model_version(&Vec::new(&env), &versions).unwrap();

    assert!(!client.is_model_version_active(&1));
    assert!(!client.is_model_version_active(&2));
    assert!(!client.is_model_version_active(&3));
}

#[test]
fn test_bulk_deregister_skips_already_deprecated() {
    let (env, client) = setup();
    client.register_model_version(&Vec::new(&env), &1);
    client.register_model_version(&Vec::new(&env), &2);
    client.deprecate_model_version(&Vec::new(&env), &1);

    let mut versions = Vec::new(&env);
    versions.push_back(1u32);
    versions.push_back(2u32);

    // Must succeed even though version 1 is already deprecated.
    client.bulk_deregister_model_version(&Vec::new(&env), &versions).unwrap();

    assert!(!client.is_model_version_active(&1));
    assert!(!client.is_model_version_active(&2));
}

#[test]
fn test_bulk_deregister_errors_on_unregistered_version() {
    let (env, client) = setup();
    client.register_model_version(&Vec::new(&env), &1);

    let mut versions = Vec::new(&env);
    versions.push_back(1u32);
    versions.push_back(99u32); // never registered

    let result = client.try_bulk_deregister_model_version(&Vec::new(&env), &versions);
    assert_eq!(result, Err(Ok(Error::ScoreNotFound)));
}

#[test]
fn test_bulk_deregister_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let mut versions = Vec::new(&env);
    versions.push_back(1u32);

    let result = client.try_bulk_deregister_model_version(&Vec::new(&env), &versions);
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_bulk_deregister_empty_list_is_noop() {
    let (env, client) = setup();
    client.register_model_version(&Vec::new(&env), &1);

    let empty: Vec<u32> = Vec::new(&env);
    client.bulk_deregister_model_version(&Vec::new(&env), &empty).unwrap();

    // Version 1 must still be active — nothing was deprecated.
    assert!(client.is_model_version_active(&1));
}
