//! Criterion benchmarks for `get_portfolio_var` (issue #1020).
//!
//! Run: `cargo bench -p ledgerlens-score --bench portfolio_var`
//!
//! Measures Value-at-Risk computation across wallet score pairs.
//! The algorithm calculates an N x N weighted covariance matrix across all scored pairs,
//! where N is the number of active asset pairs for the wallet.
//!
//! Benchmarked input sizes:
//!   - Small case: 2 pairs (minimal valid portfolio for VaR computation).
//!   - Medium case: 5 pairs (typical multi-asset portfolio).
//!   - Realistic / near-limit case: 10 pairs (exercises full O(N^2) pairwise covariance calculations).

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
use soroban_sdk::{
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

    (client, admin, service)
}

fn populate_portfolio(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    wallet: &Address,
    num_pairs: usize,
) {
    let pair_names: [&str; 10] = [
        "P_AA", "P_BB", "P_CC", "P_DD", "P_EE",
        "P_FF", "P_GG", "P_HH", "P_II", "P_JJ",
    ];

    for i in 0..num_pairs {
        let pair = Symbol::new(env, pair_names[i]);
        client.submit_score(
            &Vec::new(env),
            wallet,
            &pair,
            &(50 + (i as u32) * 3),
            &false,
            &false,
            &env.ledger().timestamp(),
            &90,
            &1,
            &None,
        );
    }
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

fn bench_get_portfolio_var(c: &mut Criterion) {
    let mut group = c.benchmark_group("get_portfolio_var");
    group.sample_size(10);

    for &num_pairs in &[2usize, 5, 10] {
        group.bench_with_input(
            BenchmarkId::new("pairs", num_pairs),
            &num_pairs,
            |b, &num_pairs| {
                b.iter(|| {
                    let env = Env::default();
                    let (client, _, _) = setup(&env);
                    let wallet = Address::generate(&env);
                    populate_portfolio(&env, &client, &wallet, num_pairs);

                    black_box(measure(&env, || {
                        client.get_portfolio_var(&wallet, &95)
                    }))
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_get_portfolio_var);
criterion_main!(benches);
