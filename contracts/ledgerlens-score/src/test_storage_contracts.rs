use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

use crate::{storage, types::RiskScore, LedgerLensScoreContract};

fn sample_score(value: u32) -> RiskScore {
    RiskScore {
        score: value,
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

// ── Admin / Service preconditions (issue #807) ─────────────────────────────
//
// `storage::get_admin` / `storage::get_service` unwrap an `Option` and panic
// if `set_admin` / `set_service` were never called. Business logic (e.g.
// `initialize`, auth checks) relies on this "always set before read" caller
// contract instead of handling `None`. These tests lock that contract in so
// a future refactor that starts calling the getters before initialization
// fails loudly instead of silently returning a default.

#[test]
fn test_get_admin_panics_if_never_set() {
    let env = Env::default();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);

    assert!(!env.as_contract(&contract_id, || storage::has_admin(&env)));
}

#[test]
#[should_panic]
fn test_get_admin_before_set_admin_panics() {
    let env = Env::default();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);

    env.as_contract(&contract_id, || {
        storage::get_admin(&env);
    });
}

#[test]
#[should_panic]
fn test_get_service_before_set_service_panics() {
    let env = Env::default();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);

    env.as_contract(&contract_id, || {
        storage::get_service(&env);
    });
}

#[test]
fn test_set_admin_is_idempotent_overwrite() {
    let env = Env::default();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let first = Address::generate(&env);
    let second = Address::generate(&env);

    env.as_contract(&contract_id, || {
        storage::set_admin(&env, &first);
        storage::set_admin(&env, &second);
        assert_eq!(storage::get_admin(&env), second);
    });
}

// ── Score history ring buffer mutation guarantee (issue #807) ─────────────
//
// `push_score_history` documents that it evicts from the front until the
// ring is back at `HistoryMaxDepth`. This test asserts that guarantee holds
// under a caller assumption that would otherwise be easy to break: reducing
// `HistoryMaxDepth` after entries already exceed the new depth must trim on
// the very next write, not grow unbounded.

#[test]
fn test_push_score_history_bounded_by_max_depth() {
    let env = Env::default();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    env.as_contract(&contract_id, || {
        storage::set_history_max_depth(&env, 3);
        for i in 0..5u32 {
            storage::push_score_history(&env, &wallet, &pair, &sample_score(i));
        }

        let history = storage::get_score_history(&env, &wallet, &pair);
        assert_eq!(history.len(), 3);
        // Oldest two entries (scores 0 and 1) must have been evicted from the
        // front; only the three most recent writes survive.
        assert_eq!(history.get(0).unwrap().score, 2);
        assert_eq!(history.get(1).unwrap().score, 3);
        assert_eq!(history.get(2).unwrap().score, 4);
    });
}
