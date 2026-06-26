//! Property-based tests for `get_aggregate_score` weighted average invariants
//! (issue #305).
//!
//! Properties verified with ≥ 10,000 generated cases each:
//!
//! P1. aggregate_score ∈ [0, 100] for all valid inputs
//! P2. aggregate_score == raw_score when only one pair is scored
//! P3. aggregate_score == floor(unweighted mean) when all pair weights are equal
//! P4. Zero-weight pairs do not affect the aggregate (same result as if absent)
//!
//! The Soroban test environment has no external proptest dependency, so we
//! use a deterministic PRNG (xorshift64) to generate pseudo-random inputs,
//! providing the same guarantees across CI runs while avoiding external deps.

#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient};

const START_TS: u64 = 1_700_000_000;

// ── Deterministic PRNG ────────────────────────────────────────────────────────

struct Xorshift64(u64);
impl Xorshift64 {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    /// Returns a value in [lo, hi] inclusive.
    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo + 1)
    }
    fn score(&mut self) -> u32 {
        self.range(0, 100) as u32
    }
    fn weight(&mut self) -> u32 {
        self.range(1, 10) as u32
    }
}

// ── Test environment ──────────────────────────────────────────────────────────

fn make_env<'a>() -> (Env, LedgerLensScoreContractClient<'a>) {
    let env = Env::default();
    env.mock_all_auths();
    env.budget().reset_unlimited();
    env.ledger().with_mut(|l| l.timestamp = START_TS);
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client)
}

/// Builds a short Symbol for pair `i` without using `format!` (no_std).
fn pair_sym(env: &Env, i: u32) -> Symbol {
    // Symbols up to 9 chars; encode as "P" followed by decimal digits.
    let digits = [
        b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9',
    ];
    let mut buf = [b'P', b'0', b'0'];
    if i < 10 {
        buf[1] = digits[i as usize];
        Symbol::new(env, core::str::from_utf8(&buf[..2]).unwrap())
    } else {
        buf[1] = digits[(i / 10) as usize];
        buf[2] = digits[(i % 10) as usize];
        Symbol::new(env, core::str::from_utf8(&buf[..3]).unwrap())
    }
}

/// Submit a score for a fresh wallet/pair in a fresh env; returns the wallet.
fn submit_once(
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
        &false, &false,
        &(env.ledger().timestamp().max(1)),
        &80,
        &1,
        &None,
    );
}

// ── P1: aggregate ∈ [0, 100] ─────────────────────────────────────────────────

/// For any combination of wallets, pairs, weights and scores in their valid
/// ranges, the aggregate score must lie in [0, 100].
#[test]
fn prop_aggregate_score_in_valid_range() {
    const CASES: u32 = 10_000;
    let mut rng = Xorshift64(0x1234_5678_ABCD_EF01);

    for case in 0..CASES {
        let (env, client) = make_env();
        let wallet = Address::generate(&env);
        let num_pairs = rng.range(1, 5) as u32;

        for i in 0..num_pairs {
            let pair = pair_sym(&env, i);
            let score = rng.score();
            let weight = rng.weight();
            // Set weight before submitting so `refresh_aggregate_cache` uses it.
            client.set_pair_weight(&Vec::new(&env), &pair, &weight);
            submit_once(&env, &client, &wallet, &pair, score);
        }

        let agg = client.get_aggregate_score(&wallet).expect(&format!(
            "case {case}: get_aggregate_score failed"
        ));
        assert!(
            agg.aggregate_score <= 100,
            "case {case}: aggregate_score={} out of range", agg.aggregate_score
        );
        // pair_count must match submitted pairs.
        assert_eq!(agg.pair_count, num_pairs, "case {case}: wrong pair_count");
    }
}

// ── P2: single pair → aggregate == raw score ─────────────────────────────────

/// When exactly one pair has been scored, the aggregate must equal
/// that pair's raw score regardless of its weight (since there is
/// only one numerator term and one denominator term).
#[test]
fn prop_single_pair_aggregate_equals_raw_score() {
    const CASES: u32 = 10_000;
    let mut rng = Xorshift64(0xDEAD_BEEF_CAFE_BABE);

    for case in 0..CASES {
        let (env, client) = make_env();
        let wallet = Address::generate(&env);
        let pair = pair_sym(&env, 0);
        let score = rng.score();
        let weight = rng.weight();

        client.set_pair_weight(&Vec::new(&env), &pair, &weight);
        submit_once(&env, &client, &wallet, &pair, score);

        let agg = client.get_aggregate_score(&wallet).expect(&format!(
            "case {case}: get_aggregate_score failed"
        ));
        assert_eq!(
            agg.aggregate_score, score,
            "case {case}: single-pair aggregate {agg_score} != raw score {score}",
            agg_score = agg.aggregate_score
        );
    }
}

// ── P3: equal weights → aggregate == floor(unweighted mean) ──────────────────

/// When all pair weights are equal and non-zero, the weighted average
/// degenerates to the arithmetic mean (integer division / floor).
#[test]
fn prop_equal_weights_aggregate_equals_unweighted_mean() {
    const CASES: u32 = 10_000;
    let mut rng = Xorshift64(0xFEED_FACE_1234_5678);

    for case in 0..CASES {
        let (env, client) = make_env();
        let wallet = Address::generate(&env);
        let num_pairs = rng.range(1, 10) as u32;
        let weight = rng.weight(); // same weight for every pair

        let mut score_sum: u64 = 0;
        for i in 0..num_pairs {
            let pair = pair_sym(&env, i);
            let score = rng.score();
            score_sum += score as u64;
            client.set_pair_weight(&Vec::new(&env), &pair, &weight);
            submit_once(&env, &client, &wallet, &pair, score);
        }

        let expected_mean = (score_sum / num_pairs as u64) as u32;
        let agg = client.get_aggregate_score(&wallet).expect(&format!(
            "case {case}: get_aggregate_score failed"
        ));
        assert_eq!(
            agg.aggregate_score, expected_mean,
            "case {case}: equal-weight mean mismatch: got {agg_score}, expected {expected_mean}",
            agg_score = agg.aggregate_score
        );
    }
}

// ── P4: zero-weight pairs do not affect the aggregate ─────────────────────────

/// Adding a pair with weight = 0 must not change the aggregate value
/// computed from the non-zero-weight pairs.
#[test]
fn prop_zero_weight_pairs_excluded_from_aggregate() {
    const CASES: u32 = 10_000;
    let mut rng = Xorshift64(0x0BAD_C0DE_5A5A_5A5A);

    for case in 0..CASES {
        // First env: only the "contributing" pairs (weight >= 1).
        let (env_a, client_a) = make_env();
        // Second env: same contributing pairs PLUS a zero-weight pair.
        let (env_b, client_b) = make_env();

        let wallet_a = Address::generate(&env_a);
        let wallet_b = Address::generate(&env_b);

        let num_pairs = rng.range(1, 5) as u32;

        for i in 0..num_pairs {
            let pair = pair_sym(&env_a, i);
            let pair_b = pair_sym(&env_b, i);
            let score = rng.score();
            let weight = rng.weight();

            client_a.set_pair_weight(&Vec::new(&env_a), &pair, &weight);
            submit_once(&env_a, &client_a, &wallet_a, &pair, score);

            client_b.set_pair_weight(&Vec::new(&env_b), &pair_b, &weight);
            submit_once(&env_b, &client_b, &wallet_b, &pair_b, score);
        }

        // Add a zero-weight pair to env_b with a very different score.
        let zero_pair = pair_sym(&env_b, num_pairs);
        client_b.set_pair_weight(&Vec::new(&env_b), &zero_pair, &0);
        // Score differs wildly; should not influence aggregate.
        let zero_score = if rng.score() < 50 { 100u32 } else { 0u32 };
        submit_once(&env_b, &client_b, &wallet_b, &zero_pair, zero_score);

        let agg_a = client_a.get_aggregate_score(&wallet_a).expect(&format!(
            "case {case}: env_a get_aggregate_score failed"
        ));
        let agg_b = client_b.get_aggregate_score(&wallet_b).expect(&format!(
            "case {case}: env_b get_aggregate_score failed"
        ));

        assert_eq!(
            agg_a.aggregate_score, agg_b.aggregate_score,
            "case {case}: zero-weight pair changed aggregate \
             (without={}, with={})",
            agg_a.aggregate_score, agg_b.aggregate_score
        );
        // pair_count still includes the zero-weight pair.
        assert_eq!(agg_b.pair_count, num_pairs + 1,
            "case {case}: pair_count should include zero-weight pair");
    }
}

// ── Edge cases ────────────────────────────────────────────────────────────────

#[test]
fn prop_all_zero_scores_aggregate_is_zero() {
    let (env, client) = make_env();
    let wallet = Address::generate(&env);
    for i in 0..5u32 {
        let pair = pair_sym(&env, i);
        submit_once(&env, &client, &wallet, &pair, 0);
    }
    let agg = client.get_aggregate_score(&wallet).unwrap();
    assert_eq!(agg.aggregate_score, 0);
}

#[test]
fn prop_all_max_scores_aggregate_is_100() {
    let (env, client) = make_env();
    let wallet = Address::generate(&env);
    for i in 0..5u32 {
        let pair = pair_sym(&env, i);
        submit_once(&env, &client, &wallet, &pair, 100);
    }
    let agg = client.get_aggregate_score(&wallet).unwrap();
    assert_eq!(agg.aggregate_score, 100);
}

#[test]
fn prop_aggregate_max_pair_score_is_correct() {
    let (env, client) = make_env();
    let wallet = Address::generate(&env);
    let pairs_and_scores = [(0u32, 30u32), (1, 80), (2, 50), (3, 95), (4, 10)];
    let mut max_expected = 0u32;
    for (i, s) in pairs_and_scores {
        let pair = pair_sym(&env, i);
        submit_once(&env, &client, &wallet, &pair, s);
        if s > max_expected { max_expected = s; }
    }
    let agg = client.get_aggregate_score(&wallet).unwrap();
    assert_eq!(agg.max_pair_score, max_expected);
}

#[test]
fn prop_pair_count_reflects_distinct_pairs() {
    let mut rng = Xorshift64(0x1111_2222_3333_4444);
    let (env, client) = make_env();
    let wallet = Address::generate(&env);
    let n = 7u32;
    for i in 0..n {
        let pair = pair_sym(&env, i);
        submit_once(&env, &client, &wallet, &pair, rng.score());
    }
    let agg = client.get_aggregate_score(&wallet).unwrap();
    assert_eq!(agg.pair_count, n);
}
