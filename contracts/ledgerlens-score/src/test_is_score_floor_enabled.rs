//! Unit tests for `is_score_floor_enabled() -> bool` — issue #232.

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
fn test_is_score_floor_enabled_false_by_default() {
    let (_env, client) = setup();
    assert!(!client.is_score_floor_enabled());
}

#[test]
fn test_is_score_floor_enabled_true_after_enable() {
    let (env, client) = setup();
    client.set_score_floor_policy(&Vec::new(&env), &true, &80, &20);
    assert!(client.is_score_floor_enabled());
}

#[test]
fn test_is_score_floor_enabled_false_after_disable() {
    let (env, client) = setup();
    client.set_score_floor_policy(&Vec::new(&env), &true, &80, &20);
    assert!(client.is_score_floor_enabled());
    client.set_score_floor_policy(&Vec::new(&env), &false, &80, &20);
    assert!(!client.is_score_floor_enabled());
}
