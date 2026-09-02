//! Criterion benchmark for `query_risk_gate_relative` at varying scored-wallet
//! population sizes (issue #1022).
//!
//! Run: `cargo bench -p ledgerlens-score --bench query_risk_gate_relative`
//!
//! The gate looks up the wallet's own score, then walks the histogram
//! buckets strictly below its bucket (at most 9 of the fixed 10 buckets) to
//! compute a percentile. That inner loop is bounded by the bucket count, not
//! by the number of scored wallets, so two population sizes are benchmarked
//! as a regression guard locking in that the gate's cost stays flat as the
//! scored population grows.

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

fn risk_gate_cost(
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

    let target = Address::generate(env);
    client.submit_score(
        &Vec::new(env),
        &target,
        asset_pair,
        &55,
        &false,
        &false,
        &(1_700_000_000 + population as u64),
        &90,
        &1,
        &None,
    );

    env.budget().reset_unlimited();
    env.budget().reset_tracker();
    black_box(client.query_risk_gate_relative(&target, asset_pair, &20));

    (env.budget().cpu_instruction_cost(), env.budget().memory_bytes_cost())
}

fn bench_query_risk_gate_relative(c: &mut Criterion) {
    let mut group = c.benchmark_group("query_risk_gate_relative");
    group.sample_size(10);

    for population in [0u32, 500] {
        group.bench_with_input(BenchmarkId::new("population", population), &population, |b, &population| {
            b.iter(|| {
                let env = Env::default();
                let (client, asset_pair) = setup(&env);
                black_box(risk_gate_cost(&env, &client, &asset_pair, population))
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_query_risk_gate_relative);
criterion_main!(benches);
