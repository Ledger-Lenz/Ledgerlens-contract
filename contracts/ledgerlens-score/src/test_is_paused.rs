//! Unit tests for `is_paused() -> bool` — issue #230.

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
fn test_is_paused_false_by_default() {
    let (_env, client) = setup();
    assert!(!client.is_paused());
}

#[test]
fn test_is_paused_true_after_pause() {
    let (env, client) = setup();
    client.pause(&Vec::new(&env));
    assert!(client.is_paused());
}

#[test]
fn test_is_paused_false_after_unpause() {
    let (env, client) = setup();
    client.pause(&Vec::new(&env));
    assert!(client.is_paused());
    client.unpause(&Vec::new(&env));
    assert!(!client.is_paused());
}
