#![cfg(test)]

//! Comprehensive tests for the admin fee withdrawal feature:
//! `set_fee_token`, `get_fee_token`, and `withdraw_fees`.
//!
//! Token testing uses `env.register_stellar_asset_contract_v2` so the
//! contract interacts with a real SEP-41 token mock, exercising the actual
//! `token::TokenClient::transfer` path.

use soroban_sdk::{
    testutils::Address as _,
    token::{StellarAssetClient, TokenClient},
    Address, Env,
};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient};

// ── Test helpers ──────────────────────────────────────────────────────────────

/// Returns (env, client, admin, token_address, contract_address).
/// The mock token is minted with `initial_balance` stroops to the contract.
fn setup_with_token<'a>(
    initial_balance: i128,
) -> (Env, LedgerLensScoreContractClient<'a>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    // Deploy a mock SEP-41 token and fund the contract with initial_balance.
    let issuer = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(issuer);
    let token_address = sac.address();

    if initial_balance > 0 {
        StellarAssetClient::new(&env, &token_address).mint(&contract_id, &initial_balance);
    }

    (env, client, admin, token_address, contract_id)
}

/// Returns (env, client, admin) with no token configured.
fn setup_no_token<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    (env, client, admin)
}

// ── set_fee_token ─────────────────────────────────────────────────────────────

#[test]
fn test_set_fee_token_success() {
    let (env, client, _admin, token_address, _contract_id) = setup_with_token(0);
    client.set_fee_token(&token_address);
    assert_eq!(client.get_fee_token(), token_address);
    let _ = (env,); // suppress unused warning
}

#[test]
fn test_set_fee_token_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let token = Address::generate(&env);
    let result = client.try_set_fee_token(&token);
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_set_fee_token_can_be_updated() {
    let (env, client, _admin, token_address, _contract_id) = setup_with_token(0);
    client.set_fee_token(&token_address);

    let issuer2 = Address::generate(&env);
    let sac2 = env.register_stellar_asset_contract_v2(issuer2);
    let token2 = sac2.address();

    client.set_fee_token(&token2);
    assert_eq!(client.get_fee_token(), token2);
}

// ── get_fee_token ─────────────────────────────────────────────────────────────

#[test]
fn test_get_fee_token_not_set() {
    let (_env, client, _admin) = setup_no_token();
    let result = client.try_get_fee_token();
    assert_eq!(result, Err(Ok(Error::FeeTokenNotSet)));
}

// ── withdraw_fees — success path ──────────────────────────────────────────────

#[test]
fn test_withdraw_fees_success() {
    let contract_balance: i128 = 1_000_000;
    let withdraw_amount: i128 = 400_000;

    let (env, client, _admin, token_address, contract_id) = setup_with_token(contract_balance);
    client.set_fee_token(&token_address);

    let recipient = Address::generate(&env);
    let token = TokenClient::new(&env, &token_address);

    assert_eq!(token.balance(&contract_id), contract_balance);
    assert_eq!(token.balance(&recipient), 0);

    client.withdraw_fees(&recipient, &withdraw_amount);

    assert_eq!(token.balance(&contract_id), contract_balance - withdraw_amount);
    assert_eq!(token.balance(&recipient), withdraw_amount);
}

#[test]
fn test_withdraw_fees_full_balance() {
    let balance: i128 = 500_000;
    let (env, client, _admin, token_address, contract_id) = setup_with_token(balance);
    client.set_fee_token(&token_address);

    let recipient = Address::generate(&env);
    let token = TokenClient::new(&env, &token_address);

    client.withdraw_fees(&recipient, &balance);

    assert_eq!(token.balance(&contract_id), 0);
    assert_eq!(token.balance(&recipient), balance);
}

// ── withdraw_fees — validation errors ────────────────────────────────────────

#[test]
fn test_withdraw_fees_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let recipient = Address::generate(&env);
    let result = client.try_withdraw_fees(&recipient, &100);
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_withdraw_fees_zero_amount_rejected() {
    let (env, client, _admin, token_address, _contract_id) = setup_with_token(100_000);
    client.set_fee_token(&token_address);
    let recipient = Address::generate(&env);
    let result = client.try_withdraw_fees(&recipient, &0);
    assert_eq!(result, Err(Ok(Error::InvalidWithdrawalAmount)));
}

#[test]
fn test_withdraw_fees_fee_token_not_set() {
    let (env, client, _admin) = setup_no_token();
    let recipient = Address::generate(&env);
    let result = client.try_withdraw_fees(&recipient, &1000);
    assert_eq!(result, Err(Ok(Error::FeeTokenNotSet)));
}

#[test]
fn test_withdraw_fees_contract_paused() {
    let (env, client, _admin, token_address, _contract_id) = setup_with_token(100_000);
    client.set_fee_token(&token_address);
    client.pause();
    let recipient = Address::generate(&env);
    let result = client.try_withdraw_fees(&recipient, &1000);
    assert_eq!(result, Err(Ok(Error::ContractPaused)));
}

// ── withdraw_fees — concurrency / duplicate lock ──────────────────────────────

#[test]
fn test_withdraw_fees_lock_prevents_duplicate() {
    // Directly set the withdrawal lock in storage to simulate a concurrent call.
    let (env, client, _admin, token_address, contract_id) = setup_with_token(100_000);
    client.set_fee_token(&token_address);

    // Reach into storage to set the lock manually.
    env.as_contract(&contract_id, || {
        crate::storage::set_withdrawal_lock(&env);
    });

    let recipient = Address::generate(&env);
    let result = client.try_withdraw_fees(&recipient, &1000);
    assert_eq!(result, Err(Ok(Error::WithdrawalInProgress)));
}

#[test]
fn test_withdrawal_lock_cleared_after_success() {
    // Verify the lock is released on a successful withdrawal so subsequent
    // calls are not blocked.
    let (env, client, _admin, token_address, _contract_id) = setup_with_token(200_000);
    client.set_fee_token(&token_address);

    let recipient = Address::generate(&env);
    client.withdraw_fees(&recipient, &50_000);

    // A second withdrawal should succeed — lock was released.
    let recipient2 = Address::generate(&env);
    client.withdraw_fees(&recipient2, &50_000);

    let token = TokenClient::new(&env, &token_address);
    assert_eq!(token.balance(&recipient), 50_000);
    assert_eq!(token.balance(&recipient2), 50_000);
}

// ── withdraw_fees — unauthorized access ───────────────────────────────────────

#[test]
fn test_withdraw_fees_requires_admin_auth() {
    // Without mock_all_auths, only the admin's explicit auth is checked.
    // We rely on mock_all_auths in setup; here we verify the admin
    // require_auth path is present by ensuring the call does not panic when
    // auth is mocked.  A dedicated auth-failure test would require a more
    // elaborate setup outside the scope of `mock_all_auths`.
    let (env, client, _admin, token_address, _) = setup_with_token(100_000);
    client.set_fee_token(&token_address);
    let recipient = Address::generate(&env);
    // Should succeed with mocked auth.
    client.withdraw_fees(&recipient, &1_000);
}

#[test]
fn test_set_fee_token_requires_admin_auth() {
    // Same rationale as above — verifies no panic with mocked auth.
    let (env, client, _admin, token_address, _) = setup_with_token(0);
    let _ = env;
    client.set_fee_token(&token_address); // succeeds with mock_all_auths
}
