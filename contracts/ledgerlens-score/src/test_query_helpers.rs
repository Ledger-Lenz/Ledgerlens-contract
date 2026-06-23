//! Unit tests for the query helper functions added by issues #97, #99, #100,
//! and the supports_interface registration added by issue #101.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
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
// Issue #101 – supports_interface: emb and cons capabilities
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_supports_interface_emb_registered() {
    let (_env, client, _admin, _service) = setup();
    assert!(
        client.supports_interface(&symbol_short!("emb")),
        "capability `emb` (embargo) must be reported as supported"
    );
}

#[test]
fn test_supports_interface_cons_registered() {
    let (_env, client, _admin, _service) = setup();
    assert!(
        client.supports_interface(&symbol_short!("cons")),
        "capability `cons` (consensus) must be reported as supported"
    );
}

#[test]
fn test_supports_interface_all_capabilities() {
    // Verifies the complete advertised capability set in one pass.
    let (env, client, _admin, _service) = setup();
    for cap in ["score", "history", "batch", "gate", "aggr", "count", "emb", "cons"] {
        let sym = Symbol::new(&env, cap);
        assert!(
            client.supports_interface(&sym),
            "capability `{cap}` must be reported as supported"
        );
    }
}

#[test]
fn test_supports_interface_long_cap_batch_attested() {
    // The longer Symbol case is separately constructed via Symbol::new.
    let (env, client, _admin, _service) = setup();
    let cap = Symbol::new(&env, "batch_attested");
    assert!(client.supports_interface(&cap));
}

#[test]
fn test_supports_interface_unknown_returns_false() {
    let (_env, client, _admin, _service) = setup();
    assert!(!client.supports_interface(&symbol_short!("foobar")));
}

// ─────────────────────────────────────────────────────────────────────────────
// Issue #97 – get_admin_signer_count
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_get_admin_signer_count_initial() {
    // After initialize(), only the one admin is registered.
    let (_env, client, _admin, _service) = setup();
    assert_eq!(client.get_admin_signer_count(), 1);
}

#[test]
fn test_get_admin_signer_count_after_transfer() {
    // After an admin transfer is accepted the count stays at 1 (there is
    // exactly one admin at any point in time).
    let (env, client, _admin, _service) = setup();

    let new_admin = Address::generate(&env);
    // transfer_admin takes (admin_signers: Vec<Address>, new_admin: Address)
    client.transfer_admin(&Vec::new(&env), &new_admin);
    client.accept_admin();

    assert_eq!(client.get_admin_signer_count(), 1);
}

// ─────────────────────────────────────────────────────────────────────────────
// Issue #100 – get_score_age
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_get_score_age_no_score_returns_zero() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // No score has ever been submitted → age is 0.
    assert_eq!(client.get_score_age(&wallet, &pair), 0);
}

#[test]
fn test_get_score_age_just_submitted() {
    // Ledger timestamp is 1_000_000 at submission, no time advances → age = 0.
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
        &90,
        &1,
        &None,
    );

    // Time has not advanced → age should be 0.
    assert_eq!(client.get_score_age(&wallet, &pair), 0);
}

#[test]
fn test_get_score_age_after_time_advance() {
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
        &90,
        &1,
        &None,
    );

    // Advance ledger by 7200 seconds (2 hours).
    env.ledger().with_mut(|l| l.timestamp = 1_007_200);

    assert_eq!(client.get_score_age(&wallet, &pair), 7200);
}

#[test]
fn test_get_score_age_unknown_pair_returns_zero() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let scored_pair = symbol_short!("XLM_USDC");
    let other_pair = symbol_short!("BTC_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // Submit for scored_pair only.
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &scored_pair,
        &40,
        &false,
        &false,
        &1_700_000_000,
        &90,
        &1,
        &None,
    );

    // other_pair was never scored → age is 0.
    assert_eq!(client.get_score_age(&wallet, &other_pair), 0);
}

// ─────────────────────────────────────────────────────────────────────────────
// Issue #99 – get_embargo_expiry
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_get_embargo_expiry_no_embargo_returns_none() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    assert!(client.get_embargo_expiry(&wallet).is_none());
}

#[test]
fn test_get_embargo_expiry_timed_embargo_returns_timestamp() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // Place a timed embargo that expires at ledger ts 2_000_000.
    // set_score_embargo(env, wallet, expiry: Option<u64>), no admin arg in client call.
    client.set_score_embargo(&wallet, &Some(2_000_000_u64));

    let expiry = client.get_embargo_expiry(&wallet);
    assert_eq!(expiry, Some(2_000_000_u64));
}

#[test]
fn test_get_embargo_expiry_indefinite_embargo_returns_none() {
    // `get_embargo_expiry` returns None for indefinite embargoes because
    // there is no concrete expiry timestamp to report.
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // None argument → indefinite embargo.
    client.set_score_embargo(&wallet, &None);

    assert!(
        client.get_embargo_expiry(&wallet).is_none(),
        "indefinite embargoes have no timed expiry"
    );
}

#[test]
fn test_get_embargo_expiry_after_expiry_returns_none() {
    // Once the ledger passes the expiry timestamp the embargo is expired;
    // `get_embargo_expiry` must return None to reflect that.
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);
    client.set_score_embargo(&wallet, &Some(1_500_000_u64));

    // Advance past the embargo expiry.
    env.ledger().with_mut(|l| l.timestamp = 1_600_000);

    assert!(
        client.get_embargo_expiry(&wallet).is_none(),
        "expired embargo should return None"
    );
}

#[test]
fn test_get_embargo_expiry_lifted_returns_none() {
    // After an explicit lift the function must return None.
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);
    client.set_score_embargo(&wallet, &Some(2_000_000_u64));

    // Confirm the embargo is active.
    assert!(client.get_embargo_expiry(&wallet).is_some());

    // Lift it.
    client.lift_score_embargo(&wallet);

    assert!(client.get_embargo_expiry(&wallet).is_none());
}
