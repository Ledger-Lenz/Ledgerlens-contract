//! Tests for set_jump_threshold / get_jump_threshold runtime setter.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, Vec,
};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient};

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client, admin)
}

#[test]
fn test_set_jump_threshold_happy_path() {
    let (env, client, _admin) = setup();
    let empty: Vec<Address> = Vec::new(&env);
    client.set_jump_threshold(&empty, &50);
    assert_eq!(client.get_jump_threshold(), 50);
}

#[test]
fn test_set_jump_threshold_emits_event() {
    let (env, client, _admin) = setup();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let c2 = LedgerLensScoreContractClient::new(&env, &contract_id);
    c2.initialize(&Address::generate(&env), &Address::generate(&env));
    let empty: Vec<Address> = Vec::new(&env);
    c2.set_jump_threshold(&empty, &45);

    let topic = (symbol_short!("jt_upd"),);
    let found = env.events().all().iter().any(|(addr, topics, data)| {
        addr == contract_id
            && topics == topic.into_val(&env)
            && data == 45u32.into_val(&env)
    });
    assert!(found, "jump_threshold_updated event not emitted");
}

#[test]
fn test_set_jump_threshold_rejects_zero() {
    let (env, client, _admin) = setup();
    let empty: Vec<Address> = Vec::new(&env);
    let result = client.try_set_jump_threshold(&empty, &0);
    assert_eq!(result, Err(Ok(Error::InvalidThreshold)));
}

#[test]
fn test_set_jump_threshold_rejects_100() {
    let (env, client, _admin) = setup();
    let empty: Vec<Address> = Vec::new(&env);
    let result = client.try_set_jump_threshold(&empty, &100);
    assert_eq!(result, Err(Ok(Error::InvalidThreshold)));
}

#[test]
fn test_set_jump_threshold_non_admin_rejected() {
    let env = Env::default();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    env.mock_all_auths();
    client.initialize(&Address::generate(&env), &Address::generate(&env));

    let env2 = Env::default();
    let c2 = LedgerLensScoreContractClient::new(&env2, &contract_id);
    let non_admin: Vec<Address> = Vec::new(&env2);
    let result = c2.try_set_jump_threshold(&non_admin, &50);
    assert!(result.is_err());
}
