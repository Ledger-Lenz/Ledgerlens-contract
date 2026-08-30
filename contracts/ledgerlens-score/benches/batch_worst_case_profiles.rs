//! Worst-case batch submission resource profiles (issue #757).
//!
//! Run: `cargo bench -p ledgerlens-score --bench batch_worst_case_profiles`
//!
//! Measures accepted, rejected, attested, and mixed batch submissions at the
//! boundary sizes that represent realistic worst cases for on-chain resource
//! consumption.  The benchmark is designed so that each group's output can be
//! compared column-for-column to isolate the cost of:
//!
//!   • Validation overhead  — rejected entries still parse and check fields.
//!   • Attestation overhead — one secp256k1_recover + O(log n) Merkle proof
//!                            walks per `submit_scores_batch_attested` call.
//!   • Storage overhead     — persistent writes + TTL extension per accepted entry.
//!   • Event overhead       — one `score` event emitted per accepted entry.
//!
//! ## Groups
//!
//! `all_accepted`       — every entry in the batch passes all checks and is
//!                        written to storage.  Upper bound on storage + event cost.
//!
//! `all_rejected`       — every entry fails validation (score > 100).  Lower
//!                        bound: no storage writes or events, only parse + check cost.
//!
//! `rate_limited`       — batch for a single wallet/pair where all entries after
//!                        the first are rejected by the cooldown.  Measures the
//!                        per-entry cooldown-check cost without storage writes.
//!
//! `attested_all_accepted` — same as `all_accepted` but via
//!                        `submit_scores_batch_attested`: adds one secp256k1_recover
//!                        + per-entry Merkle proof walk.  Comparing this group
//!                        against `all_accepted` isolates the attestation overhead.
//!
//! `attested_all_rejected` — attested batch where all entries fail score
//!                        validation.  The Merkle proof is still walked per entry,
//!                        but no storage writes occur.
//!
//! `mixed_half_accepted` — half the entries accepted, half rejected (alternating
//!                        invalid score).  Exercises the per-entry branching in
//!                        `submit_scores_batch` / `submit_scores_batch_attested`.
//!
//! ## Batch sizes benchmarked
//!
//! Sizes 1, 5, 10, and 20 (MAX_BATCH_SIZE) give a full cost-vs-size profile.
//! Size 20 is the on-chain hard cap; it is the primary CI regression anchor.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use k256::ecdsa::SigningKey;
use ledgerlens_score::{
    BatchAttestation, LedgerLensScoreContract, LedgerLensScoreContractClient, ScoreSubmission,
    ScoreSubmissionWithProof,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Bytes, BytesN, Env, Symbol, SymbolStr, TryFromVal, Vec,
};

const START_TS: u64 = 1_700_000_000;

// ── Crypto helpers (same as batch_attested.rs) ───────────────────────────────

fn signing_key(seed: u8) -> SigningKey {
    let mut bytes = [0u8; 32];
    bytes[31] = seed;
    bytes[0] = 1;
    SigningKey::from_bytes((&bytes).into()).unwrap()
}

fn pubkey_bytes(env: &Env, key: &SigningKey) -> Bytes {
    let point = key.verifying_key().to_encoded_point(true);
    Bytes::from_slice(env, point.as_bytes())
}

fn commitment(
    env: &Env,
    contract_addr: &Address,
    wallet: &Address,
    pair: &Symbol,
    score: u32,
    ts: u64,
) -> [u8; 32] {
    let pair_str = SymbolStr::try_from_val(env, &pair.to_symbol_val()).unwrap();
    let pair_bytes: &[u8] = pair_str.as_ref();
    let mut pair_buf = [0u8; 9];
    pair_buf[..pair_bytes.len()].copy_from_slice(pair_bytes);

    let mut wallet_buf = [0u8; 56];
    wallet.to_string().copy_into_slice(&mut wallet_buf);

    let mut contract_buf = [0u8; 56];
    contract_addr.to_string().copy_into_slice(&mut contract_buf);

    let mut preimage = Bytes::new(env);
    preimage.extend_from_array(&wallet_buf);
    preimage.extend_from_array(&pair_buf);
    preimage.extend_from_array(&score.to_le_bytes());
    preimage.push_back(0u8);
    preimage.push_back(0u8);
    preimage.extend_from_array(&ts.to_le_bytes());
    preimage.extend_from_array(&90u32.to_le_bytes());
    preimage.extend_from_array(&1u32.to_le_bytes());
    preimage.extend_from_array(&contract_buf);
    preimage.extend_from_array(&env.ledger().network_id().to_array());
    preimage.extend_from_array(&[0u8; 32]);
    preimage.extend_from_array(&0u32.to_le_bytes());
    env.crypto().sha256(&preimage).to_bytes().to_array()
}

fn merkle_leaf(env: &Env, c: &[u8; 32]) -> [u8; 32] {
    let mut p = [0u8; 33];
    p[0] = 0x00;
    p[1..].copy_from_slice(c);
    env.crypto().sha256(&Bytes::from_array(env, &p)).to_bytes().to_array()
}

fn merkle_internal(env: &Env, l: &[u8; 32], r: &[u8; 32]) -> [u8; 32] {
    let mut p = [0u8; 65];
    p[0] = 0x01;
    p[1..33].copy_from_slice(l);
    p[33..65].copy_from_slice(r);
    env.crypto().sha256(&Bytes::from_array(env, &p)).to_bytes().to_array()
}

fn next_pow2(n: u32) -> u32 {
    let mut p = 1u32;
    while p < n {
        p *= 2;
    }
    p
}

fn build_merkle_root(env: &Env, leaves: &[[u8; 32]]) -> [u8; 32] {
    let mut level: std::vec::Vec<[u8; 32]> = leaves.to_vec();
    while level.len() > 1 {
        let mut next = std::vec::Vec::new();
        let mut i = 0;
        while i < level.len() {
            next.push(merkle_internal(env, &level[i], &level[i + 1]));
            i += 2;
        }
        level = next;
    }
    level[0]
}

fn build_merkle_proof(
    env: &Env,
    leaves: &[[u8; 32]],
    index: u32,
) -> (std::vec::Vec<[u8; 32]>, u32) {
    let mut level: std::vec::Vec<[u8; 32]> = leaves.to_vec();
    let mut proof = std::vec::Vec::new();
    let mut flags: u32 = 0;
    let mut idx = index as usize;
    while level.len() > 1 {
        let sib = idx ^ 1;
        if (idx & 1) == 1 {
            flags |= 1 << proof.len();
        }
        proof.push(level[sib]);
        let mut next = std::vec::Vec::new();
        let mut i = 0;
        while i < level.len() {
            next.push(merkle_internal(env, &level[i], &level[i + 1]));
            i += 2;
        }
        level = next;
        idx /= 2;
    }
    (proof, flags)
}

fn sign_root(env: &Env, key: &SigningKey, root: &[u8; 32]) -> BatchAttestation {
    let digest = env.crypto().sha256(&Bytes::from_array(env, root)).to_bytes().to_array();
    let (sig, recid) = key.sign_prehash_recoverable(&digest).unwrap();
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&sig.to_bytes());
    sig_bytes[64] = recid.to_byte();
    BatchAttestation {
        merkle_root: BytesN::from_array(env, root),
        signature: BytesN::from_array(env, &sig_bytes),
    }
}

// ── Contract setup ───────────────────────────────────────────────────────────

fn setup_plain(env: &Env) -> (LedgerLensScoreContractClient<'_>, Symbol) {
    env.mock_all_auths();
    env.budget().reset_unlimited();
    env.ledger().with_mut(|l| l.timestamp = START_TS);
    let id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(env, &id);
    client.initialize(&Address::generate(env), &Address::generate(env));
    (client, Symbol::new(env, "XLM_USDC"))
}

fn setup_attested(env: &Env) -> (LedgerLensScoreContractClient<'_>, Symbol, SigningKey) {
    let (client, pair) = setup_plain(env);
    let key = signing_key(1);
    client.set_service_pubkey(&Vec::new(env), &pubkey_bytes(env, &key));
    (client, pair, key)
}

// ── Batch builders ───────────────────────────────────────────────────────────

/// Build a plain batch of `count` entries.
/// `reject` — if true, set score=200 (> 100) so every entry fails validation.
/// `same_wallet` — if true, reuse the same wallet for all entries (triggers
///   rate-limit rejection after the first).
fn build_plain_batch(
    env: &Env,
    pair: &Symbol,
    count: u32,
    reject: bool,
    same_wallet: bool,
) -> Vec<ScoreSubmission> {
    let shared_wallet = Address::generate(env);
    let mut batch = Vec::new(env);
    for i in 0..count {
        let wallet = if same_wallet { shared_wallet.clone() } else { Address::generate(env) };
        batch.push_back(ScoreSubmission {
            wallet,
            asset_pair: pair.clone(),
            score: if reject { 200 } else { 30 + (i % 50) },
            benford_flag: false,
            ml_flag: false,
            timestamp: START_TS,
            confidence: 90,
            model_version: 1,
        });
    }
    batch
}

/// Build a mixed batch: even-indexed entries accepted, odd-indexed rejected
/// (score > 100 on odd positions).
fn build_mixed_batch(env: &Env, pair: &Symbol, count: u32) -> Vec<ScoreSubmission> {
    let mut batch = Vec::new(env);
    for i in 0..count {
        batch.push_back(ScoreSubmission {
            wallet: Address::generate(env),
            asset_pair: pair.clone(),
            score: if i % 2 == 0 { 50 } else { 200 },
            benford_flag: false,
            ml_flag: false,
            timestamp: START_TS,
            confidence: 90,
            model_version: 1,
        });
    }
    batch
}

/// Build an attested batch of `count` entries, all with valid or all with
/// invalid scores depending on `reject`.
fn build_attested_batch(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    pair: &Symbol,
    key: &SigningKey,
    count: u32,
    reject: bool,
) -> (Vec<ScoreSubmissionWithProof>, BatchAttestation) {
    let padded = next_pow2(count) as usize;
    let mut subs: std::vec::Vec<ScoreSubmission> = std::vec::Vec::new();
    let mut leaves: std::vec::Vec<[u8; 32]> = std::vec::Vec::new();

    for i in 0..count {
        let wallet = Address::generate(env);
        let score = if reject { 200 } else { 30 + (i % 50) };
        let ts = START_TS + i as u64;
        let c = commitment(env, &client.address, &wallet, pair, score, ts);
        subs.push(ScoreSubmission {
            wallet,
            asset_pair: pair.clone(),
            score,
            benford_flag: false,
            ml_flag: false,
            timestamp: ts,
            confidence: 90,
            model_version: 1,
        });
        leaves.push(merkle_leaf(env, &c));
    }
    while leaves.len() < padded {
        let last = *leaves.last().unwrap();
        leaves.push(last);
    }

    let root = build_merkle_root(env, &leaves);
    let attestation = sign_root(env, key, &root);

    let mut result: Vec<ScoreSubmissionWithProof> = Vec::new(env);
    for (i, sub) in subs.into_iter().enumerate() {
        let (proof_bytes, flags) = build_merkle_proof(env, &leaves, i as u32);
        let mut proof: Vec<BytesN<32>> = Vec::new(env);
        for p in proof_bytes {
            proof.push_back(BytesN::from_array(env, &p));
        }
        result.push_back(ScoreSubmissionWithProof { submission: sub, proof, proof_flags: flags });
    }
    (result, attestation)
}

/// Build a mixed attested batch: even indices valid, odd indices score=200.
fn build_attested_mixed(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    pair: &Symbol,
    key: &SigningKey,
    count: u32,
) -> (Vec<ScoreSubmissionWithProof>, BatchAttestation) {
    let padded = next_pow2(count) as usize;
    let mut subs: std::vec::Vec<ScoreSubmission> = std::vec::Vec::new();
    let mut leaves: std::vec::Vec<[u8; 32]> = std::vec::Vec::new();

    for i in 0..count {
        let wallet = Address::generate(env);
        let score = if i % 2 == 0 { 50 } else { 200 };
        let ts = START_TS + i as u64;
        let c = commitment(env, &client.address, &wallet, pair, score, ts);
        subs.push(ScoreSubmission {
            wallet,
            asset_pair: pair.clone(),
            score,
            benford_flag: false,
            ml_flag: false,
            timestamp: ts,
            confidence: 90,
            model_version: 1,
        });
        leaves.push(merkle_leaf(env, &c));
    }
    while leaves.len() < padded {
        let last = *leaves.last().unwrap();
        leaves.push(last);
    }

    let root = build_merkle_root(env, &leaves);
    let attestation = sign_root(env, key, &root);

    let mut result: Vec<ScoreSubmissionWithProof> = Vec::new(env);
    for (i, sub) in subs.into_iter().enumerate() {
        let (proof_bytes, flags) = build_merkle_proof(env, &leaves, i as u32);
        let mut proof: Vec<BytesN<32>> = Vec::new(env);
        for p in proof_bytes {
            proof.push_back(BytesN::from_array(env, &p));
        }
        result.push_back(ScoreSubmissionWithProof { submission: sub, proof, proof_flags: flags });
    }
    (result, attestation)
}

// ── Benchmark groups ─────────────────────────────────────────────────────────

/// Group 1: plain batch — all entries accepted.
/// Cost = validation + storage writes + event emission, O(n).
fn bench_all_accepted(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch/all_accepted");
    group.sample_size(10);
    for &size in &[1u32, 5, 10, 20] {
        group.bench_with_input(BenchmarkId::new("size", size), &size, |b, &size| {
            b.iter(|| {
                let env = Env::default();
                let (client, pair) = setup_plain(&env);
                let batch = build_plain_batch(&env, &pair, size, false, false);
                env.budget().reset_unlimited();
                env.budget().reset_tracker();
                black_box(client.submit_scores_batch(&batch));
                black_box((env.budget().cpu_instruction_cost(), env.budget().memory_bytes_cost()))
            });
        });
    }
    group.finish();
}

/// Group 2: plain batch — all entries rejected (score > 100).
/// Cost = validation only, no storage writes or events, O(n).
fn bench_all_rejected(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch/all_rejected_invalid_score");
    group.sample_size(10);
    for &size in &[1u32, 5, 10, 20] {
        group.bench_with_input(BenchmarkId::new("size", size), &size, |b, &size| {
            b.iter(|| {
                let env = Env::default();
                let (client, pair) = setup_plain(&env);
                let batch = build_plain_batch(&env, &pair, size, true, false);
                env.budget().reset_unlimited();
                env.budget().reset_tracker();
                black_box(client.submit_scores_batch(&batch));
                black_box((env.budget().cpu_instruction_cost(), env.budget().memory_bytes_cost()))
            });
        });
    }
    group.finish();
}

/// Group 3: plain batch — single wallet, all entries after the first are
/// rate-limited.  Cost = one storage write + (n-1) cooldown-check rejections.
fn bench_rate_limited(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch/rate_limited_same_wallet");
    group.sample_size(10);
    for &size in &[1u32, 5, 10, 20] {
        group.bench_with_input(BenchmarkId::new("size", size), &size, |b, &size| {
            b.iter(|| {
                let env = Env::default();
                let (client, pair) = setup_plain(&env);
                let batch = build_plain_batch(&env, &pair, size, false, true);
                env.budget().reset_unlimited();
                env.budget().reset_tracker();
                black_box(client.submit_scores_batch(&batch));
                black_box((env.budget().cpu_instruction_cost(), env.budget().memory_bytes_cost()))
            });
        });
    }
    group.finish();
}

/// Group 4: plain batch — alternating valid/invalid entries (half accepted).
/// Exercises the per-entry branch in the batch loop.
fn bench_mixed_half_accepted(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch/mixed_half_accepted");
    group.sample_size(10);
    for &size in &[2u32, 10, 20] {
        group.bench_with_input(BenchmarkId::new("size", size), &size, |b, &size| {
            b.iter(|| {
                let env = Env::default();
                let (client, pair) = setup_plain(&env);
                let batch = build_mixed_batch(&env, &pair, size);
                env.budget().reset_unlimited();
                env.budget().reset_tracker();
                black_box(client.submit_scores_batch(&batch));
                black_box((env.budget().cpu_instruction_cost(), env.budget().memory_bytes_cost()))
            });
        });
    }
    group.finish();
}

/// Group 5: attested batch — all entries accepted.
/// Cost = one secp256k1_recover + O(log n) Merkle proof per entry + storage writes + events.
/// Comparing this against bench_all_accepted isolates the attestation overhead.
fn bench_attested_all_accepted(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_attested/all_accepted");
    group.sample_size(10);
    for &size in &[1u32, 5, 10, 20] {
        group.bench_with_input(BenchmarkId::new("size", size), &size, |b, &size| {
            b.iter(|| {
                let env = Env::default();
                let (client, pair, key) = setup_attested(&env);
                let (subs, attest) = build_attested_batch(&env, &client, &pair, &key, size, false);
                env.budget().reset_unlimited();
                env.budget().reset_tracker();
                black_box(client.submit_scores_batch_attested(&Vec::new(&env), &subs, &attest));
                black_box((env.budget().cpu_instruction_cost(), env.budget().memory_bytes_cost()))
            });
        });
    }
    group.finish();
}

/// Group 6: attested batch — all entries rejected (score > 100).
/// Cost = one secp256k1_recover + O(log n) Merkle proofs, no storage writes or events.
fn bench_attested_all_rejected(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_attested/all_rejected_invalid_score");
    group.sample_size(10);
    for &size in &[1u32, 5, 10, 20] {
        group.bench_with_input(BenchmarkId::new("size", size), &size, |b, &size| {
            b.iter(|| {
                let env = Env::default();
                let (client, pair, key) = setup_attested(&env);
                let (subs, attest) = build_attested_batch(&env, &client, &pair, &key, size, true);
                env.budget().reset_unlimited();
                env.budget().reset_tracker();
                black_box(client.submit_scores_batch_attested(&Vec::new(&env), &subs, &attest));
                black_box((env.budget().cpu_instruction_cost(), env.budget().memory_bytes_cost()))
            });
        });
    }
    group.finish();
}

/// Group 7: attested batch — half accepted (alternating invalid score).
/// Cost = secp256k1_recover + per-entry Merkle proof + storage for accepted half.
fn bench_attested_mixed(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_attested/mixed_half_accepted");
    group.sample_size(10);
    for &size in &[2u32, 10, 20] {
        group.bench_with_input(BenchmarkId::new("size", size), &size, |b, &size| {
            b.iter(|| {
                let env = Env::default();
                let (client, pair, key) = setup_attested(&env);
                let (subs, attest) = build_attested_mixed(&env, &client, &pair, &key, size);
                env.budget().reset_unlimited();
                env.budget().reset_tracker();
                black_box(client.submit_scores_batch_attested(&Vec::new(&env), &subs, &attest));
                black_box((env.budget().cpu_instruction_cost(), env.budget().memory_bytes_cost()))
            });
        });
    }
    group.finish();
}

criterion_group!(
    plain_benches,
    bench_all_accepted,
    bench_all_rejected,
    bench_rate_limited,
    bench_mixed_half_accepted,
);

criterion_group!(
    attested_benches,
    bench_attested_all_accepted,
    bench_attested_all_rejected,
    bench_attested_mixed,
);

criterion_main!(plain_benches, attested_benches);
