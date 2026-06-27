//! Unit tests for the two-step admin transfer pattern.
//!
//! Covers: transfer_admin, accept_admin, cancel_admin_transfer — happy path,
//! cancellation, edge cases, and non-admin / not-initialized rejections.

use soroban_sdk::{
    testutils::Address as _,
    Address, Env, Vec,
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

// ── Happy path ────────────────────────────────────────────────────────────────

#[test]
fn test_transfer_then_accept_changes_admin() {
    let (env, client, old_admin) = setup();
    let new_admin = Address::generate(&env);

    client.transfer_admin(&Vec::new(&env), &new_admin).unwrap();
    assert_eq!(client.get_pending_admin().unwrap(), new_admin);
    assert_eq!(client.get_admin(), old_admin);

    client.accept_admin().unwrap();
    assert_eq!(client.get_admin(), new_admin);
    assert!(!client.has_pending_admin_transfer());
}

// ── Cancellation ──────────────────────────────────────────────────────────────

#[test]
fn test_cancel_admin_transfer_clears_pending() {
    let (env, client, old_admin) = setup();
    let new_admin = Address::generate(&env);

    client.transfer_admin(&Vec::new(&env), &new_admin).unwrap();
    assert!(client.has_pending_admin_transfer());

    client.cancel_admin_transfer(&Vec::new(&env)).unwrap();
    assert!(!client.has_pending_admin_transfer());
    assert_eq!(client.get_admin(), old_admin);
}

// ── Non-admin rejection ───────────────────────────────────────────────────────

#[test]
fn test_accept_admin_errors_when_no_pending_transfer() {
    let (_, client, _) = setup();
    let result = client.try_accept_admin();
    assert_eq!(result, Err(Ok(Error::NoPendingAdminTransfer)));
}

#[test]
fn test_cancel_admin_transfer_errors_when_no_pending_transfer() {
    let (env, client, _) = setup();
    let result = client.try_cancel_admin_transfer(&Vec::new(&env));
    assert_eq!(result, Err(Ok(Error::NoPendingAdminTransfer)));
}

#[test]
fn test_transfer_admin_errors_when_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let new_admin = Address::generate(&env);

    let result = client.try_transfer_admin(&Vec::new(&env), &new_admin);
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_get_pending_admin_errors_when_none_set() {
    let (_, client, _) = setup();
    let result = client.try_get_pending_admin();
    assert_eq!(result, Err(Ok(Error::NoPendingAdminTransfer)));
}
