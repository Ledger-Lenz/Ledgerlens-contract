//! Tests for the admin-initiated `reset_breach_counter` function (audit-trail
//! reset of the consecutive-breach counter, distinct from the silent
//! `reset_breach_count` emergency override).

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger as _},
    Address, Env, IntoVal, Vec,
};

use crate::{
    constants::DEFAULT_COOLDOWN_SECS, Error, LedgerLensScoreContract, LedgerLensScoreContractClient,
};

const START_TS: u64 = 1_700_000_000;
const HIGH_RISK_SCORE: u32 = 90;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);

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

/// Submits a high-risk score and advances the ledger clock past the default
/// cooldown so the next submission for the same `(wallet, asset_pair)` is
/// also accepted.
fn submit_breach(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    wallet: &Address,
    pair: &soroban_sdk::Symbol,
) {
    let ts = env.ledger().timestamp();
    client.submit_score(
        &Vec::new(env),
        wallet,
        pair,
        &HIGH_RISK_SCORE,
        &true,
        &true,
        &ts,
        &95,
        &1,
        &None,
    );
    env.ledger().with_mut(|l| l.timestamp += DEFAULT_COOLDOWN_SECS);
}

// ── Counter reset behavior ───────────────────────────────────────────────────

#[test]
fn test_reset_breach_counter_zeroes_count_after_two_breaches() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit_breach(&env, &client, &wallet, &pair);
    submit_breach(&env, &client, &wallet, &pair);
    assert_eq!(client.get_breach_count(&wallet, &pair), 2);

    client.reset_breach_counter(&Vec::new(&env), &wallet, &pair);
    assert_eq!(client.get_breach_count(&wallet, &pair), 0);
}

#[test]
fn test_breach_count_increments_from_zero_after_reset() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit_breach(&env, &client, &wallet, &pair);
    submit_breach(&env, &client, &wallet, &pair);
    client.reset_breach_counter(&Vec::new(&env), &wallet, &pair);

    submit_breach(&env, &client, &wallet, &pair);
    assert_eq!(client.get_breach_count(&wallet, &pair), 1);
}

#[test]
fn test_reset_breach_counter_only_affects_targeted_pair() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("BTC_USDC");

    submit_breach(&env, &client, &wallet, &pair_a);
    submit_breach(&env, &client, &wallet, &pair_b);
    assert_eq!(client.get_breach_count(&wallet, &pair_a), 1);
    assert_eq!(client.get_breach_count(&wallet, &pair_b), 1);

    client.reset_breach_counter(&Vec::new(&env), &wallet, &pair_a);
    assert_eq!(client.get_breach_count(&wallet, &pair_a), 0);
    assert_eq!(client.get_breach_count(&wallet, &pair_b), 1);
}

// ── Event emission ───────────────────────────────────────────────────────────

#[test]
fn test_reset_breach_counter_emits_event_with_wallet_pair_and_admin() {
    let (env, client, admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit_breach(&env, &client, &wallet, &pair);

    let contract_id = client.address.clone();
    client.reset_breach_counter(&Vec::new(&env), &wallet, &pair);

    let topic = (symbol_short!("brc_rst"), wallet.clone(), pair.clone());
    let found = env.events().all().iter().any(|(addr, topics, data)| {
        if addr != contract_id || topics != topic.clone().into_val(&env) {
            return false;
        }
        let by: Address = data.into_val(&env);
        by == admin
    });
    assert!(found, "expected a brc_rst event recording the resetting admin");
}

// ── Authorization ─────────────────────────────────────────────────────────────

#[test]
fn test_reset_breach_counter_before_init_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let result = client.try_reset_breach_counter(&Vec::new(&env), &wallet, &pair);
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_reset_breach_counter_multisig_success() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit_breach(&env, &client, &wallet, &pair);

    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    client.add_admin_signer(&Vec::new(&env), &s1);
    client.add_admin_signer(&Vec::new(&env), &s2);
    client.set_admin_threshold(&Vec::new(&env), &2);

    let both = signers_vec(&env, &[s1, s2]);
    client.reset_breach_counter(&both, &wallet, &pair);
    assert_eq!(client.get_breach_count(&wallet, &pair), 0);
}

#[test]
fn test_reset_breach_counter_multisig_insufficient_signers_fails() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit_breach(&env, &client, &wallet, &pair);

    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    client.add_admin_signer(&Vec::new(&env), &s1);
    client.add_admin_signer(&Vec::new(&env), &s2);
    client.set_admin_threshold(&Vec::new(&env), &2);

    let one_signer = signers_vec(&env, &[s1]);
    let result = client.try_reset_breach_counter(&one_signer, &wallet, &pair);
    assert_eq!(result, Err(Ok(Error::InsufficientAdminSigners)));
    assert_eq!(client.get_breach_count(&wallet, &pair), 1);
}
