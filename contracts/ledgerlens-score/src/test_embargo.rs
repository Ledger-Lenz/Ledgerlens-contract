#![cfg(test)]
//! Tests for the per-wallet score embargo (regulatory hold).

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{
    constants::MAX_EMBARGOED_WALLETS, Error, LedgerLensScoreContract,
    LedgerLensScoreContractClient, ScoreSubmission,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

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
        &(env.ledger().timestamp().max(1)),
        &80,
        &1,
        &None,
    );
    env.ledger().with_mut(|l| l.timestamp += 3_601);
}

// ── is_embargoed defaults ─────────────────────────────────────────────────────

#[test]
fn test_is_embargoed_false_by_default() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    assert!(!client.is_embargoed(&wallet));
}

// ── set_score_embargo / lift_score_embargo ────────────────────────────────────

#[test]
fn test_set_indefinite_embargo() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    client.set_score_embargo(&wallet, &None);
    assert!(client.is_embargoed(&wallet));
}

#[test]
fn test_set_timed_embargo_active_within_window() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    // Embargo expires at ledger_ts = 10_000; current ts = 1.
    client.set_score_embargo(&wallet, &Some(10_000));
    assert!(client.is_embargoed(&wallet));
}

#[test]
fn test_set_timed_embargo_expired() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    // Embargo already expired at the time of creation (expiry in the past).
    env.ledger().with_mut(|l| l.timestamp = 5_000);
    client.set_score_embargo(&wallet, &Some(4_999));
    assert!(!client.is_embargoed(&wallet));
}

#[test]
fn test_timed_embargo_auto_expires() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    client.set_score_embargo(&wallet, &Some(500));
    // Before expiry.
    env.ledger().with_mut(|l| l.timestamp = 500);
    assert!(client.is_embargoed(&wallet));
    // After expiry.
    env.ledger().with_mut(|l| l.timestamp = 501);
    assert!(!client.is_embargoed(&wallet));
}

#[test]
fn test_lift_embargo_removes_it() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    client.set_score_embargo(&wallet, &None);
    assert!(client.is_embargoed(&wallet));
    client.lift_score_embargo(&wallet);
    assert!(!client.is_embargoed(&wallet));
}

#[test]
fn test_lift_embargo_noop_when_not_embargoed() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    // lift on a wallet that has no embargo — should not panic or error.
    client.lift_score_embargo(&wallet);
    assert!(!client.is_embargoed(&wallet));
}

#[test]
fn test_replacing_embargo_updates_expiry() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    // Start with a timed embargo that would expire at ts=100.
    client.set_score_embargo(&wallet, &Some(100));
    // Replace with an indefinite embargo.
    client.set_score_embargo(&wallet, &None);
    // Now advance past the original expiry; embargo must still be active.
    env.ledger().with_mut(|l| l.timestamp = 200);
    assert!(client.is_embargoed(&wallet));
}

// ── get_embargo_expiry ────────────────────────────────────────────────────────

#[test]
fn test_get_embargo_expiry_none_when_not_embargoed() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    assert_eq!(client.get_embargo_expiry(&wallet), None);
}

#[test]
fn test_get_embargo_expiry_none_for_indefinite_embargo() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    client.set_score_embargo(&wallet, &None);
    assert_eq!(client.get_embargo_expiry(&wallet), None);
}

#[test]
fn test_get_embargo_expiry_some_for_active_timed_embargo() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    client.set_score_embargo(&wallet, &Some(10_000));
    assert_eq!(client.get_embargo_expiry(&wallet), Some(10_000));
}

#[test]
fn test_get_embargo_expiry_none_after_timed_embargo_expires() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    client.set_score_embargo(&wallet, &Some(500));
    env.ledger().with_mut(|l| l.timestamp = 501);
    assert_eq!(client.get_embargo_expiry(&wallet), None);
}

#[test]
fn test_get_embargo_expiry_none_after_lift() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    client.set_score_embargo(&wallet, &Some(10_000));
    client.lift_score_embargo(&wallet);
    assert_eq!(client.get_embargo_expiry(&wallet), None);
}

#[test]
fn test_get_embargo_expiry_updates_after_replacing_embargo() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    client.set_score_embargo(&wallet, &Some(100));
    assert_eq!(client.get_embargo_expiry(&wallet), Some(100));
    client.set_score_embargo(&wallet, &Some(200));
    assert_eq!(client.get_embargo_expiry(&wallet), Some(200));
}

// ── get_score blocked by embargo ──────────────────────────────────────────────

#[test]
fn test_get_score_embargoed_returns_error() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 42);
    client.set_score_embargo(&wallet, &None);
    let result = client.try_get_score(&wallet, &pair);
    assert_eq!(result, Err(Ok(Error::ScoreEmbargoed)));
}

#[test]
fn test_get_score_not_found_when_no_score_and_embargoed() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    // No score submitted — embargo still returns ScoreEmbargoed, not ScoreNotFound.
    client.set_score_embargo(&wallet, &None);
    let result = client.try_get_score(&wallet, &pair);
    assert_eq!(result, Err(Ok(Error::ScoreEmbargoed)));
}

#[test]
fn test_get_score_available_after_lift() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 42);
    client.set_score_embargo(&wallet, &None);
    client.lift_score_embargo(&wallet);
    let score = client.get_score(&wallet, &pair);
    assert_eq!(score.score, 42);
}

#[test]
fn test_get_score_available_after_timed_expiry() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 77);
    client.set_score_embargo(&wallet, &Some(10_000));
    // Score is blocked while embargo is active.
    assert_eq!(client.try_get_score(&wallet, &pair), Err(Ok(Error::ScoreEmbargoed)));
    // Advance past expiry.
    env.ledger().with_mut(|l| l.timestamp = 10_001);
    let score = client.get_score(&wallet, &pair);
    assert_eq!(score.score, 77);
}

// ── get_aggregate_score blocked by embargo ────────────────────────────────────

#[test]
fn test_get_aggregate_score_embargoed() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 50);
    client.set_score_embargo(&wallet, &None);
    let result = client.try_get_aggregate_score(&wallet);
    assert_eq!(result, Err(Ok(Error::ScoreEmbargoed)));
}

#[test]
fn test_get_aggregate_score_available_after_lift() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 50);
    client.set_score_embargo(&wallet, &None);
    client.lift_score_embargo(&wallet);
    let agg = client.get_aggregate_score(&wallet);
    assert_eq!(agg.aggregate_score, 50);
}

// ── get_score_history returns empty under embargo ─────────────────────────────

#[test]
fn test_get_score_history_empty_when_embargoed() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 10);
    submit(&env, &client, &wallet, &pair, 20);
    client.set_score_embargo(&wallet, &None);
    let history = client.get_score_history(&wallet, &pair);
    assert_eq!(history.len(), 0);
}

#[test]
fn test_get_score_history_restored_after_lift() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 10);
    client.set_score_embargo(&wallet, &None);
    client.lift_score_embargo(&wallet);
    let history = client.get_score_history(&wallet, &pair);
    assert_eq!(history.len(), 1);
    assert_eq!(history.get(0).unwrap().score, 10);
}

// ── query_risk_gate returns false conservatively under embargo ─────────────────

#[test]
fn test_query_risk_gate_false_when_embargoed() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    // Submit a very low-risk score that would normally pass the gate.
    submit(&env, &client, &wallet, &pair, 5);
    assert!(client.query_risk_gate(&wallet, &pair, &75));
    // Place embargo — gate must now return false regardless of score.
    client.set_score_embargo(&wallet, &None);
    assert!(!client.query_risk_gate(&wallet, &pair, &75));
}

#[test]
fn test_query_risk_gate_restored_after_lift() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 5);
    client.set_score_embargo(&wallet, &None);
    assert!(!client.query_risk_gate(&wallet, &pair, &75));
    client.lift_score_embargo(&wallet);
    assert!(client.query_risk_gate(&wallet, &pair, &75));
}

#[test]
fn test_query_risk_gate_false_when_embargoed_and_no_score() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    // No score at all, embargo placed.
    client.set_score_embargo(&wallet, &None);
    assert!(!client.query_risk_gate(&wallet, &pair, &75));
}

// ── submit_score / submit_scores_batch unaffected by embargo ──────────────────

#[test]
fn test_submit_score_unaffected_by_embargo() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    client.set_score_embargo(&wallet, &None);
    // Ingestion must still succeed even while embargoed.
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &55,
        &false,
        &false,
        &(env.ledger().timestamp().max(1)),
        &80,
        &1,
        &None,
    );
}

#[test]
fn test_submit_scores_batch_unaffected_by_embargo() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    client.set_score_embargo(&wallet, &None);
    let ts = env.ledger().timestamp().max(1);
    let mut entries = Vec::new(&env);
    entries.push_back(ScoreSubmission {
        wallet: wallet.clone(),
        asset_pair: pair.clone(),
        score: 60,
        benford_flag: false,
        ml_flag: false,
        timestamp: ts,
        confidence: 80,
        model_version: 1,
    });
    let result = client.submit_scores_batch(&entries);
    assert_eq!(result.accepted_count, 1);
}

// ── Per-wallet isolation ───────────────────────────────────────────────────────

#[test]
fn test_embargo_is_per_wallet() {
    let (env, client, _admin, _service) = setup();
    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet_a, &pair, 30);
    submit(&env, &client, &wallet_b, &pair, 40);
    // Only embargo wallet_a.
    client.set_score_embargo(&wallet_a, &None);
    assert!(client.is_embargoed(&wallet_a));
    assert!(!client.is_embargoed(&wallet_b));
    // wallet_b score access unaffected.
    let score_b = client.get_score(&wallet_b, &pair);
    assert_eq!(score_b.score, 40);
    // wallet_a score blocked.
    assert_eq!(client.try_get_score(&wallet_a, &pair), Err(Ok(Error::ScoreEmbargoed)));
}

// ── Authorization: only admin can set/lift embargo ────────────────────────────

#[test]
fn test_set_embargo_requires_init() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let wallet = Address::generate(&env);
    let r = client.try_set_score_embargo(&wallet, &None);
    assert_eq!(r, Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_lift_embargo_requires_init() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let wallet = Address::generate(&env);
    let r = client.try_lift_score_embargo(&wallet);
    assert_eq!(r, Err(Ok(Error::NotInitialized)));
}

// ── Snapshot sequence ─────────────────────────────────────────────────────────

#[test]
fn test_full_embargo_lifecycle() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // 1. Score accessible before embargo.
    submit(&env, &client, &wallet, &pair, 30);
    assert_eq!(client.get_score(&wallet, &pair).score, 30);
    assert!(!client.is_embargoed(&wallet));

    // 2. Set indefinite embargo — score blocked.
    client.set_score_embargo(&wallet, &None);
    assert!(client.is_embargoed(&wallet));
    assert_eq!(client.try_get_score(&wallet, &pair), Err(Ok(Error::ScoreEmbargoed)));
    assert_eq!(client.get_score_history(&wallet, &pair).len(), 0);
    assert!(!client.query_risk_gate(&wallet, &pair, &75));

    // 3. Ingestion still works while embargoed.
    submit(&env, &client, &wallet, &pair, 55);

    // 4. Lift embargo — access restored, updated score visible.
    client.lift_score_embargo(&wallet);
    assert!(!client.is_embargoed(&wallet));
    let score = client.get_score(&wallet, &pair);
    assert_eq!(score.score, 55);
    assert!(client.get_score_history(&wallet, &pair).len() > 0);

    // 5. Low-risk score passes gate normally.
    assert!(client.query_risk_gate(&wallet, &pair, &75));
}

// ── EmbargoedWalletIndex / revoke_all_embargoes ────────────────────────────────

#[test]
fn test_embargoed_wallet_count_zero_by_default() {
    let (_env, client, _admin, _service) = setup();
    assert_eq!(client.get_embargoed_wallet_count(), 0);
}

#[test]
fn test_set_score_embargo_adds_to_index() {
    let (env, client, _admin, _service) = setup();
    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);

    client.set_score_embargo(&wallet_a, &None);
    assert_eq!(client.get_embargoed_wallet_count(), 1);

    client.set_score_embargo(&wallet_b, &Some(10_000));
    assert_eq!(client.get_embargoed_wallet_count(), 2);

    // Re-embargoing an already-embargoed wallet must not duplicate the entry.
    client.set_score_embargo(&wallet_a, &Some(20_000));
    assert_eq!(client.get_embargoed_wallet_count(), 2);
}

#[test]
fn test_lift_score_embargo_removes_from_index() {
    let (env, client, _admin, _service) = setup();
    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);
    client.set_score_embargo(&wallet_a, &None);
    client.set_score_embargo(&wallet_b, &None);
    assert_eq!(client.get_embargoed_wallet_count(), 2);

    client.lift_score_embargo(&wallet_a);
    assert_eq!(client.get_embargoed_wallet_count(), 1);

    // Lifting a wallet that was never embargoed is a no-op on the index.
    client.lift_score_embargo(&Address::generate(&env));
    assert_eq!(client.get_embargoed_wallet_count(), 1);
}

#[test]
fn test_revoke_all_embargoes_lifts_all_and_clears_index() {
    let (env, client, _admin, _service) = setup();
    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);
    let wallet_c = Address::generate(&env);

    client.set_score_embargo(&wallet_a, &None);
    client.set_score_embargo(&wallet_b, &None);
    client.set_score_embargo(&wallet_c, &Some(10_000));
    assert_eq!(client.get_embargoed_wallet_count(), 3);

    client.revoke_all_embargoes(&Vec::new(&env));

    assert_eq!(client.get_embargoed_wallet_count(), 0);
    assert!(!client.is_embargoed(&wallet_a));
    assert!(!client.is_embargoed(&wallet_b));
    assert!(!client.is_embargoed(&wallet_c));
}

#[test]
fn test_revoke_all_embargoes_noop_when_index_empty() {
    let (env, client, _admin, _service) = setup();
    client.revoke_all_embargoes(&Vec::new(&env));
    assert_eq!(client.get_embargoed_wallet_count(), 0);
}

#[test]
fn test_revoke_all_embargoes_also_clears_already_expired_timed_entries() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    // Timed embargo that will auto-expire without an explicit lift.
    client.set_score_embargo(&wallet, &Some(100));
    env.ledger().with_mut(|l| l.timestamp = 200);
    assert!(!client.is_embargoed(&wallet));
    // Still tracked in the index because it was never explicitly lifted.
    assert_eq!(client.get_embargoed_wallet_count(), 1);

    client.revoke_all_embargoes(&Vec::new(&env));
    assert_eq!(client.get_embargoed_wallet_count(), 0);
}

#[test]
fn test_revoke_all_embargoes_requires_init() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let r = client.try_revoke_all_embargoes(&Vec::new(&env));
    assert_eq!(r, Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_embargoed_wallet_index_full_rejected() {
    let (env, client, _admin, _service) = setup();

    for _ in 0..MAX_EMBARGOED_WALLETS {
        client.set_score_embargo(&Address::generate(&env), &None);
    }
    assert_eq!(client.get_embargoed_wallet_count(), MAX_EMBARGOED_WALLETS);

    let overflow_wallet = Address::generate(&env);
    let result = client.try_set_score_embargo(&overflow_wallet, &None);
    assert_eq!(result, Err(Ok(Error::EmbargoedWalletIndexFull)));
    assert!(!client.is_embargoed(&overflow_wallet));
    assert_eq!(client.get_embargoed_wallet_count(), MAX_EMBARGOED_WALLETS);
}

#[test]
fn test_embargoed_wallet_index_full_does_not_block_revoke_all() {
    let (env, client, _admin, _service) = setup();

    for _ in 0..MAX_EMBARGOED_WALLETS {
        client.set_score_embargo(&Address::generate(&env), &None);
    }
    client.revoke_all_embargoes(&Vec::new(&env));
    assert_eq!(client.get_embargoed_wallet_count(), 0);

    // Index has room again after the revoke.
    let wallet = Address::generate(&env);
    client.set_score_embargo(&wallet, &None);
    assert_eq!(client.get_embargoed_wallet_count(), 1);
}

// ── get_active_embargo_count ──────────────────────────────────────────────────

#[test]
fn test_active_embargo_count_zero_before_any_embargo() {
    let (_env, client, _admin, _service) = setup();
    assert_eq!(client.get_active_embargo_count(), 0);
}

#[test]
fn test_active_embargo_count_increments_on_new_embargo() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    client.set_score_embargo(&wallet, &None);
    assert_eq!(client.get_active_embargo_count(), 1);
}

#[test]
fn test_active_embargo_count_increments_for_multiple_wallets() {
    let (env, client, _admin, _service) = setup();
    let w1 = Address::generate(&env);
    let w2 = Address::generate(&env);
    let w3 = Address::generate(&env);
    client.set_score_embargo(&w1, &None);
    client.set_score_embargo(&w2, &Some(9_999_999));
    client.set_score_embargo(&w3, &None);
    assert_eq!(client.get_active_embargo_count(), 3);
}

#[test]
fn test_active_embargo_count_no_double_increment_on_replace() {
    // Re-embargoing an already-embargoed wallet must not increment again.
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    client.set_score_embargo(&wallet, &None);
    client.set_score_embargo(&wallet, &Some(9_999_999));
    assert_eq!(client.get_active_embargo_count(), 1);
}

#[test]
fn test_active_embargo_count_decrements_on_lift() {
    let (env, client, _admin, _service) = setup();
    let w1 = Address::generate(&env);
    let w2 = Address::generate(&env);
    client.set_score_embargo(&w1, &None);
    client.set_score_embargo(&w2, &None);
    client.lift_score_embargo(&w1);
    assert_eq!(client.get_active_embargo_count(), 1);
    client.lift_score_embargo(&w2);
    assert_eq!(client.get_active_embargo_count(), 0);
}

#[test]
fn test_active_embargo_count_lift_noop_when_not_embargoed() {
    // Lifting a wallet that was never embargoed must not underflow.
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    client.lift_score_embargo(&wallet);
    assert_eq!(client.get_active_embargo_count(), 0);
}

#[test]
fn test_active_embargo_count_decrements_on_batch_lift() {
    let (env, client, _admin, _service) = setup();
    let w1 = Address::generate(&env);
    let w2 = Address::generate(&env);
    client.set_score_embargo(&w1, &None);
    client.set_score_embargo(&w2, &None);
    assert_eq!(client.get_active_embargo_count(), 2);

    let mut wallets = Vec::new(&env);
    wallets.push_back(w1);
    wallets.push_back(w2);
    client.batch_lift_score_embargo(&Vec::new(&env), &wallets);
    assert_eq!(client.get_active_embargo_count(), 0);
}

#[test]
fn test_active_embargo_count_batch_lift_skips_non_embargoed() {
    // Only actually-embargoed wallets should decrement the counter.
    let (env, client, _admin, _service) = setup();
    let embargoed = Address::generate(&env);
    let not_embargoed = Address::generate(&env);
    client.set_score_embargo(&embargoed, &None);

    let mut wallets = Vec::new(&env);
    wallets.push_back(embargoed);
    wallets.push_back(not_embargoed);
    let lifted = client.batch_lift_score_embargo(&Vec::new(&env), &wallets);
    assert_eq!(lifted, 1);
    assert_eq!(client.get_active_embargo_count(), 0);
}

#[test]
fn test_active_embargo_count_resets_to_zero_on_revoke_all() {
    let (env, client, _admin, _service) = setup();
    let w1 = Address::generate(&env);
    let w2 = Address::generate(&env);
    client.set_score_embargo(&w1, &None);
    client.set_score_embargo(&w2, &None);
    assert_eq!(client.get_active_embargo_count(), 2);
    client.revoke_all_embargoes(&Vec::new(&env));
    assert_eq!(client.get_active_embargo_count(), 0);
}

#[test]
fn test_active_embargo_count_revoke_all_noop_when_none_set() {
    let (env, client, _admin, _service) = setup();
    client.revoke_all_embargoes(&Vec::new(&env));
    assert_eq!(client.get_active_embargo_count(), 0);
}

#[test]
fn test_active_embargo_count_set_after_revoke_all() {
    // Counter must start clean and be incrementable after a full revoke.
    let (env, client, _admin, _service) = setup();
    let w1 = Address::generate(&env);
    let w2 = Address::generate(&env);
    client.set_score_embargo(&w1, &None);
    client.revoke_all_embargoes(&Vec::new(&env));
    assert_eq!(client.get_active_embargo_count(), 0);
    client.set_score_embargo(&w2, &None);
    assert_eq!(client.get_active_embargo_count(), 1);
}

// ── peek_is_embargoed ─────────────────────────────────────────────────────────

#[test]
fn test_peek_is_embargoed_false_when_no_embargo() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    assert!(!client.peek_is_embargoed(&wallet));
}

#[test]
fn test_peek_is_embargoed_true_for_indefinite_embargo() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    client.set_score_embargo(&wallet, &None);
    assert!(client.peek_is_embargoed(&wallet));
}

#[test]
fn test_peek_is_embargoed_true_for_active_timed_embargo() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    client.set_score_embargo(&wallet, &Some(10_000));
    env.ledger().with_mut(|l| l.timestamp = 5_000);
    assert!(client.peek_is_embargoed(&wallet));
}

#[test]
fn test_peek_is_embargoed_false_after_timed_embargo_expires() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    client.set_score_embargo(&wallet, &Some(500));
    env.ledger().with_mut(|l| l.timestamp = 501);
    assert!(!client.peek_is_embargoed(&wallet));
}

#[test]
fn test_peek_is_embargoed_does_not_extend_ttl() {
    // Calling peek_is_embargoed must not change the logical expiry of the embargo.
    // We verify this by checking that a timed embargo still expires at the
    // originally-set timestamp even after multiple peek calls.
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let expiry: u64 = 500;
    client.set_score_embargo(&wallet, &Some(expiry));

    // Peek multiple times — none of these should extend the embargo.
    env.ledger().with_mut(|l| l.timestamp = 400);
    assert!(client.peek_is_embargoed(&wallet));
    assert!(client.peek_is_embargoed(&wallet));
    assert!(client.peek_is_embargoed(&wallet));

    // After the original expiry the embargo must be gone.
    env.ledger().with_mut(|l| l.timestamp = expiry + 1);
    assert!(!client.peek_is_embargoed(&wallet));
    assert!(!client.is_embargoed(&wallet));
}

#[test]
fn test_peek_is_embargoed_matches_is_embargoed() {
    // peek_is_embargoed and is_embargoed must agree on the logical result.
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);

    // No embargo.
    assert_eq!(client.peek_is_embargoed(&wallet), client.is_embargoed(&wallet));

    // Indefinite embargo.
    client.set_score_embargo(&wallet, &None);
    assert_eq!(client.peek_is_embargoed(&wallet), client.is_embargoed(&wallet));

    // Lifted.
    client.lift_score_embargo(&wallet);
    assert_eq!(client.peek_is_embargoed(&wallet), client.is_embargoed(&wallet));
}
