//! Tests for #299: Merkle governance audit chain.

use soroban_sdk::{
    testutils::Address as _,
    Address, BytesN, Env, Vec,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient};

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client)
}

// ── genesis head is all-zeros ─────────────────────────────────────────────────

#[test]
fn test_initial_governance_chain_head_is_zero() {
    let (env, client) = setup();
    let head = client.get_governance_chain_head();
    assert_eq!(head, BytesN::from_array(&env, &[0u8; 32]));
}

// ── head advances after an admin action ──────────────────────────────────────

#[test]
fn test_head_advances_on_admin_action() {
    let (env, client) = setup();
    let head_before = client.get_governance_chain_head();

    // set_gate_enforcement_mode is an admin action that appends to the chain.
    client.set_gate_enforcement_mode(&Vec::new(&env), &true);

    let head_after = client.get_governance_chain_head();
    assert_ne!(head_before, head_after, "head must change after admin action");
}

// ── chain is monotonic: each action produces a different head ─────────────────

#[test]
fn test_chain_is_monotonic() {
    let (env, client) = setup();

    client.set_gate_enforcement_mode(&Vec::new(&env), &true);
    let h1 = client.get_governance_chain_head();

    client.set_gate_enforcement_mode(&Vec::new(&env), &false);
    let h2 = client.get_governance_chain_head();

    assert_ne!(h1, h2);
}

// ── verify_governance_action with empty proof verifies a single-step chain ───

#[test]
fn test_verify_governance_action_single_step() {
    let (env, client) = setup();
    // After one action the new head = SHA256(0x01 || zeros32 || action_data).
    // We reproduce the action_data for set_gate_enforcement_mode(true): [0u8; 31, 1u8].
    let mut action_data = [0u8; 32];
    action_data[31] = 1u8; // strict = true

    // Derive action_hash = SHA256(0x01 || genesis(zeros) || action_data)
    let genesis = BytesN::<32>::from_array(&env, &[0u8; 32]);
    let mut buf = [0u8; 65];
    buf[0] = 0x01;
    buf[1..33].copy_from_slice(&genesis.to_array());
    buf[33..65].copy_from_slice(&action_data);
    let action_hash = env.crypto().sha256(&soroban_sdk::Bytes::from_array(&env, &buf));
    let action_hash_bytes = BytesN::<32>::from_array(&env, &action_hash.to_bytes().to_array());

    // Trigger the action.
    client.set_gate_enforcement_mode(&Vec::new(&env), &true);

    // With an empty proof, action_hash itself should equal the chain head.
    let result = client.verify_governance_action(&action_hash_bytes, &Vec::new(&env));
    assert!(result, "single-step chain verification must succeed");
}

// ── verify_governance_action with wrong hash returns false ───────────────────

#[test]
fn test_verify_governance_action_wrong_hash_fails() {
    let (env, client) = setup();
    client.set_gate_enforcement_mode(&Vec::new(&env), &true);

    let bad_hash = BytesN::from_array(&env, &[0xffu8; 32]);
    assert!(!client.verify_governance_action(&bad_hash, &Vec::new(&env)));
}
