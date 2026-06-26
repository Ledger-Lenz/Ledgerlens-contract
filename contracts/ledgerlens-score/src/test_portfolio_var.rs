#![cfg(test)]
//! Unit tests for #282: portfolio Value-at-Risk estimator.

use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol, Vec};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient};

struct Setup<'a> {
    env: Env,
    client: LedgerLensScoreContractClient<'a>,
}

fn setup() -> Setup<'static> {
    let env = Env::default();
    env.mock_all_auths();
    let contract = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    let client: LedgerLensScoreContractClient<'static> =
        unsafe { core::mem::transmute(client) };
    Setup { env, client }
}

fn submit(env: &Env, client: &LedgerLensScoreContractClient, wallet: &Address, pair: Symbol, score: u32) {
    client
        .submit_score(&Vec::new(env), wallet, &pair, &score, &false, &false, &1, &90, &1, &None)
        .unwrap();
}

// ── InsufficientPairData errors ───────────────────────────────────────────────

#[test]
fn var_fails_no_pairs() {
    let s = setup();
    let wallet = Address::generate(&s.env);
    assert_eq!(
        s.client.try_get_portfolio_var(&wallet, &95),
        Err(Ok(Error::InsufficientPairData))
    );
}

#[test]
fn var_fails_one_pair() {
    let s = setup();
    let wallet = Address::generate(&s.env);
    submit(&s.env, &s.client, &wallet, symbol_short!("XLM_USDC"), 50);
    assert_eq!(
        s.client.try_get_portfolio_var(&wallet, &95),
        Err(Ok(Error::InsufficientPairData))
    );
}

// ── Two uncorrelated pairs ────────────────────────────────────────────────────

#[test]
fn var_two_uncorrelated_95_in_range() {
    let s = setup();
    let wallet = Address::generate(&s.env);
    submit(&s.env, &s.client, &wallet, symbol_short!("XLM_USDC"), 60);
    submit(&s.env, &s.client, &wallet, symbol_short!("XLM_BTC"), 80);

    let var = s.client.get_portfolio_var(&wallet, &95).unwrap();
    assert!(var > 0 && var <= 100, "VaR={var}");
}

#[test]
fn var_99_gte_95_for_same_inputs() {
    let s = setup();
    let wallet = Address::generate(&s.env);
    submit(&s.env, &s.client, &wallet, symbol_short!("XLM_USDC"), 60);
    submit(&s.env, &s.client, &wallet, symbol_short!("XLM_BTC"), 80);

    let var95 = s.client.get_portfolio_var(&wallet, &95).unwrap();
    let var99 = s.client.get_portfolio_var(&wallet, &99).unwrap();
    assert!(var99 >= var95, "99% ({var99}) should be >= 95% ({var95})");
}

// ── Correlated pairs produce higher VaR ──────────────────────────────────────

#[test]
fn var_perfect_correlation_higher_than_uncorrelated() {
    let s = setup();
    let wallet = Address::generate(&s.env);
    submit(&s.env, &s.client, &wallet, symbol_short!("XLM_USDC"), 60);
    submit(&s.env, &s.client, &wallet, symbol_short!("XLM_BTC"), 80);

    let var_uncorr = s.client.get_portfolio_var(&wallet, &95).unwrap();

    s.client
        .set_pair_correlation(&symbol_short!("XLM_USDC"), &symbol_short!("XLM_BTC"), &10_000)
        .unwrap();
    let var_corr = s.client.get_portfolio_var(&wallet, &95).unwrap();

    assert!(var_corr >= var_uncorr, "corr ({var_corr}) >= uncorr ({var_uncorr})");
}

// ── Negative correlation reduces VaR ─────────────────────────────────────────

#[test]
fn var_negative_correlation_lowers_var() {
    let s = setup();
    let wallet = Address::generate(&s.env);
    submit(&s.env, &s.client, &wallet, symbol_short!("XLM_USDC"), 60);
    submit(&s.env, &s.client, &wallet, symbol_short!("XLM_BTC"), 80);

    let var_uncorr = s.client.get_portfolio_var(&wallet, &95).unwrap();

    s.client
        .set_pair_correlation(&symbol_short!("XLM_USDC"), &symbol_short!("XLM_BTC"), &-5_000)
        .unwrap();
    let var_neg = s.client.get_portfolio_var(&wallet, &95).unwrap();

    assert!(var_neg <= var_uncorr, "neg ({var_neg}) <= uncorr ({var_uncorr})");
}

// ── Three pairs ───────────────────────────────────────────────────────────────

#[test]
fn var_three_pairs_in_range() {
    let s = setup();
    let wallet = Address::generate(&s.env);
    submit(&s.env, &s.client, &wallet, symbol_short!("XLM_USDC"), 60);
    submit(&s.env, &s.client, &wallet, symbol_short!("XLM_BTC"), 65);
    submit(&s.env, &s.client, &wallet, symbol_short!("XLM_ETH"), 70);
    let var = s.client.get_portfolio_var(&wallet, &95).unwrap();
    assert!(var <= 100);
}

// ── set/get_pair_correlation round-trip and symmetry ─────────────────────────

#[test]
fn pair_correlation_round_trip_and_symmetric() {
    let s = setup();
    s.client
        .set_pair_correlation(&symbol_short!("XLM_USDC"), &symbol_short!("XLM_BTC"), &7_500)
        .unwrap();

    assert_eq!(
        s.client.get_pair_correlation(&symbol_short!("XLM_USDC"), &symbol_short!("XLM_BTC")),
        7_500
    );
    // Symmetric read
    assert_eq!(
        s.client.get_pair_correlation(&symbol_short!("XLM_BTC"), &symbol_short!("XLM_USDC")),
        7_500
    );
}

#[test]
fn pair_correlation_defaults_zero() {
    let s = setup();
    assert_eq!(
        s.client.get_pair_correlation(&symbol_short!("XLM_USDC"), &symbol_short!("XLM_BTC")),
        0
    );
}
