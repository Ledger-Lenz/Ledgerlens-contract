#![cfg(test)]

//! Tests for issue #694: structured authorization-failure telemetry that
//! does not leak which admin/service signer(s) a caller supplied were valid.
//!
//! Before this change, `require_admin_auth` / `require_service_signers_auth`
//! (and several inlined copies of the same check) returned a *different*
//! `Error` variant depending on whether a caller supplied too few signers
//! (`InsufficientAdminSigners` / `InsufficientSigners`) or supplied a
//! specific address that was not a set member
//! (`AdminSignerNotInSet` / `UnauthorizedSigner`) — and the membership check
//! ran *before* any `require_auth()` call, so an entirely unauthenticated
//! caller could plant one candidate address in a signer vector and use the
//! distinct error to test that single address for admin/service-set
//! membership, at negligible cost and with zero real signatures. Repeating
//! this once per candidate is enough to fingerprint the whole governance
//! set and target a specific key holder (phishing, endpoint compromise,
//! etc.) instead of guessing blindly.
//!
//! The fix collapses every pre-authorization denial to the same
//! `Error::Unauthorized`, paired with a coarse `AuthDenialReason` event that
//! is still operator-actionable ("too few signers" vs "signer set didn't
//! validate") without naming any address. These tests lock that event
//! schema and prove the anti-probing property directly: a caller cannot
//! distinguish "my candidate is a real signer" from "it isn't" via the
//! response.

use soroban_sdk::{
    symbol_short, testutils::Address as _, testutils::Events as _, Address, Env, IntoVal, Symbol,
    Vec,
};

use crate::{AuthDenialReason, Error, LedgerLensScoreContract, LedgerLensScoreContractClient};

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

fn signers_vec(env: &Env, addrs: &[Address]) -> Vec<Address> {
    let mut v = Vec::new(env);
    for a in addrs {
        v.push_back(a.clone());
    }
    v
}

/// Finds the most recent `auth_den` event for `gate` and decodes its
/// `AuthDenialReason` payload.
fn last_denial_reason(env: &Env, contract_id: &Address, gate: Symbol) -> Option<AuthDenialReason> {
    let topic = (symbol_short!("auth_den"), gate);
    env.events().all().iter().rev().find_map(|(addr, topics, data)| {
        if &addr != contract_id || topics != topic.clone().into_val(env) {
            return None;
        }
        Some(data.into_val(env))
    })
}

// ── Event schema ─────────────────────────────────────────────────────────────

#[test]
fn test_admin_gate_insufficient_count_emits_invalid_signer_count_event() {
    let (env, client, _admin, _service) = setup();
    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    client.add_admin_signer(&Vec::new(&env), &s1);
    client.add_admin_signer(&Vec::new(&env), &s2);
    client.set_admin_threshold(&Vec::new(&env), &2);

    let one = signers_vec(&env, &[s1]);
    let result = client.try_pause(&one);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));

    let reason = last_denial_reason(&env, &client.address, symbol_short!("admin"));
    assert_eq!(reason, Some(AuthDenialReason::InvalidSignerCount));
}

#[test]
fn test_admin_gate_bad_signer_emits_signer_validation_failed_event() {
    let (env, client, _admin, _service) = setup();
    let s1 = Address::generate(&env);
    client.add_admin_signer(&Vec::new(&env), &s1);
    client.set_admin_threshold(&Vec::new(&env), &1);

    let outsider = Address::generate(&env);
    let bad = signers_vec(&env, &[outsider]);
    let result = client.try_pause(&bad);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));

    let reason = last_denial_reason(&env, &client.address, symbol_short!("admin"));
    assert_eq!(reason, Some(AuthDenialReason::SignerValidationFailed));
}

#[test]
fn test_service_gate_insufficient_count_emits_invalid_signer_count_event() {
    let (env, client, admin, _service) = setup();
    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    let adm = signers_vec(&env, core::slice::from_ref(&admin));
    client.add_service_signer(&adm, &s1);
    client.add_service_signer(&adm, &s2);
    client.set_service_threshold(&adm, &2);

    let one = signers_vec(&env, &[s1]);
    let result = client.try_veto_parameter_change(&one, &1);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));

    let reason = last_denial_reason(&env, &client.address, symbol_short!("service"));
    assert_eq!(reason, Some(AuthDenialReason::InvalidSignerCount));
}

#[test]
fn test_service_gate_bad_signer_emits_signer_validation_failed_event() {
    let (env, client, admin, _service) = setup();
    let s1 = Address::generate(&env);
    let adm = signers_vec(&env, core::slice::from_ref(&admin));
    client.add_service_signer(&adm, &s1);
    client.set_service_threshold(&adm, &1);

    let outsider = Address::generate(&env);
    let bad = signers_vec(&env, &[outsider]);
    let result = client.try_veto_parameter_change(&bad, &1);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));

    let reason = last_denial_reason(&env, &client.address, symbol_short!("service"));
    assert_eq!(reason, Some(AuthDenialReason::SignerValidationFailed));
}

// ── Anti-probing invariant ──────────────────────────────────────────────────
//
// The core regression: a caller who plants one candidate address alongside
// an arbitrary (definitely-invalid) filler must get the exact same denial
// whether or not the candidate is a genuine set member. If this ever
// regresses back to per-address error codes, these tests fail.

#[test]
fn test_admin_gate_denial_does_not_reveal_which_candidate_was_valid() {
    let (env, client, _admin, _service) = setup();
    let real1 = Address::generate(&env);
    let real2 = Address::generate(&env);
    client.add_admin_signer(&Vec::new(&env), &real1);
    client.add_admin_signer(&Vec::new(&env), &real2);
    client.set_admin_threshold(&Vec::new(&env), &2);

    let outsider = Address::generate(&env);
    let filler = Address::generate(&env); // never added to the admin set

    // Position 0 is a genuine admin signer; the co-signer is bogus.
    let real_candidate = signers_vec(&env, &[real1.clone(), filler.clone()]);
    let result_real = client.try_pause(&real_candidate);
    let reason_real = last_denial_reason(&env, &client.address, symbol_short!("admin"));

    // Position 0 is NOT a genuine admin signer either.
    let fake_candidate = signers_vec(&env, &[outsider.clone(), filler.clone()]);
    let result_fake = client.try_pause(&fake_candidate);
    let reason_fake = last_denial_reason(&env, &client.address, symbol_short!("admin"));

    assert_eq!(result_real, result_fake);
    assert_eq!(result_real, Err(Ok(Error::Unauthorized)));
    assert_eq!(reason_real, reason_fake);
    assert_eq!(reason_real, Some(AuthDenialReason::SignerValidationFailed));
}

#[test]
fn test_service_gate_denial_does_not_reveal_which_candidate_was_valid() {
    let (env, client, admin, _service) = setup();
    let real1 = Address::generate(&env);
    let real2 = Address::generate(&env);
    let adm = signers_vec(&env, core::slice::from_ref(&admin));
    client.add_service_signer(&adm, &real1);
    client.add_service_signer(&adm, &real2);
    client.set_service_threshold(&adm, &2);

    let outsider = Address::generate(&env);
    let filler = Address::generate(&env);

    let real_candidate = signers_vec(&env, &[real1.clone(), filler.clone()]);
    let result_real = client.try_veto_parameter_change(&real_candidate, &1);

    let fake_candidate = signers_vec(&env, &[outsider.clone(), filler.clone()]);
    let result_fake = client.try_veto_parameter_change(&fake_candidate, &1);

    assert_eq!(result_real, result_fake);
    assert_eq!(result_real, Err(Ok(Error::Unauthorized)));
}

// ── Bounded resource use ─────────────────────────────────────────────────────

#[test]
fn test_admin_gate_oversized_signer_vector_rejected_without_membership_scan() {
    let (env, client, _admin, _service) = setup();
    let s1 = Address::generate(&env);
    client.add_admin_signer(&Vec::new(&env), &s1);
    client.set_admin_threshold(&Vec::new(&env), &1);

    // Far larger than MAX_ADMIN_SIGNERS (5). If the implementation validated
    // membership entry-by-entry before checking the length bound, this would
    // scale with the caller-supplied vector; instead it must be rejected by
    // the length check alone, so the event category is InvalidSignerCount
    // (not SignerValidationFailed) even though every entry is also bogus.
    let mut oversized = Vec::new(&env);
    for _ in 0..1000u32 {
        oversized.push_back(s1.clone());
    }
    let result = client.try_pause(&oversized);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));

    let reason = last_denial_reason(&env, &client.address, symbol_short!("admin"));
    assert_eq!(reason, Some(AuthDenialReason::InvalidSignerCount));
}

#[test]
fn test_service_gate_oversized_signer_vector_rejected_without_membership_scan() {
    let (env, client, admin, _service) = setup();
    let s1 = Address::generate(&env);
    let adm = signers_vec(&env, core::slice::from_ref(&admin));
    client.add_service_signer(&adm, &s1);
    client.set_service_threshold(&adm, &1);

    // Far larger than MAX_SERVICE_SIGNERS (10).
    let mut oversized = Vec::new(&env);
    for _ in 0..1000u32 {
        oversized.push_back(s1.clone());
    }
    let result = client.try_veto_parameter_change(&oversized, &1);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));

    let reason = last_denial_reason(&env, &client.address, symbol_short!("service"));
    assert_eq!(reason, Some(AuthDenialReason::InvalidSignerCount));
}

// ── Fail-closed / legitimate quorum unaffected ──────────────────────────────

#[test]
fn test_admin_gate_valid_quorum_still_succeeds_and_emits_no_denial_event() {
    let (env, client, _admin, _service) = setup();
    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    client.add_admin_signer(&Vec::new(&env), &s1);
    client.add_admin_signer(&Vec::new(&env), &s2);
    client.set_admin_threshold(&Vec::new(&env), &2);

    let quorum = signers_vec(&env, &[s1, s2]);
    client.pause(&quorum);
    assert!(client.is_paused());

    let reason = last_denial_reason(&env, &client.address, symbol_short!("admin"));
    assert_eq!(reason, None);
}

#[test]
fn test_service_gate_valid_quorum_still_succeeds() {
    let (env, client, admin, _service) = setup();
    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    let adm = signers_vec(&env, core::slice::from_ref(&admin));
    client.add_service_signer(&adm, &s1);
    client.add_service_signer(&adm, &s2);
    client.set_service_threshold(&adm, &2);

    client.propose_parameter_change(
        &adm,
        &crate::parameter_governance::param_key_cooldown(),
        &{
            let mut b = soroban_sdk::Bytes::new(&env);
            b.extend_from_array(&600u64.to_be_bytes());
            b
        },
    );

    let quorum = signers_vec(&env, &[s1, s2]);
    client.veto_parameter_change(&quorum, &1);
    let reason = last_denial_reason(&env, &client.address, symbol_short!("service"));
    assert_eq!(reason, None);
}
