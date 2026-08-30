use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec};

use crate::{storage, types::DataKeyB, types::RiskScore, LedgerLensScoreContract};

// ── Rent griefing via high-cardinality monitored wallets (issue #799) ─────
//
// The proactive rent-management index (`ScoreEntryIndex`) is deliberately
// capped at `MAX_TRACKED_SCORE_ENTRIES` (storage.rs: reindex_entry_to_back)
// so that `get_expiring_entries`'s sweep cost stays bounded no matter how
// many distinct (wallet, asset_pair) combinations the off-chain service has
// ever submitted. This test locks that cap in: if the guard in
// `reindex_entry_to_back` were ever removed or off-by-one'd, the index would
// grow unbounded under a high-cardinality submission pattern and the sweep
// would regress to an unbounded full scan.
//
// The index is pre-filled directly (one storage write of synthetic entries)
// rather than via `MAX_TRACKED_SCORE_ENTRIES` real `set_score` calls, so the
// test exercises the same cap boundary without generating hundreds of full
// score + touch-marker persistent entries.

fn sample_score() -> RiskScore {
    RiskScore {
        score: 10,
        benford_flag: false,
        ml_flag: false,
        timestamp: 1,
        confidence: 90,
        model_version: 1,
        benford_score: 0,
        ml_score: 0,
        network_score: 0,
        commitment: None,
    }
}

fn fill_index_to_cap(env: &Env, pair: &soroban_sdk::Symbol) -> Vec<(Address, soroban_sdk::Symbol)> {
    let mut index = Vec::new(env);
    for _ in 0..crate::constants::MAX_TRACKED_SCORE_ENTRIES {
        index.push_back((Address::generate(env), pair.clone()));
    }
    env.storage().persistent().set(&DataKeyB::ScoreEntryIndex, &index);
    index
}

#[test]
fn test_score_entry_index_bounded_under_high_cardinality_submissions() {
    let env = Env::default();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let pair = symbol_short!("XLM_USDC");

    env.as_contract(&contract_id, || {
        fill_index_to_cap(&env, &pair);

        // One more distinct, low-value combination beyond the cap.
        let overflow_wallet = Address::generate(&env);
        storage::track_score_entry(&env, &overflow_wallet, &pair);

        let index = storage::get_score_entry_index(&env);
        assert_eq!(index.len(), crate::constants::MAX_TRACKED_SCORE_ENTRIES);
        assert!(index.first_index_of((overflow_wallet, pair)).is_none());
    });
}

#[test]
fn test_writes_beyond_index_cap_still_persist_their_own_score() {
    let env = Env::default();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let pair = symbol_short!("XLM_USDC");

    env.as_contract(&contract_id, || {
        fill_index_to_cap(&env, &pair);

        // The write itself must still succeed and be independently readable,
        // even though it is no longer visible to the proactive-renewal sweep.
        let overflow_wallet = Address::generate(&env);
        storage::set_score(&env, &overflow_wallet, &pair, &sample_score());

        let stored = storage::get_score(&env, &overflow_wallet, &pair);
        assert!(stored.is_some());

        let index = storage::get_score_entry_index(&env);
        assert_eq!(index.len(), crate::constants::MAX_TRACKED_SCORE_ENTRIES);
        assert!(index.first_index_of((overflow_wallet, pair)).is_none());
    });
}
