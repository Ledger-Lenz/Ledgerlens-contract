#![cfg(test)]

//! Unit tests for `get_signer_tier` / `set_signer_tier`.
//!
//! Verifies:
//! - Default `(0, 100)` when no tier has been configured.
//! - Admin can set a tier and the value is returned correctly.
//! - Setting overwrites a previous tier.
//! - Multiple signers have independent, isolated tiers.
//! - Boundary values `(0, 0)`, `(100, 100)`, `(0, 100)` are accepted.
//! - Invalid ranges (`min > max`, either bound > 100) are rejected.
//! - Calling before initialization returns `NotInitialized`.

use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient};

// ── helpers ───────────────────────────────────────────────────────────────────

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    (env, client)
}

fn initialized<'a>() -> (Env, LedgerLensScoreContractClient<'a>) {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client)
}

// ── default behaviour ─────────────────────────────────────────────────────────

#[test]
fn test_get_signer_tier_default_is_full_range() {
    let (env, client) = initialized();
    let signer = Address::generate(&env);
    // When no tier has been configured, the full 0–100 range is returned.
    assert_eq!(client.get_signer_tier(&signer), (0_u32, 100_u32));
}

// ── set and get ───────────────────────────────────────────────────────────────

#[test]
fn test_set_and_get_signer_tier() {
    let (env, client) = initialized();
    let signer = Address::generate(&env);
    let no_admins: Vec<Address> = Vec::new(&env);

    client.set_signer_tier(&no_admins, &signer, &20, &80).unwrap();
    assert_eq!(client.get_signer_tier(&signer), (20_u32, 80_u32));
}

#[test]
fn test_set_signer_tier_overwrites_previous() {
    let (env, client) = initialized();
    let signer = Address::generate(&env);
    let no_admins: Vec<Address> = Vec::new(&env);

    client.set_signer_tier(&no_admins, &signer, &10, &50).unwrap();
    assert_eq!(client.get_signer_tier(&signer), (10_u32, 50_u32));

    // Overwrite with a new range.
    client.set_signer_tier(&no_admins, &signer, &60, &90).unwrap();
    assert_eq!(client.get_signer_tier(&signer), (60_u32, 90_u32));
}

// ── independence across signers ───────────────────────────────────────────────

#[test]
fn test_signer_tiers_are_independent() {
    let (env, client) = initialized();
    let signer_a = Address::generate(&env);
    let signer_b = Address::generate(&env);
    let no_admins: Vec<Address> = Vec::new(&env);

    // Partition: signer_a covers low-risk, signer_b covers high-risk.
    client.set_signer_tier(&no_admins, &signer_a, &0, &49).unwrap();
    client.set_signer_tier(&no_admins, &signer_b, &50, &100).unwrap();

    assert_eq!(client.get_signer_tier(&signer_a), (0_u32, 49_u32));
    assert_eq!(client.get_signer_tier(&signer_b), (50_u32, 100_u32));
}

#[test]
fn test_unconfigured_signer_unaffected_by_other_signer_tier() {
    let (env, client) = initialized();
    let signer_a = Address::generate(&env);
    let signer_b = Address::generate(&env);
    let no_admins: Vec<Address> = Vec::new(&env);

    client.set_signer_tier(&no_admins, &signer_a, &30, &70).unwrap();
    // signer_b was never configured — must still return the default.
    assert_eq!(client.get_signer_tier(&signer_b), (0_u32, 100_u32));
}

// ── boundary values ───────────────────────────────────────────────────────────

#[test]
fn test_set_signer_tier_min_equals_max() {
    // A single-point range is valid (signer limited to exactly score 50).
    let (env, client) = initialized();
    let signer = Address::generate(&env);
    let no_admins: Vec<Address> = Vec::new(&env);

    client.set_signer_tier(&no_admins, &signer, &50, &50).unwrap();
    assert_eq!(client.get_signer_tier(&signer), (50_u32, 50_u32));
}

#[test]
fn test_set_signer_tier_zero_zero() {
    let (env, client) = initialized();
    let signer = Address::generate(&env);
    let no_admins: Vec<Address> = Vec::new(&env);

    client.set_signer_tier(&no_admins, &signer, &0, &0).unwrap();
    assert_eq!(client.get_signer_tier(&signer), (0_u32, 0_u32));
}

#[test]
fn test_set_signer_tier_hundred_hundred() {
    let (env, client) = initialized();
    let signer = Address::generate(&env);
    let no_admins: Vec<Address> = Vec::new(&env);

    client.set_signer_tier(&no_admins, &signer, &100, &100).unwrap();
    assert_eq!(client.get_signer_tier(&signer), (100_u32, 100_u32));
}

#[test]
fn test_set_signer_tier_full_range_explicit() {
    let (env, client) = initialized();
    let signer = Address::generate(&env);
    let no_admins: Vec<Address> = Vec::new(&env);

    client.set_signer_tier(&no_admins, &signer, &0, &100).unwrap();
    assert_eq!(client.get_signer_tier(&signer), (0_u32, 100_u32));
}

// ── validation errors ─────────────────────────────────────────────────────────

#[test]
fn test_set_signer_tier_min_greater_than_max_rejected() {
    let (env, client) = initialized();
    let signer = Address::generate(&env);
    let no_admins: Vec<Address> = Vec::new(&env);

    let result = client.try_set_signer_tier(&no_admins, &signer, &80, &20);
    assert_eq!(result, Err(Ok(Error::InvalidScore)));
}

#[test]
fn test_set_signer_tier_min_above_100_rejected() {
    let (env, client) = initialized();
    let signer = Address::generate(&env);
    let no_admins: Vec<Address> = Vec::new(&env);

    let result = client.try_set_signer_tier(&no_admins, &signer, &101, &101);
    assert_eq!(result, Err(Ok(Error::InvalidScore)));
}

#[test]
fn test_set_signer_tier_max_above_100_rejected() {
    let (env, client) = initialized();
    let signer = Address::generate(&env);
    let no_admins: Vec<Address> = Vec::new(&env);

    let result = client.try_set_signer_tier(&no_admins, &signer, &0, &101);
    assert_eq!(result, Err(Ok(Error::InvalidScore)));
}

// ── not-initialized guard ─────────────────────────────────────────────────────

#[test]
fn test_set_signer_tier_before_init_fails() {
    let (env, client) = setup();
    let signer = Address::generate(&env);
    let no_admins: Vec<Address> = Vec::new(&env);

    let result = client.try_set_signer_tier(&no_admins, &signer, &0, &100);
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}
