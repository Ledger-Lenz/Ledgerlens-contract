//! Criterion benchmarks for `get_wallet_cluster` (issue #1021).
//!
//! Run: `cargo bench -p ledgerlens-score --bench wallet_cluster`
//!
//! Measures CPU instructions and memory byte costs for `get_wallet_cluster`
//! across different wallet profile states:
//!   - Unassigned: wallet with no cluster / no score recorded (returns `None`).
//!   - Small case (1 pair): wallet with a single score evaluated against cluster boundaries.
//!   - Realistic case (5 pairs): wallet with multiple scores across pairs assigned to cluster boundaries.
//!
//! `get_wallet_cluster` is an O(1) persistent storage lookup (`DataKey::WalletCluster(wallet)`),
//! so runtime is constant with respect to total cluster count, but memory footprint and
//! storage cache behavior are measured across uninitialized vs populated states.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
};

const START_TS: u64 = 1_700_000_000;

fn setup(env: &Env) -> (LedgerLensScoreContractClient<'_>, Address, Address) {
    env.mock_all_auths();
    env.budget().reset_unlimited();
    env.ledger().with_mut(|l| l.timestamp = START_TS);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let service = Address::generate(env);
    client.initialize(&admin, &service);

    // Set cluster boundaries [33, 66, 100] -> cluster 0 (<=33), cluster 1 (<=66), cluster 2 (<=100)
    let mut bounds = Vec::new(env);
    bounds.push_back(33u32);
    bounds.push_back(66u32);
    bounds.push_back(100u32);
    client.set_cluster_boundaries(&Vec::new(env), &bounds);

    (client, admin, service)
}

fn submit_score(
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
        &false,
        &false,
        &env.ledger().timestamp(),
        &90,
        &1,
        &None,
    );
}

fn measure<F: FnOnce() -> R, R>(env: &Env, f: F) -> (R, u64, u64) {
    env.budget().reset_unlimited();
    env.budget().reset_tracker();
    let res = f();
    (
        res,
        env.budget().cpu_instruction_cost(),
        env.budget().memory_bytes_cost(),
    )
}

fn bench_get_wallet_cluster(c: &mut Criterion) {
    let mut group = c.benchmark_group("get_wallet_cluster");
    group.sample_size(10);

    // Unassigned wallet (no cluster in storage)
    group.bench_function("unassigned_none", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _) = setup(&env);
            let wallet = Address::generate(&env);
            black_box(measure(&env, || client.get_wallet_cluster(&wallet)))
        });
    });

    // Small case: 1 score submitted (1 pair)
    group.bench_function("small_1_pair", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _) = setup(&env);
            let wallet = Address::generate(&env);
            let pair = symbol_short!("XLM_USDC");
            submit_score(&env, &client, &wallet, &pair, 50);

            black_box(measure(&env, || client.get_wallet_cluster(&wallet)))
        });
    });

    // Realistic case: 5 scores submitted across different pairs
    group.bench_function("realistic_5_pairs", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _) = setup(&env);
            let wallet = Address::generate(&env);
            let pairs = [
                symbol_short!("XLM_USDC"),
                symbol_short!("XLM_BTC"),
                symbol_short!("XLM_ETH"),
                symbol_short!("USDC_EUR"),
                symbol_short!("BTC_USDC"),
            ];
            for (i, p) in pairs.iter().enumerate() {
                env.ledger().with_mut(|l| l.timestamp += 3_601);
                submit_score(&env, &client, &wallet, p, 40 + (i as u32) * 5);
            }

            black_box(measure(&env, || client.get_wallet_cluster(&wallet)))
        });
    });

    group.finish();
}

criterion_group!(benches, bench_get_wallet_cluster);
criterion_main!(benches);
