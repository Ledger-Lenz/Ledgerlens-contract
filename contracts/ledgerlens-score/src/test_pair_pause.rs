//! Tests for the per-asset-pair circuit breaker (`set_pair_paused` /
//! `is_pair_paused` / `get_paused_pairs`).

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, Symbol, TryFromVal, Vec,
};

use crate::{
    constants::MAX_PAUSED_PAIRS, BatchResult, Error, LedgerLensScoreContract,
    LedgerLensScoreContractClient, ScoreSubmission,
};

/// Ledger timestamp the tests start from (an arbitrary fixed instant).
const START_TS: u64 = 1_700_000_000;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    (env, client, admin)
}

/// Builds a short, distinct `Symbol` ("P0".."P49") at runtime without
/// `alloc`/`format!`, which aren't available under this crate's
/// `#![no_std]`. Used only to generate `MAX_PAUSED_PAIRS` distinct pairs.
fn pair_symbol(env: &Env, i: u32) -> Symbol {
    let mut buf = [0u8; 3];
    buf[0] = b'P';
    if i < 10 {
        buf[1] = b'0' + i as u8;
        Symbol::new(env, core::str::from_utf8(&buf[..2]).unwrap())
    } else {
        buf[1] = b'0' + (i / 10) as u8;
        buf[2] = b'0' + (i % 10) as u8;
        Symbol::new(env, core::str::from_utf8(&buf[..3]).unwrap())
    }
}

// ── Core blocking / unblocking behaviour ───────────────────────────────────────

#[test]
fn test_pair_pause_blocks_submit_score() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_pair_paused(&pair, &true);

    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::PairPaused)));
}

#[test]
fn test_pair_pause_allows_read() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &true,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );

    client.set_pair_paused(&pair, &true);

    // get_score
    let score = client.get_score(&wallet, &pair);
    assert_eq!(score.score, 50);

    // get_score_history
    let history = client.get_score_history(&wallet, &pair);
    assert_eq!(history.len(), 1);
    assert_eq!(history.get(0).unwrap().score, 50);

    // query_risk_gate — score (50) < gate_threshold (100), so the wallet
    // still passes the gate despite the pair being paused.
    assert!(client.query_risk_gate(&wallet, &pair, &100));

    // get_aggregate_score — also unaffected by the per-pair pause.
    let aggregate = client.get_aggregate_score(&wallet);
    assert_eq!(aggregate.aggregate_score, 50);
}

#[test]
fn test_pair_unpause_restores_submissions() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_pair_paused(&pair, &true);
    let blocked = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );
    assert_eq!(blocked, Err(Ok(Error::PairPaused)));

    client.set_pair_paused(&pair, &false);
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );
    assert_eq!(client.get_score(&wallet, &pair).score, 50);
}

// ── Batch submission ──────────────────────────────────────────────────────────

#[test]
fn test_batch_pair_paused_entry_skipped() {
    let (env, client, _admin) = setup();
    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);
    let paused_pair = symbol_short!("XLM_USDC");
    let live_pair = symbol_short!("XLM_BTC");

    client.set_pair_paused(&paused_pair, &true);

    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: wallet_a.clone(),
        asset_pair: paused_pair.clone(),
        score: 50,
        benford_flag: false,
        ml_flag: false,
        timestamp: START_TS,
        confidence: 90,
        model_version: 1,
    });
    batch.push_back(ScoreSubmission {
        wallet: wallet_b.clone(),
        asset_pair: live_pair.clone(),
        score: 40,
        benford_flag: false,
        ml_flag: false,
        timestamp: START_TS,
        confidence: 90,
        model_version: 1,
    });

    let result: BatchResult = client.submit_scores_batch(&batch);
    assert_eq!(result.accepted_count, 1);
    assert_eq!(result.rejected_count, 1);

    // First entry — rejected, targeting the paused pair.
    assert!(!result.results.get(0).unwrap().accepted);
    assert_eq!(result.results.get(0).unwrap().rejection_code, Error::PairPaused as u32);

    // Second entry — accepted, targeting the live pair.
    assert!(result.results.get(1).unwrap().accepted);
    assert_eq!(result.results.get(1).unwrap().rejection_code, 0);

    // The paused pair's entry was never written.
    let not_found = client.try_get_score(&wallet_a, &paused_pair);
    assert_eq!(not_found, Err(Ok(Error::ScoreNotFound)));
    assert_eq!(client.get_score(&wallet_b, &live_pair).score, 40);
}

// ── Precedence ─────────────────────────────────────────────────────────────────

#[test]
fn test_global_pause_takes_precedence_over_pair_pause() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_pair_paused(&pair, &true);
    client.pause();

    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::ContractPaused)));
}

// ── Defaults ───────────────────────────────────────────────────────────────────

#[test]
fn test_is_pair_paused_false_by_default() {
    let (_env, client, _admin) = setup();
    let pair = symbol_short!("XLM_USDC");
    assert!(!client.is_pair_paused(&pair));
}

#[test]
fn test_get_paused_pairs_empty_by_default() {
    let (_env, client, _admin) = setup();
    assert!(client.get_paused_pairs().is_empty());
}

// ── Admin gating ───────────────────────────────────────────────────────────────

#[test]
#[should_panic]
fn test_set_pair_paused_requires_admin() {
    let env = Env::default();
    // Deliberately no `env.mock_all_auths()` here: `initialize` never calls
    // `require_auth`, so it still succeeds, but `set_pair_paused`'s
    // `admin.require_auth()` has nothing to authorize against and must panic.
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    let pair = symbol_short!("XLM_USDC");
    client.set_pair_paused(&pair, &true);
}

// ── Index management ───────────────────────────────────────────────────────────

#[test]
fn test_get_paused_pairs_reflects_state() {
    let (_env, client, _admin) = setup();
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("XLM_BTC");
    let pair_c = symbol_short!("XLM_EURC");

    client.set_pair_paused(&pair_a, &true);
    client.set_pair_paused(&pair_b, &true);
    client.set_pair_paused(&pair_c, &true);

    let paused = client.get_paused_pairs();
    assert_eq!(paused.len(), 3);
    assert!(paused.contains(&pair_a));
    assert!(paused.contains(&pair_b));
    assert!(paused.contains(&pair_c));

    client.set_pair_paused(&pair_b, &false);

    let paused = client.get_paused_pairs();
    assert_eq!(paused.len(), 2);
    assert!(paused.contains(&pair_a));
    assert!(!paused.contains(&pair_b));
    assert!(paused.contains(&pair_c));
}

#[test]
fn test_paused_pair_index_full_rejected() {
    let (env, client, _admin) = setup();

    for i in 0..MAX_PAUSED_PAIRS {
        client.set_pair_paused(&pair_symbol(&env, i), &true);
    }
    assert_eq!(client.get_paused_pairs().len(), MAX_PAUSED_PAIRS);

    let overflow_pair = Symbol::new(&env, "OVERFLOW");
    let result = client.try_set_pair_paused(&overflow_pair, &true);
    assert_eq!(result, Err(Ok(Error::PausedPairIndexFull)));

    // The index is unaffected by the rejected attempt.
    assert_eq!(client.get_paused_pairs().len(), MAX_PAUSED_PAIRS);
    assert!(!client.is_pair_paused(&overflow_pair));
}

#[test]
fn test_paused_pair_index_full_does_not_block_repause_or_unpause() {
    let (env, client, _admin) = setup();

    for i in 0..MAX_PAUSED_PAIRS {
        client.set_pair_paused(&pair_symbol(&env, i), &true);
    }

    // Re-pausing an already-paused pair is a no-op on the index, not a
    // capacity error, even though the index is at capacity.
    let already_paused = pair_symbol(&env, 0);
    client.set_pair_paused(&already_paused, &true);
    assert_eq!(client.get_paused_pairs().len(), MAX_PAUSED_PAIRS);

    // Unpausing always succeeds and frees a slot.
    client.set_pair_paused(&already_paused, &false);
    assert_eq!(client.get_paused_pairs().len(), MAX_PAUSED_PAIRS - 1);

    let overflow_pair = Symbol::new(&env, "OVERFLOW");
    client.set_pair_paused(&overflow_pair, &true);
    assert_eq!(client.get_paused_pairs().len(), MAX_PAUSED_PAIRS);
}

// ── Events ─────────────────────────────────────────────────────────────────────

#[test]
fn test_pair_paused_event_emitted() {
    let (env, client, _admin) = setup();
    let pair = symbol_short!("XLM_USDC");

    client.set_pair_paused(&pair, &true);

    let events = env.events().all();
    let (_contract_id, topics, data) = events.get(events.len() - 1);
    assert_eq!(
        topics,
        Vec::from_array(
            &env,
            [symbol_short!("pr_pause").into_val(&env), pair.clone().into_val(&env)]
        )
    );
    assert!(bool::try_from_val(&env, &data).unwrap());

    client.set_pair_paused(&pair, &false);
    let events = env.events().all();
    let (_contract_id, topics, data) = events.get(events.len() - 1);
    assert_eq!(
        topics,
        Vec::from_array(
            &env,
            [symbol_short!("pr_pause").into_val(&env), pair.clone().into_val(&env)]
        )
    );
    assert!(!bool::try_from_val(&env, &data).unwrap());
}
