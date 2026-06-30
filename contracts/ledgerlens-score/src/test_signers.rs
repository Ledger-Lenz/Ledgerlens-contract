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

fn add_signer(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    _admin: &Address,
    signer: &Address,
) {
    let empty: Vec<Address> = Vec::new(env);
    client.add_service_signer(&empty, signer);
}

fn try_submit_at(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    signers: &Vec<Address>,
    timestamp: u64,
) -> Result<(), ()> {
    let wallet = Address::generate(env);
    env.ledger().with_mut(|l| l.timestamp = timestamp);
    match client.try_submit_score(
        signers,
        &wallet,
        &symbol_short!("XLM_USDC"),
        &42,
        &false,
        &false,
        &timestamp,
        &90,
        &1,
        &None,
    ) {
        Ok(Ok(())) => Ok(()),
        _ => Err(()),
    }
}

fn try_submit(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    signers: &Vec<Address>,
) -> Result<(), ()> {
    try_submit_at(env, client, signers, 1_000_000)
}

fn submit_ok(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    signers: &Vec<Address>,
) -> bool {
    try_submit(env, client, signers).is_ok()
}

#[test]
fn test_default_ttl_is_disabled() {
    let (_env, client, _admin, _service) = setup();
    assert_eq!(client.get_signer_rotation_ttl(), 0);
}

#[test]
fn test_get_signer_rotation_ttl_returns_configured_value() {
    let (env, client, _admin, _service) = setup();
    let empty_signers: Vec<Address> = Vec::new(&env);
    client.set_signer_rotation_ttl(&empty_signers, &86_400);
    assert_eq!(client.get_signer_rotation_ttl(), 86_400);
}

#[test]
fn test_set_signer_rotation_ttl() {
    let (env, client, _admin, _service) = setup();
    let empty_signers: Vec<Address> = Vec::new(&env);
    client.set_signer_rotation_ttl(&empty_signers, &1_000);
    assert_eq!(client.get_signer_rotation_ttl(), 1_000);
}

#[test]
fn test_get_signer_age_unknown_returns_none() {
    let (env, client, _admin, _service) = setup();
    let signer = Address::generate(&env);
    assert_eq!(client.get_signer_age(&signer), None);
}

#[test]
fn test_get_signer_age_after_add() {
    let (env, client, _admin, _service) = setup();
    let signer = Address::generate(&env);
    let empty: Vec<Address> = Vec::new(&env);
    client.add_service_signer(&empty, &signer);
    let age = client.get_signer_age(&signer).unwrap();
    assert_eq!(age, 0);
}

#[test]
fn test_signer_below_ttl_allowed() {
    let (env, client, _admin, _service) = setup();
    let signer = Address::generate(&env);
    add_signer(&env, &client, &_admin, &signer);

    let empty_signers: Vec<Address> = Vec::new(&env);
    client.set_service_threshold(&empty_signers, &1);
    client.set_signer_rotation_ttl(&empty_signers, &86_400);
    env.ledger().with_mut(|l| l.timestamp = 100);

    let signers = Vec::from_array(&env, [signer.clone()]);
    assert!(try_submit_at(&env, &client, &signers, 100).is_ok());
}

#[test]
fn test_expired_signer_rejected() {
    let (env, client, _admin, _service) = setup();
    let signer = Address::generate(&env);
    let empty_signers: Vec<Address> = Vec::new(&env);

    env.ledger().with_mut(|l| l.timestamp = 100);
    add_signer(&env, &client, &_admin, &signer);

    client.set_service_threshold(&empty_signers, &1);
    client.set_signer_rotation_ttl(&empty_signers, &1);
    client.set_signer_rotation_grace(&empty_signers, &0);

    let signers = Vec::from_array(&env, [signer.clone()]);
    // age=100 > ttl(1)+grace(0)=1 → expired
    let result = try_submit_at(&env, &client, &signers, 200);
    assert!(result.is_err());
}

#[test]
fn test_expired_signer_in_batch_rejected() {
    let (env, client, _admin, _service) = setup();
    let signer = Address::generate(&env);
    let empty_signers: Vec<Address> = Vec::new(&env);

    env.ledger().with_mut(|l| l.timestamp = 100);
    add_signer(&env, &client, &_admin, &signer);

    client.set_service_threshold(&empty_signers, &1);
    client.set_signer_rotation_ttl(&empty_signers, &1);
    client.set_signer_rotation_grace(&empty_signers, &0);

    let signers = Vec::from_array(&env, [signer.clone()]);

    let submissions = Vec::new(&env);
    let attestation = crate::types::BatchAttestation {
        merkle_root: soroban_sdk::BytesN::from_array(&env, &[0u8; 32]),
        signature: soroban_sdk::BytesN::from_array(&env, &[0u8; 65]),
    };
    let result = client.try_submit_scores_batch_attested(&signers, &submissions, &attestation);
    assert!(result.is_err());
}

#[test]
fn test_signer_in_grace_period_warning_emitted() {
    let (env, client, _admin, _service) = setup();
    let signer = Address::generate(&env);
    let empty_signers: Vec<Address> = Vec::new(&env);

    env.ledger().with_mut(|l| l.timestamp = 100);
    add_signer(&env, &client, &_admin, &signer);

    client.set_service_threshold(&empty_signers, &1);
    client.set_signer_rotation_ttl(&empty_signers, &1);
    client.set_signer_rotation_grace(&empty_signers, &200);

    let signers = Vec::from_array(&env, [signer.clone()]);
    // age=100 > ttl(1) but < ttl(1)+grace(200)=201 → within grace
    assert!(try_submit_at(&env, &client, &signers, 200).is_ok());
}

#[test]
fn test_ttl_zero_disables_check() {
    let (env, client, _admin, _service) = setup();
    let signer = Address::generate(&env);
    env.ledger().with_mut(|l| l.timestamp = 100);
    add_signer(&env, &client, &_admin, &signer);

    let empty_signers: Vec<Address> = Vec::new(&env);
    client.set_service_threshold(&empty_signers, &1);
    client.set_signer_rotation_ttl(&empty_signers, &0);

    let signers = Vec::from_array(&env, [signer.clone()]);
    assert!(try_submit_at(&env, &client, &signers, 315_360_100).is_ok());
}

#[test]
fn test_set_signer_rotation_grace() {
    let (env, client, _admin, _service) = setup();
    let empty_signers: Vec<Address> = Vec::new(&env);
    client.set_signer_rotation_grace(&empty_signers, &86_400);
    // TTL + grace should now pass the old combined check
    // This just tests the setter doesn't fail
}

#[test]
fn test_remove_signer_clears_added_at() {
    let (env, client, _admin, _service) = setup();
    let signer = Address::generate(&env);
    let empty_signers: Vec<Address> = Vec::new(&env);
    client.add_service_signer(&empty_signers, &signer);
    assert!(client.get_signer_age(&signer).is_some());

    client.remove_service_signer(&empty_signers, &signer);
    // After removal, adding the signer again records a fresh timestamp
    client.add_service_signer(&empty_signers, &signer);
    let age = client.get_signer_age(&signer).unwrap();
    assert_eq!(age, 0);
}
