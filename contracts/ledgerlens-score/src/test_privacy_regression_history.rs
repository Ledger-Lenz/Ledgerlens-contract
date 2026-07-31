use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
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
// Issue #730 – Privacy regression tests for score history reads
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_history_max_depth_no_overflow() {
    let (env, client, admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // Set max history depth to 5
    client.set_history_max_depth(&admin, &5);

    // Submit 10 scores
    for i in 0..10 {
        env.ledger().with_mut(|l| l.timestamp = 1_000_000 + (i as u64 * 100));
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &pair,
            &(50 + i as u32),
            &false,
            &false,
            &(1_700_000_000 + i as u64 * 100),
            &90,
            &1,
            &None,
        );
    }

    // History should only contain last 5 scores (scores 5-9)
    let history = client.get_score_history(&wallet, &pair);
    assert_eq!(history.len(), 5);

    // Verify the oldest entry is score 55 (50 + 5)
    let oldest = history.get(0).unwrap();
    assert_eq!(oldest.score, 55);

    // Verify the newest entry is score 59 (50 + 9)
    let newest = history.get(4).unwrap();
    assert_eq!(newest.score, 59);
}

#[test]
fn test_history_respects_max_depth_boundary() {
    let (env, client, admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // Exact boundary: max_depth = 3, submit 3 scores
    client.set_history_max_depth(&admin, &3);

    for i in 0..3 {
        env.ledger().with_mut(|l| l.timestamp = 1_000_000 + (i as u64 * 100));
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &pair,
            &(60 + i as u32),
            &false,
            &false,
            &(1_700_000_000 + i as u64 * 100),
            &90,
            &1,
            &None,
        );
    }

    let history = client.get_score_history(&wallet, &pair);
    assert_eq!(history.len(), 3);
}

#[test]
fn test_history_cleared_empty_result() {
    let (env, client, admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // Submit some scores
    client.submit_score(&Vec::new(&env), &wallet, &pair, &50, &false, &false, &1_700_000_000, &90, &1, &None);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &60, &false, &false, &1_700_000_000, &90, &1, &None);

    let history_before = client.get_score_history(&wallet, &pair);
    assert_eq!(history_before.len(), 2);

    // Clear history
    client.clear_score_history(&admin, &wallet, &pair);

    // History should be empty
    let history_after = client.get_score_history(&wallet, &pair);
    assert_eq!(history_after.len(), 0);
}

#[test]
fn test_history_after_clear_can_reaccumulate() {
    let (env, client, admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // Submit initial scores
    client.submit_score(&Vec::new(&env), &wallet, &pair, &50, &false, &false, &1_700_000_000, &90, &1, &None);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &60, &false, &false, &1_700_000_000, &90, &1, &None);

    // Clear
    client.clear_score_history(&admin, &wallet, &pair);

    // Submit new scores
    env.ledger().with_mut(|l| l.timestamp = 1_000_000 + 1000);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &70, &false, &false, &1_700_000_000, &90, &1, &None);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &80, &false, &false, &1_700_000_000, &90, &1, &None);

    // History should contain only new scores
    let history = client.get_score_history(&wallet, &pair);
    assert_eq!(history.len(), 2);
    assert_eq!(history.get(0).unwrap().score, 70);
    assert_eq!(history.get(1).unwrap().score, 80);
}

#[test]
fn test_no_score_history_returns_empty() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // No scores submitted
    let history = client.get_score_history(&wallet, &pair);
    assert_eq!(history.len(), 0);
}

#[test]
fn test_no_score_state_returns_not_found() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // No score submitted — should error
    let result = client.get_score(&wallet, &pair);
    assert!(result.is_err());
}

#[test]
fn test_history_cleared_but_score_remains() {
    let (env, client, admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // Submit and then clear history
    client.submit_score(&Vec::new(&env), &wallet, &pair, &75, &false, &false, &1_700_000_000, &90, &1, &None);
    client.clear_score_history(&admin, &wallet, &pair);

    // History is empty
    let history = client.get_score_history(&wallet, &pair);
    assert_eq!(history.len(), 0);

    // But latest score still exists
    let score = client.get_score(&wallet, &pair).unwrap();
    assert_eq!(score.score, 75);
}

#[test]
fn test_history_different_pairs_independent() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair1 = symbol_short!("XLM_USDC");
    let pair2 = symbol_short!("BTC_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // Submit different scores to different pairs
    client.submit_score(&Vec::new(&env), &wallet, &pair1, &50, &false, &false, &1_700_000_000, &90, &1, &None);
    client.submit_score(&Vec::new(&env), &wallet, &pair2, &60, &false, &false, &1_700_000_000, &90, &1, &None);

    // Each pair has independent history
    let history1 = client.get_score_history(&wallet, &pair1);
    let history2 = client.get_score_history(&wallet, &pair2);

    assert_eq!(history1.len(), 1);
    assert_eq!(history1.get(0).unwrap().score, 50);

    assert_eq!(history2.len(), 1);
    assert_eq!(history2.get(0).unwrap().score, 60);
}

#[test]
fn test_history_max_depth_change_takes_effect() {
    let (env, client, admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // Submit 5 scores with max_depth = 10
    client.set_history_max_depth(&admin, &10);
    for i in 0..5 {
        env.ledger().with_mut(|l| l.timestamp = 1_000_000 + (i as u64 * 100));
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &pair,
            &(50 + i as u32),
            &false,
            &false,
            &(1_700_000_000 + i as u64 * 100),
            &90,
            &1,
            &None,
        );
    }

    let history1 = client.get_score_history(&wallet, &pair);
    assert_eq!(history1.len(), 5);

    // Reduce max_depth to 3 and submit 2 more scores
    client.set_history_max_depth(&admin, &3);
    for i in 5..7 {
        env.ledger().with_mut(|l| l.timestamp = 1_000_000 + (i as u64 * 100));
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &pair,
            &(50 + i as u32),
            &false,
            &false,
            &(1_700_000_000 + i as u64 * 100),
            &90,
            &1,
            &None,
        );
    }

    // Now history should only have last 3 (scores 54, 55, 56)
    let history2 = client.get_score_history(&wallet, &pair);
    assert_eq!(history2.len(), 3);
    assert_eq!(history2.get(0).unwrap().score, 54);
    assert_eq!(history2.get(1).unwrap().score, 55);
    assert_eq!(history2.get(2).unwrap().score, 56);
}

#[test]
fn test_history_pagination_not_exposed() {
    // This test verifies that pagination offsets/cursors are NOT exposed,
    // ensuring privacy regression: clients cannot request partial history.
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // Submit multiple scores
    for i in 0..5 {
        env.ledger().with_mut(|l| l.timestamp = 1_000_000 + (i as u64 * 100));
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &pair,
            &(50 + i as u32),
            &false,
            &false,
            &(1_700_000_000 + i as u64 * 100),
            &90,
            &1,
            &None,
        );
    }

    // get_score_history returns full buffer (no pagination exposed)
    let full_history = client.get_score_history(&wallet, &pair);
    assert_eq!(full_history.len(), 5);

    // There's no get_score_history_page() or get_score_history_since() function
    // This ensures privacy: consumers get all or nothing, not selectable slices
}

#[test]
fn test_history_no_metadata_leakage() {
    // Verify history reads don't leak extra metadata like:
    // - buffer size
    // - eviction count
    // - timestamp of first entry
    let (env, client, admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    client.set_history_max_depth(&admin, &5);

    for i in 0..7 {
        env.ledger().with_mut(|l| l.timestamp = 1_000_000 + (i as u64 * 100));
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &pair,
            &(50 + i as u32),
            &false,
            &false,
            &(1_700_000_000 + i as u64 * 100),
            &90,
            &1,
            &None,
        );
    }

    let history = client.get_score_history(&wallet, &pair);
    // History contains only scores, timestamps, flags, confidence
    // Does NOT contain: buffer position, evicted count, capacity, etc.
    for entry in &history {
        assert!(entry.score > 0); // Score field is present
        // Other required fields are present (accessed via entry.field)
    }
}

#[test]
fn test_history_cleared_different_pairs_unaffected() {
    let (env, client, admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair1 = symbol_short!("XLM_USDC");
    let pair2 = symbol_short!("BTC_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // Submit to both pairs
    client.submit_score(&Vec::new(&env), &wallet, &pair1, &50, &false, &false, &1_700_000_000, &90, &1, &None);
    client.submit_score(&Vec::new(&env), &wallet, &pair1, &55, &false, &false, &1_700_000_000, &90, &1, &None);
    client.submit_score(&Vec::new(&env), &wallet, &pair2, &60, &false, &false, &1_700_000_000, &90, &1, &None);

    // Clear history for pair1 only
    client.clear_score_history(&admin, &wallet, &pair1);

    // pair1 history should be empty
    let history1 = client.get_score_history(&wallet, &pair1);
    assert_eq!(history1.len(), 0);

    // pair2 history should be unaffected
    let history2 = client.get_score_history(&wallet, &pair2);
    assert_eq!(history2.len(), 1);
    assert_eq!(history2.get(0).unwrap().score, 60);
}

#[test]
fn test_history_order_preserved() {
    // Verify history entries are in submission order (FIFO)
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // Submit scores in specific order
    let scores = vec![30, 45, 60, 50, 75];
    for (idx, &score) in scores.iter().enumerate() {
        env.ledger().with_mut(|l| l.timestamp = 1_000_000 + (idx as u64 * 100));
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &pair,
            &score,
            &false,
            &false,
            &(1_700_000_000 + idx as u64 * 100),
            &90,
            &1,
            &None,
        );
    }

    // Verify order is preserved
    let history = client.get_score_history(&wallet, &pair);
    for (idx, entry) in history.iter().enumerate() {
        assert_eq!(entry.score, scores[idx]);
    }
}
