#!// Criterion benchmark for `get_private_aggregate_score`.
//!
//! Run: `cargo bench -p ledgerlens-score --bench get_private_aggregate_score`
//!
//! Measures the CPU/memory cost of computing a differentially-private aggregate
//! score for a wallet via `get_private_aggregate_score`.  The cost is
//! dominated by `compute_aggregate_score`, which iterates over all wallet
//! pairs and reads each pair's score from storage.  The differential privacy
//! noise computation is O(1) given the aggregate score.
//!
//! Two input-size cases are benchmarked:
//!   • Small: 1 asset pair submitted for the wallet — minimal cost, exercises
//!     the aggregate computation + DP noise path once.
//!   • Near-limit: 20 asset pairs submitted for the wallet — exercises
//!     the full O(N) iteration loop over all wallet pairs, which is the
//!     documented worst-case bound (`debug_assert!(pairs.len() <= 20)`).
//!
//! The measured cost is relevant to the resource budget a caller pays for,
//! since `get_private_aggregate_score` computes a per-wallet aggregate that
//! scales with the number of asset pairs the wallet has scores for.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
};

const START_TS: u64 = 1_700_000_000;

fn setup(env: &Env) -> (LedgerLensScoreContractClient<'_>, Address) {
    env.mock_all_auths();
    env.budget().reset_unlimited();
    env.ledger().with_mut(|l| l.timestamp = START_TS);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let service = Address::generate(env);
    client.initialize(&admin, &service);

    let wallet = Address::generate(env);
    (client, wallet)
}

fn submit_pairs(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    wallet: &Address,
    pair_count: u32,
) {
    for i in 0..pair_count {
        let pair = Symbol::new(env, &format!("ASSET_{}", i));
        client.submit_score(
            &Vec::new(env),
            wallet,
            &pair,
            &50u32,
            &false,
            &false,
            &START_TS,
            &80u32,
            &1u32,
            &None,
        );
    }
}

fn bench_get_private_aggregate_score(c: &mut Criterion) {
    let mut group = c.benchmark_group("get_private_aggregate_score");
    group.sample_size(10);

    for &pair_count in &[1u32, 20u32] {
        group.bench_with_input(
            BenchmarkId::new("pair_count", pair_count),
            &pair_count,
            |b, &pair_count| {
                b.iter(|| {
                    let env = Env::default();
                    let (client, wallet) = setup(&env);
                    submit_pairs(&env, &client, &wallet, pair_count);
                    black_box(client.get_private_aggregate_score(&wallet, &1u32));
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_get_private_aggregate_score);
criterion_main!(benches);