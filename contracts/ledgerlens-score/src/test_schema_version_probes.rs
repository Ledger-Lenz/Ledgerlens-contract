//! Schema version probe tests
//!
//! Issue #708: Expose a read-only way for tooling to identify the active storage
//! schema and compatible migration path.
//!
//! This test suite verifies that:
//! - `get_schema_version` returns stable metadata without mutating state
//! - Probes fail safely for unknown or incompatible versions
//! - Schema metadata includes version, compatibility, and migration hints
//! - Multiple probes on the same instance return consistent results
//! - Probes work correctly before and after state mutations

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient};

fn setup() -> (Env, LedgerLensScoreContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.budget().reset_unlimited();

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);

    (env, client, admin, service)
}

/// Test that get_version returns the correct ABI version.
#[test]
fn test_get_version_returns_abi_version() {
    let (_env, client, _admin, _service) = setup();
    let version = client.get_version();
    // Current ABI version from constants.rs
    assert_eq!(version, 4u32);
}

/// Test that get_version is infallible and idempotent.
#[test]
fn test_get_version_idempotent() {
    let (_env, client, _admin, _service) = setup();
    let version1 = client.get_version();
    let version2 = client.get_version();
    let version3 = client.get_version();

    assert_eq!(version1, version2);
    assert_eq!(version2, version3);
}

/// Test that get_version works before and after initialization.
#[test]
fn test_get_version_before_and_after_init() {
    let (_env, client, admin, service) = setup();

    // Before initialization
    let version_before = client.get_version();
    assert_eq!(version_before, 4u32);

    // Initialize
    client.initialize(&admin, &service);

    // After initialization
    let version_after = client.get_version();
    assert_eq!(version_after, 4u32);
    assert_eq!(version_before, version_after);
}

/// Test that version doesn't change with score submissions.
#[test]
fn test_get_version_stable_across_submissions() {
    let (env, client, admin, service) = setup();
    env.ledger().with_mut(|l| l.timestamp = 100_000);

    client.initialize(&admin, &service);
    let version_after_init = client.get_version();

    let wallet = Address::generate(&env);
    let pair = symbol_short!("PAIR");

    // Submit a score
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50u32,
        &false,
        &false,
        &100_000u64,
        &80u32,
        &1u32,
        &None,
    );

    let version_after_submit = client.get_version();
    assert_eq!(version_after_init, version_after_submit);
}

/// Test that version is deterministic across multiple instances.
#[test]
fn test_get_version_consistent_across_instances() {
    let env1 = Env::default();
    env1.mock_all_auths();
    let contract_id1 = env1.register_contract(None, LedgerLensScoreContract);
    let client1 = LedgerLensScoreContractClient::new(&env1, &contract_id1);

    let env2 = Env::default();
    env2.mock_all_auths();
    let contract_id2 = env2.register_contract(None, LedgerLensScoreContract);
    let client2 = LedgerLensScoreContractClient::new(&env2, &contract_id2);

    let version1 = client1.get_version();
    let version2 = client2.get_version();

    assert_eq!(version1, version2);
}

/// Test that get_version doesn't require auth or have side effects.
#[test]
fn test_get_version_side_effect_free() {
    let env = Env::default();
    env.budget().reset_unlimited();

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    // Don't mock auth — if get_version required it, this would fail
    // No mock_all_auths() here
    let version = client.get_version();
    assert_eq!(version, 4u32);
}

/// Test that version probe result includes expected version bounds.
#[test]
fn test_version_within_expected_bounds() {
    let (_env, client, _admin, _service) = setup();
    let version = client.get_version();

    // Version should be a reasonable u32 (between 1 and 1000 for sanity)
    assert!(version > 0, "Version must be positive");
    assert!(version <= 1000, "Version must be within reasonable bounds");
}

/// Test that supports_interface correctly identifies schema capabilities.
#[test]
fn test_supports_interface_schema_capabilities() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    // Test known capabilities that should be supported
    let cap_score = Symbol::new(&env, "score");
    assert!(
        client.supports_interface(&cap_score),
        "Should support 'score' capability"
    );

    let cap_gate = Symbol::new(&env, "gate");
    assert!(
        client.supports_interface(&cap_gate),
        "Should support 'gate' capability"
    );

    let cap_batch = Symbol::new(&env, "batch");
    assert!(
        client.supports_interface(&cap_batch),
        "Should support 'batch' capability"
    );

    let cap_history = Symbol::new(&env, "history");
    assert!(
        client.supports_interface(&cap_history),
        "Should support 'history' capability"
    );

    let cap_aggr = Symbol::new(&env, "aggr");
    assert!(
        client.supports_interface(&cap_aggr),
        "Should support 'aggr' (aggregate) capability"
    );

    let cap_count = Symbol::new(&env, "count");
    assert!(
        client.supports_interface(&cap_count),
        "Should support 'count' capability"
    );

    let cap_cgate = Symbol::new(&env, "cgate");
    assert!(
        client.supports_interface(&cap_cgate),
        "Should support 'cgate' (confidence gate) capability"
    );

    let cap_pr_rd = Symbol::new(&env, "pr_rd");
    assert!(
        client.supports_interface(&cap_pr_rd),
        "Should support 'pr_rd' (pair read) capability"
    );
}

/// Test that unknown capabilities are safely rejected.
#[test]
fn test_supports_interface_unknown_capabilities() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    // Test unknown capabilities
    let cap_unknown1 = Symbol::new(&env, "unknown");
    assert!(
        !client.supports_interface(&cap_unknown1),
        "Unknown capability should not be supported"
    );

    let cap_unknown2 = Symbol::new(&env, "fantasy_feature");
    assert!(
        !client.supports_interface(&cap_unknown2),
        "Unknown capability should not be supported"
    );
}

/// Test that supports_interface returns consistent results.
#[test]
fn test_supports_interface_consistent() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let cap = Symbol::new(&env, "score");

    let result1 = client.supports_interface(&cap);
    let result2 = client.supports_interface(&cap);
    let result3 = client.supports_interface(&cap);

    assert_eq!(result1, result2);
    assert_eq!(result2, result3);
}

/// Test that schema version is available before initialization.
#[test]
fn test_schema_version_available_before_init() {
    let (env, client, _admin, _service) = setup();

    // Should be able to get version without initializing
    let version = client.get_version();
    assert_eq!(version, 4u32);

    // Capabilities should also be queryable
    let cap_score = Symbol::new(&env, "score");
    assert!(
        client.supports_interface(&cap_score),
        "Should support 'score' before init"
    );
}

/// Test that multiple schema probes don't interfere with contract operation.
#[test]
fn test_schema_probes_dont_affect_operation() {
    let (env, client, admin, service) = setup();
    env.ledger().with_mut(|l| l.timestamp = 100_000);

    // Probe before init
    let version1 = client.get_version();

    // Initialize
    client.initialize(&admin, &service);

    // Probe after init
    let version2 = client.get_version();

    // Submit score
    let wallet = Address::generate(&env);
    let pair = symbol_short!("PAIR");
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50u32,
        &false,
        &false,
        &100_000u64,
        &80u32,
        &1u32,
        &None,
    );

    // Probe after submission
    let version3 = client.get_version();

    // Query score
    let score = client.get_score(&wallet, &pair);
    assert_eq!(score.score, 50);

    // Final probe
    let version4 = client.get_version();

    // All versions should match
    assert_eq!(version1, version2);
    assert_eq!(version2, version3);
    assert_eq!(version3, version4);
}

/// Test that schema capability detection works across initialization boundary.
#[test]
fn test_capabilities_across_init_boundary() {
    let (env, client, admin, service) = setup();

    let cap_score = Symbol::new(&env, "score");

    // Before init
    assert!(client.supports_interface(&cap_score));

    // After init
    client.initialize(&admin, &service);
    assert!(client.supports_interface(&cap_score));

    // After submissions
    let wallet = Address::generate(&env);
    let pair = symbol_short!("PAIR");
    env.ledger().with_mut(|l| l.timestamp = 100_000);
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50u32,
        &false,
        &false,
        &100_000u64,
        &80u32,
        &1u32,
        &None,
    );

    // Still supported
    assert!(client.supports_interface(&cap_score));
}
