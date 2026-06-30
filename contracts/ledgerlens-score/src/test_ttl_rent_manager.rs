use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::constants::SCORE_TTL_THRESHOLD;
use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient};

const START_SEQ: u32 = 1_000;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_sequence_number(START_SEQ);
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    // The test sandbox doesn't auto-extend the contract's own instance TTL on
    // every call (this mirrors real Soroban, where an operator must
    // periodically run `stellar contract extend` to keep a deployed
    // contract's instance entry alive). These tests jump the ledger sequence
    // far ahead to simulate dormant score entries, so bump the instance TTL
    // once up front to keep the contract itself reachable across that jump.
    env.as_contract(&contract_id, || {
        env.storage().instance().extend_ttl(1_000_000, 6_000_000);
    });

    (env, client, admin, service)
}

#[test]
fn test_get_entry_ttl_unknown_entry_errors() {
    let (env, client, _, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    assert_eq!(client.try_get_entry_ttl(&wallet, &pair), Err(Ok(Error::ScoreNotFound)));
}

#[test]
fn test_get_entry_ttl_fresh_submission_is_full_threshold() {
    let (env, client, _, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    client.submit_score(&Vec::new(&env), &wallet, &pair, &42, &true, &false, &1, &90, &1, &None);
    assert_eq!(client.get_entry_ttl(&wallet, &pair), SCORE_TTL_THRESHOLD);
}

#[test]
fn test_get_entry_ttl_decreases_with_elapsed_ledgers() {
    let (env, client, _, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    client.submit_score(&Vec::new(&env), &wallet, &pair, &42, &true, &false, &1, &90, &1, &None);

    env.ledger().set_sequence_number(START_SEQ + 1_000);
    assert_eq!(client.get_entry_ttl(&wallet, &pair), SCORE_TTL_THRESHOLD - 1_000);
}

#[test]
fn test_get_entry_ttl_floors_at_zero_past_threshold() {
    let (env, client, _, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    client.submit_score(&Vec::new(&env), &wallet, &pair, &42, &true, &false, &1, &90, &1, &None);

    env.ledger().set_sequence_number(START_SEQ + SCORE_TTL_THRESHOLD + 50_000);
    assert_eq!(client.get_entry_ttl(&wallet, &pair), 0);
}

#[test]
fn test_get_expiring_entries_empty_for_fresh_submission() {
    let (env, client, _, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    client.submit_score(&Vec::new(&env), &wallet, &pair, &42, &true, &false, &1, &90, &1, &None);

    assert!(client.get_expiring_entries(&50).is_empty());
}

#[test]
fn test_get_expiring_entries_finds_dormant_entry() {
    let (env, client, _, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    client.submit_score(&Vec::new(&env), &wallet, &pair, &42, &true, &false, &1, &90, &1, &None);

    env.ledger().set_sequence_number(START_SEQ + SCORE_TTL_THRESHOLD);
    let due = client.get_expiring_entries(&50);
    assert_eq!(due.len(), 1);
    assert_eq!(due.get(0).unwrap(), (wallet, pair));
}

#[test]
fn test_get_expiring_entries_ignores_recently_touched_entry() {
    let (env, client, _, _) = setup();
    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // A is written first, then goes dormant.
    client.submit_score(&Vec::new(&env), &wallet_a, &pair, &42, &true, &false, &1, &90, &1, &None);

    env.ledger().set_sequence_number(START_SEQ + SCORE_TTL_THRESHOLD);

    // B is written right before the sweep — it should not show up as due.
    client.submit_score(&Vec::new(&env), &wallet_b, &pair, &42, &true, &false, &1, &90, &1, &None);

    let due = client.get_expiring_entries(&50);
    assert_eq!(due.len(), 1);
    assert_eq!(due.get(0).unwrap(), (wallet_a, pair));
}

#[test]
fn test_get_expiring_entries_orders_most_overdue_first() {
    let (env, client, _, _) = setup();
    let pair = symbol_short!("XLM_USDC");
    let older = Address::generate(&env);
    client.submit_score(&Vec::new(&env), &older, &pair, &10, &false, &false, &1, &90, &1, &None);

    env.ledger().set_sequence_number(START_SEQ + 500);
    let newer = Address::generate(&env);
    client.submit_score(&Vec::new(&env), &newer, &pair, &10, &false, &false, &1, &90, &1, &None);

    env.ledger().set_sequence_number(START_SEQ + SCORE_TTL_THRESHOLD + 500);
    let due = client.get_expiring_entries(&50);
    assert_eq!(due.len(), 2);
    // `older` has been dormant longer, so it must be the most urgent entry.
    assert_eq!(due.get(0).unwrap(), (older, pair.clone()));
    assert_eq!(due.get(1).unwrap(), (newer, pair));
}

#[test]
fn test_get_expiring_entries_respects_max_entries_cap() {
    let (env, client, _, _) = setup();
    let pair = symbol_short!("XLM_USDC");
    for _ in 0..5 {
        let wallet = Address::generate(&env);
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &pair,
            &10,
            &false,
            &false,
            &1,
            &90,
            &1,
            &None,
        );
    }

    env.ledger().set_sequence_number(START_SEQ + SCORE_TTL_THRESHOLD);
    let due = client.get_expiring_entries(&3);
    assert_eq!(due.len(), 3);
}

#[test]
fn test_extend_entry_ttls_renews_dormant_entry() {
    let (env, client, _, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    client.submit_score(&Vec::new(&env), &wallet, &pair, &42, &true, &false, &1, &90, &1, &None);

    env.ledger().set_sequence_number(START_SEQ + SCORE_TTL_THRESHOLD);
    assert_eq!(client.get_expiring_entries(&50).len(), 1);

    let mut entries = Vec::new(&env);
    entries.push_back((wallet.clone(), pair.clone()));
    let renewed = client.extend_entry_ttls(&Vec::new(&env), &entries);
    assert_eq!(renewed, 1);

    // Renewal resets the dormancy clock, so the entry is no longer "due".
    assert!(client.get_expiring_entries(&50).is_empty());
    assert_eq!(client.get_entry_ttl(&wallet, &pair), SCORE_TTL_THRESHOLD);
}

#[test]
fn test_extend_entry_ttls_skips_entries_with_no_live_score() {
    let (env, client, _, _) = setup();
    let never_scored = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let mut entries = Vec::new(&env);
    entries.push_back((never_scored, pair));
    let renewed = client.extend_entry_ttls(&Vec::new(&env), &entries);
    assert_eq!(renewed, 0);
}

#[test]
fn test_extend_entry_ttls_empty_batch_is_a_noop() {
    let (env, client, _, _) = setup();
    assert_eq!(client.extend_entry_ttls(&Vec::new(&env), &Vec::new(&env)), 0);
}

#[test]
fn test_extend_entry_ttls_oversized_batch_rejected() {
    let (env, client, _, _) = setup();
    let pair = symbol_short!("XLM_USDC");
    let mut entries = Vec::new(&env);
    for _ in 0..=crate::constants::MAX_EXPIRING_ENTRIES_PER_CALL {
        entries.push_back((Address::generate(&env), pair.clone()));
    }
    assert_eq!(
        client.try_extend_entry_ttls(&Vec::new(&env), &entries),
        Err(Ok(Error::BatchTooLarge))
    );
}

#[test]
fn test_extend_entry_ttls_before_init_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    assert_eq!(
        client.try_extend_entry_ttls(&Vec::new(&env), &Vec::new(&env)),
        Err(Ok(Error::NotInitialized))
    );
}

#[test]
fn test_batch_submission_also_tracks_entries() {
    let (env, client, _, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let submission = crate::ScoreSubmission {
        wallet: wallet.clone(),
        asset_pair: pair.clone(),
        score: 50,
        benford_flag: false,
        ml_flag: false,
        timestamp: 1,
        confidence: 90,
        model_version: 1,
    };
    let mut batch = Vec::new(&env);
    batch.push_back(submission);
    client.submit_scores_batch(&batch);

    assert_eq!(client.get_entry_ttl(&wallet, &pair), SCORE_TTL_THRESHOLD);
}
