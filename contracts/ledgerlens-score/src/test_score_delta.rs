//! Tests for structured score delta events and get_score_trend (issue #51).

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger as _},
    Address, Env, IntoVal, Symbol, Vec,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient};

const START_TS: u64 = 1_700_000_000;
const COOLDOWN: u64 = 3_601; // just past the 1-hour default cooldown

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client)
}

fn submit(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    wallet: &Address,
    pair: &Symbol,
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

/// Returns the last `score_delta` event data tuple for `(wallet, pair)`.
/// Panics if none is found — each submit should produce exactly one.
fn last_delta_event(
    env: &Env,
    contract_id: &Address,
    wallet: &Address,
    pair: &Symbol,
) -> (u32, u32, u32, i32, u32) {
    let topic = (symbol_short!("scr_dlt"), wallet.clone(), pair.clone());
    for (addr, topics, data) in env.events().all().iter().rev() {
        if &addr == contract_id && topics == topic.into_val(env) {
            let (prev, new, abs, trend, consec): (u32, u32, u32, i32, u32) = data.into_val(env);
            return (prev, new, abs, trend, consec);
        }
    }
    panic!("no score_delta event found for this wallet/pair");
}

// ── First submission ─────────────────────────────────────────────────────────

#[test]
fn test_delta_event_on_first_submission() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let contract_id = env.register_contract(None, LedgerLensScoreContract);

    // Use a fresh contract so we can track events from the start.
    let c2 = LedgerLensScoreContractClient::new(&env, &contract_id);
    c2.initialize(&Address::generate(&env), &Address::generate(&env));

    submit(&env, &c2, &wallet, &pair, 50);

    let (prev, new, abs, trend, consec) = last_delta_event(&env, &contract_id, &wallet, &pair);
    assert_eq!(prev, 0, "first submission: previous_score = 0 (no prior score)");
    assert_eq!(new, 50);
    assert_eq!(abs, 0, "first submission: delta_abs = 0 (no prior to compare)");
    assert_eq!(trend, 0, "first submission: no trend yet");
    assert_eq!(consec, 0, "first submission: consecutive = 0");

    // get_score_trend after first submission.
    let t = c2.get_score_trend(&wallet, &pair);
    assert_eq!(t.trend, 0);
    assert_eq!(t.consecutive, 0);

    // suppress unused-variable warning
    let _ = client;
}

// ── Rising trend ─────────────────────────────────────────────────────────────

#[test]
fn test_delta_event_rising_trend() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let c2 = LedgerLensScoreContractClient::new(&env, &contract_id);
    c2.initialize(&Address::generate(&env), &Address::generate(&env));

    submit(&env, &c2, &wallet, &pair, 30); // first
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
    submit(&env, &c2, &wallet, &pair, 50); // rising

    let (prev, new, abs, trend, consec) = last_delta_event(&env, &contract_id, &wallet, &pair);
    assert_eq!(prev, 30);
    assert_eq!(new, 50);
    assert_eq!(abs, 20);
    assert_eq!(trend, 1);
    assert_eq!(consec, 1);

    let _ = client;
}

// ── Falling trend ────────────────────────────────────────────────────────────

#[test]
fn test_delta_event_falling_trend() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let c2 = LedgerLensScoreContractClient::new(&env, &contract_id);
    c2.initialize(&Address::generate(&env), &Address::generate(&env));

    submit(&env, &c2, &wallet, &pair, 80);
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
    submit(&env, &c2, &wallet, &pair, 60);

    let (prev, new, abs, trend, consec) = last_delta_event(&env, &contract_id, &wallet, &pair);
    assert_eq!(prev, 80);
    assert_eq!(new, 60);
    assert_eq!(abs, 20);
    assert_eq!(trend, -1);
    assert_eq!(consec, 1);

    let _ = client;
}

// ── Consecutive count increments on same direction ───────────────────────────

#[test]
fn test_delta_consecutive_count_three_rising() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let c2 = LedgerLensScoreContractClient::new(&env, &contract_id);
    c2.initialize(&Address::generate(&env), &Address::generate(&env));

    submit(&env, &c2, &wallet, &pair, 10);
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
    submit(&env, &c2, &wallet, &pair, 20); // rising, consec=1
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
    submit(&env, &c2, &wallet, &pair, 30); // rising, consec=2
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
    submit(&env, &c2, &wallet, &pair, 40); // rising, consec=3

    let (_, _, _, trend, consec) = last_delta_event(&env, &contract_id, &wallet, &pair);
    assert_eq!(trend, 1);
    assert_eq!(consec, 3);

    let t = c2.get_score_trend(&wallet, &pair);
    assert_eq!(t.trend, 1);
    assert_eq!(t.consecutive, 3);

    let _ = client;
}

// ── Direction change resets consecutive ──────────────────────────────────────

#[test]
fn test_delta_consecutive_count_resets_on_direction_change() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let c2 = LedgerLensScoreContractClient::new(&env, &contract_id);
    c2.initialize(&Address::generate(&env), &Address::generate(&env));

    submit(&env, &c2, &wallet, &pair, 10);
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
    submit(&env, &c2, &wallet, &pair, 30); // rising, consec=1
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
    submit(&env, &c2, &wallet, &pair, 50); // rising, consec=2
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
    submit(&env, &c2, &wallet, &pair, 20); // direction change: falling, consec=1

    let (_, _, _, trend, consec) = last_delta_event(&env, &contract_id, &wallet, &pair);
    assert_eq!(trend, -1);
    assert_eq!(consec, 1, "consecutive resets to 1 on direction change");

    let _ = client;
}

// ── Flat submission resets trend ─────────────────────────────────────────────

#[test]
fn test_delta_flat_submission_resets_trend() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let c2 = LedgerLensScoreContractClient::new(&env, &contract_id);
    c2.initialize(&Address::generate(&env), &Address::generate(&env));

    submit(&env, &c2, &wallet, &pair, 40);
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
    submit(&env, &c2, &wallet, &pair, 60); // rising, consec=1
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
    submit(&env, &c2, &wallet, &pair, 60); // flat

    let (prev, new, abs, trend, consec) = last_delta_event(&env, &contract_id, &wallet, &pair);
    assert_eq!(prev, 60);
    assert_eq!(new, 60);
    assert_eq!(abs, 0);
    assert_eq!(trend, 0, "flat: trend=0");
    assert_eq!(consec, 0, "flat: consecutive=0");

    let t = c2.get_score_trend(&wallet, &pair);
    assert_eq!(t.trend, 0);
    assert_eq!(t.consecutive, 0);

    let _ = client;
}

// ── Delta is per-pair ────────────────────────────────────────────────────────

#[test]
fn test_delta_is_per_pair() {
    let (env, _client) = setup();
    let wallet = Address::generate(&env);
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("XLM_BTC");
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let c2 = LedgerLensScoreContractClient::new(&env, &contract_id);
    c2.initialize(&Address::generate(&env), &Address::generate(&env));

    submit(&env, &c2, &wallet, &pair_a, 10);
    submit(&env, &c2, &wallet, &pair_b, 90);

    // Each pair starts independently at first-submission state.
    let ta = c2.get_score_trend(&wallet, &pair_a);
    let tb = c2.get_score_trend(&wallet, &pair_b);
    assert_eq!(ta.trend, 0);
    assert_eq!(ta.consecutive, 0);
    assert_eq!(tb.trend, 0);
    assert_eq!(tb.consecutive, 0);

    // Second submission on pair_a rises; pair_b is untouched.
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
    submit(&env, &c2, &wallet, &pair_a, 40);

    let ta2 = c2.get_score_trend(&wallet, &pair_a);
    let tb2 = c2.get_score_trend(&wallet, &pair_b);
    assert_eq!(ta2.trend, 1);
    assert_eq!(ta2.consecutive, 1);
    assert_eq!(tb2.trend, 0, "pair_b trend must not be affected by pair_a");
    assert_eq!(tb2.consecutive, 0);
}

// ── get_score_trend with no history returns (0, 0) ───────────────────────────

#[test]
fn test_get_score_trend_no_history_returns_zero() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let t = client.get_score_trend(&wallet, &pair);
    assert_eq!(t.trend, 0);
    assert_eq!(t.consecutive, 0);
}
