#![cfg(test)]

//! Tests for secp256k1 threshold signature aggregation:
//! `set_aggregate_service_pubkey` / `get_aggregate_service_pubkey` and the
//! `threshold_attestation` parameter on `submit_score`.
//!
//! # Design
//!
//! The threshold protocol (FROST / GG18) runs entirely off-chain.  On-chain,
//! the contract only sees a single 65-byte `(r, s, v)` ECDSA signature
//! over the same payload commitment as `ScoreAttestation`.  For test
//! purposes a plain k256 single-key signature is indistinguishable from a
//! "real" threshold signature produced by t-of-n signers — the secp256k1
//! math is identical — so we drive all tests with `k256::ecdsa::SigningKey`.
//!
//! See `docs/threshold-attestation-spec.md` for the off-chain protocol
//! details and the rationale for how the aggregate public key is derived.

use k256::ecdsa::SigningKey;
use soroban_sdk::{
    symbol_short, testutils::Address as _, Address, Bytes, BytesN, Env, Symbol, Vec,
};

use crate::{
    Error, LedgerLensScoreContract, LedgerLensScoreContractClient, ScoreAttestation,
    ThresholdAttestation,
};

// ── Test infrastructure ──────────────────────────────────────────────────────

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    (env, client, admin, service)
}

fn initialized<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);
    (env, client, admin, service)
}

/// Deterministic signing key.  `seed` drives the last byte of a 32-byte
/// scalar; `bytes[0] = 1` ensures the scalar is never zero.
fn signing_key(seed: u8) -> SigningKey {
    let mut bytes = [0u8; 32];
    bytes[31] = seed;
    bytes[0] = 1;
    SigningKey::from_bytes((&bytes).into()).unwrap()
}

fn pubkey_bytes(env: &Env, key: &SigningKey, compressed: bool) -> Bytes {
    let point = key.verifying_key().to_encoded_point(compressed);
    Bytes::from_slice(env, point.as_bytes())
}

/// Invoke the contract's private `compute_commitment` as the deployed
/// contract so `env.current_contract_address()` resolves correctly.
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

/// Produce a `ThresholdAttestation` signed by `key` over the commitment for
/// the given score payload.
#[allow(clippy::too_many_arguments)]
fn threshold_attest(
    env: &Env,
    contract_id: &Address,
    key: &SigningKey,
    wallet: &Address,
    pair: &Symbol,
    score: u32,
    benford_flag: bool,
    ml_flag: bool,
    timestamp: u64,
    confidence: u32,
    model_version: u32,
    participating_signers: Vec<Address>,
) -> ThresholdAttestation {
    let digest = commitment(
        env,
        contract_id,
        wallet,
        pair,
        score,
        benford_flag,
        ml_flag,
        timestamp,
        confidence,
        model_version,
    );
    let (sig, recid) = key.sign_prehash_recoverable(&digest).unwrap();
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&sig.to_bytes());
    sig_bytes[64] = recid.to_byte();
    ThresholdAttestation {
        commitment: BytesN::from_array(env, &digest),
        threshold_sig: BytesN::from_array(env, &sig_bytes),
        participating_signers,
    }
}

// ── set_aggregate_service_pubkey / get_aggregate_service_pubkey ──────────────

#[test]
fn test_get_aggregate_pubkey_before_set_fails() {
    let (_env, client, _admin, _service) = initialized();
    let result = client.try_get_aggregate_service_pubkey();
    assert_eq!(result, Err(Ok(Error::AggregatePubkeyNotSet)));
}

#[test]
fn test_set_aggregate_pubkey_compressed() {
    let (env, client, admin, _service) = initialized();
    let key = signing_key(1);
    let pubkey = pubkey_bytes(&env, &key, true); // 33 bytes
    client.set_aggregate_service_pubkey(&Vec::new(&env), &pubkey);
    let stored = client.get_aggregate_service_pubkey().unwrap();
    assert_eq!(stored, pubkey);
    let _ = admin; // suppress unused warning
}

#[test]
fn test_set_aggregate_pubkey_uncompressed() {
    let (env, client, _admin, _service) = initialized();
    let key = signing_key(2);
    let pubkey = pubkey_bytes(&env, &key, false); // 65 bytes
    client.set_aggregate_service_pubkey(&Vec::new(&env), &pubkey);
    let stored = client.get_aggregate_service_pubkey().unwrap();
    assert_eq!(stored, pubkey);
}

#[test]
fn test_set_aggregate_pubkey_invalid_length_rejected() {
    let (env, client, _admin, _service) = initialized();
    let bad_pubkey = Bytes::from_array(&env, &[0u8; 32]); // neither 33 nor 65
    let result = client.try_set_aggregate_service_pubkey(&Vec::new(&env), &bad_pubkey);
    assert_eq!(result, Err(Ok(Error::InvalidPubkeyLength)));
}

#[test]
fn test_set_aggregate_pubkey_rotates() {
    let (env, client, _admin, _service) = initialized();
    let key1 = signing_key(1);
    let key2 = signing_key(2);
    let pk1 = pubkey_bytes(&env, &key1, true);
    let pk2 = pubkey_bytes(&env, &key2, true);
    client.set_aggregate_service_pubkey(&Vec::new(&env), &pk1);
    assert_eq!(client.get_aggregate_service_pubkey().unwrap(), pk1);
    client.set_aggregate_service_pubkey(&Vec::new(&env), &pk2);
    assert_eq!(client.get_aggregate_service_pubkey().unwrap(), pk2);
}

// ── submit_score with threshold_attestation ──────────────────────────────────

#[test]
fn test_threshold_sig_accepted_no_service_set() {
    // When no M-of-N service set is configured, the threshold path requires
    // only a valid signature — participating_signers may be empty.
    let (env, client, _admin, _service) = initialized();
    let key = signing_key(10);
    let pubkey = pubkey_bytes(&env, &key, true);
    client.set_aggregate_service_pubkey(&Vec::new(&env), &pubkey);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let ta = threshold_attest(
        &env,
        &client.address,
        &key,
        &wallet,
        &pair,
        42,
        false,
        false,
        1000,
        90,
        1,
        Vec::new(&env),
    );
    client
        .submit_score(
            &Vec::new(&env),
            &wallet,
            &pair,
            &42,
            &false,
            &false,
            &1000,
            &90,
            &1,
            &None,
            &Some(ta),
        )
        .unwrap();
    let stored = client.get_score(&wallet, &pair).unwrap();
    assert_eq!(stored.score, 42);
}

#[test]
fn test_threshold_sig_accepted_with_service_set() {
    // When an M-of-N set is configured, participating_signers must list
    // members of the set; no require_auth call is needed.
    let (env, client, _admin, _service) = initialized();

    let signer1 = Address::generate(&env);
    let signer2 = Address::generate(&env);
    client.add_service_signer(&Vec::new(&env), &signer1);
    client.add_service_signer(&Vec::new(&env), &signer2);
    client.set_service_threshold(&Vec::new(&env), &2);

    let key = signing_key(11);
    let pubkey = pubkey_bytes(&env, &key, true);
    client.set_aggregate_service_pubkey(&Vec::new(&env), &pubkey);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let mut participants: Vec<Address> = Vec::new(&env);
    participants.push_back(signer1.clone());
    participants.push_back(signer2.clone());

    let ta = threshold_attest(
        &env,
        &client.address,
        &key,
        &wallet,
        &pair,
        55,
        true,
        false,
        2000,
        80,
        2,
        participants,
    );
    client
        .submit_score(
            &Vec::new(&env),
            &wallet,
            &pair,
            &55,
            &true,
            &false,
            &2000,
            &80,
            &2,
            &None,
            &Some(ta),
        )
        .unwrap();
    let stored = client.get_score(&wallet, &pair).unwrap();
    assert_eq!(stored.score, 55);
    assert!(stored.benford_flag);
}

#[test]
fn test_threshold_sig_wrong_key_rejected() {
    let (env, client, _admin, _service) = initialized();

    let registered_key = signing_key(20);
    let wrong_key = signing_key(21); // different key, not registered

    let pubkey = pubkey_bytes(&env, &registered_key, true);
    client.set_aggregate_service_pubkey(&Vec::new(&env), &pubkey);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Sign with the wrong key.
    let ta = threshold_attest(
        &env,
        &client.address,
        &wrong_key,
        &wallet,
        &pair,
        30,
        false,
        false,
        1000,
        70,
        1,
        Vec::new(&env),
    );
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &30,
        &false,
        &false,
        &1000,
        &70,
        &1,
        &None,
        &Some(ta),
    );
    assert_eq!(result, Err(Ok(Error::InvalidThresholdSignature)));
}

#[test]
fn test_threshold_sig_tampered_commitment_rejected() {
    let (env, client, _admin, _service) = initialized();

    let key = signing_key(30);
    let pubkey = pubkey_bytes(&env, &key, true);
    client.set_aggregate_service_pubkey(&Vec::new(&env), &pubkey);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let mut ta = threshold_attest(
        &env,
        &client.address,
        &key,
        &wallet,
        &pair,
        50,
        false,
        false,
        1000,
        80,
        1,
        Vec::new(&env),
    );

    // Flip one byte of the commitment.
    let mut bad_commitment = ta.commitment.to_array();
    bad_commitment[0] ^= 0xFF;
    ta.commitment = BytesN::from_array(&env, &bad_commitment);

    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &1000,
        &80,
        &1,
        &None,
        &Some(ta),
    );
    assert_eq!(result, Err(Ok(Error::InvalidThresholdSignature)));
}

#[test]
fn test_threshold_sig_payload_mismatch_rejected() {
    // Attestation covers score=50 but call submits score=99.
    let (env, client, _admin, _service) = initialized();

    let key = signing_key(40);
    let pubkey = pubkey_bytes(&env, &key, true);
    client.set_aggregate_service_pubkey(&Vec::new(&env), &pubkey);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let ta = threshold_attest(
        &env,
        &client.address,
        &key,
        &wallet,
        &pair,
        50,
        false,
        false,
        1000,
        80,
        1,
        Vec::new(&env),
    );

    // Submit with a different score than what was attested.
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &99, // does not match ta.commitment
        &false,
        &false,
        &1000,
        &80,
        &1,
        &None,
        &Some(ta),
    );
    assert_eq!(result, Err(Ok(Error::InvalidThresholdSignature)));
}

#[test]
fn test_threshold_sig_without_aggregate_pubkey_rejected() {
    // No aggregate pubkey registered — threshold path must fail immediately.
    let (env, client, _admin, _service) = initialized();

    let key = signing_key(50);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let ta = threshold_attest(
        &env,
        &client.address,
        &key,
        &wallet,
        &pair,
        30,
        false,
        false,
        1000,
        70,
        1,
        Vec::new(&env),
    );
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &30,
        &false,
        &false,
        &1000,
        &70,
        &1,
        &None,
        &Some(ta),
    );
    assert_eq!(result, Err(Ok(Error::AggregatePubkeyNotSet)));
}

#[test]
fn test_threshold_sig_signer_not_in_set_rejected() {
    let (env, client, _admin, _service) = initialized();

    let signer_in_set = Address::generate(&env);
    client.add_service_signer(&Vec::new(&env), &signer_in_set);
    client.set_service_threshold(&Vec::new(&env), &1);

    let key = signing_key(60);
    let pubkey = pubkey_bytes(&env, &key, true);
    client.set_aggregate_service_pubkey(&Vec::new(&env), &pubkey);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let outsider = Address::generate(&env); // NOT in the service set

    let mut participants: Vec<Address> = Vec::new(&env);
    participants.push_back(outsider);

    let ta = threshold_attest(
        &env,
        &client.address,
        &key,
        &wallet,
        &pair,
        40,
        false,
        false,
        1000,
        70,
        1,
        participants,
    );
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &40,
        &false,
        &false,
        &1000,
        &70,
        &1,
        &None,
        &Some(ta),
    );
    assert_eq!(result, Err(Ok(Error::ThresholdSignerNotInSet)));
}

#[test]
fn test_threshold_sig_below_threshold_count_rejected() {
    let (env, client, _admin, _service) = initialized();

    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    client.add_service_signer(&Vec::new(&env), &s1);
    client.add_service_signer(&Vec::new(&env), &s2);
    client.set_service_threshold(&Vec::new(&env), &2); // need 2

    let key = signing_key(70);
    let pubkey = pubkey_bytes(&env, &key, true);
    client.set_aggregate_service_pubkey(&Vec::new(&env), &pubkey);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let mut participants: Vec<Address> = Vec::new(&env);
    participants.push_back(s1.clone()); // only 1 — below threshold

    let ta = threshold_attest(
        &env,
        &client.address,
        &key,
        &wallet,
        &pair,
        35,
        false,
        false,
        1000,
        70,
        1,
        participants,
    );
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &35,
        &false,
        &false,
        &1000,
        &70,
        &1,
        &None,
        &Some(ta),
    );
    assert_eq!(result, Err(Ok(Error::InsufficientThresholdSigners)));
}

#[test]
fn test_threshold_sig_uncompressed_pubkey_accepted() {
    let (env, client, _admin, _service) = initialized();

    let key = signing_key(80);
    let pubkey = pubkey_bytes(&env, &key, false); // 65-byte uncompressed
    client.set_aggregate_service_pubkey(&Vec::new(&env), &pubkey);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let ta = threshold_attest(
        &env,
        &client.address,
        &key,
        &wallet,
        &pair,
        77,
        false,
        true,
        9000,
        95,
        3,
        Vec::new(&env),
    );
    client
        .submit_score(
            &Vec::new(&env),
            &wallet,
            &pair,
            &77,
            &false,
            &true,
            &9000,
            &95,
            &3,
            &None,
            &Some(ta),
        )
        .unwrap();
    let stored = client.get_score(&wallet, &pair).unwrap();
    assert_eq!(stored.score, 77);
    assert!(stored.ml_flag);
}

#[test]
fn test_legacy_path_still_works_when_no_threshold_attestation() {
    // Passing `threshold_attestation: None` must fall through to the legacy
    // require_auth path — existing callers are unaffected.
    let (env, client, _admin, service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    client
        .submit_score(
            &Vec::new(&env),
            &wallet,
            &pair,
            &10,
            &false,
            &false,
            &1,
            &50,
            &1,
            &None
        )
        .unwrap();
    let stored = client.get_score(&wallet, &pair).unwrap();
    assert_eq!(stored.score, 10);
    let _ = service;
}

#[test]
fn test_threshold_and_ordinary_attestation_both_present_uses_threshold() {
    // When threshold_attestation is Some, the threshold path is taken and
    // the ordinary `attestation` field is ignored.
    let (env, client, _admin, _service) = initialized();

    let threshold_key = signing_key(90);
    let service_key = signing_key(91);

    let agg_pubkey = pubkey_bytes(&env, &threshold_key, true);
    client.set_aggregate_service_pubkey(&Vec::new(&env), &agg_pubkey);

    let svc_pubkey = pubkey_bytes(&env, &service_key, true);
    client.set_service_pubkey(&Vec::new(&env), &svc_pubkey);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let digest = commitment(&env, &client.address, &wallet, &pair, 60, false, false, 5000, 80, 1);

    // Build a valid ordinary attestation (service_key) to satisfy the service
    // pubkey check IF the ordinary path were taken.
    let (sig, recid) = service_key.sign_prehash_recoverable(&digest).unwrap();
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&sig.to_bytes());
    sig_bytes[64] = recid.to_byte();
    let ordinary_att = Some(ScoreAttestation {
        commitment: BytesN::from_array(&env, &digest),
        signature: BytesN::from_array(&env, &sig_bytes),
    });

    // Build a valid threshold attestation.
    let ta = threshold_attest(
        &env,
        &client.address,
        &threshold_key,
        &wallet,
        &pair,
        60,
        false,
        false,
        5000,
        80,
        1,
        Vec::new(&env),
    );

    // Should succeed via the threshold path.
    client
        .submit_score(
            &Vec::new(&env),
            &wallet,
            &pair,
            &60,
            &false,
            &false,
            &5000,
            &80,
            &1,
            &ordinary_att,
            &Some(ta),
        )
        .unwrap();

    let stored = client.get_score(&wallet, &pair).unwrap();
    assert_eq!(stored.score, 60);
}
