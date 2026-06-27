//! Unit tests for `get_gate_callers() -> Vec<Address>` — issue #236.

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
fn test_get_gate_callers_empty_by_default() {
    let (_env, client) = setup();
    assert!(client.get_gate_callers().is_empty());
}

#[test]
fn test_get_gate_callers_returns_set_callers() {
    let (env, client) = setup();
    let caller_a = Address::generate(&env);
    let caller_b = Address::generate(&env);
    let mut callers = Vec::new(&env);
    callers.push_back(caller_a.clone());
    callers.push_back(caller_b.clone());
    client.set_gate_callers(&Vec::new(&env), &callers);
    let result = client.get_gate_callers();
    assert_eq!(result.len(), 2);
    assert!(result.contains(&caller_a));
    assert!(result.contains(&caller_b));
}

#[test]
fn test_get_gate_callers_empty_after_clear() {
    let (env, client) = setup();
    let caller = Address::generate(&env);
    let mut callers = Vec::new(&env);
    callers.push_back(caller);
    client.set_gate_callers(&Vec::new(&env), &callers);
    assert_eq!(client.get_gate_callers().len(), 1);
    client.set_gate_callers(&Vec::new(&env), &Vec::new(&env));
    assert!(client.get_gate_callers().is_empty());
}
