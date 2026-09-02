#!// Criterion benchmark for `reconcile_state`.
//!
//! Run: `cargo bench -p ledgerlens-score --bench reconcile_state`
//!
//! Measures the CPU/memory cost of reconciling two on-chain state snapshots
//! via the client method `try_reconcile_state`.  Because the comparison is
//! root-hash-only (no iteration over entries), the cost is effectively
//! independent of `entry_count` — this benchmark confirms that invariance.
//!
//! Two input-size cases are benchmarked:
//!   • Empty: no scores submitted yet (zero entry_count).
//!   • With scores: at least one score submitted (non-zero entry_count).
//!
//! The measured cost is relevant to the resource budget a caller pays for,
//! since `reconcile_state` is the on-chain half of the reconciliation workflow
//! and its cost must be predictable and invariant for budget planning.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
};
use ledgerlens_score::LedgerLensScoreContractClient;

const START_TS: u64 = 1_700_000_000;

fn setup_client(env: &Env) -> LedgerLensScoreContractClient<'_> {
    env.mock_all_auths();
    env.budget().reset_unlimited();
    env.ledger().with_mut(|l| l.timestamp = START_TS);

    let contract_id = env.register_contract(None, ledgerlens_score::LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    client
}

fn bench_reconcile_state(c: &mut Criterion) {
    let mut group = c.benchmark_group("reconcile_state");
    group.sample_size(10);

    // Empty case: no scores submitted yet → entry_count = 0 snapshots.
    group.bench_function("empty", |b| {
        b.iter(|| {
            let env = Env::default();
            let client = setup_client(&env);
            // Take two snapshots via the client's checksum method.
            let snap_a = client.try_compute_state_checksum(&Vec::new(&env)).unwrap().unwrap();
            let snap_b = client.try_compute_state_checksum(&Vec::new(&env)).unwrap().unwrap();
            let _ = black_box(client.try_reconcile_state(&Vec::new(&env), &snap_a, &snap_b));
        });
    });

    // With-scores case: submit one score then take snapshot → entry_count > 0.
    group.bench_function("with_scores", |b| {
        b.iter(|| {
            let env = Env::default();
            let client = setup_client(&env);
            // Submit a score so the snapshot has entry_count > 0.
            let wallet = Address::generate(&env);
            let pair = Symbol::new(&env, "XLM_USDC");
            client.submit_score(
                &Vec::new(&env),
                &wallet,
                &pair,
                &50u32,
                &false,
                &false,
                &START_TS,
                &80u32,
                &1u32,
                &None,
            );
            // Take two snapshots after the score is submitted.
            let snap_a = client.try_compute_state_checksum(&Vec::new(&env)).unwrap().unwrap();
            let snap_b = client.try_compute_state_checksum(&Vec::new(&env)).unwrap().unwrap();
            let _ = black_box(client.try_reconcile_state(&Vec::new(&env), &snap_a, &snap_b));
        });
    });

    group.finish();
}

criterion_group!(benches, bench_reconcile_state);
criterion_main!(benches);