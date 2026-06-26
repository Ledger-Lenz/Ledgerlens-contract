//! Tests for #298: M-of-N admin co-signatures required to propose an upgrade.

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, BytesN, Env, Vec,
};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient};

const START_TS: u64 = 1_700_000_000;

fn setup_multisig<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    // Set up 2-of-3 admin multisig
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);
    let a3 = Address::generate(&env);
    client.add_admin_signer(&Vec::new(&env), &a1);
    client.add_admin_signer(&Vec::new(&env), &a2);
    client.add_admin_signer(&Vec::new(&env), &a3);
    client.set_admin_threshold(&Vec::new(&env), &2);

    (env, client, a1, a2, a3)
}

fn dummy_hash(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0xabu8; 32])
}

// ── quorum not met: partial approval accumulated, proposal not stored ─────────

#[test]
fn test_single_signer_does_not_store_proposal() {
    let (env, client, a1, _a2, _a3) = setup_multisig();
    let hash = dummy_hash(&env);

    let mut signers: Vec<Address> = Vec::new(&env);
    signers.push_back(a1.clone());
    // Only 1 of 2 required — should return Ok but not store a proposal yet.
    client.propose_upgrade(&signers, &hash);

    assert_eq!(client.get_upgrade_approval_count(), 1);
    // No proposal stored yet.
    let result = client.try_get_pending_upgrade();
    assert_eq!(result, Err(Ok(Error::NoPendingUpgrade)));
}

// ── quorum met: proposal stored after second distinct signer ─────────────────

#[test]
fn test_quorum_met_stores_proposal() {
    let (env, client, a1, a2, _a3) = setup_multisig();
    let hash = dummy_hash(&env);

    let mut sig1: Vec<Address> = Vec::new(&env);
    sig1.push_back(a1.clone());
    client.propose_upgrade(&sig1, &hash);
    assert_eq!(client.get_upgrade_approval_count(), 1);

    let mut sig2: Vec<Address> = Vec::new(&env);
    sig2.push_back(a2.clone());
    client.propose_upgrade(&sig2, &hash);

    // Approvals cleared after threshold met.
    assert_eq!(client.get_upgrade_approval_count(), 0);
    // Proposal must now be stored.
    let proposal = client.get_pending_upgrade();
    assert_eq!(proposal.new_wasm_hash, hash);
}

// ── duplicate signer does not double-count ───────────────────────────────────

#[test]
fn test_duplicate_signer_not_double_counted() {
    let (env, client, a1, _a2, _a3) = setup_multisig();
    let hash = dummy_hash(&env);

    let mut signers: Vec<Address> = Vec::new(&env);
    signers.push_back(a1.clone());

    client.propose_upgrade(&signers, &hash);
    // Call again with the same signer — still only 1 unique approval.
    client.propose_upgrade(&signers, &hash);

    assert_eq!(client.get_upgrade_approval_count(), 1);
    assert_eq!(client.try_get_pending_upgrade(), Err(Ok(Error::NoPendingUpgrade)));
}

// ── legacy mode (no admin set): single admin proposes directly ────────────────

#[test]
fn test_legacy_mode_single_admin_propose() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    let hash = dummy_hash(&env);
    client.propose_upgrade(&Vec::new(&env), &hash);
    // In legacy mode the proposal is stored immediately.
    let proposal = client.get_pending_upgrade();
    assert_eq!(proposal.new_wasm_hash, hash);
}

// ── get_upgrade_approval_count returns 0 when no approvals pending ────────────

#[test]
fn test_approval_count_zero_initially() {
    let (_env, client, _a1, _a2, _a3) = setup_multisig();
    assert_eq!(client.get_upgrade_approval_count(), 0);
}
