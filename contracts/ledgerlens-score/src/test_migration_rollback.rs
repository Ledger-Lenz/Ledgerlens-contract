//! Issue #709 – Migration rollback fixtures for partial storage transitions.
//!
//! These tests simulate a storage migration that stopped partway through
//! (e.g. the contract ran out of budget, a transaction was aborted, or an
//! operator manually replayed only part of the migration script).  Each
//! fixture:
//!
//!  1. Writes a pre-migration storage state directly via `storage::*`.
//!  2. Applies the migration logic partially (stops after N of M records).
//!  3. Re-runs the migration from scratch (idempotent replay).
//!  4. Asserts: no duplicates, no missing records, no orphaned keys.
//!
//! "Migration" here means the transition the `migrate_storage_v4` pathway
//! performs: rebuilding the `ScoreEntryIndex` from raw `Score` keys and
//! re-stamping `AssetPairs` registrations.  The same pattern applies to any
//! future family reindex.

use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol, Vec};

use crate::{invariants, storage, types::RiskScore, LedgerLensScoreContract};

// ─── helpers ─────────────────────────────────────────────────────────────────

fn make_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn sample_score(v: u32) -> RiskScore {
    RiskScore {
        score: v,
        benford_flag: false,
        ml_flag: false,
        timestamp: 1_000,
        confidence: 90,
        model_version: 1,
        benford_score: 0,
        ml_score: 0,
        network_score: 0,
        commitment: None,
    }
}

fn register(env: &Env) -> Address {
    let id = env.register_contract(None, LedgerLensScoreContract);
    let admin = Address::generate(env);
    let service = Address::generate(env);
    crate::LedgerLensScoreContractClient::new(env, &id).initialize(&admin, &service);
    id
}

/// Seed `n` `(wallet, pair)` score records directly into storage (bypassing
/// the rate-limit / auth flow), returning the wallet addresses used.
fn seed_scores(env: &Env, n: u32, pair: &Symbol) -> Vec<Address> {
    let mut wallets: Vec<Address> = Vec::new(env);
    for i in 0..n {
        let w = Address::generate(env);
        storage::set_score(env, &w, pair, &sample_score(i * 10));
        // `set_score` maintains current indexes automatically. Remove those
        // entries to model the legacy, pre-migration state described above.
        storage::remove_score_entry(env, &w, pair);
        storage::remove_pair_for_wallet(env, &w, pair);
        wallets.push_back(w);
    }
    wallets
}

/// Re-index function that mirrors what a migration script would execute:
/// walk every (wallet, pair) whose Score key is present and ensure it is
/// tracked in `ScoreEntryIndex` and `AssetPairs`.  This is the *idempotent
/// replay* step — safe to call on already-migrated or partially-migrated data.
fn reindex_all(env: &Env, wallets: &Vec<Address>, pair: &Symbol) {
    for i in 0..wallets.len() {
        let w = wallets.get(i).unwrap();
        if storage::peek_score(env, &w, pair).is_some() {
            storage::track_score_entry(env, &w, pair);
            storage::register_pair_for_wallet(env, &w, pair);
        }
    }
}

// ─── Test: full migration succeeds ───────────────────────────────────────────

/// Baseline: seeding N scores and running the full migration produces a
/// consistent index with no duplicates or orphans.
#[test]
fn full_migration_leaves_consistent_state() {
    let env = make_env();
    let id = register(&env);
    let pair = symbol_short!("XLM_USDC");

    env.as_contract(&id, || {
        let wallets = seed_scores(&env, 5, &pair);
        reindex_all(&env, &wallets, &pair);

        // Index must contain exactly 5 entries.
        let index = storage::get_score_entry_index(&env);
        assert_eq!(index.len(), 5, "index should have 5 entries");

        // Invariants: no orphans, no duplicates.
        assert!(invariants::score_index_is_consistent(&env));
        invariants::invariant_check(&env);
    });
}

// ─── Test: partial migration (half-way abort) then replay ────────────────────

/// Simulates a migration that processes only the first 3 of 6 wallets before
/// being aborted, then is replayed from scratch (full replay over all 6).
/// Asserts the final state has exactly 6 entries with no duplicates.
#[test]
fn partial_migration_abort_then_full_replay_is_consistent() {
    let env = make_env();
    let id = register(&env);
    let pair = symbol_short!("XLM_USDC");

    env.as_contract(&id, || {
        let wallets = seed_scores(&env, 6, &pair);

        // PARTIAL: index only the first 3.
        for i in 0..3u32 {
            let w = wallets.get(i).unwrap();
            storage::track_score_entry(&env, &w, &pair);
            storage::register_pair_for_wallet(&env, &w, &pair);
        }

        // Intermediate state: 3 entries in the index, 6 live scores.
        let partial_index = storage::get_score_entry_index(&env);
        assert_eq!(partial_index.len(), 3);

        // REPLAY from scratch (idempotent).
        reindex_all(&env, &wallets, &pair);

        // Final state must have all 6, no duplicates.
        let index = storage::get_score_entry_index(&env);
        assert_eq!(index.len(), 6, "all 6 wallets must be in the index after replay");

        // No duplicate entries.
        for i in 0..index.len() {
            let (w_i, p_i) = index.get(i).unwrap();
            for j in (i + 1)..index.len() {
                let (w_j, p_j) = index.get(j).unwrap();
                assert!(!(w_i == w_j && p_i == p_j), "duplicate at positions {i},{j}");
            }
        }

        assert!(invariants::score_index_is_consistent(&env));
        invariants::invariant_check(&env);
    });
}

// ─── Test: orphaned index entry (score removed mid-migration) ────────────────

/// Simulates a migration that indexed a wallet, then the score was removed
/// (simulating a rollback of the data write).  After replay, the orphaned
/// entry must be absent.
#[test]
fn orphaned_index_entry_removed_when_score_absent() {
    let env = make_env();
    let id = register(&env);
    let pair = symbol_short!("XLM_USDC");

    env.as_contract(&id, || {
        let wallets = seed_scores(&env, 3, &pair);

        // Index all 3 entries.
        reindex_all(&env, &wallets, &pair);
        assert_eq!(storage::get_score_entry_index(&env).len(), 3);

        // Simulate a partial rollback: wallet[1]'s score disappears.
        let orphan = wallets.get(1).unwrap();
        storage::clear_score(&env, &orphan, &pair);

        // Re-run the migration; it should skip wallets whose Score is gone.
        reindex_all(&env, &wallets, &pair);

        // The orphan was already in the index; `reindex_all` calls
        // `track_score_entry` which only re-tracks when the score exists,
        // but it does NOT explicitly remove orphans.  We model the migration
        // cleanup step: remove entries from the index that have no live score.
        let raw_index = storage::get_score_entry_index(&env);
        let mut cleaned: soroban_sdk::Vec<(Address, Symbol)> = soroban_sdk::Vec::new(&env);
        for i in 0..raw_index.len() {
            let (w, p) = raw_index.get(i).unwrap();
            if storage::peek_score(&env, &w, &p).is_some() {
                cleaned.push_back((w, p));
            }
        }
        env.storage().persistent().set(&crate::types::DataKeyB::ScoreEntryIndex, &cleaned);

        let final_index = storage::get_score_entry_index(&env);
        assert_eq!(final_index.len(), 2, "orphaned entry must be removed");

        assert!(invariants::score_index_is_consistent(&env));
    });
}

// ─── Test: duplicate seeding (migration replayed twice) ──────────────────────

/// If the migration is replayed multiple times (idempotent requirement),
/// the index must not accumulate duplicates.
#[test]
fn double_replay_produces_no_duplicates() {
    let env = make_env();
    let id = register(&env);
    let pair = symbol_short!("XLM_USDC");

    env.as_contract(&id, || {
        let wallets = seed_scores(&env, 4, &pair);

        // First pass.
        reindex_all(&env, &wallets, &pair);
        let after_first = storage::get_score_entry_index(&env).len();

        // Second pass (replay).
        reindex_all(&env, &wallets, &pair);
        let after_second = storage::get_score_entry_index(&env).len();

        assert_eq!(after_first, after_second, "second replay must not add duplicate entries");
        assert_eq!(after_second, 4);
        assert!(invariants::score_index_is_consistent(&env));
    });
}

// ─── Test: multi-pair partial migration ──────────────────────────────────────

/// A migration that covers two pairs partially (aborts after pair A is done
/// but pair B is only half-indexed) then is replayed must leave both pairs
/// fully consistent.
#[test]
fn multi_pair_partial_migration_then_replay() {
    let env = make_env();
    let id = register(&env);
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("XLM_BTC");

    env.as_contract(&id, || {
        let ws_a = seed_scores(&env, 3, &pair_a);
        let ws_b = seed_scores(&env, 4, &pair_b);

        // Full index of pair A, partial index of pair B (only first 2 of 4).
        reindex_all(&env, &ws_a, &pair_a);
        for i in 0..2u32 {
            let w = ws_b.get(i).unwrap();
            storage::track_score_entry(&env, &w, &pair_b);
            storage::register_pair_for_wallet(&env, &w, &pair_b);
        }

        // Intermediate: 3 + 2 = 5 entries.
        assert_eq!(storage::get_score_entry_index(&env).len(), 5);

        // Full replay of both pairs.
        reindex_all(&env, &ws_a, &pair_a);
        reindex_all(&env, &ws_b, &pair_b);

        let final_index = storage::get_score_entry_index(&env);
        assert_eq!(final_index.len(), 7, "3 + 4 entries expected after full replay");

        assert!(invariants::score_index_is_consistent(&env));
        invariants::invariant_check(&env);
    });
}

// ─── Test: resource-bounded worst case ───────────────────────────────────────

/// Demonstrates that re-indexing `MAX_TRACKED_SCORE_ENTRIES` entries in a
/// single pass is bounded — the index caps at `MAX_TRACKED_SCORE_ENTRIES`
/// and does not grow further.
#[test]
fn migration_respects_index_capacity_cap() {
    let env = make_env();
    let id = register(&env);
    let pair = symbol_short!("XLM_USDC");
    env.budget().reset_unlimited();

    env.as_contract(&id, || {
        let cap = crate::constants::MAX_TRACKED_SCORE_ENTRIES;
        // Seed cap + 5 records.
        let wallets = seed_scores(&env, cap + 5, &pair);
        reindex_all(&env, &wallets, &pair);

        let index_len = storage::get_score_entry_index(&env).len();
        assert_eq!(index_len, cap, "index must cap at MAX_TRACKED_SCORE_ENTRIES={cap}");

        // All indexed entries must have live scores (no orphans within cap).
        assert!(invariants::score_index_is_consistent(&env));
    });
}
