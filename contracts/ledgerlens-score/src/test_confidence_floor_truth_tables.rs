//! Table-driven tests that mirror the formal confidence-floor truth tables
//! documented in `docs/score-math.md` (#722).
//!
//! The truth tables cover every combination of:
//!   - wallet score vs. per-query threshold
//!   - score confidence vs. per-query confidence threshold
//!   - score confidence vs. global minimum confidence floor
//!
//! The gate result for each row is either PASS (true) or FAIL (false).
//!
//! Legend
//! ------
//! score        : wallet score submitted to `submit_score` (0–100)
//! threshold    : per-query score threshold passed to `query_risk_gate` (0–100)
//! conf         : confidence of the submitted score (0–100)
//! query_conf   : per-query minimum confidence passed to
//!                `query_risk_gate_with_confidence`
//! global_floor : global minimum confidence set via `set_global_min_confidence`
//! expected     : expected gate result (true = PASS, false = FAIL)
//!
//! Gate logic (from interface-spec):
//!   PASS iff score >= threshold
//!            AND conf >= query_conf
//!            AND conf >= global_floor

#![cfg(test)]

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient};

// ── Test setup ────────────────────────────────────────────────────────────────

fn make_env<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.budget().reset_unlimited();
    env.ledger().with_mut(|l| l.timestamp = 1_700_000_000);
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client, admin)
}

/// Submit one score for a fresh wallet on the XLM_USDC pair.
fn submit_score(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    wallet: &Address,
    score: u32,
    confidence: u32,
) {
    env.ledger().with_mut(|l| l.timestamp += 3_601);
    client.submit_score(
        &Vec::new(env),
        wallet,
        &symbol_short!("XLM_USDC"),
        &score,
        &false,
        &false,
        &(env.ledger().timestamp()),
        &confidence,
        &1,
        &None,
    );
}

// ── Truth-table row definition ─────────────────────────────────────────────────

struct TruthRow {
    score: u32,
    threshold: u32,
    conf: u32,
    query_conf: u32,
    global_floor: u32,
    expected: bool,
    label: &'static str,
}

// ── Table 1: Score vs threshold (confidence always passes) ────────────────────
//
//  score | threshold | conf | query_conf | global_floor | expected | reason
//  ------|-----------|------|------------|--------------|----------|--------
//    80  |    70     |  90  |     0      |      0       |  true    | score >= threshold
//    70  |    70     |  90  |     0      |      0       |  true    | score == threshold (boundary)
//    69  |    70     |  90  |     0      |      0       |  false   | score < threshold
//     0  |     0     |  90  |     0      |      0       |  true    | both zero
//   100  |   100     |  90  |     0      |      0       |  true    | both max
//     0  |   100     |  90  |     0      |      0       |  false   | score 0 vs max threshold

const TABLE1: &[TruthRow] = &[
    TruthRow { score: 80, threshold: 70, conf: 90, query_conf: 0, global_floor: 0, expected: true,  label: "score_above_threshold" },
    TruthRow { score: 70, threshold: 70, conf: 90, query_conf: 0, global_floor: 0, expected: true,  label: "score_equals_threshold" },
    TruthRow { score: 69, threshold: 70, conf: 90, query_conf: 0, global_floor: 0, expected: false, label: "score_one_below_threshold" },
    TruthRow { score:  0, threshold:  0, conf: 90, query_conf: 0, global_floor: 0, expected: true,  label: "score_zero_threshold_zero" },
    TruthRow { score: 100, threshold: 100, conf: 90, query_conf: 0, global_floor: 0, expected: true,  label: "score_max_threshold_max" },
    TruthRow { score:  0, threshold: 100, conf: 90, query_conf: 0, global_floor: 0, expected: false, label: "score_zero_threshold_max" },
];

// ── Table 2: Confidence vs per-query confidence threshold ─────────────────────
//
//  score | threshold | conf | query_conf | global_floor | expected | reason
//  ------|-----------|------|------------|--------------|----------|--------
//    80  |    70     |  80  |    80      |      0       |  true    | conf == query_conf (boundary)
//    80  |    70     |  79  |    80      |      0       |  false   | conf one below query_conf
//    80  |    70     |  81  |    80      |      0       |  true    | conf above query_conf
//    80  |    70     | 100  |   100      |      0       |  true    | conf == query_conf == max
//    80  |    70     |  99  |   100      |      0       |  false   | conf one below max query_conf
//    80  |    70     |   0  |     0      |      0       |  true    | both zero

const TABLE2: &[TruthRow] = &[
    TruthRow { score: 80, threshold: 70, conf: 80, query_conf: 80,  global_floor: 0, expected: true,  label: "conf_equals_query_conf" },
    TruthRow { score: 80, threshold: 70, conf: 79, query_conf: 80,  global_floor: 0, expected: false, label: "conf_one_below_query_conf" },
    TruthRow { score: 80, threshold: 70, conf: 81, query_conf: 80,  global_floor: 0, expected: true,  label: "conf_above_query_conf" },
    TruthRow { score: 80, threshold: 70, conf: 100, query_conf: 100, global_floor: 0, expected: true,  label: "conf_max_equals_query_conf_max" },
    TruthRow { score: 80, threshold: 70, conf: 99, query_conf: 100, global_floor: 0, expected: false, label: "conf_one_below_max_query_conf" },
    TruthRow { score: 80, threshold: 70, conf:  0, query_conf:   0, global_floor: 0, expected: true,  label: "conf_zero_query_conf_zero" },
];

// ── Table 3: Confidence vs global floor ───────────────────────────────────────
//
//  score | threshold | conf | query_conf | global_floor | expected | reason
//  ------|-----------|------|------------|--------------|----------|--------
//    80  |    70     |  75  |     0      |     75       |  true    | conf == global_floor (boundary)
//    80  |    70     |  74  |     0      |     75       |  false   | conf one below global_floor
//    80  |    70     |  76  |     0      |     75       |  true    | conf above global_floor
//    80  |    70     |   0  |     0      |      0       |  true    | floor is zero, always passes
//    80  |    70     | 100  |     0      |    100       |  true    | conf == global_floor == max

const TABLE3: &[TruthRow] = &[
    TruthRow { score: 80, threshold: 70, conf: 75, query_conf: 0, global_floor: 75,  expected: true,  label: "conf_equals_global_floor" },
    TruthRow { score: 80, threshold: 70, conf: 74, query_conf: 0, global_floor: 75,  expected: false, label: "conf_one_below_global_floor" },
    TruthRow { score: 80, threshold: 70, conf: 76, query_conf: 0, global_floor: 75,  expected: true,  label: "conf_above_global_floor" },
    TruthRow { score: 80, threshold: 70, conf:  0, query_conf: 0, global_floor:  0,  expected: true,  label: "global_floor_zero" },
    TruthRow { score: 80, threshold: 70, conf: 100, query_conf: 0, global_floor: 100, expected: true,  label: "conf_max_floor_max" },
];

// ── Table 4: Combined constraints (score, per-query conf, global floor) ───────
//
//  score | threshold | conf | query_conf | global_floor | expected | reason
//  ------|-----------|------|------------|--------------|----------|--------
//    80  |    70     |  85  |    80      |     75       |  true    | all pass
//    65  |    70     |  85  |    80      |     75       |  false   | score fails
//    80  |    70     |  79  |    80      |     75       |  false   | query_conf fails
//    80  |    70     |  74  |    70      |     75       |  false   | global_floor fails
//    80  |    70     |  74  |    80      |     75       |  false   | both conf checks fail
//   100  |   100     | 100  |   100      |    100       |  true    | all at maximum

const TABLE4: &[TruthRow] = &[
    TruthRow { score: 80, threshold: 70, conf: 85, query_conf: 80, global_floor: 75, expected: true,  label: "combined_all_pass" },
    TruthRow { score: 65, threshold: 70, conf: 85, query_conf: 80, global_floor: 75, expected: false, label: "combined_score_fails" },
    TruthRow { score: 80, threshold: 70, conf: 79, query_conf: 80, global_floor: 75, expected: false, label: "combined_query_conf_fails" },
    TruthRow { score: 80, threshold: 70, conf: 74, query_conf: 70, global_floor: 75, expected: false, label: "combined_global_floor_fails" },
    TruthRow { score: 80, threshold: 70, conf: 74, query_conf: 80, global_floor: 75, expected: false, label: "combined_both_conf_fail" },
    TruthRow { score: 100, threshold: 100, conf: 100, query_conf: 100, global_floor: 100, expected: true, label: "combined_all_max" },
];

// ── Driver ────────────────────────────────────────────────────────────────────

fn run_truth_table(table: &[TruthRow], table_name: &str) {
    for row in table {
        let (env, client, _admin) = make_env();
        let wallet = Address::generate(&env);

        // Set global confidence floor.
        client.set_global_min_confidence(&row.global_floor);

        // Submit the score with the row's confidence value.
        submit_score(&env, &client, &wallet, row.score, row.conf);

        // Query the gate.
        let result = client.query_risk_gate_with_confidence(
            &wallet,
            &symbol_short!("XLM_USDC"),
            &row.threshold,
            &row.query_conf,
        );

        assert_eq!(
            result, row.expected,
            "{table_name}[{}]: score={} threshold={} conf={} query_conf={} global_floor={} \
             expected={} got={}",
            row.label,
            row.score, row.threshold, row.conf, row.query_conf, row.global_floor,
            row.expected, result
        );
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[test]
fn truth_table_1_score_vs_threshold() {
    run_truth_table(TABLE1, "Table1(score_vs_threshold)");
}

#[test]
fn truth_table_2_confidence_vs_query_conf() {
    run_truth_table(TABLE2, "Table2(conf_vs_query_conf)");
}

#[test]
fn truth_table_3_confidence_vs_global_floor() {
    run_truth_table(TABLE3, "Table3(conf_vs_global_floor)");
}

#[test]
fn truth_table_4_combined_constraints() {
    run_truth_table(TABLE4, "Table4(combined)");
}

// ── Additional boundary checks ─────────────────────────────────────────────────

/// Global min confidence of 100 means only confidence=100 passes.
#[test]
fn global_floor_100_rejects_confidence_99() {
    let (env, client, _admin) = make_env();
    let wallet = Address::generate(&env);
    client.set_global_min_confidence(&100);
    submit_score(&env, &client, &wallet, 80, 99);
    let result = client.query_risk_gate_with_confidence(
        &wallet,
        &symbol_short!("XLM_USDC"),
        &70,
        &0,
    );
    assert!(!result, "global_floor=100 should reject confidence=99");
}

/// Global min confidence of 0 never blocks a gate based on confidence alone.
#[test]
fn global_floor_0_never_blocks_on_confidence() {
    let (env, client, _admin) = make_env();
    let wallet = Address::generate(&env);
    client.set_global_min_confidence(&0);
    submit_score(&env, &client, &wallet, 80, 0);
    let result = client.query_risk_gate_with_confidence(
        &wallet,
        &symbol_short!("XLM_USDC"),
        &70,
        &0,
    );
    assert!(result, "global_floor=0 with conf=0 and query_conf=0 should pass");
}

/// Changing the global floor dynamically affects subsequent gate queries
/// for already-submitted scores.
#[test]
fn dynamic_global_floor_change_affects_gate() {
    let (env, client, _admin) = make_env();
    let wallet = Address::generate(&env);

    // Submit with confidence=75.
    submit_score(&env, &client, &wallet, 80, 75);

    // Floor=70: should pass.
    client.set_global_min_confidence(&70);
    let pass = client.query_risk_gate_with_confidence(
        &wallet, &symbol_short!("XLM_USDC"), &70, &0,
    );
    assert!(pass, "floor=70, conf=75 should pass");

    // Raise floor to 80: should now fail.
    client.set_global_min_confidence(&80);
    let fail = client.query_risk_gate_with_confidence(
        &wallet, &symbol_short!("XLM_USDC"), &70, &0,
    );
    assert!(!fail, "floor=80, conf=75 should fail");
}
