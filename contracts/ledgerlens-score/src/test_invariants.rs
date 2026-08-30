//! Tests for Issue #710: executable storage invariant checks.
//!
//! Each test has a "failing fixture" that deliberately corrupts storage to
//! produce a known-bad state, then asserts the invariant helper catches it.
//! The passing variant confirms the invariant is satisfied after a clean
//! operation through the public API.

use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

use crate::{
    invariants, storage,
    types::{DataKey, RiskScore},
    LedgerLensScoreContract,
};

fn make_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn sample_score(v: u32) -> RiskScore {
    RiskScore {
        score: v,
        benford_flag: false,
        ml_flag: false,
        timestamp: 100,
        confidence: 90,
        model_version: 1,
        benford_score: 0,
        ml_score: 0,
        network_score: 0,
        commitment: None,
    }
}

fn register(env: &Env) -> Address {
    let id = env.register_contract(None, LedgerLensScoreContract);
    let admin = Address::generate(env);
    let service = Address::generate(env);
    let client = crate::LedgerLensScoreContractClient::new(env, &id);
    client.initialize(&admin, &service);
    id
}

// ── Invariant #1: global_min_confidence ∈ [0,100] ────────────────────────────

#[test]
fn inv1_passes_after_valid_confidence_set() {
    let env = make_env();
    let id = register(&env);
    env.as_contract(&id, || {
        storage::set_global_min_confidence(&env, 50);
        assert!(invariants::score_index_is_consistent(&env));
        // full invariant_check must not panic
        invariants::invariant_check(&env);
    });
}

#[test]
#[should_panic(expected = "INVARIANT #1 VIOLATED")]
fn inv1_fails_when_confidence_exceeds_100() {
    let env = make_env();
    let id = register(&env);
    env.as_contract(&id, || {
        // Direct storage write bypasses the API validation layer — simulates
        // a migration that forgot to clamp the value.
        storage::set_global_min_confidence(&env, 101);
        invariants::invariant_check(&env);
    });
}

// ── Invariant #4: decay denominator ≠ 0 ──────────────────────────────────────

#[test]
fn inv4_passes_with_default_decay_rate() {
    let env = make_env();
    let id = register(&env);
    env.as_contract(&id, || {
        assert!(invariants::decay_rate_is_valid(&env));
    });
}

#[test]
#[should_panic(expected = "INVARIANT #4 VIOLATED")]
fn inv4_fails_when_denominator_is_zero() {
    let env = make_env();
    let id = register(&env);
    env.as_contract(&id, || {
        storage::set_decay_rate(&env, 0, 0);
        invariants::invariant_check(&env);
    });
}

// ── Invariant #7: score index entries have live Score keys ───────────────────

#[test]
fn inv7_passes_after_normal_submit() {
    let env = make_env();
    let id = register(&env);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    env.as_contract(&id, || {
        storage::set_score(&env, &wallet, &pair, &sample_score(42));
        storage::track_score_entry(&env, &wallet, &pair);
        assert!(invariants::score_index_is_consistent(&env));
    });
}

#[test]
#[should_panic(expected = "INVARIANT #7 VIOLATED")]
fn inv7_fails_when_index_entry_has_no_live_score() {
    let env = make_env();
    let id = register(&env);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    env.as_contract(&id, || {
        // Write a score then remove it directly — simulates a partial rollback
        // that erased the Score key but left the index entry intact.
        storage::set_score(&env, &wallet, &pair, &sample_score(42));
        storage::track_score_entry(&env, &wallet, &pair);
        env.storage().persistent().remove(&DataKey::Score(wallet.clone(), pair.clone()));
        // Index still contains the entry; invariant must fire.
        invariants::invariant_check(&env);
    });
}

// ── Invariant #10: history ring ≤ HistoryMaxDepth ───────────────────────────

#[test]
fn inv10_passes_after_bounded_push() {
    let env = make_env();
    let id = register(&env);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    env.as_contract(&id, || {
        storage::set_history_max_depth(&env, 3);
        for i in 0..5u32 {
            storage::push_score_history(&env, &wallet, &pair, &sample_score(i));
        }
        // push_score_history trims on write; ring must be exactly 3.
        assert!(invariants::history_rings_are_bounded(&env));
    });
}

#[test]
#[should_panic(expected = "INVARIANT #10 VIOLATED")]
fn inv10_fails_when_history_exceeds_max_depth() {
    let env = make_env();
    let id = register(&env);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    env.as_contract(&id, || {
        // Write two entries at depth=2, track the entry…
        storage::set_history_max_depth(&env, 2);
        storage::push_score_history(&env, &wallet, &pair, &sample_score(1));
        storage::push_score_history(&env, &wallet, &pair, &sample_score(2));
        storage::set_score(&env, &wallet, &pair, &sample_score(2));
        storage::track_score_entry(&env, &wallet, &pair);
        // …then lower depth to 1 WITHOUT writing again: ring is now oversized.
        storage::set_history_max_depth(&env, 1);
        invariants::invariant_check(&env);
    });
}

// ── Invariant #11: ActiveEmbargoCount matches live index ─────────────────────

#[test]
fn inv11_passes_after_set_embargo() {
    let env = make_env();
    let id = register(&env);
    let wallet = Address::generate(&env);
    env.as_contract(&id, || {
        use crate::types::EmbargoExpiry;
        storage::set_embargo(&env, &wallet, &EmbargoExpiry::Indefinite);
        storage::add_to_embargoed_index(&env, &wallet);
        storage::increment_active_embargo_count(&env);
        assert!(invariants::embargo_count_is_consistent(&env));
    });
}

#[test]
#[should_panic(expected = "INVARIANT #11 VIOLATED")]
fn inv11_fails_when_count_exceeds_live_embargoes() {
    let env = make_env();
    let id = register(&env);
    let wallet = Address::generate(&env);
    env.as_contract(&id, || {
        // Increment the counter without actually writing an embargo — simulates
        // a partial migration that updated the count but not the embargo key.
        storage::add_to_embargoed_index(&env, &wallet);
        storage::increment_active_embargo_count(&env);
        // No call to set_embargo, so peek_is_embargoed returns false.
        invariants::invariant_check(&env);
    });
}

// ── Invariant #14: PendingAdmin ≠ current Admin ──────────────────────────────

#[test]
fn inv14_passes_when_pending_is_different_address() {
    let env = make_env();
    let id = register(&env);
    env.as_contract(&id, || {
        let new_admin = Address::generate(&env);
        storage::set_pending_admin(&env, &new_admin);
        invariants::invariant_check(&env);
    });
}

#[test]
#[should_panic(expected = "INVARIANT #14 VIOLATED")]
fn inv14_fails_when_pending_admin_equals_current_admin() {
    let env = make_env();
    let id = register(&env);
    env.as_contract(&id, || {
        let current = storage::get_admin(&env);
        storage::set_pending_admin(&env, &current);
        invariants::invariant_check(&env);
    });
}

// ── Invariant #17: decay λ ∈ [0, 1] ─────────────────────────────────────────

#[test]
fn inv17_passes_with_valid_lambda() {
    let env = make_env();
    let id = register(&env);
    env.as_contract(&id, || {
        storage::set_decay_rate(&env, 999, 1000);
        assert!(invariants::decay_rate_is_valid(&env));
    });
}

#[test]
#[should_panic(expected = "INVARIANT #17 VIOLATED")]
fn inv17_fails_when_lambda_exceeds_one() {
    let env = make_env();
    let id = register(&env);
    env.as_contract(&id, || {
        // num > den means λ > 1 — scores would grow instead of decay.
        storage::set_decay_rate(&env, 2, 1);
        invariants::invariant_check(&env);
    });
}

// ── Invariant #18: HistoryMaxDepth ∈ [1, MAX_HISTORY_DEPTH] ─────────────────

#[test]
#[should_panic(expected = "INVARIANT #18 VIOLATED")]
fn inv18_fails_when_depth_is_zero() {
    let env = make_env();
    let id = register(&env);
    env.as_contract(&id, || {
        storage::set_history_max_depth(&env, 0);
        invariants::invariant_check(&env);
    });
}

#[test]
#[should_panic(expected = "INVARIANT #18 VIOLATED")]
fn inv18_fails_when_depth_exceeds_max() {
    let env = make_env();
    let id = register(&env);
    env.as_contract(&id, || {
        storage::set_history_max_depth(&env, crate::constants::MAX_HISTORY_DEPTH + 1);
        invariants::invariant_check(&env);
    });
}
