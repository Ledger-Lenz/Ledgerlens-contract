//! Model-version risk-policy compatibility checks (#723).
//!
//! The active risk policy defines a set of approved model versions.  Score
//! submissions that carry an unsupported or retired model version MUST be
//! rejected deterministically.  Submissions that carry an approved version MUST
//! be accepted.  Approved versions must also be discoverable through the read
//! API so that off-chain tooling can pre-validate before submitting.
//!
//! Tests in this file cover:
//!
//! C1. An approved (active) model version is accepted by `submit_score`.
//! C2. A version that was never registered is rejected (empty allowlist
//!     fallback: any version is accepted; non-empty allowlist: unknown version
//!     is rejected).
//! C3. A deprecated (retired) version is rejected even if it was previously
//!     active.
//! C4. `is_model_version_active` returns `true` for an active version and
//!     `false` for an unknown / deprecated version.
//! C5. `get_model_versions` exposes the full registry so integrators can
//!     enumerate approved versions without submitting.
//! C6. Submitting with a deprecated version after a valid submission keeps the
//!     last valid score intact (fail-closed: the new submission is rejected,
//!     existing score is unchanged).
//! C7. Batch submissions respect the same version policy: a single deprecated
//!     entry causes that entry to fail while the rest of the batch may succeed.

#![cfg(test)]

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Bytes, Env, Vec,
};

use crate::{
    types::ModelVersionStatus, LedgerLensScoreContract, LedgerLensScoreContractClient,
    ScoreSubmission,
};

const START_TS: u64 = 1_700_000_000;
const COOLDOWN: u64 = 3_601;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.budget().reset_unlimited();
    env.ledger().with_mut(|l| l.timestamp = START_TS);
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client, admin)
}

fn register_version(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    version: u32,
    activate_delay: u64,
) {
    let metadata = Bytes::from_array(env, &[b'v', b'1']);
    client.register_model_version(&version, &metadata, &activate_delay);
}

fn activate_version(env: &Env, client: &LedgerLensScoreContractClient, version: u32) {
    // Register with zero delay so it is immediately active.
    register_version(env, client, version, 0);
}

fn submit_with_version(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    wallet: &Address,
    score: u32,
    version: u32,
) -> Result<(), ()> {
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
    client
        .try_submit_score(
            &Vec::new(env),
            wallet,
            &symbol_short!("XLM_USDC"),
            &score,
            &false,
            &false,
            &(env.ledger().timestamp()),
            &90,
            &version,
            &None,
        )
        .map(|_| ())
        .map_err(|_| ())
}

// ── C1: Active version is accepted ────────────────────────────────────────────

#[test]
fn compat_active_version_accepted() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    activate_version(&env, &client, 42);

    let result = submit_with_version(&env, &client, &wallet, 75, 42);
    assert!(result.is_ok(), "C1: active version 42 should be accepted");

    let stored = client.get_score(&wallet, &symbol_short!("XLM_USDC"));
    assert_eq!(stored.model_version, 42, "C1: stored model_version should be 42");
    assert_eq!(stored.score, 75, "C1: stored score should be 75");
}

// ── C2: Unregistered version rejected when registry is non-empty ───────────────

#[test]
fn compat_unregistered_version_rejected_when_registry_nonempty() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    // Register one version so the registry is non-empty.
    activate_version(&env, &client, 1);

    // Version 999 was never registered.
    let result = submit_with_version(&env, &client, &wallet, 75, 999);
    assert!(result.is_err(), "C2: unregistered version 999 should be rejected");
}

// ── C2b: Empty registry allows any version ────────────────────────────────────

#[test]
fn compat_empty_registry_allows_any_version() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    // No versions registered yet.
    let result = submit_with_version(&env, &client, &wallet, 50, 999);
    assert!(result.is_ok(), "C2b: empty registry should allow any version");
}

// ── C3: Deprecated (retired) version is rejected ──────────────────────────────

#[test]
fn compat_deprecated_version_rejected() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);

    // Register and immediately activate version 7.
    activate_version(&env, &client, 7);

    // First submission succeeds.
    submit_with_version(&env, &client, &wallet, 60, 7).expect("initial submit should succeed");

    // Deprecate version 7.
    client.deprecate_model_version(&7);

    // Subsequent submission with version 7 must fail.
    let wallet2 = Address::generate(&env);
    let result = submit_with_version(&env, &client, &wallet2, 60, 7);
    assert!(result.is_err(), "C3: deprecated version 7 should be rejected");
}

// ── C4: is_model_version_active read API ──────────────────────────────────────

#[test]
fn compat_is_model_version_active_read_api() {
    let (env, client, _) = setup();

    // Unknown version: false.
    assert!(
        !client.is_model_version_active(&5),
        "C4: unregistered version should not be active"
    );

    // Register and activate version 5.
    activate_version(&env, &client, 5);
    assert!(
        client.is_model_version_active(&5),
        "C4: active version 5 should be active"
    );

    // Deprecate it.
    client.deprecate_model_version(&5);
    assert!(
        !client.is_model_version_active(&5),
        "C4: deprecated version 5 should not be active"
    );
}

// ── C5: get_model_versions exposes the registry ───────────────────────────────

#[test]
fn compat_get_model_versions_exposes_registry() {
    let (env, client, _) = setup();

    // Initially empty.
    let versions_empty = client.get_model_versions();
    assert_eq!(versions_empty.len(), 0, "C5: initial registry should be empty");

    activate_version(&env, &client, 10);
    activate_version(&env, &client, 20);

    let versions = client.get_model_versions();
    assert_eq!(versions.len(), 2, "C5: registry should have 2 versions");

    // Both versions should appear as active.
    let has_10 = versions.iter().any(|v| v.version == 10 && v.status == ModelVersionStatus::Active);
    let has_20 = versions.iter().any(|v| v.version == 20 && v.status == ModelVersionStatus::Active);
    assert!(has_10, "C5: version 10 should appear as active");
    assert!(has_20, "C5: version 20 should appear as active");
}

// ── C6: Deprecated version rejection leaves existing score intact ─────────────

#[test]
fn compat_deprecated_rejection_leaves_existing_score_intact() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);

    activate_version(&env, &client, 3);

    // Submit an initial valid score (score=70).
    submit_with_version(&env, &client, &wallet, 70, 3).expect("initial submit should succeed");
    let score_before = client.get_score(&wallet, &symbol_short!("XLM_USDC")).score;
    assert_eq!(score_before, 70, "C6: initial score should be 70");

    // Deprecate version 3.
    client.deprecate_model_version(&3);

    // Attempt to overwrite with deprecated version (score=10) — must fail.
    let result = submit_with_version(&env, &client, &wallet, 10, 3);
    assert!(result.is_err(), "C6: deprecated submission should be rejected");

    // Existing score must be unchanged.
    let score_after = client.get_score(&wallet, &symbol_short!("XLM_USDC")).score;
    assert_eq!(score_after, 70, "C6: score must remain 70 after rejected submission");
}

// ── C7: Batch respects version policy per-entry ───────────────────────────────

#[test]
fn compat_batch_deprecated_entry_fails_others_succeed() {
    let (env, client, _) = setup();

    activate_version(&env, &client, 1);
    activate_version(&env, &client, 2);
    // Register then deprecate version 9.
    activate_version(&env, &client, 9);
    client.deprecate_model_version(&9);

    let wallet_ok = Address::generate(&env);
    let wallet_bad = Address::generate(&env);
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
    let ts = env.ledger().timestamp();

    let mut submissions = Vec::new(&env);
    submissions.push_back(ScoreSubmission {
        wallet: wallet_ok.clone(),
        asset_pair: symbol_short!("XLM_USDC"),
        score: 55,
        is_flagged: false,
        is_frozen: false,
        timestamp: ts,
        confidence: 90,
        model_version: 1, // active
        attestation: None,
    });
    submissions.push_back(ScoreSubmission {
        wallet: wallet_bad.clone(),
        asset_pair: symbol_short!("XLM_USDC"),
        score: 99,
        is_flagged: false,
        is_frozen: false,
        timestamp: ts,
        confidence: 90,
        model_version: 9, // deprecated
        attestation: None,
    });

    let results = client.submit_scores_batch(&Vec::new(&env), &submissions);
    // The first entry (active version) should succeed.
    assert!(results.get(0).map(|r| r.success).unwrap_or(false),
        "C7: batch entry with active version should succeed");
    // The second entry (deprecated version) should fail.
    assert!(!results.get(1).map(|r| r.success).unwrap_or(true),
        "C7: batch entry with deprecated version should fail");

    // The valid wallet's score must be stored.
    let stored_ok = client.try_get_score(&wallet_ok, &symbol_short!("XLM_USDC"));
    assert!(stored_ok.is_ok(), "C7: valid wallet score should be stored");
}
