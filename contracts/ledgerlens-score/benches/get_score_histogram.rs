//! Criterion benchmark for `get_score_histogram` at varying scored-wallet
//! population sizes (issue #1023).
//!
//! Run: `cargo bench -p ledgerlens-score --bench get_score_histogram`
//!
//! `get_score_histogram` sums a fixed 10-bucket histogram vector maintained
//! incrementally by `submit_score` — its own cost is constant and does not
//! re-scan submitted scores. Two population sizes are still benchmarked (0
//! and 500 scored wallets) as a regression guard, to lock in that reading
//! the histogram stays flat as the scored population grows rather than
//! silently becoming an O(population) scan.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
};

fn setup(env: &Env) -> (LedgerLensScoreContractClient<'_>, Symbol) {
    env.mock_all_auths();
    env.budget().reset_unlimited();
    env.ledger().with_mut(|l| l.timestamp = 1_700_000_000);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let service = Address::generate(env);
    client.initialize(&admin, &service);

    let asset_pair = Symbol::new(env, "XLM_USDC");
    (client, asset_pair)
}

fn histogram_cost(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    asset_pair: &Symbol,
    population: u32,
) -> (u64, u64) {
    for i in 0..population {
        let wallet = Address::generate(env);
        client.submit_score(
            &Vec::new(env),
            &wallet,
            asset_pair,
            &(30 + (i % 70)),
            &false,
            &false,
            &(1_700_000_000 + i as u64),
            &90,
            &1,
            &None,
        );
    }

    env.budget().reset_unlimited();
    env.budget().reset_tracker();
    black_box(client.get_score_histogram());

    (env.budget().cpu_instruction_cost(), env.budget().memory_bytes_cost())
}

fn bench_get_score_histogram(c: &mut Criterion) {
    let mut group = c.benchmark_group("get_score_histogram");
    group.sample_size(10);

    for population in [0u32, 500] {
        group.bench_with_input(BenchmarkId::new("population", population), &population, |b, &population| {
            b.iter(|| {
                let env = Env::default();
                let (client, asset_pair) = setup(&env);
                black_box(histogram_cost(&env, &client, &asset_pair, population))
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_get_score_histogram);
criterion_main!(benches);
