//! Criterion benchmarks for `verify_membership` (issue #1019).
//!
//! Run: `cargo bench -p ledgerlens-score --bench verify_membership`
//!
//! Measures Verkle / polynomial state commitment opening proof verification.
//! Proof verification involves:
//!   - Commitment decoding (48-byte BLS12-381 G1 representation).
//!   - Proof blob decoding (evaluation point z, value element v, witness).
//!   - Evaluation point derivation (hash of wallet address and asset pair).
//!   - Value element derivation (hash of score, timestamp, evaluation point).
//!   - Witness verification against commitment root.
//!
//! Benchmarked scenarios:
//!   - Small state (1 entry): valid membership proof against a single-entry Verkle commitment.
//!   - Populated state (20 entries): valid membership proof against a multi-entry committed state.
//!   - Non-membership proof: opening proof verifying absence of an unrecorded wallet (score 0, sentinel v).
//!
//! Note on cost scaling: `verify_membership` executes in O(1) cryptographic operations
//! regardless of total contract state size due to the constant-size polynomial commitment proof structure.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, BytesN, Env, Symbol, Vec,
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

fn populate_entries(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    count: usize,
) -> (Address, Symbol, u32, u64) {
    let mut target_wallet = Address::generate(env);
    let mut target_pair = symbol_short!("XLMUSDC");
    let mut target_score = 42u32;
    let mut target_ts = START_TS;

    for i in 0..count {
        let wallet = Address::generate(env);
        let pair = symbol_short!("XLMUSDC");
        let score = 30 + (i as u32);
        let ts = START_TS + (i as u64) * 3_601;
        env.ledger().with_mut(|l| l.timestamp = ts);

        client.submit_score(
            &Vec::new(env),
            &wallet,
            &pair,
            &score,
            &false,
            &false,
            &ts,
            &90,
            &1,
            &None,
        );

        if i == 0 {
            target_wallet = wallet;
            target_pair = pair;
            target_score = score;
            target_ts = ts;
        }
    }

    (target_wallet, target_pair, target_score, target_ts)
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

fn bench_verify_membership(c: &mut Criterion) {
    let mut group = c.benchmark_group("verify_membership");
    group.sample_size(10);

    for &state_size in &[1usize, 20] {
        group.bench_with_input(
            BenchmarkId::new("valid_member_state_size", state_size),
            &state_size,
            |b, &state_size| {
                b.iter(|| {
                    let env = Env::default();
                    let (client, _, _) = setup(&env);
                    let (wallet, pair, score, timestamp) =
                        populate_entries(&env, &client, state_size);

                    let commitment = client.get_state_commitment();
                    let proof = client.get_membership_proof(&wallet, &pair);

                    black_box(measure(&env, || {
                        client.verify_membership(
                            &commitment,
                            &wallet,
                            &pair,
                            &score,
                            &timestamp,
                            &proof,
                        )
                    }))
                });
            },
        );
    }

    // Non-membership verification benchmark
    group.bench_function("non_member_proof", |b| {
        b.iter(|| {
            let env = Env::default();
            let (client, _, _) = setup(&env);
            populate_entries(&env, &client, 5);

            let absent_wallet = Address::generate(&env);
            let pair = symbol_short!("XLMUSDC");
            let commitment = client.get_state_commitment();
            let proof = client.get_membership_proof(&absent_wallet, &pair);

            black_box(measure(&env, || {
                client.verify_membership(&commitment, &absent_wallet, &pair, &0, &0, &proof)
            }))
        });
    });

    group.finish();
}

criterion_group!(benches, bench_verify_membership);
criterion_main!(benches);
