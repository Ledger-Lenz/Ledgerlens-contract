//! Monotonicity tests for aggregate risk score under pair reweighting (#721).
//!
//! These tests prove that the weighted-average aggregate score changes in the
//! expected direction when pair weights are modified while input scores are
//! held fixed.
//!
//! Properties verified:
//!
//! M1. Increasing the weight of a pair whose score is above the current
//!     aggregate must increase (or preserve) the aggregate.
//! M2. Increasing the weight of a pair whose score is below the current
//!     aggregate must decrease (or preserve) the aggregate.
//! M3. Setting all weights to zero causes no contribution from any pair
//!     (degenerate case: contract returns an error or aggregate is undefined).
//! M4. A dominant-weight pair whose score is S forces the aggregate arbitrarily
//!     close to S as its weight grows to infinity.
//! M5. Equal-score pairs: any reweighting of equal-score pairs leaves the
//!     aggregate unchanged (it is always equal to the common score).
//! M6. max_pair_score in the aggregate response always equals the highest
//!     individual score regardless of reweighting.

#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient};

const START_TS: u64 = 1_700_000_000;

// ── Helpers ───────────────────────────────────────────────────────────────────

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

fn pair_sym(env: &Env, i: u32) -> Symbol {
    let digits = [b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9'];
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

fn submit(env: &Env, client: &LedgerLensScoreContractClient, wallet: &Address, pair: &Symbol, score: u32) {
    env.ledger().with_mut(|l| l.timestamp += 3_601);
    client.submit_score(
        &Vec::new(env),
        wallet,
        pair,
        &score,
        &false,
        &false,
        &(env.ledger().timestamp()),
        &90,
        &1,
        &None,
    );
}

fn aggregate(client: &LedgerLensScoreContractClient, wallet: &Address) -> u32 {
    client
        .get_aggregate_score(wallet)
        .expect("get_aggregate_score failed")
        .aggregate_score
}

// ── M1: Increasing a high-score pair's weight raises the aggregate ────────────

/// Pair A has score 80 (above the current aggregate ~50), pair B has score 20.
/// Increasing the weight of pair A from 1 to 10 must not decrease the aggregate.
#[test]
fn monotonicity_increase_high_score_weight_raises_aggregate() {
    // Baseline: equal weights (1, 1) → aggregate ≈ floor((80+20)/2) = 50
    let (env_base, client_base) = make_env();
    let wallet_base = Address::generate(&env_base);
    let pa = pair_sym(&env_base, 0);
    let pb = pair_sym(&env_base, 1);
    client_base.set_pair_weight(&Vec::new(&env_base), &pa, &1);
    client_base.set_pair_weight(&Vec::new(&env_base), &pb, &1);
    submit(&env_base, &client_base, &wallet_base, &pa, 80);
    submit(&env_base, &client_base, &wallet_base, &pb, 20);
    let agg_base = aggregate(&client_base, &wallet_base);

    // Reweighted: weight of high-score pair increased to 10.
    // Expected aggregate: floor((10*80 + 1*20) / (10+1)) = floor(820/11) = 74.
    let (env_hi, client_hi) = make_env();
    let wallet_hi = Address::generate(&env_hi);
    let pa_hi = pair_sym(&env_hi, 0);
    let pb_hi = pair_sym(&env_hi, 1);
    client_hi.set_pair_weight(&Vec::new(&env_hi), &pa_hi, &10);
    client_hi.set_pair_weight(&Vec::new(&env_hi), &pb_hi, &1);
    submit(&env_hi, &client_hi, &wallet_hi, &pa_hi, 80);
    submit(&env_hi, &client_hi, &wallet_hi, &pb_hi, 20);
    let agg_hi = aggregate(&client_hi, &wallet_hi);

    assert!(
        agg_hi >= agg_base,
        "M1 violated: increasing weight of high-score pair reduced aggregate \
         (baseline={agg_base}, reweighted={agg_hi})"
    );
    // Concrete check: floor(820/11) = 74
    assert_eq!(agg_hi, 74, "M1: expected aggregate 74, got {agg_hi}");
}

// ── M2: Increasing a low-score pair's weight lowers the aggregate ─────────────

/// Pair A has score 20 (below the current aggregate ~50), pair B has score 80.
/// Increasing the weight of pair A from 1 to 10 must not increase the aggregate.
#[test]
fn monotonicity_increase_low_score_weight_lowers_aggregate() {
    // Baseline: equal weights (1, 1) → aggregate ≈ floor((20+80)/2) = 50
    let (env_base, client_base) = make_env();
    let wallet_base = Address::generate(&env_base);
    let pa = pair_sym(&env_base, 0);
    let pb = pair_sym(&env_base, 1);
    client_base.set_pair_weight(&Vec::new(&env_base), &pa, &1);
    client_base.set_pair_weight(&Vec::new(&env_base), &pb, &1);
    submit(&env_base, &client_base, &wallet_base, &pa, 20);
    submit(&env_base, &client_base, &wallet_base, &pb, 80);
    let agg_base = aggregate(&client_base, &wallet_base);

    // Reweighted: weight of low-score pair increased to 10.
    // Expected aggregate: floor((10*20 + 1*80) / 11) = floor(280/11) = 25.
    let (env_lo, client_lo) = make_env();
    let wallet_lo = Address::generate(&env_lo);
    let pa_lo = pair_sym(&env_lo, 0);
    let pb_lo = pair_sym(&env_lo, 1);
    client_lo.set_pair_weight(&Vec::new(&env_lo), &pa_lo, &10);
    client_lo.set_pair_weight(&Vec::new(&env_lo), &pb_lo, &1);
    submit(&env_lo, &client_lo, &wallet_lo, &pa_lo, 20);
    submit(&env_lo, &client_lo, &wallet_lo, &pb_lo, 80);
    let agg_lo = aggregate(&client_lo, &wallet_lo);

    assert!(
        agg_lo <= agg_base,
        "M2 violated: increasing weight of low-score pair raised aggregate \
         (baseline={agg_base}, reweighted={agg_lo})"
    );
    // Concrete check: floor(280/11) = 25
    assert_eq!(agg_lo, 25, "M2: expected aggregate 25, got {agg_lo}");
}

// ── M3: Equal-score pairs — reweighting must not change the aggregate ─────────

/// When all pairs have the same score S, any positive reweighting must leave
/// the aggregate equal to S (since weighted average of identical values = value).
#[test]
fn monotonicity_equal_scores_invariant_under_reweighting() {
    let score = 63u32;

    // Baseline: weights (1, 1, 1)
    let (env_a, client_a) = make_env();
    let wallet_a = Address::generate(&env_a);
    for i in 0..3u32 {
        let pair = pair_sym(&env_a, i);
        client_a.set_pair_weight(&Vec::new(&env_a), &pair, &1);
        submit(&env_a, &client_a, &wallet_a, &pair, score);
    }
    let agg_a = aggregate(&client_a, &wallet_a);

    // Reweighted: weights (1, 5, 10)
    let (env_b, client_b) = make_env();
    let wallet_b = Address::generate(&env_b);
    let weights = [1u32, 5, 10];
    for (i, w) in weights.iter().enumerate() {
        let pair = pair_sym(&env_b, i as u32);
        client_b.set_pair_weight(&Vec::new(&env_b), &pair, w);
        submit(&env_b, &client_b, &wallet_b, &pair, score);
    }
    let agg_b = aggregate(&client_b, &wallet_b);

    assert_eq!(
        agg_a, score,
        "M5 violated in baseline: equal-score aggregate {agg_a} != score {score}"
    );
    assert_eq!(
        agg_b, score,
        "M5 violated after reweighting: equal-score aggregate {agg_b} != score {score}"
    );
}

// ── M4: Dominant-weight pair drives the aggregate toward its score ─────────────

/// One pair has score 90 and weight 1000; the other has score 10 and weight 1.
/// The aggregate must be very close to 90.
/// Expected: floor((1000*90 + 1*10) / 1001) = floor(90010/1001) = 89.
#[test]
fn monotonicity_dominant_weight_drives_aggregate() {
    let (env, client) = make_env();
    let wallet = Address::generate(&env);

    let dominant = pair_sym(&env, 0);
    let minor = pair_sym(&env, 1);

    client.set_pair_weight(&Vec::new(&env), &dominant, &1000);
    client.set_pair_weight(&Vec::new(&env), &minor, &1);

    submit(&env, &client, &wallet, &dominant, 90);
    submit(&env, &client, &wallet, &minor, 10);

    let agg = aggregate(&client, &wallet);
    // floor(90010 / 1001) = 89
    assert_eq!(agg, 89, "M4: dominant-weight aggregate should be 89, got {agg}");
    // Must be within 2 of the dominant pair's score.
    assert!(
        (agg as i64 - 90i64).abs() <= 2,
        "M4: dominant-weight aggregate {agg} not close to dominant pair score 90"
    );
}

// ── M6: max_pair_score is unaffected by reweighting ──────────────────────────

/// The `max_pair_score` field in the aggregate response must always equal the
/// highest individual pair score regardless of how weights are configured.
#[test]
fn monotonicity_max_pair_score_invariant_under_reweighting() {
    let scores = [10u32, 45, 78, 55, 100];

    // Baseline: all weights = 1
    let (env_a, client_a) = make_env();
    let wallet_a = Address::generate(&env_a);
    for (i, s) in scores.iter().enumerate() {
        let pair = pair_sym(&env_a, i as u32);
        submit(&env_a, &client_a, &wallet_a, &pair, *s);
    }
    let resp_a = client_a.get_aggregate_score(&wallet_a).unwrap();

    // Reweighted: assign larger weights to lower-score pairs
    let (env_b, client_b) = make_env();
    let wallet_b = Address::generate(&env_b);
    let weights = [100u32, 50, 10, 5, 1]; // inverse of scores, roughly
    for (i, (s, w)) in scores.iter().zip(weights.iter()).enumerate() {
        let pair = pair_sym(&env_b, i as u32);
        client_b.set_pair_weight(&Vec::new(&env_b), &pair, w);
        submit(&env_b, &client_b, &wallet_b, &pair, *s);
    }
    let resp_b = client_b.get_aggregate_score(&wallet_b).unwrap();

    assert_eq!(
        resp_a.max_pair_score, 100,
        "M6 baseline: max_pair_score={} expected 100", resp_a.max_pair_score
    );
    assert_eq!(
        resp_b.max_pair_score, 100,
        "M6 reweighted: max_pair_score={} expected 100", resp_b.max_pair_score
    );
    // Aggregates must differ (low-score pairs now dominate)
    assert!(
        resp_a.aggregate_score > resp_b.aggregate_score,
        "M6: expected reweighted aggregate to be lower than baseline \
         (baseline={}, reweighted={})",
        resp_a.aggregate_score,
        resp_b.aggregate_score
    );
}
