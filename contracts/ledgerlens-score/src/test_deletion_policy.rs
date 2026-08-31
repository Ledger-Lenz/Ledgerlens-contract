#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal, Vec,
};

use crate::{
    Error, LedgerLensScoreContract, LedgerLensScoreContractClient,
};

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    let approver = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client, admin, service, approver)
}

#[test]
fn test_clear_score_allowed_when_deletion_policy_disabled() {
    let (env, client, _admin, _service, approver) = setup();
    let wallet = Address::generate(&env);
    let pair = soroban_sdk::symbol_short!("XLM_USDC");

    client.set_deletion_approval_policy(&Vec::new(&env), &false, &Some(approver));
    assert_eq!(client.try_clear_score(&Vec::new(&env), &wallet, &pair), Ok(Ok(())));
}

#[test]
fn test_enabling_policy_without_separate_approver_is_rejected() {
    let (env, client, _admin, _service, _approver) = setup();

    let result = client.try_set_deletion_approval_policy(&Vec::new(&env), &true, &None);
    assert_eq!(result, Err(Ok(Error::InvalidThreshold)));
}

#[test]
fn test_enabling_policy_with_admin_as_approver_is_rejected() {
    let (env, client, admin, _service, _approver) = setup();

    let result = client.try_set_deletion_approval_policy(&Vec::new(&env), &true, &Some(admin));
    assert_eq!(result, Err(Ok(Error::InvalidThreshold)));
}

#[test]
#[should_panic]
fn test_clear_score_requires_separate_approver_auth_when_policy_enabled() {
    let (env, client, admin, _service, approver) = setup();
    client.set_deletion_approval_policy(&Vec::new(&env), &true, &Some(approver.clone()));

    let admin_signers: Vec<Address> = Vec::new(&env);
    let wallet = Address::generate(&env);
    let pair = soroban_sdk::symbol_short!("XLM_USDC");

    client
        .mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &client.address,
                fn_name: "clear_score",
                args: (admin_signers.clone(), wallet.clone(), pair.clone()).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .clear_score(&admin_signers, &wallet, &pair);
}

#[test]
fn test_get_deletion_policy_reports_enabled_config() {
    let (env, client, _admin, _service, approver) = setup();
    client.set_deletion_approval_policy(&Vec::new(&env), &true, &Some(approver.clone()));

    let policy = client.get_deletion_approval_policy();
    assert!(policy.enabled);
    assert_eq!(policy.approver, Some(approver));
}
