//! Tests for the two-step admin transfer mechanism:
//! `transfer_admin` / `accept_admin` / `cancel_admin_transfer`.

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

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
fn test_full_two_step_transfer() {
    let (env, client, old_admin) = setup();
    let new_admin = Address::generate(&env);

    client.transfer_admin(&Vec::new(&env), &new_admin).unwrap();
    assert!(client.has_pending_admin_transfer());
    assert_eq!(client.get_pending_admin().unwrap(), new_admin);

    // Old admin is still active until acceptance.
    assert_eq!(client.get_admin(), old_admin);

    client.accept_admin().unwrap();

    assert_eq!(client.get_admin(), new_admin);
    assert!(!client.has_pending_admin_transfer());
}

// ── Cancellation ─────────────────────────────────────────────────────────────

#[test]
fn test_cancel_clears_pending_admin() {
    let (env, client, old_admin) = setup();
    let new_admin = Address::generate(&env);

    client.transfer_admin(&Vec::new(&env), &new_admin).unwrap();
    assert!(client.has_pending_admin_transfer());

    client.cancel_admin_transfer(&Vec::new(&env)).unwrap();

    assert!(!client.has_pending_admin_transfer());
    // Original admin remains unchanged after cancellation.
    assert_eq!(client.get_admin(), old_admin);
}

#[test]
fn test_cancel_with_no_pending_transfer_errors() {
    let (env, client, _) = setup();
    let result = client.try_cancel_admin_transfer(&Vec::new(&env));
    assert_eq!(result, Err(Ok(Error::NoPendingAdminTransfer)));
}

// ── accept_admin with no pending transfer ─────────────────────────────────────

#[test]
fn test_accept_admin_with_no_pending_transfer_errors() {
    let (_env, client, _) = setup();
    let result = client.try_accept_admin();
    assert_eq!(result, Err(Ok(Error::NoPendingAdminTransfer)));
}

// ── Uninitialized contract ────────────────────────────────────────────────────

#[test]
fn test_transfer_admin_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let new_admin = Address::generate(&env);

    let result = client.try_transfer_admin(&Vec::new(&env), &new_admin);
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_accept_admin_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let result = client.try_accept_admin();
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

// ── Overwrite pending with a new proposal ─────────────────────────────────────

#[test]
fn test_transfer_admin_overwrites_previous_pending() {
    let (env, client, _) = setup();
    let candidate_a = Address::generate(&env);
    let candidate_b = Address::generate(&env);

    client.transfer_admin(&Vec::new(&env), &candidate_a).unwrap();
    assert_eq!(client.get_pending_admin().unwrap(), candidate_a);

    // Proposing a new candidate replaces the old one.
    client.transfer_admin(&Vec::new(&env), &candidate_b).unwrap();
    assert_eq!(client.get_pending_admin().unwrap(), candidate_b);

    client.accept_admin().unwrap();
    assert_eq!(client.get_admin(), candidate_b);
}
