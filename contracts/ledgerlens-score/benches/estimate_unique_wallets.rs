#!// Criterion benchmark for `estimate_unique_wallets`.
//!
//! Run: `cargo bench -p ledgerlens-score --bench estimate_unique_wallets`
//!
//! Measures the CPU/memory cost of estimating the number of unique wallets
//! scored for an asset pair using HyperLogLog.  The cost scales with the
//! HLL precision (number of registers = 2^precision) and the number of
//! unique wallets previously submitted for the pair, since the sketch
//! iterates over all registers and counts zeros.
//!
//! Two input-size cases are benchmarked:
//!   • Small: 5 unique wallets submitted — HLL sketch has few non-zero
//!     registers, the fast-path linear-counting correction may apply.
//!   • Realistic/near-limit: 50 unique wallets submitted — the sketch is
//!     filled close to the HLL depth limit, exercising the full estimate
//!     loop and zero-count correction.
//!
//! The measured cost is relevant to the resource budget a caller pays for,
//! since `estimate_unique_wallets` is used to determine the score-weighted
//! wallet count for an asset pair.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
};

const START_TS: u64 = 1_700_000_000;

fn setup(env: &Env) -> (LedgerLensScoreContractClient<'_>, Symbol) {
    env.mock_all_auths();
    env.budget().reset_unlimited();
    env.ledger().with_mut(|l| l.timestamp = START_TS);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let service = Address::generate(env);
    client.initialize(&admin, &service);

    let asset_pair = Symbol::new(env, "XLM_USDC");
    (client, asset_pair)
}

fn submit_wallets(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    asset_pair: &Symbol,
    count: u32,
) {
    for i in 0..count {
        let wallet = Address::generate(env);
        env.ledger().with_mut(|l| l.timestamp = START_TS + (i as u64) * 10);
        client.submit_score(
            &Vec::new(env),
            &wallet,
            asset_pair,
            &50u32,
            &false,
            &false,
            &env.ledger().timestamp(),
            &80u32,
            &1u32,
            &None,
        );
    }
}

fn bench_estimate_unique_wallets(c: &mut Criterion) {
    let mut group = c.benchmark_group("estimate_unique_wallets");
    group.sample_size(10);

    for &submitted in &[5u32, 50u32] {
        group.bench_with_input(
            BenchmarkId::new("submitted", submitted),
            &submitted,
            |b, &submitted| {
                b.iter(|| {
                    let env = Env::default();
                    let (client, asset_pair) = setup(&env);
                    submit_wallets(&env, &client, &asset_pair, submitted);
                    black_box(client.estimate_unique_wallets(&asset_pair));
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_estimate_unique_wallets);
criterion_main!(benches);