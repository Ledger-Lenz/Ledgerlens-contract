//! Criterion benchmark for `get_interpolated_score` at varying score-history
//! sizes (issue #1024).
//!
//! Run: `cargo bench -p ledgerlens-score --bench get_interpolated_score`
//!
//! The function does an exact-match linear scan over the full history
//! followed by (on a miss) a second linear scan to find the bracketing pair
//! to interpolate between, so its cost scales with history length up to the
//! `MAX_HISTORY_DEPTH` (50) cap.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
};

const MAX_HISTORY_DEPTH: u32 = 50;

fn setup(env: &Env) -> (LedgerLensScoreContractClient<'_>, Address, Symbol) {
    env.mock_all_auths();
    env.budget().reset_unlimited();
    env.ledger().with_mut(|l| l.timestamp = 1_700_000_000);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let service = Address::generate(env);
    client.initialize(&admin, &service);

    let wallet = Address::generate(env);
    let asset_pair = Symbol::new(env, "XLM_USDC");
    (client, wallet, asset_pair)
}

/// Fills `count` history entries, then measures the cost of interpolating a
/// timestamp that falls between the two oldest entries — the worst case,
/// since it forces the exact-match scan to run to completion (a miss) before
/// the bracketing scan finds a match near the start of the ring.
fn interpolate_cost(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    wallet: &Address,
    asset_pair: &Symbol,
    count: u32,
) -> (u64, u64) {
    let mut first_ts = 0u64;
    let mut second_ts = 0u64;
    for i in 0..count {
        let ts = env.ledger().timestamp();
        if i == 0 {
            first_ts = ts;
        } else if i == 1 {
            second_ts = ts;
        }
        client.submit_score(
            &Vec::new(env),
            wallet,
            asset_pair,
            &(30 + (i % 50)),
            &false,
            &false,
            &ts,
            &90,
            &1,
            &None,
        );
        env.ledger().with_mut(|l| l.timestamp += 3_601);
    }

    let target = first_ts + (second_ts.saturating_sub(first_ts)) / 2;

    env.budget().reset_unlimited();
    env.budget().reset_tracker();
    black_box(client.get_interpolated_score(wallet, asset_pair, &target));

    (env.budget().cpu_instruction_cost(), env.budget().memory_bytes_cost())
}

fn bench_get_interpolated_score(c: &mut Criterion) {
    let mut group = c.benchmark_group("get_interpolated_score");
    group.sample_size(10);

    for size in [2u32, MAX_HISTORY_DEPTH] {
        group.bench_with_input(BenchmarkId::new("history_len", size), &size, |b, &size| {
            b.iter(|| {
                let env = Env::default();
                let (client, wallet, asset_pair) = setup(&env);
                black_box(interpolate_cost(&env, &client, &wallet, &asset_pair, size))
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_get_interpolated_score);
criterion_main!(benches);
