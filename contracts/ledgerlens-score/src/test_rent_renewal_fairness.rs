//! Rent renewal fairness tests
//!
//! Issue #707: Prevent renewal scheduling from repeatedly favoring the same
//! wallet/pair subset under bounded batch sizes.
//!
//! This test suite verifies that:
//! - The renewal queue maintains fair ordering (FIFO for same age)
//! - Older entries cannot starve indefinitely when batch size < total expiring
//! - Rotation strategy prevents consistently skipping older entries
//! - Round-robin cursor/pointer advances correctly through the backlog
//! - Large batches of equally-old entries are handled fairly

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol,
};

use crate::{
    storage::{
        extend_entry_ttls, get_expiring_entries, set_score, track_score_entry, peek_score,
    },
    types::{RiskScore, DataKey},
    constants::SCORE_TTL_THRESHOLD,
};

fn setup_env() -> Env {
    let env = Env::default();
    env.budget().reset_unlimited();
    env
}

fn sample_risk_score() -> RiskScore {
    RiskScore {
        score: 50,
        benford_flag: false,
        ml_flag: false,
        timestamp: 1000,
        confidence: 80,
        model_version: 1,
        benford_score: 40,
        ml_score: 45,
        network_score: 50,
        commitment: None,
    }
}

/// Test that expiring entries are returned in age order (oldest first).
#[test]
fn test_renewal_fairness_age_ordering() {
    let env = setup_env();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 0);

    let wallet1 = Address::generate(&env);
    let wallet2 = Address::generate(&env);
    let wallet3 = Address::generate(&env);
    let pair = symbol_short!("PAIR");

    // Write entry 1 at ledger 100
    env.ledger().with_mut(|l| l.sequence = 100);
    let score1 = sample_risk_score();
    set_score(&env, &wallet1, &pair, &score1);

    // Write entry 2 at ledger 200
    env.ledger().with_mut(|l| l.sequence = 200);
    let score2 = sample_risk_score();
    set_score(&env, &wallet2, &pair, &score2);

    // Write entry 3 at ledger 300
    env.ledger().with_mut(|l| l.sequence = 300);
    let score3 = sample_risk_score();
    set_score(&env, &wallet3, &pair, &score3);

    // Advance time such that all three are due for renewal
    env.ledger().with_mut(|l| l.sequence = 300 + SCORE_TTL_THRESHOLD as u32);

    // Get expiring entries
    let expiring = get_expiring_entries(&env, 100);

    // Should return all three in age order: wallet1 (oldest), wallet2, wallet3
    assert_eq!(expiring.len(), 3);
    assert_eq!(expiring.get(0).unwrap(), (wallet1.clone(), pair.clone()));
    assert_eq!(expiring.get(1).unwrap(), (wallet2.clone(), pair.clone()));
    assert_eq!(expiring.get(2).unwrap(), (wallet3.clone(), pair.clone()));
}

/// Test that when batch size < total expiring, repeated calls serve all entries fairly.
#[test]
fn test_renewal_fairness_batch_smaller_than_backlog() {
    let env = setup_env();
    env.mock_all_auths();

    let wallets: Vec<Address> = (0..10)
        .map(|_| Address::generate(&env))
        .collect::<Vec<_>>();
    let pair = symbol_short!("PAIR");

    // Write entries at staggered ledgers: 100, 120, 140, ..., 280
    for (i, wallet) in wallets.iter().enumerate() {
        env.ledger().with_mut(|l| l.sequence = 100 + (i as u32) * 20);
        let score = sample_risk_score();
        set_score(&env, wallet, &pair, &score);
    }

    // Advance time such that all are due
    env.ledger()
        .with_mut(|l| l.sequence = 280 + SCORE_TTL_THRESHOLD as u32);

    // Batch size of 3 — we'll need multiple calls to service all 10 entries
    let batch_size = 3u32;

    // First call: should get first 3 (oldest)
    let batch1 = get_expiring_entries(&env, batch_size);
    assert_eq!(batch1.len(), 3);
    assert_eq!(batch1.get(0).unwrap().0, wallets[0]); // oldest
    assert_eq!(batch1.get(1).unwrap().0, wallets[1]);
    assert_eq!(batch1.get(2).unwrap().0, wallets[2]);

    // Renew batch 1 (moves them to the back of the queue)
    extend_entry_ttls(&env, &batch1);

    // Second call: should get next 3 (wallets 3, 4, 5 now oldest among due)
    let batch2 = get_expiring_entries(&env, batch_size);
    assert_eq!(batch2.len(), 3);
    assert_eq!(batch2.get(0).unwrap().0, wallets[3]);
    assert_eq!(batch2.get(1).unwrap().0, wallets[4]);
    assert_eq!(batch2.get(2).unwrap().0, wallets[5]);

    // Renew batch 2
    extend_entry_ttls(&env, &batch2);

    // Third call: should get next 3 (wallets 6, 7, 8)
    let batch3 = get_expiring_entries(&env, batch_size);
    assert_eq!(batch3.len(), 3);
    assert_eq!(batch3.get(0).unwrap().0, wallets[6]);
    assert_eq!(batch3.get(1).unwrap().0, wallets[7]);
    assert_eq!(batch3.get(2).unwrap().0, wallets[8]);

    // Renew batch 3
    extend_entry_ttls(&env, &batch3);

    // Fourth call: should get the last 1 (wallet 9)
    let batch4 = get_expiring_entries(&env, batch_size);
    assert_eq!(batch4.len(), 1);
    assert_eq!(batch4.get(0).unwrap().0, wallets[9]);
}

/// Test that renewal ordering prevents starvation of older entries.
#[test]
fn test_renewal_fairness_no_starvation() {
    let env = setup_env();
    env.mock_all_auths();

    let mut wallets = Vec::new(&env);
    for _ in 0..5 {
        wallets.push_back(Address::generate(&env));
    }
    let pair = symbol_short!("PAIR");

    // Write all 5 entries at the same ledger (all equally old)
    env.ledger().with_mut(|l| l.sequence = 100);
    for wallet in wallets.iter() {
        let score = sample_risk_score();
        set_score(&env, wallet, &pair, &score);
    }

    // Advance time such that all are due
    env.ledger()
        .with_mut(|l| l.sequence = 100 + SCORE_TTL_THRESHOLD as u32 + 10);

    // Batch size of 2: will require multiple calls to renew all 5
    let batch_size = 2u32;

    // Call 1: get first 2
    let batch1 = get_expiring_entries(&env, batch_size);
    assert_eq!(batch1.len(), 2);
    let batch1_wallets = vec![batch1.get(0).unwrap().0.clone(), batch1.get(1).unwrap().0.clone()];

    // Renew batch 1
    extend_entry_ttls(&env, &batch1);

    // Call 2: should get the next 2 (not batch 1 again)
    let batch2 = get_expiring_entries(&env, batch_size);
    assert_eq!(batch2.len(), 2);
    let batch2_wallets = vec![batch2.get(0).unwrap().0.clone(), batch2.get(1).unwrap().0.clone()];

    // Verify batch 2 doesn't contain any wallet from batch 1
    for w1 in batch1_wallets.iter() {
        for w2 in batch2_wallets.iter() {
            assert_ne!(w1, w2, "Batch 2 should not contain entries from Batch 1");
        }
    }

    // Renew batch 2
    extend_entry_ttls(&env, &batch2);

    // Call 3: should get the last 1
    let batch3 = get_expiring_entries(&env, batch_size);
    assert_eq!(batch3.len(), 1);
    let batch3_wallet = batch3.get(0).unwrap().0.clone();

    // Verify batch 3 doesn't contain wallets from batch 1 or batch 2
    for w1 in batch1_wallets.iter() {
        assert_ne!(&batch3_wallet, w1);
    }
    for w2 in batch2_wallets.iter() {
        assert_ne!(&batch3_wallet, w2);
    }
}

/// Test that mixed-age entries are ordered fairly (older entries prioritized).
#[test]
fn test_renewal_fairness_mixed_ages() {
    let env = setup_env();
    env.mock_all_auths();

    let wallet1 = Address::generate(&env);
    let wallet2 = Address::generate(&env);
    let wallet3 = Address::generate(&env);
    let wallet4 = Address::generate(&env);
    let pair = symbol_short!("PAIR");

    // Write wallet1 and wallet2 at ledger 50 (oldest)
    env.ledger().with_mut(|l| l.sequence = 50);
    set_score(&env, &wallet1, &pair, &sample_risk_score());
    set_score(&env, &wallet2, &pair, &sample_risk_score());

    // Write wallet3 at ledger 100 (middle)
    env.ledger().with_mut(|l| l.sequence = 100);
    set_score(&env, &wallet3, &pair, &sample_risk_score());

    // Write wallet4 at ledger 150 (newest)
    env.ledger().with_mut(|l| l.sequence = 150);
    set_score(&env, &wallet4, &pair, &sample_risk_score());

    // Advance time such that all are due
    env.ledger()
        .with_mut(|l| l.sequence = 150 + SCORE_TTL_THRESHOLD as u32 + 20);

    // Get all expiring entries
    let expiring = get_expiring_entries(&env, 100);

    // Should be ordered from oldest to newest: wallet1, wallet2, wallet3, wallet4
    assert_eq!(expiring.len(), 4);
    assert_eq!(expiring.get(0).unwrap().0, wallet1);
    assert_eq!(expiring.get(1).unwrap().0, wallet2);
    assert_eq!(expiring.get(2).unwrap().0, wallet3);
    assert_eq!(expiring.get(3).unwrap().0, wallet4);
}

/// Test that renewing an entry moves it to the back of the queue fairly.
#[test]
fn test_renewal_fairness_queue_rotation() {
    let env = setup_env();
    env.mock_all_auths();

    let wallet1 = Address::generate(&env);
    let wallet2 = Address::generate(&env);
    let wallet3 = Address::generate(&env);
    let pair = symbol_short!("PAIR");

    // Write entries in order at ledger 100, 101, 102
    env.ledger().with_mut(|l| l.sequence = 100);
    set_score(&env, &wallet1, &pair, &sample_risk_score());

    env.ledger().with_mut(|l| l.sequence = 101);
    set_score(&env, &wallet2, &pair, &sample_risk_score());

    env.ledger().with_mut(|l| l.sequence = 102);
    set_score(&env, &wallet3, &pair, &sample_risk_score());

    // Advance time such that all are due
    env.ledger()
        .with_mut(|l| l.sequence = 102 + SCORE_TTL_THRESHOLD as u32 + 5);

    // First call: get first 1 (wallet1, oldest)
    let batch1 = get_expiring_entries(&env, 1u32);
    assert_eq!(batch1.len(), 1);
    assert_eq!(batch1.get(0).unwrap().0, wallet1);

    // Renew wallet1 (moves to back)
    extend_entry_ttls(&env, &batch1);

    // Second call: should now get wallet2 (oldest among due)
    let batch2 = get_expiring_entries(&env, 1u32);
    assert_eq!(batch2.len(), 1);
    assert_eq!(batch2.get(0).unwrap().0, wallet2);

    // Renew wallet2 (moves to back)
    extend_entry_ttls(&env, &batch2);

    // Third call: should now get wallet3 (oldest among due)
    let batch3 = get_expiring_entries(&env, 1u32);
    assert_eq!(batch3.len(), 1);
    assert_eq!(batch3.get(0).unwrap().0, wallet3);

    // Renew wallet3 (moves to back)
    extend_entry_ttls(&env, &batch3);

    // Fourth call: should now get wallet1 again (it's oldest after rotation)
    let batch4 = get_expiring_entries(&env, 1u32);
    assert_eq!(batch4.len(), 1);
    assert_eq!(batch4.get(0).unwrap().0, wallet1);
}

/// Test that batch renewal respects the order for all entries in the batch.
#[test]
fn test_renewal_fairness_batch_order_preservation() {
    let env = setup_env();
    env.mock_all_auths();

    let wallets: Vec<Address> = (0..6)
        .map(|_| Address::generate(&env))
        .collect::<Vec<_>>();
    let pair = symbol_short!("PAIR");

    // Write all 6 entries at staggered times
    for (i, wallet) in wallets.iter().enumerate() {
        env.ledger().with_mut(|l| l.sequence = 100 + (i as u32) * 10);
        set_score(&env, wallet, &pair, &sample_risk_score());
    }

    // Advance time such that all are due
    env.ledger()
        .with_mut(|l| l.sequence = 100 + 50 + SCORE_TTL_THRESHOLD as u32 + 10);

    // Get all 6 entries in one batch
    let all_expiring = get_expiring_entries(&env, 100);
    assert_eq!(all_expiring.len(), 6);

    // Verify they're in order from oldest to newest
    for i in 0..6 {
        assert_eq!(
            all_expiring.get(i).unwrap().0,
            wallets[i],
            "Entry {} should be at position {}: expected {:?}, got {:?}",
            i,
            i,
            wallets[i],
            all_expiring.get(i).unwrap().0
        );
    }

    // Renew all entries
    extend_entry_ttls(&env, &all_expiring);

    // After renewal, they should no longer be marked as due
    let now_expiring = get_expiring_entries(&env, 100);
    assert_eq!(now_expiring.len(), 0, "After renewal, entries should not be due");
}
