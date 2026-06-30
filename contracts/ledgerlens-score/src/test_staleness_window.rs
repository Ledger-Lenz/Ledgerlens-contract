//! Tests for set_staleness_window / get_staleness_window.

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
fn test_set_staleness_window_happy_path() {
    let (env, client, _admin) = setup();
    let empty: Vec<Address> = Vec::new(&env);
    client.set_staleness_window(&empty, &3_600);
    assert_eq!(client.get_staleness_window(), 3_600);
}

#[test]
fn test_set_staleness_window_emits_event() {
    let (env, client, _admin) = setup();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let c2 = LedgerLensScoreContractClient::new(&env, &contract_id);
    c2.initialize(&Address::generate(&env), &Address::generate(&env));
    let empty: Vec<Address> = Vec::new(&env);
    c2.set_staleness_window(&empty, &7_200);

    let topic = (symbol_short!("sw_upd"),);
    let found = env.events().all().iter().any(|(addr, topics, data)| {
        addr == contract_id
            && topics == topic.into_val(&env)
            && data == 7_200u64.into_val(&env)
    });
    assert!(found, "staleness_window_updated event not emitted");
}

#[test]
fn test_set_staleness_window_rejects_zero() {
    let (env, client, _admin) = setup();
    let empty: Vec<Address> = Vec::new(&env);
    let result = client.try_set_staleness_window(&empty, &0);
    assert_eq!(result, Err(Ok(Error::InvalidStalenessWindow)));
}

#[test]
fn test_set_staleness_window_non_admin_rejected() {
    let env = Env::default();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    // initialize without mocking all auths, then only mock admin for init
    env.mock_all_auths();
    client.initialize(&admin, &service);

    // Now use a fresh env without mock_all_auths so auth check actually fires
    let env2 = Env::default();
    let c2 = LedgerLensScoreContractClient::new(&env2, &contract_id);
    let non_admin: Vec<Address> = Vec::new(&env2);
    let result = c2.try_set_staleness_window(&non_admin, &3_600);
    assert!(result.is_err());
}
