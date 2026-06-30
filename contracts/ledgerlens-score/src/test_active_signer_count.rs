#![cfg(test)]
//! Dedicated tests for `get_active_signer_count() -> u32`.
//!
//! A signer is *active* when `get_signer_age(signer) <= get_signer_rotation_ttl()`.

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient};

const BASE_TS: u64 = 1_700_000_000;
const TTL: u64 = 3_600; // 1 hour

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = BASE_TS);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    client.set_signer_rotation_ttl(&Vec::new(&env), &TTL);

    (env, client)
}

// ── Empty set ─────────────────────────────────────────────────────────────────

#[test]
fn test_get_active_signer_count_empty() {
    let (_env, client) = setup();
    assert_eq!(client.get_active_signer_count(), 0);
}

// ── Single signer within TTL ──────────────────────────────────────────────────

#[test]
fn test_get_active_signer_count_single_within_ttl() {
    let (env, client) = setup();
    let signer = Address::generate(&env);

    // Add signer at BASE_TS; advance one hour minus one second (still within TTL).
    client.add_service_signer(&Vec::new(&env), &signer);
    env.ledger().with_mut(|l| l.timestamp = BASE_TS + TTL - 1);
    assert_eq!(client.get_active_signer_count(), 1);
}

// ── Single signer past TTL ────────────────────────────────────────────────────

#[test]
fn test_get_active_signer_count_single_past_ttl() {
    let (env, client) = setup();
    let signer = Address::generate(&env);

    // Add signer at BASE_TS; advance one second past the TTL.
    client.add_service_signer(&Vec::new(&env), &signer);
    env.ledger().with_mut(|l| l.timestamp = BASE_TS + TTL + 1);
    assert_eq!(client.get_active_signer_count(), 0);
}

// ── Boundary: age == TTL is still active ──────────────────────────────────────

#[test]
fn test_get_active_signer_count_at_boundary() {
    let (env, client) = setup();
    let signer = Address::generate(&env);

    // age == TTL: condition is age <= TTL so signer is still counted.
    client.add_service_signer(&Vec::new(&env), &signer);
    env.ledger().with_mut(|l| l.timestamp = BASE_TS + TTL);
    assert_eq!(client.get_active_signer_count(), 1);
}

// ── Mixed: one active, one expired ───────────────────────────────────────────

#[test]
fn test_get_active_signer_count_mixed() {
    let (env, client) = setup();
    let signer1 = Address::generate(&env);
    let signer2 = Address::generate(&env);

    // signer1 added at BASE_TS.
    client.add_service_signer(&Vec::new(&env), &signer1);

    // signer2 added TTL/2 seconds later.
    env.ledger().with_mut(|l| l.timestamp = BASE_TS + TTL / 2);
    client.add_service_signer(&Vec::new(&env), &signer2);

    // At BASE_TS + TTL + 1: signer1 age = TTL+1 (expired), signer2 age = TTL/2+1 (active).
    env.ledger().with_mut(|l| l.timestamp = BASE_TS + TTL + 1);
    assert_eq!(client.get_active_signer_count(), 1);
}

// ── Both active ───────────────────────────────────────────────────────────────

#[test]
fn test_get_active_signer_count_two_active() {
    let (env, client) = setup();
    let signer1 = Address::generate(&env);
    let signer2 = Address::generate(&env);

    client.add_service_signer(&Vec::new(&env), &signer1);
    client.add_service_signer(&Vec::new(&env), &signer2);

    // No time has passed — both age=0 ≤ TTL.
    assert_eq!(client.get_active_signer_count(), 2);
}
