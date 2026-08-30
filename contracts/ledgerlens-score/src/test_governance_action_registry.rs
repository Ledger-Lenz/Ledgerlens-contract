//! Tests for the governance action registry (issue #701).
//!
//! Coverage areas
//! ──────────────
//! 1. **Registry coverage** — every constant that exists in
//!    `governance_actions` is present in `all_actions()`, and no discriminant
//!    is reused.
//! 2. **Name round-trips** — `action_name(d)` returns a non-empty string for
//!    every known discriminant, and `is_known_action` matches correctly.
//! 3. **Audit-chain stability** — calling a privileged action advances the
//!    on-chain audit root, and the `gov_action` event carries the expected
//!    `action_id` and `action_name`.
//! 4. **Reserved / unknown sentinel** — discriminant `0x00` and values outside
//!    the registry produce safe fallback values.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, Symbol, Vec,
};

use crate::{
    governance_actions::{
        self, GOV_ACTION_ADD_SERVICE_SIGNER, GOV_ACTION_NAME_ADD_SERVICE_SIGNER,
        GOV_ACTION_NAME_PAUSE, GOV_ACTION_NAME_PROPOSE_UPGRADE, GOV_ACTION_NAME_SET_ADMIN_THRESHOLD,
        GOV_ACTION_NAME_SET_SERVICE, GOV_ACTION_NAME_UNPAUSE, GOV_ACTION_PAUSE,
        GOV_ACTION_PROPOSE_UPGRADE, GOV_ACTION_RESERVED, GOV_ACTION_SET_ADMIN_THRESHOLD,
        GOV_ACTION_SET_SERVICE, GOV_ACTION_UNPAUSE,
    },
    LedgerLensScoreContract, LedgerLensScoreContractClient,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 100_000);
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client, admin, service)
}

// ── 1. Registry coverage ──────────────────────────────────────────────────────

/// Every entry returned by `all_actions()` must map back to the correct name
/// through `action_name()`.
#[test]
fn test_all_actions_names_are_consistent() {
    for (discriminant, expected_name) in governance_actions::all_actions() {
        let got = governance_actions::action_name(*discriminant);
        assert_eq!(
            got, *expected_name,
            "action_name({:#04x}) returned {:?} but all_actions() says {:?}",
            discriminant, got, expected_name
        );
    }
}

/// All discriminants in `all_actions()` must be unique — no two entries may
/// share a discriminant value.
#[test]
fn test_discriminants_are_unique() {
    let actions = governance_actions::all_actions();
    for i in 0..actions.len() {
        for j in (i + 1)..actions.len() {
            assert_ne!(
                actions[i].0, actions[j].0,
                "discriminant {:#04x} is assigned to both {:?} and {:?}",
                actions[i].0, actions[i].1, actions[j].1
            );
        }
    }
}

/// Every name string must be non-empty and fit within Soroban's 9-character
/// symbol limit so callers can safely use it with `Symbol::new`.
#[test]
fn test_all_action_names_fit_symbol_limit() {
    for (discriminant, name) in governance_actions::all_actions() {
        assert!(
            !name.is_empty(),
            "action name for discriminant {:#04x} is empty",
            discriminant
        );
        assert!(
            name.len() <= 9,
            "action name {:?} for discriminant {:#04x} exceeds 9-char Soroban symbol limit",
            name, discriminant
        );
    }
}

/// The registry must cover all six expected actions.
#[test]
fn test_registry_has_expected_entry_count() {
    assert_eq!(
        governance_actions::all_actions().len(),
        6,
        "registry should contain exactly 6 defined actions"
    );
}

// ── 2. Name round-trips ───────────────────────────────────────────────────────

#[test]
fn test_action_name_set_service() {
    assert_eq!(governance_actions::action_name(GOV_ACTION_SET_SERVICE), GOV_ACTION_NAME_SET_SERVICE);
}

#[test]
fn test_action_name_add_service_signer() {
    assert_eq!(
        governance_actions::action_name(GOV_ACTION_ADD_SERVICE_SIGNER),
        GOV_ACTION_NAME_ADD_SERVICE_SIGNER
    );
}

#[test]
fn test_action_name_set_admin_threshold() {
    assert_eq!(
        governance_actions::action_name(GOV_ACTION_SET_ADMIN_THRESHOLD),
        GOV_ACTION_NAME_SET_ADMIN_THRESHOLD
    );
}

#[test]
fn test_action_name_pause() {
    assert_eq!(governance_actions::action_name(GOV_ACTION_PAUSE), GOV_ACTION_NAME_PAUSE);
}

#[test]
fn test_action_name_unpause() {
    assert_eq!(governance_actions::action_name(GOV_ACTION_UNPAUSE), GOV_ACTION_NAME_UNPAUSE);
}

#[test]
fn test_action_name_propose_upgrade() {
    assert_eq!(
        governance_actions::action_name(GOV_ACTION_PROPOSE_UPGRADE),
        GOV_ACTION_NAME_PROPOSE_UPGRADE
    );
}

/// Unknown discriminants return the "unknown" fallback — they must not panic.
#[test]
fn test_action_name_unknown_discriminant_returns_fallback() {
    assert_eq!(governance_actions::action_name(0xFF), "unknown");
    assert_eq!(governance_actions::action_name(0x07), "unknown");
}

/// `is_known_action` returns `true` for all registry entries and `false` for
/// the reserved value and out-of-range values.
#[test]
fn test_is_known_action_coverage() {
    for (discriminant, _) in governance_actions::all_actions() {
        assert!(
            governance_actions::is_known_action(*discriminant),
            "is_known_action({:#04x}) should be true",
            discriminant
        );
    }
    assert!(
        !governance_actions::is_known_action(GOV_ACTION_RESERVED),
        "reserved discriminant 0x00 must not be considered known"
    );
    assert!(!governance_actions::is_known_action(0xFF));
    assert!(!governance_actions::is_known_action(0x07));
}

// ── 3. Audit-chain stability ──────────────────────────────────────────────────

/// Calling `add_service_signer` advances the governance chain head and emits
/// a `gov_action` event with the correct `action_id` (0x02) and name.
#[test]
fn test_add_service_signer_emits_gov_action_event() {
    let (env, client, _admin, _service) = setup();

    let head_before = client.get_governance_chain_head();

    let new_signer = Address::generate(&env);
    client.add_service_signer(&Vec::new(&env), &new_signer);

    // Chain head must have advanced.
    let head_after = client.get_governance_chain_head();
    assert_ne!(head_before, head_after, "chain head must change after add_service_signer");

    // The most recent event must be gov_action with action_id = GOV_ACTION_ADD_SERVICE_SIGNER.
    let events = env.events().all();
    let found = events.iter().any(|(topics, data): (soroban_sdk::Vec<soroban_sdk::Val>, soroban_sdk::Val)| {
        if topics.len() < 2 {
            return false;
        }
        let topic0: Result<Symbol, _> = soroban_sdk::Symbol::try_from_val(&env, &topics.get(0).unwrap());
        let Ok(t0) = topic0 else { return false };
        if t0 != symbol_short!("gov_actn") {
            // try the full name form
            if t0 != Symbol::new(&env, "gov_action") {
                return false;
            }
        }
        // Decode data tuple: (action_id: u32, action_name: Symbol, new_head: BytesN<32>)
        let decoded: Result<(u32, Symbol, soroban_sdk::BytesN<32>), _> =
            <(u32, Symbol, soroban_sdk::BytesN<32>)>::try_from_val(&env, &data);
        if let Ok((action_id, action_name, _new_head)) = decoded {
            return action_id == GOV_ACTION_ADD_SERVICE_SIGNER as u32
                && action_name == Symbol::new(&env, GOV_ACTION_NAME_ADD_SERVICE_SIGNER);
        }
        false
    });
    assert!(found, "gov_action event for add_service_signer not found in event log");
}

/// Calling `pause` advances the chain head.
#[test]
fn test_pause_advances_chain_head() {
    let (env, client, _admin, _service) = setup();
    let head_before = client.get_governance_chain_head();
    client.pause(&Vec::new(&env));
    let head_after = client.get_governance_chain_head();
    assert_ne!(head_before, head_after, "chain head must change after pause");
}

/// Calling `unpause` after `pause` advances the chain head again.
#[test]
fn test_unpause_advances_chain_head() {
    let (env, client, _admin, _service) = setup();
    client.pause(&Vec::new(&env));
    let head_after_pause = client.get_governance_chain_head();
    client.unpause(&Vec::new(&env));
    let head_after_unpause = client.get_governance_chain_head();
    assert_ne!(head_after_pause, head_after_unpause, "chain head must change after unpause");
}

/// Two distinct admin actions must produce distinct chain heads (monotonicity).
#[test]
fn test_chain_is_monotonic_across_different_actions() {
    let (env, client, _admin, _service) = setup();

    let signer_a = Address::generate(&env);
    client.add_service_signer(&Vec::new(&env), &signer_a);
    let h1 = client.get_governance_chain_head();

    let signer_b = Address::generate(&env);
    client.add_service_signer(&Vec::new(&env), &signer_b);
    let h2 = client.get_governance_chain_head();

    assert_ne!(h1, h2, "each admin action must produce a new chain head");
}

/// `set_service` advances the chain head (deprecated path still audited).
#[test]
fn test_set_service_advances_chain_head() {
    let (env, client, _admin, _service) = setup();
    let head_before = client.get_governance_chain_head();
    let new_service = Address::generate(&env);
    #[allow(deprecated)]
    client.set_service(&new_service);
    let head_after = client.get_governance_chain_head();
    assert_ne!(head_before, head_after, "chain head must change after set_service");
}

// ── 4. Reserved / unknown sentinel ───────────────────────────────────────────

/// The `0x00` reserved discriminant must never be treated as a known action.
#[test]
fn test_reserved_discriminant_is_not_known() {
    assert_eq!(
        governance_actions::action_name(GOV_ACTION_RESERVED),
        "unknown",
        "reserved discriminant 0x00 must map to 'unknown', not a real action name"
    );
    assert!(
        !governance_actions::is_known_action(GOV_ACTION_RESERVED),
        "is_known_action must return false for the reserved discriminant"
    );
}

/// The genesis chain head (before any admin action) is the all-zeros sentinel.
#[test]
fn test_genesis_chain_head_is_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    let head = client.get_governance_chain_head();
    assert_eq!(
        head,
        soroban_sdk::BytesN::from_array(&env, &[0u8; 32]),
        "governance chain head must be all-zeros before any admin action"
    );
}
