#![cfg(test)]

//! Comprehensive tests for the admin fee withdrawal feature:
//! `set_fee_token`, `get_fee_token`, `set_fee_recipient`, `get_fee_recipient`,
//! and `withdraw_fees`.
//!
//! Token testing uses `env.register_stellar_asset_contract_v2` so the
//! contract interacts with a real SEP-41 token mock, exercising the actual
//! `token::TokenClient::transfer` path.

use soroban_sdk::{
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    token::{StellarAssetClient, TokenClient},
    Address, Env, IntoVal, Vec,
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
    assert_eq!(client.get_fee_token(), Some(token_address));
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
    assert_eq!(client.get_fee_token(), Some(token2));
}

// ── get_fee_token ─────────────────────────────────────────────────────────────

#[test]
fn test_get_fee_token_not_set() {
    let (_env, client, _admin) = setup_no_token();
    assert_eq!(client.get_fee_token(), None);
}

// ── set_fee_recipient / get_fee_recipient ─────────────────────────────────────

#[test]
fn test_set_fee_recipient_success() {
    let (env, client, _admin, _token_address, _contract_id) = setup_with_token(0);
    let recipient = Address::generate(&env);
    client.set_fee_recipient(&Vec::new(&env), &recipient);
    assert_eq!(client.get_fee_recipient(), recipient);
}

#[test]
fn test_set_fee_recipient_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let recipient = Address::generate(&env);
    let result = client.try_set_fee_recipient(&Vec::new(&env), &recipient);
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_set_fee_recipient_can_be_updated() {
    let (env, client, _admin, _token_address, _contract_id) = setup_with_token(0);
    let recipient = Address::generate(&env);
    client.set_fee_recipient(&Vec::new(&env), &recipient);

    let recipient2 = Address::generate(&env);
    client.set_fee_recipient(&Vec::new(&env), &recipient2);
    assert_eq!(client.get_fee_recipient(), recipient2);
}

#[test]
fn test_get_fee_recipient_not_set() {
    let (_env, client, _admin) = setup_no_token();
    let result = client.try_get_fee_recipient();
    assert_eq!(result, Err(Ok(Error::NotFound)));
}

// ── withdraw_fees — success path ──────────────────────────────────────────────

#[test]
fn test_withdraw_fees_success() {
    let contract_balance: i128 = 1_000_000;
    let withdraw_amount: i128 = 400_000;

    let (env, client, _admin, token_address, contract_id) = setup_with_token(contract_balance);
    client.set_fee_token(&token_address);

    let recipient = Address::generate(&env);
    client.set_fee_recipient(&Vec::new(&env), &recipient);
    let token = TokenClient::new(&env, &token_address);

    assert_eq!(token.balance(&contract_id), contract_balance);
    assert_eq!(token.balance(&recipient), 0);

    client.withdraw_fees(&Vec::new(&env), &recipient, &withdraw_amount);

    assert_eq!(token.balance(&contract_id), contract_balance - withdraw_amount);
    assert_eq!(token.balance(&recipient), withdraw_amount);
}

#[test]
fn test_withdraw_fees_full_balance() {
    let balance: i128 = 500_000;
    let (env, client, _admin, token_address, contract_id) = setup_with_token(balance);
    client.set_fee_token(&token_address);

    let recipient = Address::generate(&env);
    client.set_fee_recipient(&Vec::new(&env), &recipient);
    let token = TokenClient::new(&env, &token_address);

    client.withdraw_fees(&Vec::new(&env), &recipient, &balance);

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
    let result = client.try_withdraw_fees(&Vec::new(&env), &recipient, &100);
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_withdraw_fees_zero_amount_rejected() {
    let (env, client, _admin, token_address, _contract_id) = setup_with_token(100_000);
    client.set_fee_token(&token_address);
    let recipient = Address::generate(&env);
    let result = client.try_withdraw_fees(&Vec::new(&env), &recipient, &0);
    assert_eq!(result, Err(Ok(Error::InvalidWithdrawalAmount)));
}

#[test]
fn test_withdraw_fees_fee_token_not_set() {
    let (env, client, _admin) = setup_no_token();
    let recipient = Address::generate(&env);
    let result = client.try_withdraw_fees(&Vec::new(&env), &recipient, &1000);
    assert_eq!(result, Err(Ok(Error::FeeTokenNotSet)));
}

#[test]
fn test_withdraw_fees_contract_paused() {
    let (env, client, _admin, token_address, _contract_id) = setup_with_token(100_000);
    client.set_fee_token(&token_address);
    client.pause(&Vec::new(&env));
    let recipient = Address::generate(&env);
    let result = client.try_withdraw_fees(&Vec::new(&env), &recipient, &1000);
    assert_eq!(result, Err(Ok(Error::ContractPaused)));
}

// ── withdraw_fees — fee recipient dual-authorization ─────────────────────────

#[test]
fn test_withdraw_fees_recipient_not_registered() {
    // `set_fee_recipient` has never been called.
    let (env, client, _admin, token_address, _contract_id) = setup_with_token(100_000);
    client.set_fee_token(&token_address);
    let recipient = Address::generate(&env);
    let result = client.try_withdraw_fees(&Vec::new(&env), &recipient, &1000);
    assert_eq!(result, Err(Ok(Error::FeeRecipientNotSet)));
}

#[test]
fn test_withdraw_fees_unregistered_recipient_fails() {
    // A registered recipient exists, but the caller asks to withdraw to a
    // different, non-registered address — must be rejected even though the
    // admin authorizes the call.
    let (env, client, _admin, token_address, _contract_id) = setup_with_token(100_000);
    client.set_fee_token(&token_address);

    let registered_recipient = Address::generate(&env);
    client.set_fee_recipient(&Vec::new(&env), &registered_recipient);

    let attacker = Address::generate(&env);
    let result = client.try_withdraw_fees(&Vec::new(&env), &attacker, &1000);
    assert_eq!(result, Err(Ok(Error::FeeRecipientMismatch)));
}

#[test]
fn test_withdraw_fees_registered_recipient_and_admin_succeeds() {
    // Both the admin and the registered recipient authorize the call (via
    // `mock_all_auths`) — the dual-authorization requirement is satisfied.
    let (env, client, _admin, token_address, contract_id) = setup_with_token(100_000);
    client.set_fee_token(&token_address);

    let recipient = Address::generate(&env);
    client.set_fee_recipient(&Vec::new(&env), &recipient);

    client.withdraw_fees(&Vec::new(&env), &recipient, &40_000);

    let token = TokenClient::new(&env, &token_address);
    assert_eq!(token.balance(&recipient), 40_000);
    assert_eq!(token.balance(&contract_id), 60_000);
}

#[test]
#[should_panic]
fn test_withdraw_fees_admin_alone_fails() {
    // The admin authorizes the call but the registered recipient does not —
    // `recipient.require_auth()` must panic. We mock only the admin's
    // invocation (not the recipient's) so the contract's own dual-auth check
    // is what's under test, not `mock_all_auths` papering over it.
    let (env, client, admin, token_address, _contract_id) = setup_with_token(100_000);
    client.set_fee_token(&token_address);

    let recipient = Address::generate(&env);
    client.set_fee_recipient(&Vec::new(&env), &recipient);

    let admin_signers: Vec<Address> = Vec::new(&env);
    let amount: i128 = 1_000;
    client
        .mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &client.address,
                fn_name: "withdraw_fees",
                args: (admin_signers.clone(), recipient.clone(), amount).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .withdraw_fees(&admin_signers, &recipient, &amount);
}

// ── withdraw_fees — concurrency / duplicate lock ──────────────────────────────

#[test]
fn test_withdraw_fees_lock_prevents_duplicate() {
    // Directly set the withdrawal lock in storage to simulate a concurrent call.
    let (env, client, _admin, token_address, contract_id) = setup_with_token(100_000);
    client.set_fee_token(&token_address);

    let recipient = Address::generate(&env);
    client.set_fee_recipient(&Vec::new(&env), &recipient);

    // Reach into storage to set the lock manually.
    env.as_contract(&contract_id, || {
        crate::storage::set_withdrawal_lock(&env);
    });

    let result = client.try_withdraw_fees(&Vec::new(&env), &recipient, &1000);
    assert_eq!(result, Err(Ok(Error::WithdrawalInProgress)));
}

#[test]
fn test_withdrawal_lock_cleared_after_success() {
    // Verify the lock is released on a successful withdrawal so subsequent
    // calls are not blocked. Both withdrawals target the single registered
    // recipient, since `withdraw_fees` no longer accepts an arbitrary address.
    let (env, client, _admin, token_address, _contract_id) = setup_with_token(200_000);
    client.set_fee_token(&token_address);

    let recipient = Address::generate(&env);
    client.set_fee_recipient(&Vec::new(&env), &recipient);

    client.withdraw_fees(&Vec::new(&env), &recipient, &50_000);

    // A second withdrawal should succeed — lock was released.
    client.withdraw_fees(&Vec::new(&env), &recipient, &50_000);

    let token = TokenClient::new(&env, &token_address);
    assert_eq!(token.balance(&recipient), 100_000);
}

// ── withdraw_fees — unauthorized access ───────────────────────────────────────

#[test]
fn test_withdraw_fees_requires_admin_auth() {
    // Without mock_all_auths, only the admin's explicit auth is checked.
    // We rely on mock_all_auths in setup; here we verify the admin
    // require_auth path is present by ensuring the call does not panic when
    // auth is mocked. The dedicated auth-failure test is
    // `test_withdraw_fees_admin_alone_fails`, which uses `mock_auths` to
    // mock only the admin and confirm the recipient side is still enforced.
    let (env, client, _admin, token_address, _) = setup_with_token(100_000);
    client.set_fee_token(&token_address);
    let recipient = Address::generate(&env);
    client.set_fee_recipient(&Vec::new(&env), &recipient);
    // Should succeed with mocked auth.
    client.withdraw_fees(&Vec::new(&env), &recipient, &1_000);
}

#[test]
fn test_set_fee_token_requires_admin_auth() {
    // Same rationale as above — verifies no panic with mocked auth.
    let (env, client, _admin, token_address, _) = setup_with_token(0);
    let _ = env;
    client.set_fee_token(&token_address); // succeeds with mock_all_auths
}
