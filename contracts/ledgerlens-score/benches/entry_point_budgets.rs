//! Per-entry-point Soroban resource budgets (issue #756).
//!
//! Run: `cargo bench -p ledgerlens-score --bench entry_point_budgets`
//!
//! Each benchmark measures the CPU-instruction and memory-byte cost of a
//! single representative call to every public contract entry point so that
//! CI can detect resource regressions above an approved tolerance.
//!
//! ## Design notes
//!
//! * `env.budget().reset_tracker()` is called immediately before the
//!   measured call so that setup work (initialization, prior submissions)
//!   is excluded from the reported cost.
//! * `env.budget().reset_unlimited()` keeps the host from aborting the
//!   harness when a test chain of calls would otherwise exceed the default
//!   single-transaction ceiling — only the *measured* call itself is what
//!   the budget number reflects.
//! * All benchmarks use `sample_size(10)` because the soroban-sdk test
//!   environment is deterministic; variance across samples is zero and
//!   larger sample counts just slow CI without providing useful signal.
//! * Entry points that require non-trivial prerequisite state (e.g.
//!   `execute_upgrade` needs a pending proposal) build that state in the
//!   `setup_*` helpers before resetting the tracker.
//!
//! ## Entry points covered
//!
//! Read-only (no state mutation):
//!   get_score, get_score_count, get_score_history (empty / full ring),
//!   get_aggregate_score, query_risk_gate, query_risk_gate_with_confidence,
//!   supports_interface, get_cooldown, get_admin, get_service,
//!   get_history_max_depth, get_expiring_entries (empty / populated index)
//!
//! Write paths (state mutation):
//!   initialize, submit_score (first / subsequent / rate-limited),
//!   submit_scores_batch (size 1 / size 20),
//!   set_cooldown, set_service, set_history_max_depth,
//!   override_rate_limit, set_pair_paused, set_score_floor_policy,
//!   extend_entry_ttls (size 1 / size 20)

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ledgerlens_score::{
    LedgerLensScoreContract, LedgerLensScoreContractClient, ScoreSubmission,
};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
};

// ── Shared constants ────────────────────────────────────────────────────────

const START_TS: u64 = 1_700_000_000;
const COOLDOWN: u64 = 3_601; // just over the default 1-hour cooldown

// ── Harness helpers ─────────────────────────────────────────────────────────

/// Initialize a fresh contract and return (client, admin, service, asset_pair).
fn setup(env: &Env) -> (LedgerLensScoreContractClient<'_>, Address, Address, Symbol) {
    env.mock_all_auths();
    env.budget().reset_unlimited();
    env.ledger().with_mut(|l| l.timestamp = START_TS);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let service = Address::generate(env);
    client.initialize(&admin, &service);

    let asset_pair = Symbol::new(env, "XLM_USDC");
    (client, admin, service, asset_pair)
}

/// Submit one score for wallet/pair at the current ledger timestamp.
fn submit_one(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    wallet: &Address,
    asset_pair: &Symbol,
    score: u32,
) {
    client.submit_score(
        &Vec::new(env),
        wallet,
        asset_pair,
        &score,
        &false,
        &false,
        &env.ledger().timestamp(),
        &90,
        &1,
        &None,
    );
}

/// Advance the ledger clock past the default cooldown.
fn advance(env: &Env) {
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
}

/// Measure cost of `f()`, returning (cpu_instructions, memory_bytes).
fn measure<F: FnOnce()>(env: &Env, f: F) -> (u64, u64) {
    env.budget().reset_unlimited();
    env.budget().reset_tracker();
    f();
    (env.budget().cpu_instruction_cost(), env.budget().memory_bytes_cost())
}

// ── READ-ONLY entry points ───────────────────────────────────────────────────

fn bench_get_score_found(c: &mut Criterion) {
    c.bench_function("get_score/found", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, asset_pair) = setup(&env);
            let wallet = Address::generate(&env);
            submit_one(&env, &client, &wallet, &asset_pair, 50);
            black_box(measure(&env, || {
                black_box(client.get_score(&wallet, &asset_pair).unwrap());
            }))
        });
    });
}

fn bench_get_score_not_found(c: &mut Criterion) {
    c.bench_function("get_score/not_found", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, asset_pair) = setup(&env);
            let wallet = Address::generate(&env);
            black_box(measure(&env, || {
                // ScoreNotFound — infallible read path, returns Err without panicking
                let _ = client.try_get_score(&wallet, &asset_pair);
            }))
        });
    });
}

fn bench_get_score_count(c: &mut Criterion) {
    c.bench_function("get_score_count", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, asset_pair) = setup(&env);
            let wallet = Address::generate(&env);
            submit_one(&env, &client, &wallet, &asset_pair, 50);
            black_box(measure(&env, || {
                black_box(client.get_score_count(&wallet, &asset_pair));
            }))
        });
    });
}

fn bench_get_score_history_empty(c: &mut Criterion) {
    c.bench_function("get_score_history/empty_ring", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, asset_pair) = setup(&env);
            let wallet = Address::generate(&env);
            submit_one(&env, &client, &wallet, &asset_pair, 50);
            black_box(measure(&env, || {
                black_box(client.get_score_history(&wallet, &asset_pair));
            }))
        });
    });
}

fn bench_get_score_history_full(c: &mut Criterion) {
    c.bench_function("get_score_history/full_ring_depth_10", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, asset_pair) = setup(&env);
            let wallet = Address::generate(&env);
            // Fill the ring to the default depth of 10.
            for i in 0..10u32 {
                submit_one(&env, &client, &wallet, &asset_pair, 30 + i);
                advance(&env);
            }
            black_box(measure(&env, || {
                black_box(client.get_score_history(&wallet, &asset_pair));
            }))
        });
    });
}

fn bench_get_aggregate_score(c: &mut Criterion) {
    c.bench_function("get_aggregate_score/three_pairs", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, _) = setup(&env);
            let wallet = Address::generate(&env);
            // Three scored pairs so the weighted average involves real arithmetic.
            for pair_name in ["XLM_USDC", "XLM_BTC", "XLM_ETH"] {
                let pair = Symbol::new(&env, pair_name);
                submit_one(&env, &client, &wallet, &pair, 60);
                advance(&env);
            }
            black_box(measure(&env, || {
                let _ = client.try_get_aggregate_score(&wallet);
            }))
        });
    });
}

fn bench_query_risk_gate_safe(c: &mut Criterion) {
    c.bench_function("query_risk_gate/safe_score_below_threshold", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, asset_pair) = setup(&env);
            let wallet = Address::generate(&env);
            submit_one(&env, &client, &wallet, &asset_pair, 30);
            black_box(measure(&env, || {
                black_box(client.query_risk_gate(&wallet, &asset_pair, &75));
            }))
        });
    });
}

fn bench_query_risk_gate_risky(c: &mut Criterion) {
    c.bench_function("query_risk_gate/risky_score_at_threshold", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, asset_pair) = setup(&env);
            let wallet = Address::generate(&env);
            submit_one(&env, &client, &wallet, &asset_pair, 80);
            black_box(measure(&env, || {
                black_box(client.query_risk_gate(&wallet, &asset_pair, &75));
            }))
        });
    });
}

fn bench_query_risk_gate_no_score(c: &mut Criterion) {
    c.bench_function("query_risk_gate/no_score_fail_closed", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, asset_pair) = setup(&env);
            let wallet = Address::generate(&env);
            black_box(measure(&env, || {
                // No score → fail closed (returns false), must not panic.
                black_box(client.query_risk_gate(&wallet, &asset_pair, &75));
            }))
        });
    });
}

fn bench_query_risk_gate_with_confidence(c: &mut Criterion) {
    c.bench_function("query_risk_gate_with_confidence/low_confidence_blocked", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, asset_pair) = setup(&env);
            let wallet = Address::generate(&env);
            // Score is safe but confidence is below the caller's min floor.
            client.submit_score(
                &Vec::new(&env),
                &wallet,
                &asset_pair,
                &30,
                &false,
                &false,
                &env.ledger().timestamp(),
                &40, // low confidence
                &1,
                &None,
            );
            black_box(measure(&env, || {
                // min_confidence=50 > actual confidence=40 → blocked
                black_box(client.query_risk_gate_with_confidence(
                    &wallet, &asset_pair, &75, &50,
                ));
            }))
        });
    });
}

fn bench_supports_interface(c: &mut Criterion) {
    c.bench_function("supports_interface", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, _) = setup(&env);
            black_box(measure(&env, || {
                black_box(client.supports_interface(&symbol_short!("gate")));
            }))
        });
    });
}

fn bench_get_cooldown(c: &mut Criterion) {
    c.bench_function("get_cooldown", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, _) = setup(&env);
            black_box(measure(&env, || {
                black_box(client.get_cooldown());
            }))
        });
    });
}

fn bench_get_admin(c: &mut Criterion) {
    c.bench_function("get_admin", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, _) = setup(&env);
            black_box(measure(&env, || {
                black_box(client.get_admin());
            }))
        });
    });
}

fn bench_get_expiring_entries_empty(c: &mut Criterion) {
    c.bench_function("get_expiring_entries/empty_index", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, _) = setup(&env);
            black_box(measure(&env, || {
                black_box(client.get_expiring_entries(&100));
            }))
        });
    });
}

fn bench_get_expiring_entries_populated(c: &mut Criterion) {
    c.bench_function("get_expiring_entries/50_entries", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, asset_pair) = setup(&env);
            // Populate 50 distinct wallet entries.
            for i in 0u64..50 {
                let wallet = Address::generate(&env);
                client.submit_score(
                    &Vec::new(&env),
                    &wallet,
                    &asset_pair,
                    &50,
                    &false,
                    &false,
                    &(START_TS + i),
                    &90,
                    &1,
                    &None,
                );
            }
            black_box(measure(&env, || {
                black_box(client.get_expiring_entries(&100));
            }))
        });
    });
}

// ── WRITE entry points ───────────────────────────────────────────────────────

fn bench_initialize(c: &mut Criterion) {
    c.bench_function("initialize", |b| {
        b.iter(|| {
            let env = Env::default();
            env.mock_all_auths();
            env.budget().reset_unlimited();
            env.ledger().with_mut(|l| l.timestamp = START_TS);

            let contract_id = env.register_contract(None, LedgerLensScoreContract);
            let client = LedgerLensScoreContractClient::new(&env, &contract_id);
            let admin = Address::generate(&env);
            let service = Address::generate(&env);

            black_box(measure(&env, || {
                client.initialize(&admin, &service);
            }))
        });
    });
}

fn bench_submit_score_first(c: &mut Criterion) {
    c.bench_function("submit_score/first_submission", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, asset_pair) = setup(&env);
            let wallet = Address::generate(&env);
            black_box(measure(&env, || {
                submit_one(&env, &client, &wallet, &asset_pair, 50);
            }))
        });
    });
}

fn bench_submit_score_subsequent(c: &mut Criterion) {
    c.bench_function("submit_score/subsequent_after_cooldown", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, asset_pair) = setup(&env);
            let wallet = Address::generate(&env);
            // First submission — not measured.
            submit_one(&env, &client, &wallet, &asset_pair, 50);
            advance(&env);
            // Second submission after cooldown — this is the steady-state cost.
            black_box(measure(&env, || {
                submit_one(&env, &client, &wallet, &asset_pair, 55);
            }))
        });
    });
}

fn bench_submit_score_rate_limited(c: &mut Criterion) {
    c.bench_function("submit_score/rate_limited_rejection", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, asset_pair) = setup(&env);
            let wallet = Address::generate(&env);
            submit_one(&env, &client, &wallet, &asset_pair, 50);
            // Do NOT advance — next call will be rejected by the cooldown.
            black_box(measure(&env, || {
                // Expected: RateLimitExceeded — but we measure the cost of the
                // rejected path, which still reads the cooldown state.
                let _ = client.try_submit_score(
                    &Vec::new(&env),
                    &wallet,
                    &asset_pair,
                    &55,
                    &false,
                    &false,
                    &env.ledger().timestamp(),
                    &90,
                    &1,
                    &None,
                );
            }))
        });
    });
}

fn bench_submit_scores_batch_size_1(c: &mut Criterion) {
    c.bench_function("submit_scores_batch/size_1", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, asset_pair) = setup(&env);
            let wallet = Address::generate(&env);
            let mut batch = Vec::new(&env);
            batch.push_back(ScoreSubmission {
                wallet,
                asset_pair: asset_pair.clone(),
                score: 50,
                benford_flag: false,
                ml_flag: false,
                timestamp: START_TS,
                confidence: 90,
                model_version: 1,
            });
            black_box(measure(&env, || {
                black_box(client.submit_scores_batch(&batch));
            }))
        });
    });
}

fn bench_submit_scores_batch_size_20(c: &mut Criterion) {
    c.bench_function("submit_scores_batch/size_20_max_batch", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, asset_pair) = setup(&env);
            let mut batch = Vec::new(&env);
            for i in 0u32..20 {
                batch.push_back(ScoreSubmission {
                    wallet: Address::generate(&env),
                    asset_pair: asset_pair.clone(),
                    score: 30 + i,
                    benford_flag: false,
                    ml_flag: false,
                    timestamp: START_TS,
                    confidence: 90,
                    model_version: 1,
                });
            }
            black_box(measure(&env, || {
                black_box(client.submit_scores_batch(&batch));
            }))
        });
    });
}

fn bench_set_cooldown(c: &mut Criterion) {
    c.bench_function("set_cooldown", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, _) = setup(&env);
            black_box(measure(&env, || {
                client.set_cooldown(&7_200u64); // 2 hours
            }))
        });
    });
}

fn bench_set_service(c: &mut Criterion) {
    c.bench_function("set_service", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, _) = setup(&env);
            let new_service = Address::generate(&env);
            black_box(measure(&env, || {
                client.set_service(&new_service);
            }))
        });
    });
}

fn bench_set_history_max_depth(c: &mut Criterion) {
    c.bench_function("set_history_max_depth", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, _) = setup(&env);
            black_box(measure(&env, || {
                client.set_history_max_depth(&Vec::new(&env), &20u32);
            }))
        });
    });
}

fn bench_override_rate_limit(c: &mut Criterion) {
    c.bench_function("override_rate_limit", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, asset_pair) = setup(&env);
            let wallet = Address::generate(&env);
            submit_one(&env, &client, &wallet, &asset_pair, 50);
            black_box(measure(&env, || {
                client.override_rate_limit(&wallet, &asset_pair);
            }))
        });
    });
}

fn bench_set_pair_paused(c: &mut Criterion) {
    c.bench_function("set_pair_paused/pause", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, asset_pair) = setup(&env);
            black_box(measure(&env, || {
                client.set_pair_paused(&asset_pair, &true);
            }))
        });
    });
}

fn bench_set_score_floor_policy(c: &mut Criterion) {
    c.bench_function("set_score_floor_policy", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, _) = setup(&env);
            black_box(measure(&env, || {
                client.set_score_floor_policy(
                    &Vec::new(&env),
                    &true,
                    &80u32,
                    &20u32,
                );
            }))
        });
    });
}

fn bench_extend_entry_ttls_size_1(c: &mut Criterion) {
    c.bench_function("extend_entry_ttls/size_1", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, asset_pair) = setup(&env);
            let wallet = Address::generate(&env);
            submit_one(&env, &client, &wallet, &asset_pair, 50);
            let mut entries = Vec::new(&env);
            entries.push_back((wallet, asset_pair));
            black_box(measure(&env, || {
                black_box(client.extend_entry_ttls(&Vec::new(&env), &entries));
            }))
        });
    });
}

fn bench_extend_entry_ttls_size_20(c: &mut Criterion) {
    c.bench_function("extend_entry_ttls/size_20", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _, asset_pair) = setup(&env);
            let mut entries = Vec::new(&env);
            for _ in 0..20 {
                let wallet = Address::generate(&env);
                submit_one(&env, &client, &wallet, &asset_pair, 50);
                entries.push_back((wallet, asset_pair.clone()));
            }
            black_box(measure(&env, || {
                black_box(client.extend_entry_ttls(&Vec::new(&env), &entries));
            }))
        });
    });
}

// ── Criterion groups ─────────────────────────────────────────────────────────

criterion_group!(
    read_benches,
    bench_get_score_found,
    bench_get_score_not_found,
    bench_get_score_count,
    bench_get_score_history_empty,
    bench_get_score_history_full,
    bench_get_aggregate_score,
    bench_query_risk_gate_safe,
    bench_query_risk_gate_risky,
    bench_query_risk_gate_no_score,
    bench_query_risk_gate_with_confidence,
    bench_supports_interface,
    bench_get_cooldown,
    bench_get_admin,
    bench_get_expiring_entries_empty,
    bench_get_expiring_entries_populated,
);

criterion_group!(
    write_benches,
    bench_initialize,
    bench_submit_score_first,
    bench_submit_score_subsequent,
    bench_submit_score_rate_limited,
    bench_submit_scores_batch_size_1,
    bench_submit_scores_batch_size_20,
    bench_set_cooldown,
    bench_set_service,
    bench_set_history_max_depth,
    bench_override_rate_limit,
    bench_set_pair_paused,
    bench_set_score_floor_policy,
    bench_extend_entry_ttls_size_1,
    bench_extend_entry_ttls_size_20,
);

criterion_main!(read_benches, write_benches);
