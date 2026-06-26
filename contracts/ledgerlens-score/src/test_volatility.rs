#![cfg(test)]

use soroban_sdk::{symbol_short, testutils::Ledger as _, Address, Env, Vec};

use crate::{test::initialized, LedgerLensScoreContract, LedgerLensScoreContractClient};

fn submit(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    wallet: &Address,
    pair: &soroban_sdk::Symbol,
    score: u32,
) {
    client.submit_score(&Vec::new(env), wallet, pair, &score, &false, &false, &1, &90, &1, &None);
    env.ledger().with_mut(|l| l.timestamp += 3_601); // advance past cooldown
}

#[test]
fn test_volatility_zero_before_submissions() {
    let (_env, client, _admin, _service) = initialized();
    let pair = symbol_short!("XLM_USDC");
    assert_eq!(client.get_pair_volatility(&pair), 0);
}

#[test]
fn test_volatility_zero_with_one_submission() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 50);
    assert_eq!(client.get_pair_volatility(&pair), 0);
}

#[test]
fn test_volatility_nonzero_with_two_different_scores() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 40);
    let w2 = Address::generate(&env);
    submit(&env, &client, &w2, &pair, 60);
    // std dev of [40, 60] = 10, scaled ×100 = 1000
    assert!(client.get_pair_volatility(&pair) > 0);
}

#[test]
fn test_volatility_zero_for_identical_scores() {
    let (env, client, _admin, _service) = initialized();
    let pair = symbol_short!("XLM_USDC");
    for _ in 0..3 {
        let w = Address::generate(&env);
        submit(&env, &client, &w, &pair, 50);
    }
    // All same score → std dev = 0
    assert_eq!(client.get_pair_volatility(&pair), 0);
}

#[test]
fn test_volatility_window_default() {
    let (_env, client, _admin, _service) = initialized();
    assert_eq!(client.get_pair_volatility_window(), 86_400);
}

#[test]
fn test_set_volatility_window() {
    let (env, client, admin, _service) = initialized();
    client.set_pair_volatility_window(&Vec::from_array(&env, [admin.clone()]), &3600);
    assert_eq!(client.get_pair_volatility_window(), 3600);
}

#[test]
fn test_volatility_resets_after_window_expires() {
    let (env, client, admin, _service) = initialized();
    let pair = symbol_short!("XLM_USDC");
    // Set a short window
    client.set_pair_volatility_window(&Vec::from_array(&env, [admin.clone()]), &60);

    let w1 = Address::generate(&env);
    submit(&env, &client, &w1, &pair, 10);
    let w2 = Address::generate(&env);
    submit(&env, &client, &w2, &pair, 90);
    assert!(client.get_pair_volatility(&pair) > 0);

    // Jump past the window so next submission resets state
    env.ledger().with_mut(|l| l.timestamp += 100);
    let w3 = Address::generate(&env);
    submit(&env, &client, &w3, &pair, 50);
    // After reset, only 1 sample → volatility = 0
    assert_eq!(client.get_pair_volatility(&pair), 0);
}
