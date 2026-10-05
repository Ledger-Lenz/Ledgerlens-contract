#![cfg(test)]
//! Unit tests for #281: fee-per-query model on `query_risk_gate`.

use soroban_sdk::{
    symbol_short, testutils::Address as _, token::StellarAssetClient, Address, Env, Vec,
};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient};

// ── helpers ───────────────────────────────────────────────────────────────────

struct Setup<'a> {
    env: Env,
    client: LedgerLensScoreContractClient<'a>,
    token: Address,
}

fn setup_with_token(initial_balance: i128) -> Setup<'static> {
    let env = Env::default();
    env.mock_all_auths();

    let contract = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    let sac = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let token = sac.address();

    if initial_balance > 0 {
        StellarAssetClient::new(&env, &token).mint(&contract, &initial_balance);
    }
    client.set_fee_token(&token);

    // SAFETY: env is moved into Setup and outlives client.
    let client: LedgerLensScoreContractClient<'static> =
        unsafe { core::mem::transmute(client) };
    Setup { env, client, token }
}

fn setup_no_token() -> Setup<'static> {
    let env = Env::default();
    env.mock_all_auths();
    let contract = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    let client: LedgerLensScoreContractClient<'static> =
        unsafe { core::mem::transmute(client) };
    Setup { env, client, token: Address::generate(&env) }
}

// ── set_gate_query_fee ────────────────────────────────────────────────────────

#[test]
fn set_gate_query_fee_requires_init() {
    let env = Env::default();
    env.mock_all_auths();
    let contract = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract);
    assert_eq!(
        client.try_set_gate_query_fee(&100),
        Err(Ok(Error::NotInitialized))
    );
}

#[test]
fn set_gate_query_fee_admin_succeeds() {
    let s = setup_with_token(0);
    s.client.set_gate_query_fee(&500).unwrap();
}

// ── get_accumulated_fees ──────────────────────────────────────────────────────

#[test]
fn accumulated_fees_starts_at_zero() {
    let s = setup_with_token(0);
    assert_eq!(s.client.get_accumulated_fees(), 0);
}

// ── zero-fee bypass ───────────────────────────────────────────────────────────

#[test]
fn zero_fee_no_transfer_no_accumulation() {
    let s = setup_with_token(0);
    let wallet = Address::generate(&s.env);
    let pair = symbol_short!("XLM_USDC");
    // Default fee = 0: gate call works, no accumulation.
    s.client.query_risk_gate(&wallet, &pair, &75);
    assert_eq!(s.client.get_accumulated_fees(), 0);
}

// ── fee charged and accumulated ───────────────────────────────────────────────

#[test]
fn fee_charged_and_accumulated() {
    let s = setup_with_token(0);
    let fee: i128 = 200;
    s.client.set_gate_query_fee(&fee).unwrap();

    let wallet = Address::generate(&s.env);
    let pair = symbol_short!("XLM_USDC");
    StellarAssetClient::new(&s.env, &s.token).mint(&wallet, &1_000);

    s.client
        .submit_score(
            &Vec::new(&s.env),
            &wallet,
            &pair,
            &30,
            &false,
            &false,
            &1,
            &90,
            &1,
            &None,
        )
        .unwrap();

    let result = s.client.query_risk_gate(&wallet, &pair, &75);
    assert!(result);
    assert_eq!(s.client.get_accumulated_fees(), fee);
}

#[test]
fn fee_accumulates_across_multiple_calls() {
    let s = setup_with_token(0);
    let fee: i128 = 100;
    s.client.set_gate_query_fee(&fee).unwrap();

    let wallet = Address::generate(&s.env);
    let pair = symbol_short!("XLM_USDC");
    StellarAssetClient::new(&s.env, &s.token).mint(&wallet, &10_000);

    s.client
        .submit_score(
            &Vec::new(&s.env),
            &wallet,
            &pair,
            &30,
            &false,
            &false,
            &1,
            &90,
            &1,
            &None,
        )
        .unwrap();

    s.client.query_risk_gate(&wallet, &pair, &75);
    s.client.query_risk_gate(&wallet, &pair, &75);
    s.client.query_risk_gate(&wallet, &pair, &75);

    assert_eq!(s.client.get_accumulated_fees(), fee * 3);
}

// ── fee skipped gracefully when no token configured ───────────────────────────

#[test]
fn fee_skipped_silently_when_no_token() {
    let s = setup_no_token();
    s.client.set_gate_query_fee(&999).unwrap();

    let wallet = Address::generate(&s.env);
    let pair = symbol_short!("XLM_USDC");

    // No panic — gate stays infallible.
    let result = s.client.query_risk_gate(&wallet, &pair, &75);
    assert!(!result); // no score → false
    assert_eq!(s.client.get_accumulated_fees(), 0);
}

// ── per-consumer gate quota ──────────────────────────────────────────────────

fn configure_quota(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    consumer: &Address,
    capacity: u32,
    refill_rate: u32,
) {
    client
        .set_consumer_read_quota(&Vec::new(env), consumer, &capacity, &refill_rate)
        .unwrap();
}

fn submit_safe_score(s: &Setup<'_>, wallet: &Address) {
    s.client
        .submit_score(
            &Vec::new(&s.env),
            wallet,
            &symbol_short!("XLM_USDC"),
            &30,
            &false,
            &false,
            &1,
            &90,
            &1,
            &None,
        )
        .unwrap();
}

fn metered_query(
    s: &Setup<'_>,
    consumer: &Address,
    wallet: &Address,
) -> bool {
    s.client.query_risk_gate_metered(
        consumer,
        wallet,
        &symbol_short!("XLM_USDC"),
        &75,
    )
}

#[test]
fn consumer_read_quota_refill_boundary_and_partial_refill() {
    let s = setup_no_token();
    let consumer = Address::generate(&s.env);
    let wallet = Address::generate(&s.env);
    configure_quota(&s.env, &s.client, &consumer, 2, 1);

    assert!(!metered_query(&s, &consumer, &wallet));
    assert!(!metered_query(&s, &consumer, &wallet));
    assert_eq!(s.client.get_consumer_read_quota_remaining(&consumer), 0);

    let start = s.env.ledger().timestamp();
    s.env.ledger().with_mut(|l| l.timestamp = start + 1);
    assert_eq!(s.client.get_consumer_read_quota_remaining(&consumer), 1);
    s.env.ledger().with_mut(|l| l.timestamp = start + 2);
    assert_eq!(s.client.get_consumer_read_quota_remaining(&consumer), 2);
}

#[test]
fn consumer_read_quota_saturates_for_huge_elapsed_time() {
    let s = setup_no_token();
    let consumer = Address::generate(&s.env);
    let wallet = Address::generate(&s.env);
    configure_quota(&s.env, &s.client, &consumer, 3, u32::MAX);
    for _ in 0..3 {
        assert!(!metered_query(&s, &consumer, &wallet));
    }

    s.env.ledger().with_mut(|l| l.timestamp = u64::MAX);
    assert_eq!(s.client.get_consumer_read_quota_remaining(&consumer), 3);
}

#[test]
fn consumer_read_quota_exhaustion_fails_closed() {
    let s = setup_no_token();
    let consumer = Address::generate(&s.env);
    let wallet = Address::generate(&s.env);
    configure_quota(&s.env, &s.client, &consumer, 1, 0);

    assert!(!metered_query(&s, &consumer, &wallet));
    assert_eq!(s.client.get_consumer_read_quota_remaining(&consumer), 0);
    assert!(!metered_query(&s, &consumer, &wallet));
}

#[test]
fn clearing_consumer_quota_restores_unmetered_metered_queries() {
    let s = setup_no_token();
    let consumer = Address::generate(&s.env);
    let wallet = Address::generate(&s.env);
    submit_safe_score(&s, &wallet);
    configure_quota(&s.env, &s.client, &consumer, 1, 0);

    assert!(metered_query(&s, &consumer, &wallet));
    assert!(!metered_query(&s, &consumer, &wallet));
    s.client
        .clear_consumer_read_quota(&Vec::new(&s.env), &consumer)
        .unwrap();
    assert!(metered_query(&s, &consumer, &wallet));
    assert!(metered_query(&s, &consumer, &wallet));
}

#[test]
fn consumer_quota_set_and_clear_require_admin_authorization() {
    let s = setup_no_token();
    let consumer = Address::generate(&s.env);

    assert!(s
        .client
        .mock_auths(&[])
        .try_set_consumer_read_quota(&Vec::new(&s.env), &consumer, &1, &1)
        .is_err());
    assert!(s
        .client
        .mock_auths(&[])
        .try_clear_consumer_read_quota(&Vec::new(&s.env), &consumer)
        .is_err());
}

#[test]
fn metered_query_requires_consumer_authorization() {
    let s = setup_no_token();
    let consumer = Address::generate(&s.env);
    let wallet = Address::generate(&s.env);

    assert!(s
        .client
        .mock_auths(&[])
        .try_query_risk_gate_metered(
            &consumer,
            &wallet,
            &symbol_short!("XLM_USDC"),
            &75,
        )
        .is_err());
}

#[test]
fn consumer_quota_checks_allowlist_before_consuming_tokens() {
    let s = setup_no_token();
    let consumer = Address::generate(&s.env);
    let wallet = Address::generate(&s.env);
    submit_safe_score(&s, &wallet);
    configure_quota(&s.env, &s.client, &consumer, 1, 0);
    s.client.set_gate_enforcement_mode(&Vec::new(&s.env), &true);

    assert!(!metered_query(&s, &consumer, &wallet));
    assert_eq!(s.client.get_consumer_read_quota_remaining(&consumer), 1);

    let mut callers = Vec::new(&s.env);
    callers.push_back(consumer.clone());
    s.client.set_gate_callers(&Vec::new(&s.env), &callers);
    assert!(metered_query(&s, &consumer, &wallet));
    assert_eq!(s.client.get_consumer_read_quota_remaining(&consumer), 0);
}

#[test]
fn metered_quota_exhaustion_does_not_change_query_fee_accounting() {
    let s = setup_with_token(0);
    let consumer = Address::generate(&s.env);
    let wallet = Address::generate(&s.env);
    StellarAssetClient::new(&s.env, &s.token).mint(&wallet, &1_000);
    submit_safe_score(&s, &wallet);
    configure_quota(&s.env, &s.client, &consumer, 1, 0);
    s.client.set_gate_query_fee(&123).unwrap();

    assert!(metered_query(&s, &consumer, &wallet));
    let fees_before_exhausted_call = s.client.get_accumulated_fees();
    assert!(!metered_query(&s, &consumer, &wallet));
    assert_eq!(s.client.get_accumulated_fees(), fees_before_exhausted_call);
}
