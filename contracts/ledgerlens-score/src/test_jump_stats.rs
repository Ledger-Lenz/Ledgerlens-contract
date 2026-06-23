//! Tests for `JumpStats` / `get_jump_stats` (issue #119).
//!
//! Tracks the largest score-jump anomaly observed for a (wallet, asset_pair)
//! pair, persisted independently of the `ScoreJumpAnomalyEvent` emitted by
//! `submit_score`.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient};

const START_TS: u64 = 1_700_000_000;
const COOLDOWN: u64 = 3_601;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client, contract_id)
}

fn submit(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    wallet: &Address,
    pair: &soroban_sdk::Symbol,
    score: u32,
) {
    client.submit_score(
        &Vec::new(env),
        wallet,
        pair,
        &score,
        &false,
        &false,
        &env.ledger().timestamp(),
        &90,
        &1,
        &None,
    );
}

// ── No jump ever recorded ────────────────────────────────────────────────────

#[test]
fn test_no_jump_returns_zero_sentinel() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    assert_eq!(client.get_jump_stats(&wallet, &pair), (0, 0));

    // A single submission alone (no previous score) cannot trigger a jump.
    submit(&env, &client, &wallet, &pair, 50);
    assert_eq!(client.get_jump_stats(&wallet, &pair), (0, 0));
}

// ── Sub-threshold deltas never update JumpStats ──────────────────────────────

#[test]
fn test_sub_threshold_jump_not_recorded() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 10);
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);

    // delta = 20, below the default threshold of 30 — no jump recorded.
    submit(&env, &client, &wallet, &pair, 30);

    assert_eq!(client.get_jump_stats(&wallet, &pair), (0, 0));
}

// ── Small jump then a larger jump: largest one wins ──────────────────────────

#[test]
fn test_largest_jump_is_recorded() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // First submission: no previous score, no jump possible.
    submit(&env, &client, &wallet, &pair, 10);
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);

    // Second: 10 -> 45, delta = 35 (> 30 threshold) — first recorded jump.
    submit(&env, &client, &wallet, &pair, 45);
    let first_jump_ts = env.ledger().timestamp();
    assert_eq!(client.get_jump_stats(&wallet, &pair), (35, first_jump_ts));
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);

    // Third: 45 -> 50, delta = 5 — below threshold, max_jump unchanged.
    submit(&env, &client, &wallet, &pair, 50);
    assert_eq!(client.get_jump_stats(&wallet, &pair), (35, first_jump_ts));
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);

    // Fourth: 50 -> 5, delta = 45 (> 35) — new, larger jump overwrites max.
    submit(&env, &client, &wallet, &pair, 5);
    let largest_jump_ts = env.ledger().timestamp();
    assert_eq!(client.get_jump_stats(&wallet, &pair), (45, largest_jump_ts));
}

// ── A subsequent smaller jump never overwrites a previously larger one ──────

#[test]
fn test_smaller_jump_does_not_overwrite_recorded_max() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 10);
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);

    // 10 -> 80, delta = 70.
    submit(&env, &client, &wallet, &pair, 80);
    let big_jump_ts = env.ledger().timestamp();
    assert_eq!(client.get_jump_stats(&wallet, &pair), (70, big_jump_ts));
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);

    // 80 -> 40, delta = 40 (> threshold, but smaller than 70) — max stays.
    submit(&env, &client, &wallet, &pair, 40);
    assert_eq!(client.get_jump_stats(&wallet, &pair), (70, big_jump_ts));
}

// ── JumpStats is tracked independently per (wallet, asset_pair) ─────────────

#[test]
fn test_jump_stats_independent_per_pair() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("BTC_USDC");

    submit(&env, &client, &wallet, &pair_a, 10);
    submit(&env, &client, &wallet, &pair_b, 10);
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);

    // Only pair_a jumps.
    submit(&env, &client, &wallet, &pair_a, 60);
    let ts = env.ledger().timestamp();

    assert_eq!(client.get_jump_stats(&wallet, &pair_a), (50, ts));
    assert_eq!(client.get_jump_stats(&wallet, &pair_b), (0, 0));
}
