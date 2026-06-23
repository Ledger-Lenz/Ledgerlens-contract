//! Tests for the model version registry feature.
//!
//! Covers: register_model_version, deprecate_model_version,
//! is_model_version_active, get_model_versions, and version enforcement inside
//! submit_score / submit_scores_batch.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{
    constants::MAX_MODEL_VERSIONS, BatchResult, Error, LedgerLensScoreContract,
    LedgerLensScoreContractClient, ScoreSubmission,
};

const START_TS: u64 = 1_700_000_000;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client, admin)
}

// ── Empty registry ────────────────────────────────────────────────────────────

#[test]
fn test_empty_registry_allows_any_version() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    // With no versions registered, any model_version value must be accepted.
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &symbol_short!("XLM_USDC"),
        &42,
        &false,
        &false,
        &START_TS,
        &90,
        &999,
        &None,
    );
    assert!(result.is_ok());
    assert_eq!(
        client.get_score(&wallet, &symbol_short!("XLM_USDC")).model_version,
        999
    );
}

// ── Active version ────────────────────────────────────────────────────────────

#[test]
fn test_active_version_accepted() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    client.register_model_version(&Vec::new(&env), &1);
    assert!(client.is_model_version_active(&1));

    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &symbol_short!("XLM_USDC"),
        &42,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );
    assert!(result.is_ok());
    assert_eq!(
        client.get_score(&wallet, &symbol_short!("XLM_USDC")).model_version,
        1
    );
}

// ── Unregistered version ──────────────────────────────────────────────────────

#[test]
fn test_unregistered_version_rejected() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    // Register only version 1 — version 2 has never been registered.
    client.register_model_version(&Vec::new(&env), &1);

    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &symbol_short!("XLM_USDC"),
        &42,
        &false,
        &false,
        &START_TS,
        &90,
        &2,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::ModelVersionNotRegistered)));
}

// ── Deprecated version ────────────────────────────────────────────────────────

#[test]
fn test_deprecated_version_rejected() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    client.register_model_version(&Vec::new(&env), &1);
    client.deprecate_model_version(&Vec::new(&env), &1);
    assert!(!client.is_model_version_active(&1));

    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &symbol_short!("XLM_USDC"),
        &42,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::ModelVersionDeprecated)));
}

// ── Irreversible deprecation ──────────────────────────────────────────────────

#[test]
fn test_deprecation_is_irreversible() {
    let (env, client, _) = setup();
    client.register_model_version(&Vec::new(&env), &1);
    client.deprecate_model_version(&Vec::new(&env), &1);

    // Attempting to deprecate again must fail.
    let result = client.try_deprecate_model_version(&Vec::new(&env), &1);
    assert_eq!(result, Err(Ok(Error::ModelVersionAlreadyDeprecated)));

    // Attempting to re-register a deprecated version must also fail.
    let result2 = client.try_register_model_version(&Vec::new(&env), &1);
    assert_eq!(result2, Err(Ok(Error::ModelVersionAlreadyRegistered)));

    // The version stays inactive after both failed operations.
    assert!(!client.is_model_version_active(&1));
}

// ── Registry cap ──────────────────────────────────────────────────────────────

#[test]
fn test_registry_cap_enforced() {
    let (env, client, _) = setup();
    // Fill the registry up to the cap.
    for i in 0..MAX_MODEL_VERSIONS {
        client.register_model_version(&Vec::new(&env), &i);
    }
    // One more registration must be rejected.
    let result = client.try_register_model_version(&Vec::new(&env), &MAX_MODEL_VERSIONS);
    assert_eq!(result, Err(Ok(Error::ModelVersionRegistryFull)));
}

// ── Batch: per-entry deprecated-version rejection ─────────────────────────────

#[test]
fn test_batch_deprecated_version_entry_rejected() {
    let (env, client, _) = setup();
    client.register_model_version(&Vec::new(&env), &1);
    client.register_model_version(&Vec::new(&env), &2);
    client.deprecate_model_version(&Vec::new(&env), &1);

    let wallet1 = Address::generate(&env);
    let wallet2 = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    // Entry 0: deprecated version — should be per-entry rejected.
    batch.push_back(ScoreSubmission {
        wallet: wallet1.clone(),
        asset_pair: pair.clone(),
        score: 50,
        benford_flag: false,
        ml_flag: false,
        timestamp: START_TS,
        confidence: 90,
        model_version: 1,
    });
    // Entry 1: active version — should be accepted.
    batch.push_back(ScoreSubmission {
        wallet: wallet2.clone(),
        asset_pair: pair.clone(),
        score: 30,
        benford_flag: false,
        ml_flag: false,
        timestamp: START_TS,
        confidence: 90,
        model_version: 2,
    });

    let result: BatchResult = client.submit_scores_batch(&batch);
    assert_eq!(result.accepted_count, 1);
    assert_eq!(result.rejected_count, 1);

    let entry0 = result.results.get(0).unwrap();
    assert!(!entry0.accepted);
    assert_eq!(entry0.rejection_code, Error::ModelVersionDeprecated as u32);

    let entry1 = result.results.get(1).unwrap();
    assert!(entry1.accepted);
    assert_eq!(entry1.rejection_code, 0);

    // Rejected entry must not have stored a score.
    assert_eq!(
        client.try_get_score(&wallet1, &pair),
        Err(Ok(Error::ScoreNotFound))
    );
    // Accepted entry's score is readable.
    assert_eq!(client.get_score(&wallet2, &pair).score, 30);
    assert_eq!(client.get_score(&wallet2, &pair).model_version, 2);
}

// ── Snapshot: full lifecycle ───────────────────────────────────────────────────

#[test]
fn test_model_version_snapshot() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Register versions 1 and 2, then deprecate 1.
    client.register_model_version(&Vec::new(&env), &1);
    client.register_model_version(&Vec::new(&env), &2);
    client.deprecate_model_version(&Vec::new(&env), &1);

    // Submit a score under the still-active version 2.
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &55,
        &false,
        &false,
        &START_TS,
        &90,
        &2,
        &None,
    );

    // Registry must report version 1 as deprecated and version 2 as active.
    let versions = client.get_model_versions();
    assert_eq!(versions.len(), 2);
    assert_eq!(versions.get(0).unwrap(), (1_u32, false));
    assert_eq!(versions.get(1).unwrap(), (2_u32, true));

    assert!(!client.is_model_version_active(&1));
    assert!(client.is_model_version_active(&2));

    // The submitted score is stored with the correct model_version.
    let score = client.get_score(&wallet, &pair);
    assert_eq!(score.score, 55);
    assert_eq!(score.model_version, 2);
}
