#![cfg(test)]

//! Tests for the admin M-of-N multi-signature governance feature.
//!
//! In legacy mode (AdminSet empty, threshold == 0) all admin functions fall
//! back to the single stored admin key.  Once at least one signer has been
//! added and a threshold set, every admin call must supply at least M valid
//! admin-set members.

use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient};

// ── Test helpers ──────────────────────────────────────────────────────────────

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

// ── 1. After initialize, admin set is empty and threshold is 0 (legacy mode) ─

#[test]
fn test_admin_multisig_init_state() {
    let (env, client, _admin, _service) = setup();
    assert_eq!(client.get_admin_signers().len(), 0);
    assert_eq!(client.get_admin_threshold(), 0);
    // In legacy mode an admin call with empty signers vec succeeds
    // (mock_all_auths covers the single-admin auth).
    client.set_risk_threshold(&Vec::new(&env), &80);
    assert_eq!(client.get_risk_threshold(), 80);
}

// ── 2. Legacy admin can add first signer ─────────────────────────────────────

#[test]
fn test_add_first_admin_signer_in_legacy_mode() {
    let (env, client, _admin, _service) = setup();
    let signer = Address::generate(&env);
    // Empty admin_signers triggers legacy path (single admin key required).
    client.add_admin_signer(&Vec::new(&env), &signer);
    assert_eq!(client.get_admin_signers().len(), 1);
    assert!(client.get_admin_signers().contains(&signer));
}

// ── 3. Adding beyond MAX_ADMIN_SIGNERS (5) returns AdminSetFull ───────────────

#[test]
fn test_add_admin_signer_full() {
    let (env, client, _admin, _service) = setup();
    for _ in 0..5 {
        let s = Address::generate(&env);
        client.add_admin_signer(&Vec::new(&env), &s);
    }
    assert_eq!(client.get_admin_signers().len(), 5);
    let extra = Address::generate(&env);
    let result = client.try_add_admin_signer(&Vec::new(&env), &extra);
    assert_eq!(result, Err(Ok(Error::AdminSetFull)));
}

// ── 4. Adding a duplicate signer returns SignerAlreadyInSet ──────────────────

#[test]
fn test_add_admin_signer_duplicate() {
    let (env, client, _admin, _service) = setup();
    let signer = Address::generate(&env);
    client.add_admin_signer(&Vec::new(&env), &signer);
    let result = client.try_add_admin_signer(&Vec::new(&env), &signer);
    assert_eq!(result, Err(Ok(Error::SignerAlreadyInSet)));
}

// ── 5. In multisig mode, providing M-of-N signers succeeds ───────────────────

#[test]
fn test_require_admin_auth_multisig_success() {
    let (env, client, _admin, _service) = setup();
    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    let s3 = Address::generate(&env);
    client.add_admin_signer(&Vec::new(&env), &s1);
    client.add_admin_signer(&Vec::new(&env), &s2);
    client.add_admin_signer(&Vec::new(&env), &s3);
    client.set_admin_threshold(&Vec::new(&env), &2);

    // Supplying exactly M=2 valid signers should succeed.
    let signers = signers_vec(&env, &[s1.clone(), s2.clone()]);
    client.set_risk_threshold(&signers, &60);
    assert_eq!(client.get_risk_threshold(), 60);
}

// ── 6. Providing fewer than M signers returns InsufficientAdminSigners ────────

#[test]
fn test_require_admin_auth_insufficient_signers() {
    let (env, client, _admin, _service) = setup();
    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    client.add_admin_signer(&Vec::new(&env), &s1);
    client.add_admin_signer(&Vec::new(&env), &s2);
    client.set_admin_threshold(&Vec::new(&env), &2);

    // Supplying only 1 signer when threshold is 2.
    let one_signer = signers_vec(&env, core::slice::from_ref(&s1));
    let result = client.try_set_risk_threshold(&one_signer, &50);
    assert_eq!(result, Err(Ok(Error::InsufficientAdminSigners)));
}

// ── 7. Zero signers in multisig mode returns InsufficientAdminSigners ─────────

#[test]
fn test_require_admin_auth_zero_signers_in_multisig_mode() {
    let (env, client, _admin, _service) = setup();
    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    client.add_admin_signer(&Vec::new(&env), &s1);
    client.add_admin_signer(&Vec::new(&env), &s2);
    client.set_admin_threshold(&Vec::new(&env), &1);

    // Zero signers when threshold > 0.
    let result = client.try_pause(&Vec::new(&env));
    assert_eq!(result, Err(Ok(Error::InsufficientAdminSigners)));
}

// ── 8. Signer not in the admin set returns AdminSignerNotInSet ────────────────

#[test]
fn test_require_admin_auth_signer_not_in_set() {
    let (env, client, _admin, _service) = setup();
    let s1 = Address::generate(&env);
    client.add_admin_signer(&Vec::new(&env), &s1);
    client.set_admin_threshold(&Vec::new(&env), &1);

    let outsider = Address::generate(&env);
    let bad_signers = signers_vec(&env, &[outsider]);
    let result = client.try_pause(&bad_signers);
    assert_eq!(result, Err(Ok(Error::AdminSignerNotInSet)));
}

// ── 9. Remove signer auto-adjusts threshold ───────────────────────────────────

#[test]
fn test_remove_admin_signer_auto_adjusts_threshold() {
    let (env, client, _admin, _service) = setup();
    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    client.add_admin_signer(&Vec::new(&env), &s1);
    client.add_admin_signer(&Vec::new(&env), &s2);
    client.set_admin_threshold(&Vec::new(&env), &2);

    // Removing s2 leaves set size 1 but threshold was 2 — should auto-reduce.
    let two_signers = signers_vec(&env, &[s1.clone(), s2.clone()]);
    client.remove_admin_signer(&two_signers, &s2);
    assert_eq!(client.get_admin_signers().len(), 1);
    assert_eq!(client.get_admin_threshold(), 1); // auto-reduced from 2 to 1
}

// ── 10. Remove non-existent signer returns AdminSignerNotInSet ───────────────

#[test]
fn test_remove_admin_signer_not_in_set() {
    let (env, client, _admin, _service) = setup();
    let s1 = Address::generate(&env);
    client.add_admin_signer(&Vec::new(&env), &s1);
    client.set_admin_threshold(&Vec::new(&env), &1);

    let outsider = Address::generate(&env);
    let signer_vec = signers_vec(&env, core::slice::from_ref(&s1));
    let result = client.try_remove_admin_signer(&signer_vec, &outsider);
    assert_eq!(result, Err(Ok(Error::AdminSignerNotInSet)));
}

// ── 11. set_admin_threshold rejects 0 or value > set size ────────────────────

#[test]
fn test_set_admin_threshold_validation() {
    let (env, client, _admin, _service) = setup();
    let s1 = Address::generate(&env);
    client.add_admin_signer(&Vec::new(&env), &s1);

    // 0 is invalid.
    let result = client.try_set_admin_threshold(&Vec::new(&env), &0);
    assert_eq!(result, Err(Ok(Error::InvalidThreshold)));

    // Greater than set size (1) is invalid.
    let result = client.try_set_admin_threshold(&Vec::new(&env), &2);
    assert_eq!(result, Err(Ok(Error::InvalidThreshold)));

    // Exactly 1 is valid.
    client.set_admin_threshold(&Vec::new(&env), &1);
    assert_eq!(client.get_admin_threshold(), 1);
}

// ── 12. pause / unpause work with M-of-N in multisig mode ───────────────────

#[test]
fn test_pause_unpause_multisig() {
    let (env, client, _admin, _service) = setup();
    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    client.add_admin_signer(&Vec::new(&env), &s1);
    client.add_admin_signer(&Vec::new(&env), &s2);
    client.set_admin_threshold(&Vec::new(&env), &2);

    let both = signers_vec(&env, &[s1.clone(), s2.clone()]);
    assert!(!client.is_paused());
    client.pause(&both);
    assert!(client.is_paused());
    client.unpause(&both);
    assert!(!client.is_paused());
}

// ── 13. transfer_admin works with M-of-N in multisig mode ────────────────────

#[test]
fn test_transfer_admin_multisig() {
    let (env, client, _admin, _service) = setup();
    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    client.add_admin_signer(&Vec::new(&env), &s1);
    client.add_admin_signer(&Vec::new(&env), &s2);
    client.set_admin_threshold(&Vec::new(&env), &2);

    let new_admin = Address::generate(&env);
    let both = signers_vec(&env, &[s1.clone(), s2.clone()]);
    client.transfer_admin(&both, &new_admin);
    assert!(client.has_pending_admin_transfer());
    // New admin accepts (they just call require_auth on themselves).
    client.accept_admin();
    assert_eq!(client.get_admin(), new_admin);
}
