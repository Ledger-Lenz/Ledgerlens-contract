#![cfg(test)]

use k256::ecdsa::SigningKey;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Bytes, BytesN, Env, Symbol, Vec,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient, ModelSubmission, ScoreAttestation};

const START_TS: u64 = 1_700_000_000;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);
    let cid = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &cid);
    client.initialize(&Address::generate(&env), &Address::generate(&env));
    (env, client)
}

fn signing_key(seed: u8) -> SigningKey {
    let mut b = [0u8; 32];
    b[0] = 1;
    b[31] = seed;
    SigningKey::from_bytes((&b).into()).unwrap()
}

fn pubkey_bytes(env: &Env, key: &SigningKey) -> Bytes {
    let pt = key.verifying_key().to_encoded_point(true);
    Bytes::from_slice(env, pt.as_bytes())
}

fn attest(env: &Env, key: &SigningKey, digest: [u8; 32]) -> ScoreAttestation {
    let (sig, recid) = key.sign_prehash_recoverable(&digest).unwrap();
    let mut b = [0u8; 65];
    b[..64].copy_from_slice(&sig.to_bytes());
    b[64] = recid.to_byte();
    ScoreAttestation {
        commitment: BytesN::from_array(env, &digest),
        signature: BytesN::from_array(env, &b),
    }
}

fn make_submission(
    env: &Env,
    client: &LedgerLensScoreContractClient<'_>,
    key: &SigningKey,
    model: &Address,
    wallet: &Address,
    pair: &Symbol,
    score: u32,
    ts: u64,
    model_version: u32,
) -> ModelSubmission {
    let digest = env.as_contract(&client.address, || {
        LedgerLensScoreContract::compute_commitment(
            env, wallet, pair, score, false, false, ts, 80, model_version,
        )
        .unwrap()
        .to_bytes()
        .to_array()
    });
    ModelSubmission {
        model_version,
        model: model.clone(),
        score,
        confidence: 80,
        benford_flag: false,
        ml_flag: false,
        attestation: attest(env, key, digest),
    }
}

/// Commit all submissions then reveal; returns normally or panics.
fn do_consensus(
    env: &Env,
    client: &LedgerLensScoreContractClient<'_>,
    wallet: &Address,
    pair: &Symbol,
    submissions: &Vec<ModelSubmission>,
    ts: u64,
) {
    let mut nonces = Vec::new(env);
    for i in 0..submissions.len() {
        let sub = submissions.get(i).unwrap();
        let nonce = (i as u64) + 100;
        nonces.push_back(nonce);
        let mut buf = [0u8; 12];
        buf[0..4].copy_from_slice(&sub.score.to_be_bytes());
        buf[4..12].copy_from_slice(&nonce.to_be_bytes());
        let hash = env.crypto().sha256(&soroban_sdk::Bytes::from_array(env, &buf));
        client.commit_consensus(&sub.model, wallet, pair, &hash.to_bytes());
    }
    client.reveal_consensus(&Vec::new(env), wallet, pair, submissions, &nonces, &ts);
}

/// Fresh signers all get equal weight → weighted mean equals plain mean.
#[test]
fn test_fresh_signers_equal_weight() {
    let (env, client) = setup();
    let key = signing_key(1);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key));

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let m1 = Address::generate(&env);
    let m2 = Address::generate(&env);
    let m3 = Address::generate(&env);

    let mut subs = Vec::new(&env);
    subs.push_back(make_submission(&env, &client, &key, &m1, &wallet, &pair, 60, START_TS, 1));
    subs.push_back(make_submission(&env, &client, &key, &m2, &wallet, &pair, 60, START_TS, 2));
    subs.push_back(make_submission(&env, &client, &key, &m3, &wallet, &pair, 60, START_TS, 3));

    do_consensus(&env, &client, &wallet, &pair, &subs, START_TS);

    assert_eq!(client.get_score(&wallet, &pair).score, 60);
    assert!(client.get_signer_accuracy(&m1).is_some());
    assert!(client.get_signer_accuracy(&m2).is_some());
    assert!(client.get_signer_accuracy(&m3).is_some());
}

/// After multiple rounds a consistently accurate signer has lower MAD than a noisy one.
#[test]
fn test_accuracy_converges_after_deviations() {
    let (env, client) = setup();
    let key = signing_key(2);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key));
    client.set_consensus_config(&2, &20); // wide epsilon

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let accurate = Address::generate(&env);
    let noisy = Address::generate(&env);

    for round in 0..2u64 {
        let ts = START_TS + round * 4000;
        env.ledger().with_mut(|l| l.timestamp = ts);

        let s_acc = make_submission(&env, &client, &key, &accurate, &wallet, &pair, 50, ts, 1);
        let s_noisy = make_submission(&env, &client, &key, &noisy, &wallet, &pair, 70, ts, 2);

        let mut subs = Vec::new(&env);
        subs.push_back(s_acc);
        subs.push_back(s_noisy);

        // use round-specific nonces to avoid replay
        let mut nonces = Vec::new(&env);
        for i in 0..subs.len() {
            let sub = subs.get(i).unwrap();
            let nonce = round * 10 + i as u64 + 1;
            nonces.push_back(nonce);
            let mut buf = [0u8; 12];
            buf[0..4].copy_from_slice(&sub.score.to_be_bytes());
            buf[4..12].copy_from_slice(&nonce.to_be_bytes());
            let hash = env.crypto().sha256(&soroban_sdk::Bytes::from_array(&env, &buf));
            client.commit_consensus(&sub.model, &wallet, &pair, &hash.to_bytes());
        }
        client.reveal_consensus(&Vec::new(&env), &wallet, &pair, &subs, &nonces, &ts);
    }

    let acc_rec = client.get_signer_accuracy(&accurate).unwrap();
    let noisy_rec = client.get_signer_accuracy(&noisy).unwrap();

    assert!(
        acc_rec.mad_scaled < noisy_rec.mad_scaled,
        "accurate MAD={} should be < noisy MAD={}",
        acc_rec.mad_scaled,
        noisy_rec.mad_scaled
    );
    assert_eq!(acc_rec.count, 2);
    assert_eq!(noisy_rec.count, 2);
}

/// reset_signer_accuracy clears the record.
#[test]
fn test_reset_signer_accuracy() {
    let (env, client) = setup();
    let key = signing_key(3);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key));

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let m1 = Address::generate(&env);
    let m2 = Address::generate(&env);

    let mut subs = Vec::new(&env);
    subs.push_back(make_submission(&env, &client, &key, &m1, &wallet, &pair, 55, START_TS, 1));
    subs.push_back(make_submission(&env, &client, &key, &m2, &wallet, &pair, 57, START_TS, 2));
    do_consensus(&env, &client, &wallet, &pair, &subs, START_TS);

    assert!(client.get_signer_accuracy(&m1).is_some());
    client.reset_signer_accuracy(&Vec::new(&env), &m1);
    assert!(client.get_signer_accuracy(&m1).is_none());
    // m2 not affected
    assert!(client.get_signer_accuracy(&m2).is_some());
}

/// get_signer_accuracy returns None for unknown signer.
#[test]
fn test_get_signer_accuracy_none_for_unknown() {
    let (_env, client) = setup();
    assert!(client.get_signer_accuracy(&Address::generate(&_env)).is_none());
}
