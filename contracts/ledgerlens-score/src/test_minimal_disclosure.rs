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
// Issue #729 – Minimal disclosure query helpers
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_is_score_risky_below_threshold() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // Submit score below default threshold (75)
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

    assert!(!client.is_score_risky(&wallet, &pair));
}

#[test]
fn test_is_score_risky_above_threshold() {
    let (env, client, admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // Set threshold and submit score above it
    client.set_risk_threshold(&Vec::new(&env), &70);
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &75,
        &false,
        &false,
        &1_700_000_000,
        &90,
        &1,
        &None,
    );

    assert!(client.is_score_risky(&wallet, &pair));
}

#[test]
fn test_is_score_risky_no_score_found() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // No score submitted — should be safe (fail-closed)
    assert!(!client.is_score_risky(&wallet, &pair));
}

#[test]
fn test_is_score_risky_embargoed_wallet() {
    let (env, client, admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // Submit a safe score
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &30,
        &false,
        &false,
        &1_700_000_000,
        &90,
        &1,
        &None,
    );
    assert!(!client.is_score_risky(&wallet, &pair));

    // Embargo the wallet — should now be risky (fail-closed)
    client.set_score_embargo(&wallet, &Some(2_000_000));
    assert!(client.is_score_risky(&wallet, &pair));
}

#[test]
fn test_is_score_risky_with_confidence_no_score() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let (breached, confidence) = client.is_score_risky_with_confidence(&wallet, &pair);
    assert!(!breached);
    assert_eq!(confidence, 0);
}

#[test]
fn test_is_score_risky_with_confidence_below_threshold() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &40,
        &false,
        &false,
        &1_700_000_000,
        &85,
        &1,
        &None,
    );

    let (breached, confidence) = client.is_score_risky_with_confidence(&wallet, &pair);
    assert!(!breached);
    assert_eq!(confidence, 85);
}

#[test]
fn test_is_score_risky_with_confidence_above_threshold() {
    let (env, client, admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    client.set_risk_threshold(&Vec::new(&env), &65);
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &78,
        &false,
        &false,
        &1_700_000_000,
        &92,
        &1,
        &None,
    );

    let (breached, confidence) = client.is_score_risky_with_confidence(&wallet, &pair);
    assert!(breached);
    assert_eq!(confidence, 92);
}

#[test]
fn test_is_score_risky_with_confidence_embargoed() {
    let (env, client, admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &20,
        &false,
        &false,
        &1_700_000_000,
        &90,
        &1,
        &None,
    );

    // Embargo
    client.set_score_embargo(&wallet, &Some(2_000_000));

    let (breached, confidence) = client.is_score_risky_with_confidence(&wallet, &pair);
    assert!(breached);
    assert_eq!(confidence, 100); // Embargo = high-confidence breach
}

#[test]
fn test_is_wallet_safe_passes_gate() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

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

    assert!(client.is_wallet_safe(&wallet, &pair));
}

#[test]
fn test_is_wallet_safe_fails_gate() {
    let (env, client, admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    client.set_risk_threshold(&Vec::new(&env), &60);
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &80,
        &false,
        &false,
        &1_700_000_000,
        &90,
        &1,
        &None,
    );

    assert!(!client.is_wallet_safe(&wallet, &pair));
}

#[test]
fn test_is_wallet_safe_no_score() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    assert!(client.is_wallet_safe(&wallet, &pair));
}

#[test]
fn test_is_aggregate_risky_no_scores() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    assert!(!client.is_aggregate_risky(&wallet));
}

#[test]
fn test_is_aggregate_risky_all_safe() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair1 = symbol_short!("XLM_USDC");
    let pair2 = symbol_short!("BTC_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair1,
        &40,
        &false,
        &false,
        &1_700_000_000,
        &90,
        &1,
        &None,
    );
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair2,
        &50,
        &false,
        &false,
        &1_700_000_000,
        &90,
        &1,
        &None,
    );

    assert!(!client.is_aggregate_risky(&wallet));
}

#[test]
fn test_is_aggregate_risky_one_risky() {
    let (env, client, admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair1 = symbol_short!("XLM_USDC");
    let pair2 = symbol_short!("BTC_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    client.set_risk_threshold(&Vec::new(&env), &60);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair1,
        &50,
        &false,
        &false,
        &1_700_000_000,
        &90,
        &1,
        &None,
    );
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair2,
        &85,
        &false,
        &false,
        &1_700_000_000,
        &90,
        &1,
        &None,
    );

    // One risky pair means aggregate is risky
    assert!(client.is_aggregate_risky(&wallet));
}

#[test]
fn test_is_aggregate_risky_embargoed() {
    let (env, client, admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair1 = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair1,
        &30,
        &false,
        &false,
        &1_700_000_000,
        &90,
        &1,
        &None,
    );

    // All safe initially
    assert!(!client.is_aggregate_risky(&wallet));

    // Embargo the wallet
    client.set_score_embargo(&wallet, &Some(2_000_000));

    // Now aggregate should be risky (fail-closed)
    assert!(client.is_aggregate_risky(&wallet));
}

#[test]
fn test_minimal_disclosure_no_field_leakage() {
    let (env, client, admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    client.set_risk_threshold(&Vec::new(&env), &70);
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &75,
        &true,
        &true,
        &1_700_000_000,
        &88,
        &2,
        &None,
    );

    // Minimal disclosure queries should NOT expose:
    // - benford_flag, ml_flag
    // - benford_score, ml_score, network_score
    // - Exact timestamp
    // - Exact confidence (except with_confidence variant)

    // is_score_risky only reveals: RISKY or SAFE
    assert!(client.is_score_risky(&wallet, &pair));

    // is_wallet_safe only reveals: SAFE or NOT_SAFE
    assert!(!client.is_wallet_safe(&wallet, &pair));

    // is_score_risky_with_confidence only reveals: RISKY/SAFE + confidence level
    let (breached, confidence) = client.is_score_risky_with_confidence(&wallet, &pair);
    assert!(breached);
    assert_eq!(confidence, 88); // Only this is revealed
}

#[test]
fn test_minimal_disclosure_deterministic_across_calls() {
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
        &90,
        &1,
        &None,
    );

    // Verify deterministic results
    let result1 = client.is_score_risky(&wallet, &pair);
    let result2 = client.is_score_risky(&wallet, &pair);
    assert_eq!(result1, result2);

    let (breach1, conf1) = client.is_score_risky_with_confidence(&wallet, &pair);
    let (breach2, conf2) = client.is_score_risky_with_confidence(&wallet, &pair);
    assert_eq!(breach1, breach2);
    assert_eq!(conf1, conf2);
}
