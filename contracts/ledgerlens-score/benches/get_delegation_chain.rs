#!// Criterion benchmark for `get_delegation_chain`.
//!
//! Run: `cargo bench -p ledgerlens-score --bench get_delegation_chain`
//!
//! Measures the CPU/memory cost of walking the full delegation chain for a
//! wallet using `get_delegation_chain`.  The cost scales with the number of
//! delegation hops traversed (each hop reads storage and performs cycle
//! detection), bounded by `MAX_DELEGATION_DEPTH` (= 5).
//!
//! Two input-size cases are benchmarked:
//!   • Small: depth = 1 (wallet + 1 delegate — 2 addresses in chain).
//!   • Near-limit: depth = 5 (= MAX_DELEGATION_DEPTH — 6 addresses in
//!     chain, the maximum possible before cycle detection stops the walk).
//!
//! The measured cost is relevant to the resource budget a caller pays for,
//! since `get_delegation_chain` walks the full delegation chain and its cost
//! is a key component of delegation-related resource billing.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ledgerlens_score::LedgerLensScoreContractClient;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env,
};

const START_TS: u64 = 1_700_000_000;

fn setup_client(env: &Env) -> LedgerLensScoreContractClient<'_> {
    env.mock_all_auths();
    env.budget().reset_unlimited();
    env.ledger().with_mut(|l| l.timestamp = START_TS);

    let contract_id = env.register_contract(None, ledgerlens_score::LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let service = Address::generate(env);
    client.initialize(&admin, &service);

    client
}

fn bench_get_delegation_chain(c: &mut Criterion) {
    let mut group = c.benchmark_group("get_delegation_chain");
    group.sample_size(10);

    for &depth in &[1u32, 5u32] {
        group.bench_with_input(
            BenchmarkId::new("depth", depth),
            &depth,
            |b, &depth| {
                b.iter(|| {
                    let env = Env::default();
                    let client = setup_client(&env);
                    // Generate the wallet whose delegation chain we'll walk.
                    let wallet = Address::generate(&env);
                    // Build a delegation chain of the given depth:
                    // wallet -> d1 -> d2 -> ... -> dN
                    let mut current = wallet.clone();
                    for _ in 1..=depth {
                        let next = Address::generate(&env);
                        client.set_score_delegate(&current, &next);
                        current = next;
                    }
                    black_box(client.get_delegation_chain(&wallet));
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_get_delegation_chain);
criterion_main!(benches);