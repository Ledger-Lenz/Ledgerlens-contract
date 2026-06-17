#![cfg(test)]

use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol, Vec};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient, ScoreSubmission};

// ── Test helpers ──────────────────────────────────────────────────────────────

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);

    (env, client, admin, service)
}

fn initialized<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);
    (env, client, admin, service)
}

// ── Initialization ────────────────────────────────────────────────────────────

#[test]
fn test_initialize() {
    let (_env, client, admin, service) = setup();
    client.initialize(&admin, &service);
    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.get_service(), service);
}

#[test]
fn test_initialize_twice_fails() {
    let (_env, client, admin, service) = setup();
    client.initialize(&admin, &service);
    let result = client.try_initialize(&admin, &service);
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

// ── Score submission & retrieval ──────────────────────────────────────────────

#[test]
fn test_submit_and_get_score() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    client.submit_score(&wallet, &asset_pair, &87, &true, &true, &1_700_000_000, &92, &1);

    let score = client.get_score(&wallet, &asset_pair);
    assert_eq!(score.score, 87);
    assert!(score.benford_flag);
    assert!(score.ml_flag);
    assert_eq!(score.timestamp, 1_700_000_000);
    assert_eq!(score.confidence, 92);
    assert_eq!(score.model_version, 1);
}

#[test]
fn test_get_score_not_found() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    let result = client.try_get_score(&wallet, &asset_pair);
    assert_eq!(result, Err(Ok(Error::ScoreNotFound)));
}

#[test]
fn test_submit_score_invalid_score_range_rejected() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    let result = client.try_submit_score(&wallet, &asset_pair, &101, &false, &false, &0, &50, &1);
    assert_eq!(result, Err(Ok(Error::InvalidScore)));
}

#[test]
fn test_submit_score_invalid_confidence_range_rejected() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    let result = client.try_submit_score(&wallet, &asset_pair, &50, &false, &false, &0, &101, &1);
    assert_eq!(result, Err(Ok(Error::InvalidConfidence)));
}

#[test]
fn test_submit_score_overwrites_previous() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    client.submit_score(&wallet, &asset_pair, &40, &false, &false, &1000, &70, &1);
    client.submit_score(&wallet, &asset_pair, &80, &true, &true, &2000, &90, &2);

    let score = client.get_score(&wallet, &asset_pair);
    assert_eq!(score.score, 80);
    assert_eq!(score.model_version, 2);
}

#[test]
fn test_scores_are_independent_across_pairs() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let pair1 = symbol_short!("XLM_USDC");
    let pair2 = symbol_short!("XLM_BTC");

    client.submit_score(&wallet, &pair1, &30, &false, &false, &1, &60, &1);
    client.submit_score(&wallet, &pair2, &90, &true, &true, &2, &95, &1);

    assert_eq!(client.get_score(&wallet, &pair1).score, 30);
    assert_eq!(client.get_score(&wallet, &pair2).score, 90);
}

// ── Service rotation ──────────────────────────────────────────────────────────

#[test]
fn test_set_service_rotates_authorised_account() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let new_service = Address::generate(&env);
    client.set_service(&new_service);

    assert_eq!(client.get_service(), new_service);

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");
    client.submit_score(&wallet, &asset_pair, &10, &false, &false, &0, &10, &1);
}

// ── Pause circuit breaker ─────────────────────────────────────────────────────

#[test]
fn test_pause_and_unpause() {
    let (_env, client, _admin, _service) = initialized();

    assert!(!client.is_paused());
    client.pause();
    assert!(client.is_paused());
    client.unpause();
    assert!(!client.is_paused());
}

#[test]
fn test_submit_score_blocked_when_paused() {
    let (env, client, _admin, _service) = initialized();

    client.pause();

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");
    let result = client.try_submit_score(&wallet, &asset_pair, &50, &false, &false, &0, &50, &1);
    assert_eq!(result, Err(Ok(Error::ContractPaused)));
}

#[test]
fn test_batch_blocked_when_paused() {
    let (env, client, _admin, _service) = initialized();

    client.pause();

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");
    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet,
        asset_pair,
        score: 50,
        benford_flag: false,
        ml_flag: false,
        timestamp: 0,
        confidence: 70,
        model_version: 1,
    });
    let result = client.try_submit_scores_batch(&batch);
    assert_eq!(result, Err(Ok(Error::ContractPaused)));
}

#[test]
fn test_submit_succeeds_after_unpause() {
    let (env, client, _admin, _service) = initialized();

    client.pause();
    client.unpause();

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");
    client.submit_score(&wallet, &asset_pair, &55, &false, &true, &999, &80, &1);
    assert_eq!(client.get_score(&wallet, &asset_pair).score, 55);
}

// ── Two-step admin transfer ────────────────────────────────────────────────────

#[test]
fn test_transfer_and_accept_admin() {
    let (env, client, admin, _service) = initialized();

    let new_admin = Address::generate(&env);
    client.transfer_admin(&new_admin);

    // Old admin still in place until the new one accepts.
    assert_eq!(client.get_admin(), admin);

    client.accept_admin();
    assert_eq!(client.get_admin(), new_admin);
}

#[test]
fn test_cancel_admin_transfer() {
    let (env, client, admin, _service) = initialized();

    let new_admin = Address::generate(&env);
    client.transfer_admin(&new_admin);
    client.cancel_admin_transfer();

    // Old admin is still in place after cancellation.
    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_cancel_without_pending_fails() {
    let (_env, client, _admin, _service) = initialized();
    let result = client.try_cancel_admin_transfer();
    assert_eq!(result, Err(Ok(Error::NoPendingAdminTransfer)));
}

#[test]
fn test_accept_admin_without_transfer_fails() {
    let (_env, client, _admin, _service) = initialized();
    let result = client.try_accept_admin();
    assert_eq!(result, Err(Ok(Error::NoPendingAdminTransfer)));
}

#[test]
fn test_new_admin_can_manage_service_after_transfer() {
    let (env, client, _admin, _service) = initialized();

    let new_admin = Address::generate(&env);
    client.transfer_admin(&new_admin);
    client.accept_admin();

    let new_service = Address::generate(&env);
    client.set_service(&new_service);
    assert_eq!(client.get_service(), new_service);
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_get_pending_admin_no_transfer() {
    let (_, client, _, _) = initialized();

    let _ = client.get_pending_admin();
}

#[test]
fn test_get_pending_admin_returns_nominee() {
    let (env, client, admin, _service) = initialized();

    let new_admin = Address::generate(&env);
    client.transfer_admin(&new_admin);

    // Old admin still in place until the new one accepts.
    assert_eq!(client.get_admin(), admin);

    let pending_admin = client.get_pending_admin();

    assert_eq!(pending_admin, new_admin);
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_get_pending_admin_cleared_after_accept() {
    let (env, client, admin, _service) = initialized();

    let new_admin = Address::generate(&env);
    client.transfer_admin(&new_admin);

    // Old admin still in place until the new one accepts.
    assert_eq!(client.get_admin(), admin);

    client.accept_admin();
    assert_eq!(client.get_admin(), new_admin);

    let _ = client.get_pending_admin();
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_get_pending_admin_cleared_after_cancel() {
    let (env, client, admin, _service) = initialized();

    let new_admin = Address::generate(&env);
    client.transfer_admin(&new_admin);

    // Old admin still in place until the new one accepts.
    assert_eq!(client.get_admin(), admin);

    client.cancel_admin_transfer();

    let _ = client.get_pending_admin();
}

#[test]
fn test_has_pending_admin_transfer_false_initially() {
    let (_, client, _, _) = initialized();

    let pending = client.has_pending_admin_transfer();

    assert!(!pending)
}

#[test]
fn test_has_pending_admin_transfer_true_during() {
    let (env, client, admin, _service) = initialized();

    let new_admin = Address::generate(&env);
    client.transfer_admin(&new_admin);

    // Old admin still in place until the new one accepts.
    assert_eq!(client.get_admin(), admin);

    let pending = client.has_pending_admin_transfer();

    assert!(pending)
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_get_pending_admin_before_init_fails() {
    let (_, client, _, _) = setup();
    let _ = client.get_pending_admin();
}

// ── Watchlist management ──────────────────────────────────────────────────────

#[test]
fn test_watchlist_add_and_query() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    assert!(!client.is_watchlisted(&wallet));

    client.set_watchlist(&wallet, &true);
    assert!(client.is_watchlisted(&wallet));
}

#[test]
fn test_watchlist_remove() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    client.set_watchlist(&wallet, &true);
    assert!(client.is_watchlisted(&wallet));

    client.set_watchlist(&wallet, &false);
    assert!(!client.is_watchlisted(&wallet));
}

#[test]
fn test_watchlist_is_per_wallet() {
    let (env, client, _admin, _service) = initialized();

    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);

    client.set_watchlist(&wallet_a, &true);
    assert!(client.is_watchlisted(&wallet_a));
    assert!(!client.is_watchlisted(&wallet_b));
}

// ── Risk threshold ────────────────────────────────────────────────────────────

#[test]
fn test_default_risk_threshold_is_75() {
    let (_env, client, _admin, _service) = initialized();
    assert_eq!(client.get_risk_threshold(), 75);
}

#[test]
fn test_set_risk_threshold() {
    let (_env, client, _admin, _service) = initialized();
    client.set_risk_threshold(&80);
    assert_eq!(client.get_risk_threshold(), 80);
}

#[test]
fn test_risk_threshold_boundary_values() {
    let (_env, client, _admin, _service) = initialized();

    client.set_risk_threshold(&0);
    assert_eq!(client.get_risk_threshold(), 0);

    client.set_risk_threshold(&100);
    assert_eq!(client.get_risk_threshold(), 100);
}

#[test]
fn test_risk_threshold_above_100_rejected() {
    let (_env, client, _admin, _service) = initialized();
    let result = client.try_set_risk_threshold(&101);
    assert_eq!(result, Err(Ok(Error::InvalidScore)));
}

// ── Score history ─────────────────────────────────────────────────────────────

#[test]
fn test_score_history_empty_initially() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    assert_eq!(client.get_score_history(&wallet, &asset_pair).len(), 0);
}

#[test]
fn test_score_history_accumulates_in_order() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    client.submit_score(&wallet, &asset_pair, &10, &false, &false, &1, &50, &1);
    client.submit_score(&wallet, &asset_pair, &20, &false, &false, &2, &60, &1);
    client.submit_score(&wallet, &asset_pair, &30, &false, &false, &3, &70, &1);

    let history = client.get_score_history(&wallet, &asset_pair);
    assert_eq!(history.len(), 3);
    assert_eq!(history.get(0).unwrap().score, 10);
    assert_eq!(history.get(1).unwrap().score, 20);
    assert_eq!(history.get(2).unwrap().score, 30);
}

#[test]
fn test_score_history_max_depth_enforced() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    // 12 entries — two are evicted once the ring is full (max depth = 10).
    for i in 0u32..12 {
        client.submit_score(&wallet, &asset_pair, &(i * 8), &false, &false, &(i as u64), &50, &1);
    }

    let history = client.get_score_history(&wallet, &asset_pair);
    assert_eq!(history.len(), 10);
    // Oldest retained: i=2 → score=16
    assert_eq!(history.get(0).unwrap().score, 16);
    // Newest: i=11 → score=88
    assert_eq!(history.get(9).unwrap().score, 88);
}

#[test]
fn test_score_history_is_per_pair() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let pair1 = symbol_short!("XLM_USDC");
    let pair2 = symbol_short!("XLM_BTC");

    client.submit_score(&wallet, &pair1, &10, &false, &false, &1, &50, &1);
    client.submit_score(&wallet, &pair2, &90, &true, &true, &2, &95, &1);

    assert_eq!(client.get_score_history(&wallet, &pair1).len(), 1);
    assert_eq!(client.get_score_history(&wallet, &pair2).len(), 1);
    assert_eq!(client.get_score_history(&wallet, &pair1).get(0).unwrap().score, 10);
    assert_eq!(client.get_score_history(&wallet, &pair2).get(0).unwrap().score, 90);
}

// ── Batch submission ──────────────────────────────────────────────────────────

#[test]
fn test_submit_scores_batch_writes_all_entries() {
    let (env, client, _admin, _service) = initialized();

    let wallet1 = Address::generate(&env);
    let wallet2 = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: wallet1.clone(),
        asset_pair: asset_pair.clone(),
        score: 45,
        benford_flag: false,
        ml_flag: false,
        timestamp: 1000,
        confidence: 80,
        model_version: 2,
    });
    batch.push_back(ScoreSubmission {
        wallet: wallet2.clone(),
        asset_pair: asset_pair.clone(),
        score: 85,
        benford_flag: true,
        ml_flag: true,
        timestamp: 2000,
        confidence: 90,
        model_version: 2,
    });

    let accepted = client.submit_scores_batch(&batch);
    assert_eq!(accepted, 2);

    assert_eq!(client.get_score(&wallet1, &asset_pair).score, 45);
    assert_eq!(client.get_score(&wallet2, &asset_pair).score, 85);
}

#[test]
fn test_submit_scores_batch_skips_invalid_entries() {
    let (env, client, _admin, _service) = initialized();

    let wallet_ok = Address::generate(&env);
    let wallet_bad = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: wallet_bad.clone(),
        asset_pair: asset_pair.clone(),
        score: 200, // invalid — > 100
        benford_flag: false,
        ml_flag: false,
        timestamp: 0,
        confidence: 50,
        model_version: 1,
    });
    batch.push_back(ScoreSubmission {
        wallet: wallet_ok.clone(),
        asset_pair: asset_pair.clone(),
        score: 60,
        benford_flag: false,
        ml_flag: false,
        timestamp: 1,
        confidence: 75,
        model_version: 1,
    });

    let accepted = client.submit_scores_batch(&batch);
    assert_eq!(accepted, 1);

    assert_eq!(client.get_score(&wallet_ok, &asset_pair).score, 60);
    assert_eq!(client.try_get_score(&wallet_bad, &asset_pair), Err(Ok(Error::ScoreNotFound)));
}

#[test]
fn test_batch_empty_returns_error() {
    let (env, client, _admin, _service) = initialized();

    let empty: Vec<ScoreSubmission> = Vec::new(&env);
    let result = client.try_submit_scores_batch(&empty);
    assert_eq!(result, Err(Ok(Error::EmptyBatch)));
}

#[test]
fn test_batch_too_large_returns_error() {
    let (env, client, _admin, _service) = initialized();

    let asset_pair = symbol_short!("XLM_USDC");
    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);

    // 21 entries — one over the MAX_BATCH_SIZE (20) cap.
    for _ in 0..21 {
        batch.push_back(ScoreSubmission {
            wallet: Address::generate(&env),
            asset_pair: asset_pair.clone(),
            score: 50,
            benford_flag: false,
            ml_flag: false,
            timestamp: 0,
            confidence: 70,
            model_version: 1,
        });
    }

    let result = client.try_submit_scores_batch(&batch);
    assert_eq!(result, Err(Ok(Error::BatchTooLarge)));
}

#[test]
fn test_batch_also_populates_score_history() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: wallet.clone(),
        asset_pair: asset_pair.clone(),
        score: 55,
        benford_flag: false,
        ml_flag: false,
        timestamp: 1,
        confidence: 80,
        model_version: 1,
    });

    client.submit_scores_batch(&batch);

    let history = client.get_score_history(&wallet, &asset_pair);
    assert_eq!(history.len(), 1);
    assert_eq!(history.get(0).unwrap().score, 55);
}

// ── Contract version ──────────────────────────────────────────────────────────

#[test]
fn test_get_version_returns_one() {
    let (_env, client, _admin, _service) = initialized();
    assert_eq!(client.get_version(), 1);
}

// ── Not-initialized guards ────────────────────────────────────────────────────

#[test]
fn test_get_admin_before_init_fails() {
    let (_env, client, _, _) = setup();
    let result = client.try_get_admin();
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_submit_score_before_init_fails() {
    let (env, client, _, _) = setup();
    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");
    let result = client.try_submit_score(&wallet, &asset_pair, &50, &false, &false, &0, &50, &1);
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_pause_before_init_fails() {
    let (_env, client, _, _) = setup();
    let result = client.try_pause();
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

// ── Cross-asset aggregate risk ────────────────────────────────────────────────

#[test]
fn test_aggregate_single_pair() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    client.submit_score(&wallet, &pair, &60, &false, &false, &1, &90, &1);

    let aggregate = client.get_aggregate_score(&wallet);
    assert_eq!(aggregate.aggregate_score, 60);
    assert_eq!(aggregate.pair_count, 1);
}

#[test]
fn test_aggregate_equal_weights() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let pair1 = symbol_short!("XLM_USDC");
    let pair2 = symbol_short!("XLM_BTC");
    let pair3 = symbol_short!("XLM_ETH");

    client.submit_score(&wallet, &pair1, &30, &false, &false, &1, &90, &1);
    client.submit_score(&wallet, &pair2, &60, &false, &false, &2, &90, &1);
    client.submit_score(&wallet, &pair3, &90, &false, &false, &3, &90, &1);

    // (30 + 60 + 90) / 3 = 60
    assert_eq!(client.get_aggregate_score(&wallet).aggregate_score, 60);
}

#[test]
fn test_aggregate_weighted() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("XLM_BTC");
    let pair_c = symbol_short!("XLM_ETH");

    client.set_pair_weight(&pair_a, &1);
    client.set_pair_weight(&pair_b, &2);
    client.set_pair_weight(&pair_c, &1);

    client.submit_score(&wallet, &pair_a, &20, &false, &false, &1, &90, &1);
    client.submit_score(&wallet, &pair_b, &80, &false, &false, &2, &90, &1);
    client.submit_score(&wallet, &pair_c, &40, &false, &false, &3, &90, &1);

    // (20*1 + 80*2 + 40*1) / (1 + 2 + 1) = 220 / 4 = 55
    assert_eq!(client.get_aggregate_score(&wallet).aggregate_score, 55);
}

#[test]
fn test_aggregate_max_pair_tracked() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let pair1 = symbol_short!("XLM_USDC");
    let pair2 = symbol_short!("XLM_BTC");

    client.submit_score(&wallet, &pair1, &30, &false, &false, &1, &90, &1);
    client.submit_score(&wallet, &pair2, &90, &false, &false, &2, &90, &1);

    let aggregate = client.get_aggregate_score(&wallet);
    assert_eq!(aggregate.max_pair_score, 90);
    assert_eq!(aggregate.max_pair, pair2);
}

#[test]
fn test_aggregate_flag_counts() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let pair1 = symbol_short!("XLM_USDC");
    let pair2 = symbol_short!("XLM_BTC");
    let pair3 = symbol_short!("XLM_ETH");

    client.submit_score(&wallet, &pair1, &30, &true, &false, &1, &90, &1);
    client.submit_score(&wallet, &pair2, &60, &true, &true, &2, &90, &1);
    client.submit_score(&wallet, &pair3, &90, &false, &false, &3, &90, &1);

    let aggregate = client.get_aggregate_score(&wallet);
    assert_eq!(aggregate.benford_flag_count, 2);
    assert_eq!(aggregate.ml_flag_count, 1);
}

#[test]
fn test_aggregate_updates_on_rescore() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("XLM_BTC");

    client.submit_score(&wallet, &pair_a, &20, &false, &false, &1, &90, &1);
    client.submit_score(&wallet, &pair_b, &40, &false, &false, &2, &90, &1);
    assert_eq!(client.get_aggregate_score(&wallet).aggregate_score, 30);

    // Re-submitting pair A with a higher score must shift the aggregate.
    client.submit_score(&wallet, &pair_a, &80, &false, &false, &3, &90, &1);
    assert_eq!(client.get_aggregate_score(&wallet).aggregate_score, 60);
}

#[test]
fn test_aggregate_wallet_not_found() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let result = client.try_get_aggregate_score(&wallet);
    assert_eq!(result, Err(Ok(Error::ScoreNotFound)));
}

#[test]
fn test_aggregate_pair_deduplication() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    for i in 0..5u64 {
        client.submit_score(&wallet, &pair, &(50 + i as u32), &false, &false, &i, &90, &1);
    }

    let aggregate = client.get_aggregate_score(&wallet);
    assert_eq!(aggregate.pair_count, 1);
    assert_eq!(aggregate.aggregate_score, 54);
}

#[test]
fn test_aggregate_weight_zero_excluded() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("XLM_BTC");

    client.set_pair_weight(&pair_b, &0);

    client.submit_score(&wallet, &pair_a, &70, &false, &false, &1, &90, &1);
    client.submit_score(&wallet, &pair_b, &10, &false, &false, &2, &90, &1);

    // pair_b's weight is 0, so only pair_a contributes to the average.
    let aggregate = client.get_aggregate_score(&wallet);
    assert_eq!(aggregate.aggregate_score, 70);
    assert_eq!(aggregate.pair_count, 2);
}

#[test]
fn test_aggregate_overflow_protection() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);

    let pair_names = [
        "P0", "P1", "P2", "P3", "P4", "P5", "P6", "P7", "P8", "P9", "PA", "PB", "PC", "PD", "PE",
        "PF", "PG", "PH", "PI", "PJ",
    ];
    assert_eq!(pair_names.len(), 20);

    for (i, name) in pair_names.iter().enumerate() {
        let pair = Symbol::new(&env, name);
        client.set_pair_weight(&pair, &u32::MAX);
        client.submit_score(&wallet, &pair, &50, &false, &false, &(i as u64), &90, &1);
    }

    let result = client.try_get_aggregate_score(&wallet);
    assert_eq!(result, Err(Ok(Error::ArithmeticOverflow)));
}

#[test]
fn test_set_pair_weight() {
    let (_env, client, _admin, _service) = initialized();
    let pair = symbol_short!("XLM_USDC");

    client.set_pair_weight(&pair, &3);
    assert_eq!(client.get_pair_weight(&pair), 3);
}

#[test]
fn test_get_pair_weight_defaults_to_one() {
    let (_env, client, _admin, _service) = initialized();
    let pair = symbol_short!("XLM_USDC");
    assert_eq!(client.get_pair_weight(&pair), 1);
}
