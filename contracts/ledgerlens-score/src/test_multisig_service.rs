#![cfg(test)]

use soroban_sdk::{testutils::Address as _, vec, Address, Env, Symbol};

use crate::{
    errors::Error,
    LedgerLensScoreContract, LedgerLensScoreContractClient,
};

fn setup_env<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client, admin, service)
}

fn admin_signers(env: &Env, admin: &Address) -> soroban_sdk::Vec<Address> {
    vec![env, admin.clone()]
}

fn dummy_pair(env: &Env) -> Symbol {
    Symbol::new(env, "XLM_USDC")
}

fn submit(
    client: &LedgerLensScoreContractClient,
    signers: &soroban_sdk::Vec<Address>,
    wallet: &Address,
    pair: &Symbol,
    score: u32,
) -> Result<(), soroban_sdk::InvokeError> {
    client.try_submit_score(
        signers,
        wallet,
        pair,
        &score,
        &false,
        &false,
        &1_700_000_000u64,
        &80u32,
        &1u32,
        &None,
    )
    .map(|_| ())
}

fn contract_error(err: soroban_sdk::InvokeError) -> u32 {
    match err {
        soroban_sdk::InvokeError::Contract(code) => code,
        _ => panic!("expected contract error"),
    }
}

#[test]
fn legacy_single_service_works_before_multisig_configured() {
    let (env, client, _admin, service) = setup_env();
    let wallet = Address::generate(&env);
    let pair = dummy_pair(&env);
    let signers = vec![&env, service.clone()];
    submit(&client, &signers, &wallet, &pair, 30).unwrap();
    assert_eq!(client.get_score(&wallet, &pair).score, 30);
}

#[test]
fn single_signer_threshold_one_succeeds() {
    let (env, client, admin, _service) = setup_env();
    let signer_a = Address::generate(&env);
    let adm = admin_signers(&env, &admin);
    client.add_service_signer(&adm, &signer_a);
    client.set_service_threshold(&adm, &1u32);

    let wallet = Address::generate(&env);
    let pair = dummy_pair(&env);
    let signers = vec![&env, signer_a.clone()];
    submit(&client, &signers, &wallet, &pair, 55).unwrap();
    assert_eq!(client.get_score(&wallet, &pair).score, 55);
}

#[test]
fn all_n_signers_required_succeeds() {
    let (env, client, admin, _service) = setup_env();
    let signer_a = Address::generate(&env);
    let signer_b = Address::generate(&env);
    let signer_c = Address::generate(&env);
    let adm = admin_signers(&env, &admin);
    client.add_service_signer(&adm, &signer_a);
    client.add_service_signer(&adm, &signer_b);
    client.add_service_signer(&adm, &signer_c);
    client.set_service_threshold(&adm, &3u32);

    let wallet = Address::generate(&env);
    let pair = dummy_pair(&env);
    let signers = vec![&env, signer_a.clone(), signer_b.clone(), signer_c.clone()];
    submit(&client, &signers, &wallet, &pair, 72).unwrap();
    assert_eq!(client.get_score(&wallet, &pair).score, 72);
}

#[test]
fn fewer_than_threshold_fails_with_insufficient_signers() {
    let (env, client, admin, _service) = setup_env();
    let signer_a = Address::generate(&env);
    let signer_b = Address::generate(&env);
    let adm = admin_signers(&env, &admin);
    client.add_service_signer(&adm, &signer_a);
    client.add_service_signer(&adm, &signer_b);
    client.set_service_threshold(&adm, &2u32);

    let wallet = Address::generate(&env);
    let pair = dummy_pair(&env);
    let signers = vec![&env, signer_a.clone()];
    let err = submit(&client, &signers, &wallet, &pair, 40).unwrap_err();
    assert_eq!(contract_error(err), Error::InsufficientSigners as u32);
}

#[test]
fn unknown_signer_fails_with_unauthorized_signer() {
    let (env, client, admin, _service) = setup_env();
    let signer_a = Address::generate(&env);
    let adm = admin_signers(&env, &admin);
    client.add_service_signer(&adm, &signer_a);
    client.set_service_threshold(&adm, &1u32);

    let wallet = Address::generate(&env);
    let pair = dummy_pair(&env);
    let stranger = Address::generate(&env);
    let signers = vec![&env, stranger];
    let err = submit(&client, &signers, &wallet, &pair, 50).unwrap_err();
    assert_eq!(contract_error(err), Error::UnauthorizedSigner as u32);
}

#[test]
fn legacy_service_path_still_works_after_multisig_set_configured() {
    let (env, client, admin, service) = setup_env();
    let signer_a = Address::generate(&env);
    let adm = admin_signers(&env, &admin);
    client.add_service_signer(&adm, &signer_a);
    client.set_service_threshold(&adm, &1u32);

    let wallet = Address::generate(&env);
    let pair = dummy_pair(&env);
    let signers = vec![&env, service.clone()];
    submit(&client, &signers, &wallet, &pair, 65).unwrap();
    assert_eq!(client.get_score(&wallet, &pair).score, 65);
}

#[test]
fn removing_last_multisig_signer_falls_back_to_legacy() {
    let (env, client, admin, service) = setup_env();
    let signer_a = Address::generate(&env);
    let adm = admin_signers(&env, &admin);
    client.add_service_signer(&adm, &signer_a);
    client.set_service_threshold(&adm, &1u32);
    client.remove_service_signer(&adm, &signer_a);

    let wallet = Address::generate(&env);
    let pair = dummy_pair(&env);
    let signers = vec![&env, service.clone()];
    submit(&client, &signers, &wallet, &pair, 20).unwrap();
    assert_eq!(client.get_score(&wallet, &pair).score, 20);
}
