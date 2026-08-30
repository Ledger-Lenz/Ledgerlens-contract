#![cfg(test)]

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient};

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);

    client.initialize(&admin, &service);

    (env, client, admin, service)
}

#[test]
fn escalation_threshold_defaults_to_five() {
    let (_env, client, _admin, _service) = setup();

    assert_eq!(client.get_escalation_threshold(), 5);
}

#[test]
fn breach_count_defaults_to_zero() {
    let (env, client, _admin, _service) = setup();

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    assert_eq!(client.get_breach_count(&wallet, &pair), 0);
}

#[test]
fn breach_count_increments_after_consecutive_breaches() {
    let (env, client, _admin, _service) = setup();

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_risk_threshold(&Vec::new(&env), &80);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &90,
        &true,
        &true,
        &1,
        &95,
        &1,
        &None,
    );

    assert_eq!(client.get_breach_count(&wallet, &pair), 1);

    env.ledger().with_mut(|l| l.timestamp += 3601);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &95,
        &true,
        &true,
        &2,
        &95,
        &1,
        &None,
    );

    assert_eq!(client.get_breach_count(&wallet, &pair), 2);
}

#[test]
fn breach_count_resets_after_non_breach_submission() {
    let (env, client, _admin, _service) = setup();

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_risk_threshold(&Vec::new(&env), &80);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &90,
        &true,
        &true,
        &1,
        &95,
        &1,
        &None,
    );

    assert_eq!(client.get_breach_count(&wallet, &pair), 1);

    env.ledger().with_mut(|l| l.timestamp += 3601);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &40,
        &false,
        &false,
        &2,
        &95,
        &1,
        &None,
    );

    assert_eq!(client.get_breach_count(&wallet, &pair), 0);
}
