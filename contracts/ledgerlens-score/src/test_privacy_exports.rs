use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient};

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    (env, client, admin, service)
}

// ─────────────────────────────────────────────────────────────────────────────
// Issue #727 – Privacy-preserving export modes
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_get_score_export_public_below_threshold() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // Submit a low score (below default threshold 75)
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &1_700_000_000,
        &90,
        &1,
        &None,
    );

    let export = client.get_score_export_public(&wallet, &pair).unwrap();
    assert_eq!(export.wallet, wallet);
    assert_eq!(export.asset_pair, pair);
    // Public export only shows 0 for passing gate
    assert_eq!(export.risk_gate_decision, 0);
}

#[test]
fn test_get_score_export_public_above_threshold() {
    let (env, client, admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // Set a higher threshold to test
    client.set_threshold(&admin, &80);

    // Submit a score above threshold
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &85,
        &false,
        &false,
        &1_700_000_000,
        &90,
        &1,
        &None,
    );

    let export = client.get_score_export_public(&wallet, &pair).unwrap();
    // Public export shows score value when breached
    assert_eq!(export.risk_gate_decision, 85);
}

#[test]
fn test_get_score_export_public_not_found() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let result = client.get_score_export_public(&wallet, &pair);
    assert!(result.is_err());
}

#[test]
fn test_get_score_export_operator_includes_details() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &65,
        &false,
        &false,
        &1_700_000_000,
        &85,
        &2,
        &None,
    );

    let export = client.get_score_export_operator(&wallet, &pair).unwrap();
    assert_eq!(export.wallet, wallet);
    assert_eq!(export.asset_pair, pair);
    assert_eq!(export.score, 65);
    assert_eq!(export.confidence, 85);
    assert_eq!(export.model_version, 2);
    assert!(!export.is_embargoed);
}

#[test]
fn test_get_score_export_operator_detects_embargo() {
    let (env, client, admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &65,
        &false,
        &false,
        &1_700_000_000,
        &85,
        &1,
        &None,
    );

    // Set embargo
    client.set_score_embargo(&admin, &wallet, &Some(2_000_000));

    let export = client.get_score_export_operator(&wallet, &pair).unwrap();
    assert!(export.is_embargoed);
}

#[test]
fn test_get_score_export_auditor_full_disclosure() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &72,
        &true,
        &false,
        &1_700_000_000,
        &88,
        &3,
        &None,
    );

    let export = client.get_score_export_auditor(&wallet, &pair).unwrap();
    assert_eq!(export.wallet, wallet);
    assert_eq!(export.asset_pair, pair);
    assert_eq!(export.score, 72);
    assert!(export.benford_flag);
    assert!(!export.ml_flag);
    assert_eq!(export.confidence, 88);
    assert_eq!(export.model_version, 3);
}

#[test]
fn test_get_score_export_auditor_bypasses_embargo() {
    let (env, client, admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &60,
        &false,
        &false,
        &1_700_000_000,
        &90,
        &1,
        &None,
    );

    // Set embargo
    client.set_score_embargo(&admin, &wallet, &Some(2_000_000));

    // Auditor export should still work even with embargo
    let export = client.get_score_export_auditor(&wallet, &pair).unwrap();
    assert_eq!(export.score, 60);
    assert!(export.is_embargoed);
}

#[test]
fn test_get_score_export_auditor_not_found() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let result = client.get_score_export_auditor(&wallet, &pair);
    assert!(result.is_err());
}

#[test]
fn test_export_modes_deterministic() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &75,
        &false,
        &false,
        &1_700_000_000,
        &92,
        &1,
        &None,
    );

    // Verify deterministic results across multiple reads
    let pub1 = client.get_score_export_public(&wallet, &pair).unwrap();
    let pub2 = client.get_score_export_public(&wallet, &pair).unwrap();
    assert_eq!(pub1.risk_gate_decision, pub2.risk_gate_decision);

    let op1 = client.get_score_export_operator(&wallet, &pair).unwrap();
    let op2 = client.get_score_export_operator(&wallet, &pair).unwrap();
    assert_eq!(op1.score, op2.score);
    assert_eq!(op1.confidence, op2.confidence);

    let aud1 = client.get_score_export_auditor(&wallet, &pair).unwrap();
    let aud2 = client.get_score_export_auditor(&wallet, &pair).unwrap();
    assert_eq!(aud1.benford_score, aud2.benford_score);
}
