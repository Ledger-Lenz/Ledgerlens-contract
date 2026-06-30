#![cfg(test)]

//! Tests for the automatic service quorum reduction protocol.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{
    constants::DEFAULT_QUORUM_FAILURE_WINDOW_SECS, Error, LedgerLensScoreContract,
    LedgerLensScoreContractClient,
};

const START_TS: u64 = 1_700_000_000;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    (env, client, admin)
}

fn setup_multisig<'a>(
    env: &Env,
    client: &LedgerLensScoreContractClient<'a>,
    num_signers: u32,
    threshold: u32,
) -> Vec<Address> {
    let mut signers = Vec::new(env);
    for _ in 0..num_signers {
        let signer = Address::generate(env);
        client.add_service_signer(&Vec::new(env), &signer);
        signers.push_back(signer);
    }
    if threshold > 0 {
        client.set_service_threshold(&Vec::new(env), &threshold);
    }
    signers
}

#[test]
fn test_last_global_submission_time_updated() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    assert_eq!(client.get_last_global_submission_time(), 0);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );

    assert_eq!(client.get_last_global_submission_time(), START_TS);
}

#[test]
fn test_request_reduction_fails_before_window_elapses() {
    let (env, client, _) = setup();
    setup_multisig(&env, &client, 3, 3);

    client.submit_score(
        &Vec::new(&env),
        &Address::generate(&env),
        &symbol_short!("XLM_USDC"),
        &50,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );

    let window = client.get_quorum_failure_window();
    env.ledger().with_mut(|l| l.timestamp += window - 1);

    let result = client.try_request_quorum_reduction(&Vec::new(&env), &2);
    assert_eq!(result, Err(Ok(Error::QuorumFailureWindowNotElapsed)));
}

#[test]
fn test_request_reduction_succeeds_after_window() {
    let (env, client, _) = setup();
    setup_multisig(&env, &client, 3, 3);

    client.submit_score(
        &Vec::new(&env),
        &Address::generate(&env),
        &symbol_short!("XLM_USDC"),
        &50,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );

    let window = client.get_quorum_failure_window();
    env.ledger().with_mut(|l| l.timestamp += window);

    client.request_quorum_reduction(&Vec::new(&env), &2);
    assert_eq!(client.get_service_threshold(), 2);
}

#[test]
fn test_restore_quorum() {
    let (env, client, _) = setup();
    setup_multisig(&env, &client, 3, 3);

    let window = client.get_quorum_failure_window();
    env.ledger().with_mut(|l| l.timestamp += window);

    client.request_quorum_reduction(&Vec::new(&env), &2);
    assert_eq!(client.get_service_threshold(), 2);

    client.restore_quorum(&Vec::new(&env));
    assert_eq!(client.get_service_threshold(), 3);
}

#[test]
fn test_restore_quorum_fails_if_not_reduced() {
    let (env, client, _) = setup();
    setup_multisig(&env, &client, 3, 3);

    let result = client.try_restore_quorum(&Vec::new(&env));
    assert_eq!(result, Err(Ok(Error::InvalidThreshold)));
}

#[test]
fn test_set_and_get_quorum_failure_window() {
    let (env, client, _) = setup();
    assert_eq!(
        client.get_quorum_failure_window(),
        DEFAULT_QUORUM_FAILURE_WINDOW_SECS
    );

    let new_window = 3600;
    client.set_quorum_failure_window(&Vec::new(&env), &new_window);
    assert_eq!(client.get_quorum_failure_window(), new_window);
}

#[test]
fn test_reduced_quorum_allows_submission() {
    let (env, client, _) = setup();
    let signers = setup_multisig(&env, &client, 3, 3);

    let window = client.get_quorum_failure_window();
    env.ledger().with_mut(|l| l.timestamp += window);

    client.request_quorum_reduction(&Vec::new(&env), &2);

    let mut reduced_signers = Vec::new(&env);
    reduced_signers.push_back(signers.get(0).unwrap());
    reduced_signers.push_back(signers.get(1).unwrap());

    let result = client.try_submit_score(
        &reduced_signers,
        &Address::generate(&env),
        &symbol_short!("XLM_USDC"),
        &50, &false, &false, &env.ledger().timestamp(), &90, &1, &None
    );
    assert!(result.is_ok());
}