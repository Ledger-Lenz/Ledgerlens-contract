//! Tests for #302: Strict gate enforcement mode.

use soroban_sdk::{
    symbol_short,
    testutils::Address as _,
    Address, Env, Vec,
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

fn submit_score(env: &Env, client: &LedgerLensScoreContractClient, wallet: &Address, score: u32) {
    client.submit_score(
        &Vec::new(env),
        wallet,
        &symbol_short!("XLM_USDC"),
        &score,
        &false,
        &false,
        &1_700_000_000u64,
        &80u32,
        &1u32,
        &None,
    );
}

// ── advisory mode (default): gate always callable ─────────────────────────────

#[test]
fn test_advisory_mode_gate_open_by_default() {
    let (env, client) = setup();
    assert!(!client.get_gate_enforcement_mode());

    let wallet = Address::generate(&env);
    submit_score(&env, &client, &wallet, 50);

    // In advisory mode, any caller can query the gate.
    let result = client.query_risk_gate(&wallet, &symbol_short!("XLM_USDC"), &75u32);
    assert!(result); // score 50 < threshold 75
}

// ── strict mode: contract itself is the caller, not in allowlist → false ─────

#[test]
fn test_strict_mode_unlisted_caller_returns_false() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    submit_score(&env, &client, &wallet, 50);

    client.set_gate_enforcement_mode(&Vec::new(&env), &true);
    assert!(client.get_gate_enforcement_mode());

    // The test calls query_risk_gate directly on the contract itself; the
    // contract's address is not in GateCallers, so strict mode returns false.
    let result = client.query_risk_gate(&wallet, &symbol_short!("XLM_USDC"), &75u32);
    assert!(!result, "unlisted caller must be rejected in strict mode");
}

// ── toggling: disable strict mode restores advisory behaviour ─────────────────

#[test]
fn test_toggle_strict_to_advisory_restores_gate() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    submit_score(&env, &client, &wallet, 50);

    client.set_gate_enforcement_mode(&Vec::new(&env), &true);
    // Strict mode: gate returns false for this caller.
    assert!(!client.query_risk_gate(&wallet, &symbol_short!("XLM_USDC"), &75u32));

    client.set_gate_enforcement_mode(&Vec::new(&env), &false);
    // Advisory mode: gate returns true (score < threshold, no embargo).
    assert!(client.query_risk_gate(&wallet, &symbol_short!("XLM_USDC"), &75u32));
}

// ── get_gate_enforcement_mode default ────────────────────────────────────────

#[test]
fn test_gate_enforcement_mode_default_false() {
    let (_env, client) = setup();
    assert!(!client.get_gate_enforcement_mode());
}

// ── set_gate_enforcement_mode is admin-only (auth mocked, just verify setter) ─

#[test]
fn test_set_gate_enforcement_mode_persists() {
    let (env, client) = setup();
    client.set_gate_enforcement_mode(&Vec::new(&env), &true);
    assert!(client.get_gate_enforcement_mode());
    client.set_gate_enforcement_mode(&Vec::new(&env), &false);
    assert!(!client.get_gate_enforcement_mode());
}
