//! Tests for the rate-limit override audit log (#296).
//! Covers: log append on override, ring-buffer overflow eviction, and log read.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Bytes, Env, Vec,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient};

const START_TS: u64 = 1_700_000_000;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);
    let id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &Address::generate(&env));
    (env, client, admin)
}

fn justification(env: &Env, msg: &[u8]) -> Bytes {
    Bytes::from_slice(env, msg)
}

// ── Log starts empty ──────────────────────────────────────────────────────────

#[test]
fn test_log_initially_empty() {
    let (_env, client, _admin) = setup();
    assert_eq!(client.get_rate_limit_override_log().len(), 0);
}

// ── Single override appends one entry ─────────────────────────────────────────

#[test]
fn test_single_override_appends_entry() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.override_rate_limit(&Vec::new(&env), &wallet, &pair, &justification(&env, b"urgent fix"));

    let log = client.get_rate_limit_override_log();
    assert_eq!(log.len(), 1);
    let entry = log.get(0).unwrap();
    assert_eq!(entry.wallet, wallet);
    assert_eq!(entry.asset_pair, pair);
    assert_eq!(entry.timestamp, START_TS);
}

// ── Multiple overrides accumulate in order ────────────────────────────────────

#[test]
fn test_multiple_overrides_accumulate() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("XLM_BTC");

    client.override_rate_limit(&Vec::new(&env), &wallet, &pair_a, &justification(&env, b"reason a"));
    client.override_rate_limit(&Vec::new(&env), &wallet, &pair_b, &justification(&env, b"reason b"));

    let log = client.get_rate_limit_override_log();
    assert_eq!(log.len(), 2);
    assert_eq!(log.get(0).unwrap().asset_pair, pair_a);
    assert_eq!(log.get(1).unwrap().asset_pair, pair_b);
}

// ── justification hash is non-zero and deterministic ─────────────────────────

#[test]
fn test_justification_hash_is_deterministic() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let just = justification(&env, b"deterministic");

    client.override_rate_limit(&Vec::new(&env), &wallet, &pair, &just);

    let hash1 = client.get_rate_limit_override_log().get(0).unwrap().justification_hash.to_array();

    // New contract instance, same input → same hash.
    let (env2, client2, _) = setup();
    let wallet2 = Address::generate(&env2);
    let pair2 = symbol_short!("XLM_USDC");
    let just2 = justification(&env2, b"deterministic");
    client2.override_rate_limit(&Vec::new(&env2), &wallet2, &pair2, &just2);
    let hash2 = client2.get_rate_limit_override_log().get(0).unwrap().justification_hash.to_array();

    assert_eq!(hash1, hash2);
}

// ── Ring buffer evicts oldest on overflow ─────────────────────────────────────

#[test]
fn test_ring_buffer_evicts_oldest() {
    let (env, client, _admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let cap = crate::constants::MAX_RATE_LIMIT_OVERRIDE_LOG;

    // Fill the buffer to exactly the cap.
    for _ in 0..cap {
        client.override_rate_limit(
            &Vec::new(&env),
            &wallet,
            &pair,
            &justification(&env, b"fill"),
        );
    }
    assert_eq!(client.get_rate_limit_override_log().len(), cap);

    // One more push must not grow beyond cap.
    client.override_rate_limit(&Vec::new(&env), &wallet, &pair, &justification(&env, b"overflow"));
    assert_eq!(client.get_rate_limit_override_log().len(), cap);
}
