use soroban_sdk::{symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
};

use crate::{
    BatchResult, Error, LedgerLensScoreContract, LedgerLensScoreContractClient, ScoreQuery,
    ScoreSubmission,
};

// ── Test helpers ──────────────────────────────────────────────────────────────

pub fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.budget().reset_unlimited();

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);

    (env, client, admin, service)
}

pub fn initialized<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let (env, client, admin, service) = setup();
    env.ledger().with_mut(|l| l.timestamp = 100_000);
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

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &87,
        &true,
        &true,
        &1_700_000_000,
        &92,
        &1,
        &None,
    );

    let score = client.get_score(&wallet, &asset_pair);
    assert_eq!(score.score, 87);
    assert!(score.benford_flag);
    assert!(score.ml_flag);
    assert_eq!(score.timestamp, 1_700_000_000);
    assert_eq!(score.confidence, 92);
    assert_eq!(score.model_version, 1);
}

#[test]
fn test_get_score_exists_returns_true_when_score_present() {
    let (env, client, admin, service) = initialized();
    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &50,
        &false,
        &false,
        &1_700_000_000,
        &80,
        &1,
        &None,
    );

    assert!(client.get_score_exists(&wallet, &asset_pair));
}

#[test]
fn test_get_score_exists_returns_false_when_no_score() {
    let (env, client, admin, service) = initialized();
    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    assert!(!client.get_score_exists(&wallet, &asset_pair));
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
fn test_get_scores_batch_empty() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let queries = Vec::new(&env);
    let results = client.get_scores_batch(&queries);
    assert_eq!(results.len(), 0);
}

#[test]
fn test_get_scores_batch_single_entry() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &64,
        &false,
        &true,
        &1,
        &88,
        &1,
        &None,
    );

    let mut queries = Vec::new(&env);
    queries.push_back(ScoreQuery { wallet, asset_pair });

    let results = client.get_scores_batch(&queries);
    let entry = results.get(0).unwrap();
    assert_eq!(entry.index, 0);
    assert!(entry.found);
    assert_eq!(entry.score.clone().unwrap().score, 64);
}

#[test]
fn test_get_scores_batch_max_batch() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let asset_pair = symbol_short!("XLM_USDC");
    let mut queries = Vec::new(&env);
    for i in 0..crate::constants::BATCH_READ_MAX {
        let wallet = Address::generate(&env);
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &asset_pair,
            &(i % 100),
            &false,
            &false,
            &(i as u64 + 1),
            &90,
            &1,
            &None,
        );
        queries.push_back(ScoreQuery { wallet, asset_pair: asset_pair.clone() });
    }

    let results = client.get_scores_batch(&queries);
    assert_eq!(results.len(), crate::constants::BATCH_READ_MAX);
    for i in 0..results.len() {
        let entry = results.get(i).unwrap();
        assert_eq!(entry.index, i);
        assert!(entry.found);
        assert_eq!(entry.score.clone().unwrap().score, i % 100);
    }
}

#[test]
fn test_get_scores_batch_too_large() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let asset_pair = symbol_short!("XLM_USDC");
    let mut queries = Vec::new(&env);
    for _ in 0..=crate::constants::BATCH_READ_MAX {
        queries.push_back(ScoreQuery {
            wallet: Address::generate(&env),
            asset_pair: asset_pair.clone(),
        });
    }

    let result = client.try_get_scores_batch(&queries);
    assert_eq!(result, Err(Ok(Error::BatchTooLarge)));
}

#[test]
fn test_get_scores_batch_partial_hit_miss() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet_hit = Address::generate(&env);
    let wallet_miss = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");
    client.submit_score(
        &Vec::new(&env),
        &wallet_hit,
        &asset_pair,
        &71,
        &false,
        &false,
        &1,
        &90,
        &1,
        &None,
    );

    let mut queries = Vec::new(&env);
    queries.push_back(ScoreQuery { wallet: wallet_hit, asset_pair: asset_pair.clone() });
    queries.push_back(ScoreQuery { wallet: wallet_miss, asset_pair });

    let results = client.get_scores_batch(&queries);
    assert!(results.get(0).unwrap().found);
    assert_eq!(results.get(0).unwrap().score.score, 71);
    assert!(!results.get(1).unwrap().found);
    assert_eq!(results.get(1).unwrap().score, crate::MaybeRiskScore::None);
}

#[test]
fn test_get_scores_batch_embargoed_entry() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &55,
        &false,
        &false,
        &1,
        &90,
        &1,
        &None,
    );
    client.set_score_embargo(&wallet, &None);

    let mut queries = Vec::new(&env);
    queries.push_back(ScoreQuery { wallet, asset_pair });

    let results = client.get_scores_batch(&queries);
    let entry = results.get(0).unwrap();
    assert_eq!(entry.index, 0);
    assert!(!entry.found);
    assert_eq!(entry.score, crate::MaybeRiskScore::None);
}

#[test]
fn test_get_scores_batch_delegated_wallet() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let sub_wallet = Address::generate(&env);
    let custodian = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");
    client.set_score_delegate(&sub_wallet, &custodian);
    client.submit_score(
        &Vec::new(&env),
        &custodian,
        &asset_pair,
        &82,
        &true,
        &false,
        &1,
        &95,
        &1,
        &None,
    );

    let mut queries = Vec::new(&env);
    queries.push_back(ScoreQuery { wallet: sub_wallet, asset_pair });

    let results = client.get_scores_batch(&queries);
    let entry = results.get(0).unwrap();
    assert!(entry.found);
    assert_eq!(entry.score.clone().unwrap().score, 82);
}

#[test]
fn test_get_min_score_returns_zero() {
    let (_env, client, admin, service) = setup();
    client.initialize(&admin, &service);
    assert_eq!(client.get_min_score(), 0);
}

#[test]
fn test_get_max_score_returns_hundred() {
    let (_env, client, admin, service) = setup();
    client.initialize(&admin, &service);
    assert_eq!(client.get_max_score(), 100);
}

#[test]
fn test_submit_score_invalid_score_range_rejected() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &101,
        &false,
        &false,
        &0,
        &50,
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::InvalidScore)));
}

#[test]
fn test_submit_score_invalid_confidence_range_rejected() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &50,
        &false,
        &false,
        &0,
        &101,
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::InvalidConfidence)));
}

#[test]
fn test_submit_score_overwrites_previous() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &40,
        &false,
        &false,
        &1000,
        &70,
        &1,
        &None,
    );
    env.ledger().with_mut(|l| l.timestamp += 3_601); // past the default cooldown
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &80,
        &true,
        &true,
        &2000,
        &90,
        &2,
        &None,
    );

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

    client.submit_score(&Vec::new(&env), &wallet, &pair1, &30, &false, &false, &1, &60, &1, &None);
    client.submit_score(&Vec::new(&env), &wallet, &pair2, &90, &true, &true, &2, &95, &1, &None);

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
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &10,
        &false,
        &false,
        &1,
        &10,
        &1,
        &None,
    );
}

// ── Pause circuit breaker ─────────────────────────────────────────────────────

#[test]
fn test_pause_and_unpause() {
    let (env, client, _admin, _service) = initialized();

    assert!(!client.is_paused());
    client.pause(&Vec::new(&env));
    assert!(client.is_paused());
    client.unpause(&Vec::new(&env));
    assert!(!client.is_paused());
}

#[test]
fn test_submit_score_blocked_when_paused() {
    let (env, client, _admin, _service) = initialized();

    client.pause(&Vec::new(&env));

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &50,
        &false,
        &false,
        &0,
        &50,
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::ContractPaused)));
}

#[test]
fn test_batch_blocked_when_paused() {
    let (env, client, _admin, _service) = initialized();

    client.pause(&Vec::new(&env));

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

    client.pause(&Vec::new(&env));
    client.unpause(&Vec::new(&env));

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &55,
        &false,
        &true,
        &999,
        &80,
        &1,
        &None,
    );
    assert_eq!(client.get_score(&wallet, &asset_pair).score, 55);
}

// ── Two-step admin transfer ────────────────────────────────────────────────────

#[test]
fn test_transfer_and_accept_admin() {
    let (env, client, admin, _service) = initialized();

    let new_admin = Address::generate(&env);
    client.transfer_admin(&Vec::new(&env), &new_admin);

    // Old admin still in place until the new one accepts.
    assert_eq!(client.get_admin(), admin);

    client.accept_admin();
    assert_eq!(client.get_admin(), new_admin);
}

#[test]
fn test_cancel_admin_transfer() {
    let (env, client, admin, _service) = initialized();

    let new_admin = Address::generate(&env);
    client.transfer_admin(&Vec::new(&env), &new_admin);
    client.cancel_admin_transfer(&Vec::new(&env));

    // Old admin is still in place after cancellation.
    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_cancel_without_pending_fails() {
    let (env, client, _admin, _service) = initialized();
    let result = client.try_cancel_admin_transfer(&Vec::new(&env));
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
    client.transfer_admin(&Vec::new(&env), &new_admin);
    client.accept_admin();

    let new_service = Address::generate(&env);
    client.set_service(&new_service);
    assert_eq!(client.get_service(), new_service);
}

#[test]
fn test_get_pending_admin_no_transfer() {
    let (_, client, _, _) = initialized();

    assert_eq!(client.get_pending_admin(), None);
}

#[test]
fn test_get_pending_admin_returns_nominee() {
    let (env, client, admin, _service) = initialized();

    let new_admin = Address::generate(&env);
    client.transfer_admin(&Vec::new(&env), &new_admin);

    // Old admin still in place until the new one accepts.
    assert_eq!(client.get_admin(), admin);

    let pending_admin = client.get_pending_admin();

    assert_eq!(pending_admin, Some(new_admin));
}

#[test]
fn test_get_pending_admin_cleared_after_accept() {
    let (env, client, admin, _service) = initialized();

    let new_admin = Address::generate(&env);
    client.transfer_admin(&Vec::new(&env), &new_admin);

    // Old admin still in place until the new one accepts.
    assert_eq!(client.get_admin(), admin);

    client.accept_admin();
    assert_eq!(client.get_admin(), new_admin);

    assert_eq!(client.get_pending_admin(), None);
}

#[test]
fn test_get_pending_admin_cleared_after_cancel() {
    let (env, client, admin, _service) = initialized();

    let new_admin = Address::generate(&env);
    client.transfer_admin(&Vec::new(&env), &new_admin);

    // Old admin still in place until the new one accepts.
    assert_eq!(client.get_admin(), admin);

    client.cancel_admin_transfer(&Vec::new(&env));

    assert_eq!(client.get_pending_admin(), None);
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
    client.transfer_admin(&Vec::new(&env), &new_admin);

    // Old admin still in place until the new one accepts.
    assert_eq!(client.get_admin(), admin);

    let pending = client.has_pending_admin_transfer();

    assert!(pending)
}

#[test]
fn test_get_pending_admin_before_init_is_none() {
    let (_, client, _, _) = setup();
    assert_eq!(client.get_pending_admin(), None);
}

// ── Watchlist management ──────────────────────────────────────────────────────

#[test]
fn test_watchlist_add_and_query() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    assert!(!client.is_watchlisted(&wallet));

    client.set_watchlist(&Vec::new(&env), &wallet, &true);
    assert!(client.is_watchlisted(&wallet));
}

#[test]
fn test_watchlist_remove() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    client.set_watchlist(&Vec::new(&env), &wallet, &true);
    assert!(client.is_watchlisted(&wallet));

    client.set_watchlist(&Vec::new(&env), &wallet, &false);
    assert!(!client.is_watchlisted(&wallet));
}

#[test]
fn test_watchlist_is_per_wallet() {
    let (env, client, _admin, _service) = initialized();

    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);

    client.set_watchlist(&Vec::new(&env), &wallet_a, &true);
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
    let (env, client, _admin, _service) = initialized();
    client.set_risk_threshold(&Vec::new(&env), &80);
    assert_eq!(client.get_risk_threshold(), 80);
}

#[test]
fn test_risk_threshold_boundary_values() {
    let (env, client, _admin, _service) = initialized();

    client.set_risk_threshold(&Vec::new(&env), &0);
    assert_eq!(client.get_risk_threshold(), 0);

    client.set_risk_threshold(&Vec::new(&env), &100);
    assert_eq!(client.get_risk_threshold(), 100);
}

#[test]
fn test_risk_threshold_above_100_rejected() {
    let (env, client, _admin, _service) = initialized();
    let result = client.try_set_risk_threshold(&Vec::new(&env), &101);
    assert_eq!(result, Err(Ok(Error::InvalidScore)));
}

// ── get_score_threshold ───────────────────────────────────────────────────────

#[test]
fn test_get_score_threshold_matches_admin_set_value() {
    let (env, client, _admin, _service) = initialized();
    // Default before any admin update.
    assert_eq!(client.get_score_threshold(), 75);
    // After admin sets a new threshold, get_score_threshold reflects it.
    client.set_risk_threshold(&Vec::new(&env), &60);
    assert_eq!(client.get_score_threshold(), 60);
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

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &10,
        &false,
        &false,
        &1,
        &50,
        &1,
        &None,
    );
    env.ledger().with_mut(|l| l.timestamp += 3_601);
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &20,
        &false,
        &false,
        &2,
        &60,
        &1,
        &None,
    );
    env.ledger().with_mut(|l| l.timestamp += 3_601);
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &30,
        &false,
        &false,
        &3,
        &70,
        &1,
        &None,
    );

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
        env.ledger().with_mut(|l| l.timestamp += 3_601); // past the default cooldown
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &asset_pair,
            &(i * 8),
            &false,
            &false,
            &(i as u64 + 1),
            &50,
            &1,
            &None,
        );
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

    client.submit_score(&Vec::new(&env), &wallet, &pair1, &10, &false, &false, &1, &50, &1, &None);
    client.submit_score(&Vec::new(&env), &wallet, &pair2, &90, &true, &true, &2, &95, &1, &None);

    assert_eq!(client.get_score_history(&wallet, &pair1).len(), 1);
    assert_eq!(client.get_score_history(&wallet, &pair2).len(), 1);
    assert_eq!(client.get_score_history(&wallet, &pair1).get(0).unwrap().score, 10);
    assert_eq!(client.get_score_history(&wallet, &pair2).get(0).unwrap().score, 90);
}

// ── Configurable history depth ────────────────────────────────────────────────

#[test]
fn test_default_history_depth_is_10() {
    let (_env, client, _admin, _service) = initialized();
    assert_eq!(client.get_history_max_depth(), 10);
}

#[test]
fn test_set_history_max_depth_increases_ring() {
    // Set depth to 20, submit 15 entries — all 15 should be retained.
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    client.set_history_max_depth(&Vec::new(&env), &20);

    for i in 0u32..15 {
        env.ledger().with_mut(|l| l.timestamp += 3_601);
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &asset_pair,
            &(i * 5),
            &false,
            &false,
            &(i as u64 + 1),
            &50,
            &1,
            &None,
        );
    }

    let history = client.get_score_history(&wallet, &asset_pair);
    assert_eq!(history.len(), 15);
    // Oldest retained is entry 0 (score = 0).
    assert_eq!(history.get(0).unwrap().score, 0);
    // Newest is entry 14 (score = 70).
    assert_eq!(history.get(14).unwrap().score, 70);
}

#[test]
fn test_set_history_max_depth_decreases_ring_on_next_write() {
    // Submit 5 entries at the default depth (10), then reduce to 3.
    // The next submission must trigger eviction down to 3.
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    for i in 0u32..5 {
        env.ledger().with_mut(|l| l.timestamp += 3_601);
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &asset_pair,
            &(i * 10),
            &false,
            &false,
            &(i as u64 + 1),
            &50,
            &1,
            &None,
        );
    }
    assert_eq!(client.get_score_history(&wallet, &asset_pair).len(), 5);

    // Reduce depth to 3.
    client.set_history_max_depth(&Vec::new(&env), &3);

    // One more submission triggers the eviction pass.
    env.ledger().with_mut(|l| l.timestamp += 3_601);
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &99,
        &false,
        &false,
        &100,
        &50,
        &1,
        &None,
    );

    let history = client.get_score_history(&wallet, &asset_pair);
    assert_eq!(history.len(), 3);
    // Newest entry must be the one just submitted (score = 99).
    assert_eq!(history.get(2).unwrap().score, 99);
}

#[test]
fn test_history_depth_zero_rejected() {
    let (env, client, _admin, _service) = initialized();
    let result = client.try_set_history_max_depth(&Vec::new(&env), &0);
    assert_eq!(result, Err(Ok(Error::InvalidHistoryDepth)));
}

#[test]
fn test_history_depth_above_ceiling_rejected() {
    let (env, client, _admin, _service) = initialized();
    // MAX_HISTORY_DEPTH is 50; 51 must be rejected.
    let result = client.try_set_history_max_depth(&Vec::new(&env), &51);
    assert_eq!(result, Err(Ok(Error::InvalidHistoryDepth)));
}

#[test]
fn test_history_depth_at_ceiling_accepted() {
    let (env, client, _admin, _service) = initialized();
    // Exactly 50 is the ceiling — must succeed.
    client.set_history_max_depth(&Vec::new(&env), &50);
    assert_eq!(client.get_history_max_depth(), 50);
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

    let result: BatchResult = client.submit_scores_batch(&batch);
    assert_eq!(result.accepted_count, 2);
    assert_eq!(result.rejected_count, 0);
    assert_eq!(result.results.len(), 2);
    for i in 0..2 {
        assert!(result.results.get(i).unwrap().accepted);
        assert_eq!(result.results.get(i).unwrap().rejection_code, 0);
    }

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

    let result: BatchResult = client.submit_scores_batch(&batch);
    assert_eq!(result.accepted_count, 1);
    assert_eq!(result.rejected_count, 1);
    assert_eq!(result.results.len(), 2);

    // First entry (invalid score) — rejected with code 4 (InvalidScore).
    let r0 = result.results.get(0).unwrap();
    assert!(!r0.accepted);
    assert_eq!(r0.rejection_code, 4);

    // Second entry — accepted.
    let r1 = result.results.get(1).unwrap();
    assert!(r1.accepted);
    assert_eq!(r1.rejection_code, 0);

    assert_eq!(client.get_score(&wallet_ok, &asset_pair).score, 60);
    assert_eq!(client.try_get_score(&wallet_bad, &asset_pair), Err(Ok(Error::ScoreNotFound)));

    // Score count must reflect only the accepted entry, not the skipped one.
    assert_eq!(client.get_score_count(&wallet_ok, &asset_pair), 1);
    assert_eq!(client.get_score_count(&wallet_bad, &asset_pair), 0);
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

    let result: BatchResult = client.submit_scores_batch(&batch);
    assert!(result.accepted_count >= 1);

    let history = client.get_score_history(&wallet, &asset_pair);
    assert_eq!(history.len(), 1);
    assert_eq!(history.get(0).unwrap().score, 55);
}

// ── Batch structured result ───────────────────────────────────────────────────

#[test]
fn test_batch_result_all_accepted() {
    let (env, client, _admin, _service) = initialized();

    let wallet1 = Address::generate(&env);
    let wallet2 = Address::generate(&env);
    let wallet3 = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: wallet1,
        asset_pair: pair.clone(),
        score: 10,
        benford_flag: false,
        ml_flag: false,
        timestamp: 1,
        confidence: 50,
        model_version: 1,
    });
    batch.push_back(ScoreSubmission {
        wallet: wallet2,
        asset_pair: pair.clone(),
        score: 50,
        benford_flag: false,
        ml_flag: false,
        timestamp: 2,
        confidence: 70,
        model_version: 1,
    });
    batch.push_back(ScoreSubmission {
        wallet: wallet3,
        asset_pair: pair.clone(),
        score: 90,
        benford_flag: true,
        ml_flag: true,
        timestamp: 3,
        confidence: 95,
        model_version: 2,
    });

    let result: BatchResult = client.submit_scores_batch(&batch);
    assert_eq!(result.accepted_count, 3);
    assert_eq!(result.rejected_count, 0);
    assert_eq!(result.results.len(), 3);
    for i in 0..3 {
        assert!(result.results.get(i).unwrap().accepted);
        assert_eq!(result.results.get(i).unwrap().rejection_code, 0);
    }
}

#[test]
fn test_batch_result_mixed() {
    let (env, client, _admin, _service) = initialized();

    let wallet_ok = Address::generate(&env);
    let wallet_bad = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: wallet_ok.clone(),
        asset_pair: pair.clone(),
        score: 50,
        benford_flag: false,
        ml_flag: false,
        timestamp: 1,
        confidence: 70,
        model_version: 1,
    });
    batch.push_back(ScoreSubmission {
        wallet: wallet_bad.clone(),
        asset_pair: pair.clone(),
        score: 101, // invalid — > 100
        benford_flag: false,
        ml_flag: false,
        timestamp: 2,
        confidence: 70,
        model_version: 1,
    });

    let result: BatchResult = client.submit_scores_batch(&batch);
    assert_eq!(result.accepted_count, 1);
    assert_eq!(result.rejected_count, 1);
    assert_eq!(result.results.len(), 2);

    let r0 = result.results.get(0).unwrap();
    assert!(r0.accepted);
    assert_eq!(r0.rejection_code, 0);

    let r1 = result.results.get(1).unwrap();
    assert!(!r1.accepted);
    assert_eq!(r1.rejection_code, 4); // InvalidScore
}

#[test]
fn test_batch_result_index_correct() {
    let (env, client, _admin, _service) = initialized();

    let wallet1 = Address::generate(&env);
    let wallet2 = Address::generate(&env);
    let wallet3 = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: wallet1,
        asset_pair: pair.clone(),
        score: 50,
        benford_flag: false,
        ml_flag: false,
        timestamp: 1,
        confidence: 70,
        model_version: 1,
    });
    batch.push_back(ScoreSubmission {
        wallet: wallet2,
        asset_pair: pair.clone(),
        score: 200, // invalid — will be rejected at index 1
        benford_flag: false,
        ml_flag: false,
        timestamp: 2,
        confidence: 70,
        model_version: 1,
    });
    batch.push_back(ScoreSubmission {
        wallet: wallet3,
        asset_pair: pair.clone(),
        score: 60,
        benford_flag: false,
        ml_flag: false,
        timestamp: 3,
        confidence: 80,
        model_version: 1,
    });

    let result: BatchResult = client.submit_scores_batch(&batch);
    assert_eq!(result.accepted_count, 2);
    assert_eq!(result.rejected_count, 1);
    assert_eq!(result.results.len(), 3);

    assert_eq!(result.results.get(0).unwrap().index, 0);
    assert_eq!(result.results.get(1).unwrap().index, 1);
    assert_eq!(result.results.get(2).unwrap().index, 2);

    // The rejected entry is at index 1.
    assert!(result.results.get(0).unwrap().accepted);
    assert!(!result.results.get(1).unwrap().accepted);
    assert!(result.results.get(2).unwrap().accepted);
    assert_eq!(result.results.get(1).unwrap().rejection_code, 4); // InvalidScore
}

#[test]
fn test_batch_result_all_rejected() {
    let (env, client, _admin, _service) = initialized();

    let wallet1 = Address::generate(&env);
    let wallet2 = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    // Score > 100
    batch.push_back(ScoreSubmission {
        wallet: wallet1,
        asset_pair: pair.clone(),
        score: 200,
        benford_flag: false,
        ml_flag: false,
        timestamp: 1,
        confidence: 50,
        model_version: 1,
    });
    // Confidence > 100
    batch.push_back(ScoreSubmission {
        wallet: wallet2,
        asset_pair: pair.clone(),
        score: 50,
        benford_flag: false,
        ml_flag: false,
        timestamp: 2,
        confidence: 200,
        model_version: 1,
    });

    let result: BatchResult = client.submit_scores_batch(&batch);
    assert_eq!(result.accepted_count, 0);
    assert_eq!(result.rejected_count, 2);
    assert_eq!(result.results.len(), 2);
    for i in 0..2 {
        assert!(!result.results.get(i).unwrap().accepted);
    }
}

#[test]
fn test_batch_result_vec_length_matches_input() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Single entry.
    let mut batch1: Vec<ScoreSubmission> = Vec::new(&env);
    batch1.push_back(ScoreSubmission {
        wallet: wallet.clone(),
        asset_pair: pair.clone(),
        score: 50,
        benford_flag: false,
        ml_flag: false,
        timestamp: 1,
        confidence: 70,
        model_version: 1,
    });
    let result1: BatchResult = client.submit_scores_batch(&batch1);
    assert_eq!(result1.results.len(), 1);

    // Multiple entries.
    let mut batch5: Vec<ScoreSubmission> = Vec::new(&env);
    batch5.push_back(ScoreSubmission {
        wallet: Address::generate(&env),
        asset_pair: pair.clone(),
        score: 10,
        benford_flag: false,
        ml_flag: false,
        timestamp: 1,
        confidence: 50,
        model_version: 1,
    });
    batch5.push_back(ScoreSubmission {
        wallet: Address::generate(&env),
        asset_pair: pair.clone(),
        score: 20,
        benford_flag: false,
        ml_flag: false,
        timestamp: 2,
        confidence: 60,
        model_version: 1,
    });
    batch5.push_back(ScoreSubmission {
        wallet: Address::generate(&env),
        asset_pair: pair.clone(),
        score: 30,
        benford_flag: false,
        ml_flag: false,
        timestamp: 3,
        confidence: 70,
        model_version: 1,
    });
    batch5.push_back(ScoreSubmission {
        wallet: Address::generate(&env),
        asset_pair: pair.clone(),
        score: 40,
        benford_flag: false,
        ml_flag: false,
        timestamp: 4,
        confidence: 80,
        model_version: 1,
    });
    batch5.push_back(ScoreSubmission {
        wallet: Address::generate(&env),
        asset_pair: pair.clone(),
        score: 50,
        benford_flag: false,
        ml_flag: false,
        timestamp: 5,
        confidence: 90,
        model_version: 1,
    });
    let result5: BatchResult = client.submit_scores_batch(&batch5);
    assert_eq!(result5.results.len(), 5);
}

// ── Contract version ──────────────────────────────────────────────────────────

#[test]
fn test_get_version_returns_three() {
    let (_env, client, _admin, _service) = initialized();
    assert_eq!(client.get_version(), 3);
}

#[test]
fn test_get_contract_version_returns_three() {
    let (_env, client, _admin, _service) = initialized();
    assert_eq!(client.get_contract_version(), 3);
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
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &50,
        &false,
        &false,
        &0,
        &50,
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_pause_before_init_fails() {
    let (env, client, _, _) = setup();
    let result = client.try_pause(&Vec::new(&env));
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

// ── Cross-asset aggregate risk ────────────────────────────────────────────────

#[test]
fn test_aggregate_single_pair() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    client.submit_score(&Vec::new(&env), &wallet, &pair, &60, &false, &false, &1, &90, &1, &None);

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

    client.submit_score(&Vec::new(&env), &wallet, &pair1, &30, &false, &false, &1, &90, &1, &None);
    client.submit_score(&Vec::new(&env), &wallet, &pair2, &60, &false, &false, &2, &90, &1, &None);
    client.submit_score(&Vec::new(&env), &wallet, &pair3, &90, &false, &false, &3, &90, &1, &None);

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

    client.set_pair_weight(&Vec::new(&env), &pair_a, &1);
    client.set_pair_weight(&Vec::new(&env), &pair_b, &2);
    client.set_pair_weight(&Vec::new(&env), &pair_c, &1);

    client.submit_score(&Vec::new(&env), &wallet, &pair_a, &20, &false, &false, &1, &90, &1, &None);
    client.submit_score(&Vec::new(&env), &wallet, &pair_b, &80, &false, &false, &2, &90, &1, &None);
    client.submit_score(&Vec::new(&env), &wallet, &pair_c, &40, &false, &false, &3, &90, &1, &None);

    // (20*1 + 80*2 + 40*1) / (1 + 2 + 1) = 220 / 4 = 55
    assert_eq!(client.get_aggregate_score(&wallet).aggregate_score, 55);
}

#[test]
fn test_aggregate_max_pair_tracked() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let pair1 = symbol_short!("XLM_USDC");
    let pair2 = symbol_short!("XLM_BTC");

    client.submit_score(&Vec::new(&env), &wallet, &pair1, &30, &false, &false, &1, &90, &1, &None);
    client.submit_score(&Vec::new(&env), &wallet, &pair2, &90, &false, &false, &2, &90, &1, &None);

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

    client.submit_score(&Vec::new(&env), &wallet, &pair1, &30, &true, &false, &1, &90, &1, &None);
    client.submit_score(&Vec::new(&env), &wallet, &pair2, &60, &true, &true, &2, &90, &1, &None);
    client.submit_score(&Vec::new(&env), &wallet, &pair3, &90, &false, &false, &3, &90, &1, &None);

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

    client.submit_score(&Vec::new(&env), &wallet, &pair_a, &20, &false, &false, &1, &90, &1, &None);
    client.submit_score(&Vec::new(&env), &wallet, &pair_b, &40, &false, &false, &2, &90, &1, &None);
    assert_eq!(client.get_aggregate_score(&wallet).aggregate_score, 30);

    // Re-submitting pair A with a higher score must shift the aggregate.
    env.ledger().with_mut(|l| l.timestamp += 3_601); // past the default cooldown
    client.submit_score(&Vec::new(&env), &wallet, &pair_a, &80, &false, &false, &3, &90, &1, &None);
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
        env.ledger().with_mut(|l| l.timestamp += 3_601); // past the default cooldown
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &pair,
            &(50 + i as u32),
            &false,
            &false,
            &(i + 1),
            &90,
            &1,
            &None,
        );
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

    client.set_pair_weight(&Vec::new(&env), &pair_b, &0);

    client.submit_score(&Vec::new(&env), &wallet, &pair_a, &70, &false, &false, &1, &90, &1, &None);
    client.submit_score(&Vec::new(&env), &wallet, &pair_b, &10, &false, &false, &2, &90, &1, &None);

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
        client.set_pair_weight(&Vec::new(&env), &pair, &u32::MAX);
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &pair,
            &50,
            &false,
            &false,
            &(i as u64 + 1),
            &90,
            &1,
            &None,
        );
    }

    let result = client.try_get_aggregate_score(&wallet);
    assert_eq!(result, Err(Ok(Error::ArithmeticOverflow)));
}

#[test]
fn test_set_pair_weight() {
    let (env, client, _admin, _service) = initialized();
    let pair = symbol_short!("XLM_USDC");

    client.set_pair_weight(&Vec::new(&env), &pair, &3);
    assert_eq!(client.get_pair_weight(&pair), 3);
}

#[test]
fn test_get_pair_weight_defaults_to_one() {
    let (_env, client, _admin, _service) = initialized();
    let pair = symbol_short!("XLM_USDC");
    assert_eq!(client.get_pair_weight(&pair), 1);
}

#[test]
fn test_set_pair_weight_batch() {
    let (env, client, _admin, _service) = initialized();
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("XLM_BTC");

    let mut entries: Vec<(Symbol, u32)> = Vec::new(&env);
    entries.push_back((pair_a.clone(), 2));
    entries.push_back((pair_b.clone(), 5));

    client.set_pair_weight_batch(&Vec::new(&env), &entries);

    assert_eq!(client.get_pair_weight(&pair_a), 2);
    assert_eq!(client.get_pair_weight(&pair_b), 5);
}

#[test]
fn test_set_pair_weight_batch_partial_overlap_updates_only_changed_pairs() {
    let (env, client, _admin, _service) = initialized();
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("XLM_BTC");
    let pair_c = symbol_short!("XLM_ETH");

    client.set_pair_weight(&Vec::new(&env), &pair_a, &1);
    client.set_pair_weight(&Vec::new(&env), &pair_b, &1);

    // Second batch only touches pair_b and introduces pair_c; pair_a must
    // remain at its previously configured weight.
    let mut entries: Vec<(Symbol, u32)> = Vec::new(&env);
    entries.push_back((pair_b.clone(), 9));
    entries.push_back((pair_c.clone(), 4));

    client.set_pair_weight_batch(&Vec::new(&env), &entries);

    assert_eq!(client.get_pair_weight(&pair_a), 1);
    assert_eq!(client.get_pair_weight(&pair_b), 9);
    assert_eq!(client.get_pair_weight(&pair_c), 4);
}

#[test]
fn test_set_pair_weight_batch_empty_returns_error() {
    let (env, client, _admin, _service) = initialized();

    let empty: Vec<(Symbol, u32)> = Vec::new(&env);
    let result = client.try_set_pair_weight_batch(&Vec::new(&env), &empty);
    assert_eq!(result, Err(Ok(Error::EmptyBatch)));
}

#[test]
fn test_set_pair_weight_batch_too_large_returns_error() {
    let (env, client, _admin, _service) = initialized();

    let pair_names = [
        "P0", "P1", "P2", "P3", "P4", "P5", "P6", "P7", "P8", "P9", "PA", "PB", "PC", "PD", "PE",
        "PF", "PG", "PH", "PI", "PJ", "PK",
    ];
    assert_eq!(pair_names.len(), 21); // one over the MAX_BATCH_SIZE (20) cap.

    let mut entries: Vec<(Symbol, u32)> = Vec::new(&env);
    for name in pair_names.iter() {
        entries.push_back((Symbol::new(&env, name), 1));
    }

    let result = client.try_set_pair_weight_batch(&Vec::new(&env), &entries);
    assert_eq!(result, Err(Ok(Error::BatchTooLarge)));
}

// ── M-of-N multi-signature service authorization ──────────────────────────────

/// Helper: add N signers and set threshold M on an already-initialized client.
fn setup_multisig<'a>(
    env: &Env,
    client: &LedgerLensScoreContractClient<'a>,
    n: u32,
    m: u32,
) -> Vec<Address> {
    let mut signers: Vec<Address> = Vec::new(env);
    for _ in 0..n {
        let s = Address::generate(env);
        client.add_service_signer(&Vec::new(env), &s);
        signers.push_back(s);
    }
    client.set_service_threshold(&Vec::new(env), &m);
    signers
}

#[test]
fn test_multisig_submit_exactly_threshold() {
    // M=2, N=3; provide exactly 2 valid signers → accepted.
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);
    let signers = setup_multisig(&env, &client, 3, 2);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Pass only the first two (threshold == 2).
    let mut two: Vec<Address> = Vec::new(&env);
    two.push_back(signers.get(0).unwrap());
    two.push_back(signers.get(1).unwrap());

    client.submit_score(&two, &wallet, &pair, &55, &false, &false, &1, &80, &1, &None);
    assert_eq!(client.get_score(&wallet, &pair).score, 55);
}

#[test]
fn test_multisig_submit_above_threshold() {
    // M=2, N=3; provide all 3 signers → accepted.
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);
    let signers = setup_multisig(&env, &client, 3, 2);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.submit_score(&signers, &wallet, &pair, &70, &false, &false, &1, &80, &1, &None);
    assert_eq!(client.get_score(&wallet, &pair).score, 70);
}

#[test]
fn test_multisig_submit_below_threshold() {
    // M=2, N=3; provide 1 signer → InsufficientSigners.
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);
    let signers = setup_multisig(&env, &client, 3, 2);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let mut one: Vec<Address> = Vec::new(&env);
    one.push_back(signers.get(0).unwrap());

    let result =
        client.try_submit_score(&one, &wallet, &pair, &55, &false, &false, &1, &80, &1, &None);
    assert_eq!(result, Err(Ok(Error::InsufficientSigners)));
}

#[test]
fn test_multisig_unauthorized_signer_rejected() {
    // Address not in service set → UnauthorizedSigner.
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);
    setup_multisig(&env, &client, 3, 2);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let outsider = Address::generate(&env);
    let mut signers: Vec<Address> = Vec::new(&env);
    signers.push_back(outsider);
    signers.push_back(Address::generate(&env)); // also not in set

    let result =
        client.try_submit_score(&signers, &wallet, &pair, &55, &false, &false, &1, &80, &1, &None);
    assert_eq!(result, Err(Ok(Error::UnauthorizedSigner)));
}

#[test]
fn test_get_admin_signer_count_tracks_set_size() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    assert_eq!(client.get_admin_signer_count(), 0);

    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    client.add_admin_signer(&Vec::new(&env), &s1);
    client.add_admin_signer(&Vec::new(&env), &s2);
    assert_eq!(client.get_admin_signer_count(), 2);

    client.set_admin_threshold(&Vec::new(&env), &2);
    let mut admin_signers = Vec::new(&env);
    admin_signers.push_back(s1.clone());
    admin_signers.push_back(s2.clone());
    client.remove_admin_signer(&admin_signers, &s2);
    assert_eq!(client.get_admin_signer_count(), 1);
}

#[test]
fn test_add_signer_beyond_max_rejected() {
    // Adding an 11th signer → ServiceSetFull.
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    for _ in 0..10 {
        client.add_service_signer(&Vec::new(&env), &Address::generate(&env));
    }

    let eleventh = Address::generate(&env);
    let result = client.try_add_service_signer(&Vec::new(&env), &eleventh);
    assert_eq!(result, Err(Ok(Error::ServiceSetFull)));
}

#[test]
fn test_duplicate_signer_rejected() {
    // Adding the same address twice → SignerAlreadyInSet.
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let signer = Address::generate(&env);
    client.add_service_signer(&Vec::new(&env), &signer);

    let result = client.try_add_service_signer(&Vec::new(&env), &signer);
    assert_eq!(result, Err(Ok(Error::SignerAlreadyInSet)));
}

#[test]
fn test_remove_nonexistent_signer() {
    // Removing an address not in the set → SignerNotInSet.
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let outsider = Address::generate(&env);
    let result = client.try_remove_service_signer(&Vec::new(&env), &outsider);
    assert_eq!(result, Err(Ok(Error::SignerNotInSet)));
}

#[test]
fn test_threshold_zero_rejected() {
    // Setting threshold to 0 → InvalidThreshold.
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);
    client.add_service_signer(&Vec::new(&env), &Address::generate(&env));

    let result = client.try_set_service_threshold(&Vec::new(&env), &0);
    assert_eq!(result, Err(Ok(Error::InvalidThreshold)));
}

#[test]
fn test_threshold_above_set_size_rejected() {
    // N=2, threshold=3 → InvalidThreshold.
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);
    client.add_service_signer(&Vec::new(&env), &Address::generate(&env));
    client.add_service_signer(&Vec::new(&env), &Address::generate(&env));

    let result = client.try_set_service_threshold(&Vec::new(&env), &3);
    assert_eq!(result, Err(Ok(Error::InvalidThreshold)));
}

#[test]
fn test_1_of_1_behaves_like_original() {
    // Single signer, threshold=1 — backward-compatible with the old single-service path.
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);
    let signers = setup_multisig(&env, &client, 1, 1);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.submit_score(&signers, &wallet, &pair, &42, &false, &true, &100, &90, &1, &None);
    assert_eq!(client.get_score(&wallet, &pair).score, 42);
}

#[test]
fn test_remove_signer_reduces_set() {
    // Set shrinks after removal; threshold auto-adjusted if it exceeds new set size.
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    client.add_service_signer(&Vec::new(&env), &s1);
    client.add_service_signer(&Vec::new(&env), &s2);
    client.set_service_threshold(&Vec::new(&env), &2);

    // Remove one signer — threshold must auto-adjust from 2 to 1.
    client.remove_service_signer(&Vec::new(&env), &s2);

    let remaining = client.get_service_signers();
    assert_eq!(remaining.len(), 1);
    // Threshold auto-reduced to set size (1).
    assert_eq!(client.get_service_threshold(), 1);

    // Submission with the remaining single signer should succeed.
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let mut one: Vec<Address> = Vec::new(&env);
    one.push_back(s1);
    client.submit_score(&one, &wallet, &pair, &33, &false, &false, &1, &70, &1, &None);
    assert_eq!(client.get_score(&wallet, &pair).score, 33);
}

#[test]
fn test_get_service_signer_count_zero() {
    // No signers added — count must be 0.
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);
    assert_eq!(client.get_service_signer_count(), 0);
}

#[test]
fn test_get_service_signer_count_one() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);
    let s1 = Address::generate(&env);
    client.add_service_signer(&Vec::new(&env), &s1);
    assert_eq!(client.get_service_signer_count(), 1);
}

#[test]
fn test_get_service_signer_count_n() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);
    for _ in 0..3 {
        client.add_service_signer(&Vec::new(&env), &Address::generate(&env));
    }
    assert_eq!(client.get_service_signer_count(), 3);
    // Count decreases on removal.
    let signers = client.get_service_signers();
    client.remove_service_signer(&Vec::new(&env), &signers.get(0).unwrap());
    assert_eq!(client.get_service_signer_count(), 2);
}

// ── Staleness window ──────────────────────────────────────────────────────────

#[test]
fn test_is_score_stale_no_score() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    assert!(client.is_score_stale(&wallet, &pair));
}

#[test]
fn test_is_score_stale_fresh_score() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let ts: u64 = 1_700_000_000;
    env.ledger().with_mut(|l| l.timestamp = ts);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &50, &false, &false, &ts, &80, &1, &None);

    assert!(!client.is_score_stale(&wallet, &pair));
}

#[test]
fn test_is_score_stale_after_window() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let ts: u64 = 1_700_000_000;
    let window = client.get_staleness_window();

    env.ledger().with_mut(|l| l.timestamp = ts);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &50, &false, &false, &ts, &80, &1, &None);

    // Advance ledger past the window boundary.
    env.ledger().with_mut(|l| l.timestamp = ts + window + 1);
    assert!(client.is_score_stale(&wallet, &pair));
}

#[test]
fn test_is_score_stale_exactly_at_window() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let ts: u64 = 1_700_000_000;
    let window = client.get_staleness_window();

    env.ledger().with_mut(|l| l.timestamp = ts);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &50, &false, &false, &ts, &80, &1, &None);

    // Exactly at the window boundary: age == window, not > window → fresh.
    env.ledger().with_mut(|l| l.timestamp = ts + window);
    assert!(!client.is_score_stale(&wallet, &pair));
}

#[test]
fn test_set_staleness_window_zero_rejected() {
    let (env, client, _admin, _service) = initialized();
    let result = client.try_set_staleness_window(&Vec::new(&env), &0);
    assert_eq!(result, Err(Ok(Error::InvalidStalenessWindow)));
}

#[test]
fn test_default_staleness_window_is_7_days() {
    let (_env, client, _admin, _service) = initialized();
    assert_eq!(client.get_staleness_window(), 604_800);
}

// ── Score count ───────────────────────────────────────────────────────────────

#[test]
fn test_score_count_starts_at_zero() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    assert_eq!(client.get_score_count(&wallet, &asset_pair), 0);
}

#[test]
fn test_score_count_increments_on_submit() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &30,
        &false,
        &false,
        &1,
        &60,
        &1,
        &None,
    );
    assert_eq!(client.get_score_count(&wallet, &asset_pair), 1);

    env.ledger().with_mut(|l| l.timestamp += 3_601);
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &50,
        &false,
        &false,
        &2,
        &70,
        &1,
        &None,
    );
    assert_eq!(client.get_score_count(&wallet, &asset_pair), 2);
}

#[test]
fn test_score_count_exceeds_history_depth() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    // Submit 15 scores — the ring buffer caps at 10, but count should be 15.
    for i in 0u32..15 {
        env.ledger().with_mut(|l| l.timestamp += 3_601); // past the default cooldown
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &asset_pair,
            &(i * 5),
            &false,
            &false,
            &(i as u64 + 1),
            &50,
            &1,
            &None,
        );
    }

    assert_eq!(client.get_score_count(&wallet, &asset_pair), 15);

    // Confirm the history ring is capped at 10.
    let history = client.get_score_history(&wallet, &asset_pair);
    assert_eq!(history.len(), 10);
}

#[test]
fn test_score_count_increments_via_batch() {
    let (env, client, _admin, _service) = initialized();

    let wallet1 = Address::generate(&env);
    let wallet2 = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: wallet1.clone(),
        asset_pair: asset_pair.clone(),
        score: 30,
        benford_flag: false,
        ml_flag: false,
        timestamp: 1,
        confidence: 60,
        model_version: 1,
    });
    batch.push_back(ScoreSubmission {
        wallet: wallet2.clone(),
        asset_pair: asset_pair.clone(),
        score: 70,
        benford_flag: false,
        ml_flag: false,
        timestamp: 2,
        confidence: 80,
        model_version: 1,
    });

    let result: BatchResult = client.submit_scores_batch(&batch);
    assert_eq!(result.accepted_count, 2);

    assert_eq!(client.get_score_count(&wallet1, &asset_pair), 1);
    assert_eq!(client.get_score_count(&wallet2, &asset_pair), 1);
}

#[test]
fn test_score_count_is_per_pair() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let pair1 = symbol_short!("XLM_USDC");
    let pair2 = symbol_short!("XLM_BTC");

    client.submit_score(&Vec::new(&env), &wallet, &pair1, &30, &false, &false, &1, &60, &1, &None);
    env.ledger().with_mut(|l| l.timestamp += 3_601);
    client.submit_score(&Vec::new(&env), &wallet, &pair1, &40, &false, &false, &2, &70, &1, &None);
    client.submit_score(&Vec::new(&env), &wallet, &pair2, &90, &true, &true, &3, &95, &1, &None);

    assert_eq!(client.get_score_count(&wallet, &pair1), 2);
    assert_eq!(client.get_score_count(&wallet, &pair2), 1);
}

// ── Wallet pair list / count ──────────────────────────────────────────────────

#[test]
fn test_wallet_pair_list_empty_initially() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pairs = client.get_wallet_pair_list(&wallet);
    assert_eq!(pairs.len(), 0);
}

#[test]
fn test_wallet_pair_count_empty_initially() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    assert_eq!(client.get_wallet_pair_count(&wallet), 0);
}

#[test]
fn test_wallet_pair_list_single_pair() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    client.submit_score(&Vec::new(&env), &wallet, &pair, &50, &false, &false, &1, &90, &1, &None);
    let pairs = client.get_wallet_pair_list(&wallet);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs.get(0).unwrap(), pair);
}

#[test]
fn test_wallet_pair_count_single_pair() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    client.submit_score(&Vec::new(&env), &wallet, &pair, &50, &false, &false, &1, &90, &1, &None);
    assert_eq!(client.get_wallet_pair_count(&wallet), 1);
}

#[test]
fn test_wallet_pair_list_multiple_pairs() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair1 = symbol_short!("XLM_USDC");
    let pair2 = symbol_short!("XLM_BTC");
    let pair3 = symbol_short!("XLM_ETH");

    client.submit_score(&Vec::new(&env), &wallet, &pair1, &50, &false, &false, &1, &90, &1, &None);
    client.submit_score(&Vec::new(&env), &wallet, &pair2, &30, &false, &true, &2, &85, &1, &None);
    client.submit_score(&Vec::new(&env), &wallet, &pair3, &70, &true, &true, &3, &95, &1, &None);

    let pairs = client.get_wallet_pair_list(&wallet);
    assert_eq!(pairs.len(), 3);

    // All three pairs should be present (order may vary).
    let mut found = 0u32;
    for i in 0..pairs.len() {
        let p = pairs.get(i).unwrap();
        if p == pair1 || p == pair2 || p == pair3 {
            found += 1;
        }
    }
    assert_eq!(found, 3);
}

#[test]
fn test_wallet_pair_count_multiple_pairs() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &symbol_short!("XLM_USDC"),
        &50,
        &false,
        &false,
        &1,
        &90,
        &1,
        &None,
    );
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &symbol_short!("XLM_BTC"),
        &30,
        &false,
        &true,
        &2,
        &85,
        &1,
        &None,
    );
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &symbol_short!("XLM_ETH"),
        &70,
        &true,
        &true,
        &3,
        &95,
        &1,
        &None,
    );

    assert_eq!(client.get_wallet_pair_count(&wallet), 3);
}

#[test]
fn test_wallet_pair_list_is_per_wallet() {
    let (env, client, _admin, _service) = initialized();
    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.submit_score(&Vec::new(&env), &wallet_a, &pair, &50, &false, &false, &1, &90, &1, &None);

    assert_eq!(client.get_wallet_pair_list(&wallet_a).len(), 1);
    assert_eq!(client.get_wallet_pair_list(&wallet_b).len(), 0);
    assert_eq!(client.get_wallet_pair_count(&wallet_a), 1);
    assert_eq!(client.get_wallet_pair_count(&wallet_b), 0);
}

#[test]
fn test_wallet_pair_list_deduplicates() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Submit twice for the same pair — should still count as 1.
    client.submit_score(&Vec::new(&env), &wallet, &pair, &50, &false, &false, &1, &90, &1, &None);
    env.ledger().with_mut(|l| l.timestamp += 3_601);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &60, &false, &false, &2, &90, &1, &None);

    assert_eq!(client.get_wallet_pair_list(&wallet).len(), 1);
    assert_eq!(client.get_wallet_pair_count(&wallet), 1);
}

#[test]
fn test_set_staleness_window_updates_stale_check() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let ts: u64 = 1_700_000_000;
    env.ledger().with_mut(|l| l.timestamp = ts);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &50, &false, &false, &ts, &80, &1, &None);

    // Set a very narrow window (10 seconds).
    client.set_staleness_window(&Vec::new(&env), &10);

    // Advance by 11 seconds — should be stale now.
    env.ledger().with_mut(|l| l.timestamp = ts + 11);
    assert!(client.is_score_stale(&wallet, &pair));
}

// ── GDPR / data-erasure ───────────────────────────────────────────────────────

#[test]
fn test_clear_score_history_removes_all_entries() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.submit_score(&Vec::new(&env), &wallet, &pair, &10, &false, &false, &1, &50, &1, &None);
    env.ledger().with_mut(|l| l.timestamp += 3_601);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &20, &false, &false, &2, &60, &1, &None);

    assert_eq!(client.get_score_history(&wallet, &pair).len(), 2);
    client.clear_score_history(&Vec::new(&env), &wallet, &pair);
    assert_eq!(client.get_score_history(&wallet, &pair).len(), 0);
}

#[test]
fn test_clear_score_history_on_empty_is_noop() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Should not panic when no history exists.
    client.clear_score_history(&Vec::new(&env), &wallet, &pair);
    assert_eq!(client.get_score_history(&wallet, &pair).len(), 0);
}

#[test]
fn test_clear_score_removes_latest_score() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.submit_score(&Vec::new(&env), &wallet, &pair, &42, &false, &false, &1, &80, &1, &None);
    client.clear_score(&Vec::new(&env), &wallet, &pair);

    let result = client.try_get_score(&wallet, &pair);
    assert_eq!(result, Err(Ok(Error::ScoreNotFound)));
}

#[test]
fn test_clear_score_on_nonexistent_is_noop() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Should not panic when no score exists.
    client.clear_score(&Vec::new(&env), &wallet, &pair);
    let result = client.try_get_score(&wallet, &pair);
    assert_eq!(result, Err(Ok(Error::ScoreNotFound)));
}

#[test]
fn test_clear_score_does_not_affect_other_pairs() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("XLM_BTC");

    client.submit_score(&Vec::new(&env), &wallet, &pair_a, &10, &false, &false, &1, &50, &1, &None);
    client.submit_score(&Vec::new(&env), &wallet, &pair_b, &20, &false, &false, &1, &60, &1, &None);

    client.clear_score(&Vec::new(&env), &wallet, &pair_a);

    assert_eq!(client.try_get_score(&wallet, &pair_a), Err(Ok(Error::ScoreNotFound)));
    assert_eq!(client.get_score(&wallet, &pair_b).score, 20);
}

#[test]
fn test_clear_history_does_not_affect_latest_score() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.submit_score(&Vec::new(&env), &wallet, &pair, &55, &false, &false, &1, &70, &1, &None);
    client.clear_score_history(&Vec::new(&env), &wallet, &pair);

    // Latest score must still be retrievable.
    assert_eq!(client.get_score(&wallet, &pair).score, 55);
    // History is gone.
    assert_eq!(client.get_score_history(&wallet, &pair).len(), 0);
}

#[test]
fn test_clear_score_does_not_affect_history() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.submit_score(&Vec::new(&env), &wallet, &pair, &33, &false, &false, &1, &80, &1, &None);
    client.clear_score(&Vec::new(&env), &wallet, &pair);

    // History ring must still contain the entry.
    assert_eq!(client.get_score_history(&wallet, &pair).len(), 1);
    assert_eq!(client.get_score_history(&wallet, &pair).get(0).unwrap().score, 33);
}

// ── Wallet Score Delegation ───────────────────────────────────────────────────

#[test]
fn test_delegate_inherits_custodian_score() {
    let (env, client, _admin, _service) = initialized();
    let custodian = Address::generate(&env);
    let sub_wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // 1. Set delegate
    client.set_score_delegate(&sub_wallet, &custodian);

    // 2. Submit score to custodian
    client.submit_score(
        &Vec::new(&env),
        &custodian,
        &pair,
        &40,
        &false,
        &false,
        &1,
        &80,
        &1,
        &None,
    );

    // 3. Sub-wallet inherits score
    let score = client.get_score(&sub_wallet, &pair);
    assert_eq!(score.score, 40);

    // 4. Sub-wallet inherits gate check
    let is_safe = client.query_risk_gate(&sub_wallet, &pair, &50);
    assert!(is_safe);

    // 5. Sub-wallet inherits aggregate score
    let aggregate = client.get_aggregate_score(&sub_wallet);
    assert_eq!(aggregate.aggregate_score, 40);
}

#[test]
fn test_delegate_direct_score_overrides_delegation() {
    let (env, client, _admin, _service) = initialized();
    let custodian = Address::generate(&env);
    let sub_wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_score_delegate(&sub_wallet, &custodian);
    client.submit_score(
        &Vec::new(&env),
        &custodian,
        &pair,
        &80,
        &false,
        &false,
        &1,
        &80,
        &1,
        &None,
    );

    // Sub-wallet overrides with its own score
    client.submit_score(
        &Vec::new(&env),
        &sub_wallet,
        &pair,
        &10,
        &false,
        &false,
        &1,
        &80,
        &1,
        &None,
    );

    let score = client.get_score(&sub_wallet, &pair);
    assert_eq!(score.score, 10); // Not 80
}

#[test]
fn test_cyclic_delegation_rejected() {
    let (env, client, _admin, _service) = initialized();
    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);

    // A -> A is rejected
    assert_eq!(
        client.try_set_score_delegate(&wallet_a, &wallet_a),
        Err(Ok(Error::CyclicDelegation))
    );

    // A -> B -> A is rejected
    client.set_score_delegate(&wallet_a, &wallet_b);
    assert_eq!(
        client.try_set_score_delegate(&wallet_b, &wallet_a),
        Err(Ok(Error::CyclicDelegation))
    );
}

#[test]
fn test_remove_delegate_clears_fallback() {
    let (env, client, _admin, _service) = initialized();
    let custodian = Address::generate(&env);
    let sub_wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_score_delegate(&sub_wallet, &custodian);
    client.submit_score(
        &Vec::new(&env),
        &custodian,
        &pair,
        &40,
        &false,
        &false,
        &1,
        &80,
        &1,
        &None,
    );

    // Works with delegate
    assert_eq!(client.get_score(&sub_wallet, &pair).score, 40);

    client.remove_score_delegate(&sub_wallet);

    // Fallback is gone
    assert_eq!(client.try_get_score(&sub_wallet, &pair), Err(Ok(Error::ScoreNotFound)));
}

#[test]
fn test_delegate_propagates_embargo() {
    let (env, client, _admin, _service) = initialized();
    let custodian = Address::generate(&env);
    let sub_wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_score_delegate(&sub_wallet, &custodian);
    // Submit embargo score (e.g. 90) which is >= gate_threshold of 75
    client.submit_score(
        &Vec::new(&env),
        &custodian,
        &pair,
        &90,
        &false,
        &false,
        &1,
        &80,
        &1,
        &None,
    );

    let is_safe = client.query_risk_gate(&sub_wallet, &pair, &75);
    assert!(!is_safe); // Embargo propagates
}

#[test]
fn test_delegate_snapshot() {
    let (env, client, _admin, _service) = initialized();
    let custodian = Address::generate(&env);
    let sub_wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_score_delegate(&sub_wallet, &custodian);
    client.submit_score(
        &Vec::new(&env),
        &custodian,
        &pair,
        &20,
        &false,
        &false,
        &1,
        &80,
        &1,
        &None,
    );

    assert_eq!(client.get_score(&sub_wallet, &pair).score, 20);

    // Update custodian's score
    env.ledger().with_mut(|l| l.timestamp += 3_601);
    client.submit_score(
        &Vec::new(&env),
        &custodian,
        &pair,
        &50,
        &false,
        &false,
        &2,
        &80,
        &1,
        &None,
    );

    // Sub-wallet immediately sees the new score without any update to itself
    assert_eq!(client.get_score(&sub_wallet, &pair).score, 50);
}

// ─── Wallet Relationship Graph Tests ────────────────────────────────────────

#[test]
fn test_add_link_is_bidirectional() {
    let (env, client, _admin, _service) = initialized();

    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    client.add_counterparty_link(&wallet_a, &wallet_b, &asset_pair);

    let links_a = client.get_counterparties(&wallet_a, &asset_pair);
    let links_b = client.get_counterparties(&wallet_b, &asset_pair);

    assert_eq!(links_a.len(), 1);
    assert_eq!(links_b.len(), 1);
    assert_eq!(links_a.get(0).unwrap(), wallet_b);
    assert_eq!(links_b.get(0).unwrap(), wallet_a);
}

#[test]
fn test_self_link_rejected() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    let result = client.try_add_counterparty_link(&wallet, &wallet, &asset_pair);
    assert_eq!(result, Err(Ok(Error::CounterpartyLinkFull)));
}

#[test]
fn test_contagion_boosts_linked_wallets() {
    let (env, client, _admin, _service) = initialized();

    let anchor = Address::generate(&env);
    let counterparty = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    // Submit a score for the counterparty (score: 40)
    client.submit_score(
        &Vec::new(&env),
        &counterparty,
        &asset_pair,
        &40,
        &false,
        &false,
        &1,
        &80,
        &1,
        &None,
    );

    // Add counterparty link
    client.add_counterparty_link(&anchor, &counterparty, &asset_pair);

    // Propagate contagion with boost of 30
    let affected = client.propagate_contagion(&anchor, &asset_pair, &30);

    assert_eq!(affected, 1);

    // Counterparty score should be boosted to 70 (40 + 30)
    let score = client.get_score(&counterparty, &asset_pair);
    assert_eq!(score.score, 70);
}

#[test]
fn test_contagion_boost_capped_at_100() {
    let (env, client, _admin, _service) = initialized();

    let anchor = Address::generate(&env);
    let counterparty = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    // Submit a score for the counterparty (score: 90)
    client.submit_score(
        &Vec::new(&env),
        &counterparty,
        &asset_pair,
        &90,
        &false,
        &false,
        &1,
        &80,
        &1,
        &None,
    );

    // Add counterparty link
    client.add_counterparty_link(&anchor, &counterparty, &asset_pair);

    // Propagate contagion with boost of 30 (should cap at 100)
    let affected = client.propagate_contagion(&anchor, &asset_pair, &30);

    assert_eq!(affected, 1);

    // Counterparty score should be capped at 100
    let score = client.get_score(&counterparty, &asset_pair);
    assert_eq!(score.score, 100);
}

#[test]
fn test_contagion_missing_score_treated_as_zero() {
    let (env, client, _admin, _service) = initialized();

    let anchor = Address::generate(&env);
    let counterparty = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    // No score submitted for counterparty

    // Add counterparty link
    client.add_counterparty_link(&anchor, &counterparty, &asset_pair);

    // Propagate contagion with boost of 50
    let affected = client.propagate_contagion(&anchor, &asset_pair, &50);

    assert_eq!(affected, 1);

    // Counterparty score should be 50 (0 + 50)
    let score = client.get_score(&counterparty, &asset_pair);
    assert_eq!(score.score, 50);
}

#[test]
fn test_counterparty_link_cap_enforced() {
    let (env, client, _admin, _service) = initialized();

    let wallet_a = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    // Add 50 counterparties (MAX_COUNTERPARTY_LINKS_PER_WALLET = 50)
    for _i in 0..50 {
        let counterparty = Address::generate(&env);
        client.add_counterparty_link(&wallet_a, &counterparty, &asset_pair);
    }

    let links = client.get_counterparties(&wallet_a, &asset_pair);
    assert_eq!(links.len(), 50);

    // The 51st should fail
    let extra = Address::generate(&env);
    let result = client.try_add_counterparty_link(&wallet_a, &extra, &asset_pair);
    assert_eq!(result, Err(Ok(Error::ServiceSetFull)));
}

#[test]
fn test_contagion_events_per_affected_wallet() {
    let (env, client, _admin, _service) = initialized();

    let anchor = Address::generate(&env);
    let counterparty1 = Address::generate(&env);
    let counterparty2 = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    client.add_counterparty_link(&anchor, &counterparty1, &asset_pair);
    client.add_counterparty_link(&anchor, &counterparty2, &asset_pair);

    // Submit scores for both counterparties
    client.submit_score(
        &Vec::new(&env),
        &counterparty1,
        &asset_pair,
        &20,
        &false,
        &false,
        &1,
        &80,
        &1,
        &None,
    );
    client.submit_score(
        &Vec::new(&env),
        &counterparty2,
        &asset_pair,
        &30,
        &false,
        &false,
        &1,
        &80,
        &1,
        &None,
    );

    let affected = client.propagate_contagion(&anchor, &asset_pair, &40);

    assert_eq!(affected, 2);

    // Both scores should be boosted
    let score1 = client.get_score(&counterparty1, &asset_pair);
    let score2 = client.get_score(&counterparty2, &asset_pair);
    assert_eq!(score1.score, 60); // 20 + 40
    assert_eq!(score2.score, 70); // 30 + 40
}

#[test]
fn test_contagion_does_not_affect_unlinked_wallets() {
    let (env, client, _admin, _service) = initialized();

    let anchor = Address::generate(&env);
    let linked = Address::generate(&env);
    let unlinked = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    client.add_counterparty_link(&anchor, &linked, &asset_pair);

    // Submit scores for both wallets
    client.submit_score(
        &Vec::new(&env),
        &linked,
        &asset_pair,
        &20,
        &false,
        &false,
        &1,
        &80,
        &1,
        &None,
    );
    client.submit_score(
        &Vec::new(&env),
        &unlinked,
        &asset_pair,
        &20,
        &false,
        &false,
        &1,
        &80,
        &1,
        &None,
    );

    let affected = client.propagate_contagion(&anchor, &asset_pair, &50);

    assert_eq!(affected, 1);

    // Linked wallet boosted, unlinked unchanged
    let linked_score = client.get_score(&linked, &asset_pair);
    let unlinked_score = client.get_score(&unlinked, &asset_pair);
    assert_eq!(linked_score.score, 70); // 20 + 50
    assert_eq!(unlinked_score.score, 20); // Unchanged
}

#[test]
fn test_contagion_bypasses_rate_limits() {
    let (env, client, _admin, _service) = initialized();

    let anchor = Address::generate(&env);
    let counterparty = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    // Add link and submit score
    client.add_counterparty_link(&anchor, &counterparty, &asset_pair);
    client.submit_score(
        &Vec::new(&env),
        &counterparty,
        &asset_pair,
        &30,
        &false,
        &false,
        &1,
        &80,
        &1,
        &None,
    );

    // Verify the score was set
    let initial_score = client.get_score(&counterparty, &asset_pair);
    assert_eq!(initial_score.score, 30);

    // Contagion propagation works regardless of cooldown
    let affected = client.propagate_contagion(&anchor, &asset_pair, &40);
    assert_eq!(affected, 1);

    let score = client.get_score(&counterparty, &asset_pair);
    assert_eq!(score.score, 70); // 30 + 40, despite cooldown not having elapsed
}

#[test]
fn test_contagion_depth_returns_graph_degree() {
    let (env, client, _admin, _service) = initialized();

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    // Initially 0
    assert_eq!(client.get_contagion_depth(&wallet, &asset_pair), 0);

    // Add 3 counterparties
    let c1 = Address::generate(&env);
    let c2 = Address::generate(&env);
    let c3 = Address::generate(&env);

    client.add_counterparty_link(&wallet, &c1, &asset_pair);
    assert_eq!(client.get_contagion_depth(&wallet, &asset_pair), 1);

    client.add_counterparty_link(&wallet, &c2, &asset_pair);
    assert_eq!(client.get_contagion_depth(&wallet, &asset_pair), 2);

    client.add_counterparty_link(&wallet, &c3, &asset_pair);
    assert_eq!(client.get_contagion_depth(&wallet, &asset_pair), 3);
}

#[test]
fn test_remove_counterparty_link() {
    let (env, client, _admin, _service) = initialized();

    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    client.add_counterparty_link(&wallet_a, &wallet_b, &asset_pair);

    // Verify link exists
    let links_a = client.get_counterparties(&wallet_a, &asset_pair);
    assert_eq!(links_a.len(), 1);

    // Remove the link
    client.remove_counterparty_link(&wallet_a, &wallet_b, &asset_pair);

    let links_a = client.get_counterparties(&wallet_a, &asset_pair);
    let links_b = client.get_counterparties(&wallet_b, &asset_pair);
    assert_eq!(links_a.len(), 0);
    assert_eq!(links_b.len(), 0);
}

#[test]
fn test_remove_nonexistent_link_fails() {
    let (env, client, _admin, _service) = initialized();

    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    let result = client.try_remove_counterparty_link(&wallet_a, &wallet_b, &asset_pair);
    assert_eq!(result, Err(Ok(Error::CounterpartyLinkFull)));
}

// ── get_score_variance tests ──────────────────────────────────────────────────

#[test]
fn test_get_score_variance_empty() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");
    assert_eq!(client.get_score_variance(&wallet, &asset_pair), 0);
}

#[test]
fn test_get_score_variance_single_entry() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &50,
        &false,
        &false,
        &1,
        &80,
        &1,
        &None,
    );
    assert_eq!(client.get_score_variance(&wallet, &asset_pair), 0);
}

#[test]
fn test_get_score_variance_identical_scores() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");
    for i in 0u32..4 {
        env.ledger().with_mut(|l| l.timestamp += 3_601);
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &asset_pair,
            &42,
            &false,
            &false,
            &(i as u64 + 1),
            &80,
            &1,
            &None,
        );
    }
    assert_eq!(client.get_score_variance(&wallet, &asset_pair), 0);
}

#[test]
fn test_get_score_variance_known_values() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");
    // scores: [2, 4, 6, 8]; mean=5; var=((3^2)+(1^2)+(1^2)+(3^2))/4 = 20/4 = 5; scaled = 5*100 = 500
    for i in 0u32..4 {
        env.ledger().with_mut(|l| l.timestamp += 3_601);
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &asset_pair,
            &(2 + i * 2),
            &false,
            &false,
            &(i as u64 + 1),
            &80,
            &1,
            &None,
        );
    }
    assert_eq!(client.get_score_variance(&wallet, &asset_pair), 500);
}

#[test]
fn test_get_score_variance_embargoed() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");
    env.ledger().with_mut(|l| l.timestamp += 3_601);
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &10,
        &false,
        &false,
        &1,
        &80,
        &1,
        &None,
    );
    env.ledger().with_mut(|l| l.timestamp += 3_601);
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &20,
        &false,
        &false,
        &2,
        &80,
        &1,
        &None,
    );
    let admin = client.get_admin();
    env.ledger().with_mut(|l| l.timestamp += 1);
    client.set_score_embargo(&wallet, &None);
    assert_eq!(client.get_score_variance(&wallet, &asset_pair), 0);
}

// ── Differential privacy ──────────────────────────────────────────────────────

#[test]
fn test_set_get_privacy_epsilon() {
    let (env, client, _admin, _service) = initialized();
    assert_eq!(client.get_privacy_epsilon(), 0);

    client.set_privacy_epsilon(&Vec::new(&env), &100);
    assert_eq!(client.get_privacy_epsilon(), 100);

    client.set_privacy_epsilon(&Vec::new(&env), &1);
    assert_eq!(client.get_privacy_epsilon(), 1);

    client.set_privacy_epsilon(&Vec::new(&env), &0);
    assert_eq!(client.get_privacy_epsilon(), 0);
}

#[test]
fn test_private_aggregate_score_differs_from_exact() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 100_000);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &70, &true, &false, &100_000, &80, &1, &None);

    let exact = client.get_aggregate_score(&wallet).aggregate_score;
    assert_eq!(exact, 70);

    // With epsilon = 0, private = exact
    assert_eq!(client.get_private_aggregate_score(&wallet, &0), exact);

    // With epsilon = 100 (ε=1.0), noise is applied
    client.set_privacy_epsilon(&Vec::new(&env), &100);
    let private = client.get_private_aggregate_score(&wallet, &0);
    assert_ne!(private, exact, "noised score should differ from exact");
}

#[test]
fn test_private_aggregate_noise_within_bounds() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 100_000);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &50, &false, &false, &100_000, &80, &1, &None);

    let exact = client.get_aggregate_score(&wallet).aggregate_score;
    client.set_privacy_epsilon(&Vec::new(&env), &100); // ε = 1.0

    for seed in 0u32..10 {
        let private = client.get_private_aggregate_score(&wallet, &seed);
        assert!(private <= 100, "score must not exceed 100");
        let diff = (private as i64 - exact as i64).abs();
        assert!(diff <= 300, "noise must be within ±3*S/ε");
    }
}

#[test]
fn test_private_aggregate_reproducible() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 100_000);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &60, &false, &true, &100_000, &90, &1, &None);

    client.set_privacy_epsilon(&Vec::new(&env), &100);

    // Same ledger sequence + seed → same result
    let a = client.get_private_aggregate_score(&wallet, &42);
    let b = client.get_private_aggregate_score(&wallet, &42);
    assert_eq!(a, b, "noise must be deterministic per (ledger_seq, seed)");
}

#[test]
fn test_private_aggregate_different_seeds_differ() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 100_000);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &50, &true, &false, &100_000, &85, &1, &None);

    client.set_privacy_epsilon(&Vec::new(&env), &100);

    let a = client.get_private_aggregate_score(&wallet, &1);
    let b = client.get_private_aggregate_score(&wallet, &2);
    assert!(
        a != b || client.get_private_aggregate_score(&wallet, &3) != b,
        "different seeds should eventually produce different noise"
    );
}

#[test]
fn test_private_aggregate_zero_when_no_scores() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);

    client.set_privacy_epsilon(&Vec::new(&env), &100);
    assert_eq!(client.get_private_aggregate_score(&wallet, &0), 0);
}

#[test]
fn test_private_aggregate_different_ledger_seq_differs() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.ledger().with_mut(|l| l.timestamp = 100_000);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &40, &false, &false, &100_000, &75, &1, &None);

    client.set_privacy_epsilon(&Vec::new(&env), &100);

    env.ledger().with_mut(|l| l.sequence_number = 1);
    let a = client.get_private_aggregate_score(&wallet, &0);

    env.ledger().with_mut(|l| l.sequence_number = 2);
    let b = client.get_private_aggregate_score(&wallet, &0);

    assert_ne!(
        a, b,
        "different ledger sequences should produce different noise"
    );
}

#[test]
fn test_supports_interface_dprv() {
    let (_env, client, _admin, _service) = initialized();

    assert!(client.supports_interface(&symbol_short!("dprv")));
    assert!(client.supports_interface(&symbol_short!("score")));
    assert!(!client.supports_interface(&Symbol::new(&_env, "nonexistent")));
}

#[test]
fn test_set_privacy_epsilon_not_initialized_fails() {
    let (env, client, _admin, _service) = setup();
    let result = client.try_set_privacy_epsilon(&Vec::new(&env), &100);
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_private_aggregate_score_not_initialized_fails() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    assert_eq!(client.get_private_aggregate_score(&wallet, &0), 0);
}
