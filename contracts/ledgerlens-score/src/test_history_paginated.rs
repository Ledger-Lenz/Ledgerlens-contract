//! Boundary tests for `get_score_history_paginated` — the windowed,
//! most-recent-first view over the score-history ring buffer (issue #108).
//!
//! These pin the read semantics third parties rely on: `offset` counted from
//! the newest entry, `limit` capping the slice, out-of-bounds offsets yielding
//! an empty `Vec` (never an error), and the read leaving the ring untouched.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient};

fn initialized<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    (env, client, admin, service)
}

/// Seeds `count` scores for `(wallet, asset_pair)` with scores
/// `10, 20, 30, …`, advancing past the cooldown between each so every
/// submission lands. The newest entry therefore has the highest score.
fn seed_scores(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    wallet: &Address,
    asset_pair: &Symbol,
    count: u32,
) {
    for i in 1..=count {
        client.submit_score(
            &Vec::new(env),
            wallet,
            asset_pair,
            &(i * 10),
            &false,
            &false,
            &(i as u64),
            &50,
            &1,
            &None,
        );
        env.ledger().with_mut(|l| l.timestamp += 3_601);
    }
}

// ── Most-recent slice ─────────────────────────────────────────────────────────

#[test]
fn test_paginated_offset0_limit1_returns_most_recent() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    seed_scores(&env, &client, &wallet, &asset_pair, 3); // 10, 20, 30 (newest = 30)

    let page = client.get_score_history_paginated(&wallet, &asset_pair, &0, &1);
    assert_eq!(page.len(), 1);
    assert_eq!(page.get(0).unwrap().score, 30);
}

#[test]
fn test_paginated_returns_most_recent_first() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    seed_scores(&env, &client, &wallet, &asset_pair, 5); // 10..50 (newest = 50)

    let page = client.get_score_history_paginated(&wallet, &asset_pair, &0, &3);
    assert_eq!(page.len(), 3);
    // Most-recent first: 50, 40, 30.
    assert_eq!(page.get(0).unwrap().score, 50);
    assert_eq!(page.get(1).unwrap().score, 40);
    assert_eq!(page.get(2).unwrap().score, 30);
}

#[test]
fn test_paginated_offset_skips_most_recent() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    seed_scores(&env, &client, &wallet, &asset_pair, 5); // 10..50

    // Skip the newest two (50, 40); next two are 30, 20.
    let page = client.get_score_history_paginated(&wallet, &asset_pair, &2, &2);
    assert_eq!(page.len(), 2);
    assert_eq!(page.get(0).unwrap().score, 30);
    assert_eq!(page.get(1).unwrap().score, 20);
}

// ── Boundary conditions ───────────────────────────────────────────────────────

#[test]
fn test_paginated_empty_history_returns_empty() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    let page = client.get_score_history_paginated(&wallet, &asset_pair, &0, &5);
    assert_eq!(page.len(), 0);
}

#[test]
fn test_paginated_offset_at_len_returns_empty() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    seed_scores(&env, &client, &wallet, &asset_pair, 3);

    // offset == len: nothing left to return, but no error.
    let page = client.get_score_history_paginated(&wallet, &asset_pair, &3, &1);
    assert_eq!(page.len(), 0);
}

#[test]
fn test_paginated_offset_beyond_len_returns_empty() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    seed_scores(&env, &client, &wallet, &asset_pair, 3);

    let page = client.get_score_history_paginated(&wallet, &asset_pair, &100, &10);
    assert_eq!(page.len(), 0);
}

#[test]
fn test_paginated_limit_zero_returns_empty() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    seed_scores(&env, &client, &wallet, &asset_pair, 3);

    let page = client.get_score_history_paginated(&wallet, &asset_pair, &0, &0);
    assert_eq!(page.len(), 0);
}

#[test]
fn test_paginated_limit_exceeds_available_returns_all_remaining() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    seed_scores(&env, &client, &wallet, &asset_pair, 4); // 10..40

    // A limit far larger than what exists (and above MAX_HISTORY_DEPTH) is
    // clamped and simply returns everything from the offset to the oldest.
    let page = client.get_score_history_paginated(&wallet, &asset_pair, &0, &u32::MAX);
    assert_eq!(page.len(), 4);
    assert_eq!(page.get(0).unwrap().score, 40); // newest
    assert_eq!(page.get(3).unwrap().score, 10); // oldest
}

#[test]
fn test_paginated_window_straddles_end_truncates() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    seed_scores(&env, &client, &wallet, &asset_pair, 3); // 10, 20, 30

    // offset 1 leaves only two entries (20, 30); a limit of 5 returns just
    // those two rather than padding or erroring.
    let page = client.get_score_history_paginated(&wallet, &asset_pair, &1, &5);
    assert_eq!(page.len(), 2);
    assert_eq!(page.get(0).unwrap().score, 20);
    assert_eq!(page.get(1).unwrap().score, 10);
}

// ── Read-only guarantee ───────────────────────────────────────────────────────

#[test]
fn test_paginated_does_not_mutate_ring() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    seed_scores(&env, &client, &wallet, &asset_pair, 4);

    // Page through it a few different ways…
    let _ = client.get_score_history_paginated(&wallet, &asset_pair, &0, &1);
    let _ = client.get_score_history_paginated(&wallet, &asset_pair, &2, &2);
    let _ = client.get_score_history_paginated(&wallet, &asset_pair, &10, &10);

    // …the full ring is still intact and in oldest-first order.
    let full = client.get_score_history(&wallet, &asset_pair);
    assert_eq!(full.len(), 4);
    assert_eq!(full.get(0).unwrap().score, 10);
    assert_eq!(full.get(3).unwrap().score, 40);
}

#[test]
fn test_paginated_is_per_pair() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair1 = symbol_short!("XLM_USDC");
    let pair2 = symbol_short!("XLM_BTC");

    seed_scores(&env, &client, &wallet, &pair1, 2); // 10, 20
    seed_scores(&env, &client, &wallet, &pair2, 1); // 10

    let p1 = client.get_score_history_paginated(&wallet, &pair1, &0, &10);
    let p2 = client.get_score_history_paginated(&wallet, &pair2, &0, &10);
    assert_eq!(p1.len(), 2);
    assert_eq!(p2.len(), 1);
    assert_eq!(p1.get(0).unwrap().score, 20);
    assert_eq!(p2.get(0).unwrap().score, 10);
}
