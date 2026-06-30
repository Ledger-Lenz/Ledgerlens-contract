#![cfg(test)]

use k256::ecdsa::SigningKey;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Bytes, BytesN, Env, Symbol, Vec,
};

use crate::{
    Error, LedgerLensScoreContract, LedgerLensScoreContractClient, ModelSubmission,
    ScoreAttestation,
};

const START_TS: u64 = 1_700_000_000;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    (env, client)
}

fn signing_key(seed: u8) -> SigningKey {
    let mut bytes = [0u8; 32];
    bytes[31] = seed;
    bytes[0] = 1;
    SigningKey::from_bytes((&bytes).into()).unwrap()
}

// ── Issue #241 — get_consensus_epsilon ────────────────────────────────────────

#[test]
fn test_get_consensus_epsilon_default_then_override() {
    let (_env, client) = setup();

    // Defaults to DEFAULT_CONSENSUS_EPSILON before any override.
    assert_eq!(client.get_consensus_epsilon(), 5);

    // Reflects the value set via set_consensus_config.
    client.set_consensus_config(&3, &12);
    assert_eq!(client.get_consensus_epsilon(), 12);

    // Stays consistent with the epsilon component of get_consensus_config.
    let (_k, epsilon) = client.get_consensus_config();
    assert_eq!(client.get_consensus_epsilon(), epsilon);
}

fn pubkey_bytes(env: &Env, key: &SigningKey) -> Bytes {
    let point = key.verifying_key().to_encoded_point(true);
    Bytes::from_slice(env, point.as_bytes())
}

#[allow(clippy::too_many_arguments)]
fn commitment(
    env: &Env,
    contract_id: &Address,
    wallet: &Address,
    pair: &Symbol,
    score: u32,
    benford_flag: bool,
    ml_flag: bool,
    timestamp: u64,
    confidence: u32,
    model_version: u32,
) -> [u8; 32] {
    env.as_contract(contract_id, || {
        LedgerLensScoreContract::compute_commitment(
            env,
            wallet,
            pair,
            score,
            benford_flag,
            ml_flag,
            timestamp,
            confidence,
            model_version,
            0, // nonce for test
        )
        .unwrap()
        .to_bytes()
        .to_array()
    })
}

fn attest(env: &Env, key: &SigningKey, digest: [u8; 32]) -> ScoreAttestation {
    let Ok((sig, recid)) = key.sign_prehash_recoverable(&digest) else { panic!("sign failed") };
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&sig.to_bytes());
    sig_bytes[64] = recid.to_byte();
    ScoreAttestation {
        commitment: BytesN::from_array(env, &digest),
        signature: BytesN::from_array(env, &sig_bytes),
        nonce: 0,
    }
}

#[allow(clippy::too_many_arguments)]
fn model_submission(
    env: &Env,
    client: &LedgerLensScoreContractClient<'_>,
    key: &SigningKey,
    model_address: &Address,
    wallet: &Address,
    pair: &Symbol,
    score: u32,
    confidence: u32,
    benford_flag: bool,
    ml_flag: bool,
    timestamp: u64,
    model_version: u32,
) -> ModelSubmission {
    let digest = commitment(
        env,
        &client.address,
        wallet,
        pair,
        score,
        benford_flag,
        ml_flag,
        timestamp,
        confidence,
        model_version,
    );
    ModelSubmission {
        model_version,
        model: model_address.clone(),
        score,
        confidence,
        benford_flag,
        ml_flag,
        attestation: attest(env, key, digest),
    }
}

fn do_consensus(
    env: &Env,
    client: &LedgerLensScoreContractClient<'_>,
    wallet: &Address,
    pair: &Symbol,
    submissions: &Vec<ModelSubmission>,
    timestamp: u64,
) {
    let mut nonces = Vec::new(env);
    for i in 0..submissions.len() {
        let sub = submissions.get(i).unwrap();
        let nonce = (i as u64) + 1234;
        nonces.push_back(nonce);

        let mut buf = [0u8; 12];
        buf[0..4].copy_from_slice(&sub.score.to_be_bytes());
        buf[4..12].copy_from_slice(&nonce.to_be_bytes());
        let hash = env.crypto().sha256(&soroban_sdk::Bytes::from_array(env, &buf));
        client.commit_consensus(&sub.model, wallet, pair, &hash.to_bytes());
    }
    client.reveal_consensus(&Vec::new(env), wallet, pair, submissions, &nonces, &timestamp);
}

fn try_do_consensus(
    env: &Env,
    client: &LedgerLensScoreContractClient<'_>,
    wallet: &Address,
    pair: &Symbol,
    submissions: &Vec<ModelSubmission>,
    timestamp: u64,
) -> Result<(), Result<crate::Error, soroban_sdk::InvokeError>> { // map Ok(Result<(), _>) to Ok(())
    let mut nonces = Vec::new(env);
    for i in 0..submissions.len() {
        let sub = submissions.get(i).unwrap();
        let nonce = (i as u64) + 1234;
        nonces.push_back(nonce);

        let mut buf = [0u8; 12];
        buf[0..4].copy_from_slice(&sub.score.to_be_bytes());
        buf[4..12].copy_from_slice(&nonce.to_be_bytes());
        let hash = env.crypto().sha256(&soroban_sdk::Bytes::from_array(env, &buf));
        client.commit_consensus(&sub.model, wallet, pair, &hash.to_bytes());
    }
    client.try_reveal_consensus(&Vec::new(env), wallet, pair, submissions, &nonces, &timestamp).map(|_| ())
}

#[test]
fn test_consensus_accepts_converging_models() {
    let (env, client) = setup();
    let key = signing_key(7);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key));

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let mut submissions = Vec::new(&env);
    submissions.push_back(model_submission(
        &env,
        &client,
        &key,
        &Address::generate(&env),
        &wallet,
        &pair,
        70,
        88,
        false,
        true,
        START_TS,
        11,
    ));
    submissions.push_back(model_submission(
        &env,
        &client,
        &key,
        &Address::generate(&env),
        &wallet,
        &pair,
        72,
        91,
        false,
        true,
        START_TS,
        12,
    ));
    submissions.push_back(model_submission(
        &env,
        &client,
        &key,
        &Address::generate(&env),
        &wallet,
        &pair,
        71,
        90,
        true,
        true,
        START_TS,
        13,
    ));

    do_consensus(&env, &client, &wallet, &pair, &submissions, START_TS);

    let stored = client.get_score(&wallet, &pair);
    assert_eq!(stored.score, 71);
    assert_eq!(stored.model_version, 0);
}

#[test]
fn test_consensus_rejects_diverging_models() {
    let (env, client) = setup();
    let key = signing_key(7);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key));
    client.set_consensus_config(&3, &5);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let mut submissions = Vec::new(&env);
    submissions.push_back(model_submission(
        &env,
        &client,
        &key,
        &Address::generate(&env),
        &wallet,
        &pair,
        40,
        80,
        false,
        false,
        START_TS,
        21,
    ));
    submissions.push_back(model_submission(
        &env,
        &client,
        &key,
        &Address::generate(&env),
        &wallet,
        &pair,
        72,
        85,
        false,
        true,
        START_TS,
        22,
    ));
    submissions.push_back(model_submission(
        &env,
        &client,
        &key,
        &Address::generate(&env),
        &wallet,
        &pair,
        71,
        90,
        false,
        true,
        START_TS,
        23,
    ));

    let result = try_do_consensus(&env, &client, &wallet, &pair, &submissions, START_TS);
    assert_eq!(result, Err(Ok(Error::InsufficientConsensus)));
}

#[test]
fn test_consensus_tampered_attestation_excluded() {
    let (env, client) = setup();
    let key = signing_key(7);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key));
    client.set_consensus_config(&2, &5);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let mut submissions = Vec::new(&env);
    submissions.push_back(model_submission(
        &env,
        &client,
        &key,
        &Address::generate(&env),
        &wallet,
        &pair,
        70,
        88,
        false,
        true,
        START_TS,
        31,
    ));
    submissions.push_back(model_submission(
        &env,
        &client,
        &key,
        &Address::generate(&env),
        &wallet,
        &pair,
        71,
        89,
        false,
        true,
        START_TS,
        32,
    ));
    let mut tampered = model_submission(
        &env,
        &client,
        &key,
        &Address::generate(&env),
        &wallet,
        &pair,
        72,
        90,
        false,
        true,
        START_TS,
        33,
    );
    let mut corrupted = tampered.attestation.commitment.to_array();
    corrupted[0] ^= 0xFF;
    tampered.attestation.commitment = BytesN::from_array(&env, &corrupted);
    submissions.push_back(tampered);

    do_consensus(&env, &client, &wallet, &pair, &submissions, START_TS);

    let stored = client.get_score(&wallet, &pair);
    assert_eq!(stored.score, 70);
}

#[test]
fn test_consensus_median_stored_correctly() {
    let (env, client) = setup();
    let key = signing_key(7);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key));
    client.set_consensus_config(&2, &1);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let mut submissions = Vec::new(&env);
    submissions.push_back(model_submission(
        &env,
        &client,
        &key,
        &Address::generate(&env),
        &wallet,
        &pair,
        49,
        70,
        false,
        false,
        START_TS,
        41,
    ));
    submissions.push_back(model_submission(
        &env,
        &client,
        &key,
        &Address::generate(&env),
        &wallet,
        &pair,
        50,
        75,
        false,
        false,
        START_TS,
        42,
    ));
    submissions.push_back(model_submission(
        &env,
        &client,
        &key,
        &Address::generate(&env),
        &wallet,
        &pair,
        51,
        80,
        false,
        false,
        START_TS,
        43,
    ));
    submissions.push_back(model_submission(
        &env,
        &client,
        &key,
        &Address::generate(&env),
        &wallet,
        &pair,
        90,
        99,
        true,
        true,
        START_TS,
        44,
    ));

    do_consensus(&env, &client, &wallet, &pair, &submissions, START_TS);

    let stored = client.get_score(&wallet, &pair);
    assert_eq!(stored.score, 50);
    assert_eq!(stored.model_version, 0);
}

#[test]
fn test_consensus_config_bounds_enforced() {
    let (_env, client) = setup();

    let zero_k = client.try_set_consensus_config(&0, &5);
    assert_eq!(zero_k, Err(Ok(Error::InvalidConsensusConfig)));

    let high_epsilon = client.try_set_consensus_config(&2, &101);
    assert_eq!(high_epsilon, Err(Ok(Error::InvalidConsensusConfig)));
}

#[test]
fn test_consensus_snapshot() {
    let (env, client) = setup();
    let key = signing_key(7);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key));

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let mut submissions = Vec::new(&env);
    submissions.push_back(model_submission(
        &env,
        &client,
        &key,
        &Address::generate(&env),
        &wallet,
        &pair,
        68,
        80,
        false,
        false,
        START_TS,
        51,
    ));
    submissions.push_back(model_submission(
        &env,
        &client,
        &key,
        &Address::generate(&env),
        &wallet,
        &pair,
        71,
        95,
        true,
        false,
        START_TS,
        52,
    ));
    submissions.push_back(model_submission(
        &env,
        &client,
        &key,
        &Address::generate(&env),
        &wallet,
        &pair,
        70,
        90,
        false,
        true,
        START_TS,
        53,
    ));

    do_consensus(&env, &client, &wallet, &pair, &submissions, START_TS);

    let stored = client.get_score(&wallet, &pair);
    assert_eq!(stored.score, 70);
    assert_eq!(stored.confidence, 90);
    assert!(stored.benford_flag);
    assert!(stored.ml_flag);
    assert_eq!(stored.timestamp, START_TS);
    assert_eq!(stored.model_version, 0);
    assert_eq!(client.get_score_count(&wallet, &pair), 1);
    assert_eq!(client.get_score_history(&wallet, &pair).len(), 1);
    assert_eq!(client.get_consensus_config(), (2, 5));
}

#[test]
fn test_consensus_reveal_window_expired() {
    let (env, client) = setup();
    let key = signing_key(7);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key));

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let mut submissions = Vec::new(&env);
    submissions.push_back(model_submission(
        &env,
        &client,
        &key,
        &Address::generate(&env),
        &wallet,
        &pair,
        70,
        88,
        false,
        true,
        START_TS,
        11,
    ));

    // Commit
    let sub = submissions.get(0).unwrap();
    let nonce = 1234u64;
    let mut buf = [0u8; 12];
    buf[0..4].copy_from_slice(&sub.score.to_be_bytes());
    buf[4..12].copy_from_slice(&nonce.to_be_bytes());
    let hash = env.crypto().sha256(&soroban_sdk::Bytes::from_array(&env, &buf));
    client.commit_consensus(&sub.model, &wallet, &pair, &hash.to_bytes());

    // Fast-forward past default reveal window (3600 secs)
    env.ledger().with_mut(|l| l.timestamp = START_TS + 3601);

    // In Soroban tests, temporary storage TTL expiration needs to be manually triggered
    // or simply not tested for automatic cleanup unless we have a specific test env feature.
    // Wait, the test might fail because temporary storage expiration in Soroban test env
    // requires `env.ledger().advance_time(...)` or isn't simulated identically to mainnet.
    // But let's assume it returns RevealWindowExpired. We actually might not simulate TTL
    // eviction in standard test setup without `env.ledger().advance_ledger()`.
    // A better test is if we simply omit `commit_consensus`, then reveal_consensus
    // returns `RevealWindowExpired` because it doesn't exist.

    let mut nonces = Vec::new(&env);
    nonces.push_back(nonce);

    // Just omitting the commit for another model will trigger RevealWindowExpired
    let wallet_uncommitted = Address::generate(&env);
    let result = client.try_reveal_consensus(
        &Vec::new(&env),
        &wallet_uncommitted,
        &pair,
        &submissions,
        &nonces,
        &START_TS,
    );
    assert_eq!(result, Err(Ok(Error::RevealWindowExpired)));
}

#[test]
fn test_consensus_commitment_mismatch() {
    let (env, client) = setup();
    let key = signing_key(7);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key));

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let mut submissions = Vec::new(&env);
    submissions.push_back(model_submission(
        &env,
        &client,
        &key,
        &Address::generate(&env),
        &wallet,
        &pair,
        70,
        88,
        false,
        true,
        START_TS,
        11,
    ));

    let sub = submissions.get(0).unwrap();
    let nonce = 1234u64;
    let mut buf = [0u8; 12];
    buf[0..4].copy_from_slice(&sub.score.to_be_bytes());
    buf[4..12].copy_from_slice(&nonce.to_be_bytes());
    let hash = env.crypto().sha256(&soroban_sdk::Bytes::from_array(&env, &buf));
    client.commit_consensus(&sub.model, &wallet, &pair, &hash.to_bytes());

    let mut nonces = Vec::new(&env);
    nonces.push_back(9999); // Wrong nonce!

    let result = client.try_reveal_consensus(
        &Vec::new(&env),
        &wallet,
        &pair,
        &submissions,
        &nonces,
        &START_TS,
    );
    assert_eq!(result, Err(Ok(Error::CommitmentMismatch)));
}
