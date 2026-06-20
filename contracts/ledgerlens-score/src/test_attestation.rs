//! Tests for the score-attestation feature: `set_service_pubkey` /
//! `get_service_pubkey`, and the `attestation` parameter on `submit_score`.
//!
//! Signatures are produced with a real secp256k1 key (via the `k256` crate,
//! a test-only dependency) so these tests exercise `verify_attestation`
//! end-to-end rather than mocking the crypto. The commitment digest itself
//! is computed by calling the contract's own (private, but visible to this
//! sibling module) `compute_commitment` from inside `env.as_contract` —
//! this keeps the tests honest about the *behaviour* of the byte layout
//! (see `docs/attestation-spec.md`) without duplicating it.

use k256::ecdsa::SigningKey;
use soroban_sdk::{
    symbol_short, testutils::Address as _, Address, Bytes, BytesN, Env, Symbol, Vec,
};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient, ScoreAttestation};

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

/// A fixed, arbitrary secp256k1 signing key standing in for the off-chain
/// detection pipeline's key. Deterministic so test failures are reproducible.
fn signing_key(seed: u8) -> SigningKey {
    let mut bytes = [0u8; 32];
    bytes[31] = seed;
    bytes[0] = 1; // avoid an all-zero scalar
    SigningKey::from_bytes((&bytes).into()).unwrap()
}

fn pubkey_bytes(env: &Env, key: &SigningKey, compressed: bool) -> Bytes {
    let point = key.verifying_key().to_encoded_point(compressed);
    Bytes::from_slice(env, point.as_bytes())
}

/// Computes the same commitment digest `submit_score` will recompute, by
/// invoking the contract's own `compute_commitment` "as" the deployed
/// contract (so `env.current_contract_address()` resolves correctly).
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
        )
        .unwrap()
        .to_bytes()
        .to_array()
    })
}

fn attest(env: &Env, key: &SigningKey, digest: [u8; 32]) -> ScoreAttestation {
    let (sig, recid) = key.sign_prehash_recoverable(&digest).unwrap();
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&sig.to_bytes());
    sig_bytes[64] = recid.to_byte();
    ScoreAttestation {
        commitment: BytesN::from_array(env, &digest),
        signature: BytesN::from_array(env, &sig_bytes),
    }
}

// ── set_service_pubkey / get_service_pubkey ───────────────────────────────────

#[test]
fn test_get_service_pubkey_before_set_fails() {
    let (_env, client, _admin, _service) = initialized();
    let result = client.try_get_service_pubkey();
    assert_eq!(result, Err(Ok(Error::ServicePubkeyNotSet)));
}

#[test]
fn test_set_service_pubkey_before_init_fails() {
    let (env, client, _admin, _service) = setup();
    let pubkey = Bytes::from_array(&env, &[0u8; 33]);
    let result = client.try_set_service_pubkey(&Vec::new(&env), &pubkey);
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_set_service_pubkey_rejects_invalid_length() {
    let (env, client, _admin, _service) = initialized();
    let pubkey = Bytes::from_array(&env, &[0u8; 32]);
    let result = client.try_set_service_pubkey(&Vec::new(&env), &pubkey);
    assert_eq!(result, Err(Ok(Error::InvalidPubkeyLength)));
}

#[test]
fn test_set_and_get_service_pubkey_compressed() {
    let (env, client, _admin, _service) = initialized();
    let key = signing_key(1);
    let pubkey = pubkey_bytes(&env, &key, true);
    assert_eq!(pubkey.len(), 33);

    client.set_service_pubkey(&Vec::new(&env), &pubkey);
    assert_eq!(client.get_service_pubkey(), pubkey);
}

#[test]
fn test_set_and_get_service_pubkey_uncompressed() {
    let (env, client, _admin, _service) = initialized();
    let key = signing_key(1);
    let pubkey = pubkey_bytes(&env, &key, false);
    assert_eq!(pubkey.len(), 65);

    client.set_service_pubkey(&Vec::new(&env), &pubkey);
    assert_eq!(client.get_service_pubkey(), pubkey);
}

#[test]
fn test_set_service_pubkey_rotates() {
    let (env, client, _admin, _service) = initialized();
    let pubkey_a = pubkey_bytes(&env, &signing_key(1), true);
    let pubkey_b = pubkey_bytes(&env, &signing_key(2), true);

    client.set_service_pubkey(&Vec::new(&env), &pubkey_a);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_b);
    assert_eq!(client.get_service_pubkey(), pubkey_b);
}

// ── submit_score opt-in enforcement ───────────────────────────────────────────

#[test]
fn test_submit_score_without_pubkey_configured_allows_missing_attestation() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &42,
        &false,
        &false,
        &1,
        &90,
        &1,
        &None,
    );
    assert!(result.is_ok());
}

#[test]
fn test_submit_score_with_pubkey_configured_requires_attestation() {
    let (env, client, _admin, _service) = initialized();
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &signing_key(1), true));

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &42,
        &false,
        &false,
        &1,
        &90,
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::InvalidAttestation)));
}

#[test]
fn test_submit_score_with_valid_attestation_compressed_pubkey_succeeds() {
    let (env, client, _admin, _service) = initialized();
    let key = signing_key(1);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key, true));

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let digest = commitment(&env, &client.address, &wallet, &pair, 42, true, false, 1, 90, 1);
    let attestation = attest(&env, &key, digest);

    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &42,
        &true,
        &false,
        &1,
        &90,
        &1,
        &Some(attestation),
    );
    assert!(result.is_ok());
    assert_eq!(client.get_score(&wallet, &pair).score, 42);
}

#[test]
fn test_submit_score_with_valid_attestation_uncompressed_pubkey_succeeds() {
    let (env, client, _admin, _service) = initialized();
    let key = signing_key(1);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key, false));

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let digest = commitment(&env, &client.address, &wallet, &pair, 42, false, true, 1, 90, 1);
    let attestation = attest(&env, &key, digest);

    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &42,
        &false,
        &true,
        &1,
        &90,
        &1,
        &Some(attestation),
    );
    assert!(result.is_ok());
}

#[test]
fn test_submit_score_with_attestation_for_different_payload_rejected() {
    let (env, client, _admin, _service) = initialized();
    let key = signing_key(1);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key, true));

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    // Attestation is valid, but for score 42 — the call below submits 43.
    let digest = commitment(&env, &client.address, &wallet, &pair, 42, false, false, 1, 90, 1);
    let attestation = attest(&env, &key, digest);

    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &43,
        &false,
        &false,
        &1,
        &90,
        &1,
        &Some(attestation),
    );
    assert_eq!(result, Err(Ok(Error::InvalidAttestation)));
}

#[test]
fn test_submit_score_with_tampered_commitment_field_rejected() {
    let (env, client, _admin, _service) = initialized();
    let key = signing_key(1);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key, true));

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let digest = commitment(&env, &client.address, &wallet, &pair, 42, false, false, 1, 90, 1);
    let mut attestation = attest(&env, &key, digest);
    // Corrupt the (otherwise untrusted) commitment field directly; the
    // signature still matches the *original* digest, but the contract
    // recomputes the commitment independently and must reject the mismatch.
    let mut corrupted = [0u8; 32];
    corrupted[0] = digest[0] ^ 0xFF;
    corrupted[1..].copy_from_slice(&digest[1..]);
    attestation.commitment = BytesN::from_array(&env, &corrupted);

    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &42,
        &false,
        &false,
        &1,
        &90,
        &1,
        &Some(attestation),
    );
    assert_eq!(result, Err(Ok(Error::InvalidAttestation)));
}

#[test]
fn test_submit_score_signed_by_wrong_key_rejected() {
    let (env, client, _admin, _service) = initialized();
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &signing_key(1), true));

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let digest = commitment(&env, &client.address, &wallet, &pair, 42, false, false, 1, 90, 1);
    // Signed by a different key than the one registered.
    let attestation = attest(&env, &signing_key(2), digest);

    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &42,
        &false,
        &false,
        &1,
        &90,
        &1,
        &Some(attestation),
    );
    assert_eq!(result, Err(Ok(Error::InvalidAttestation)));
}

#[test]
fn test_submit_score_with_out_of_range_recovery_id_rejected() {
    let (env, client, _admin, _service) = initialized();
    let key = signing_key(1);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key, true));

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let digest = commitment(&env, &client.address, &wallet, &pair, 42, false, false, 1, 90, 1);
    let mut attestation = attest(&env, &key, digest);
    let mut sig = attestation.signature.to_array();
    sig[64] = 2; // only 0/1 are valid recovery ids
    attestation.signature = BytesN::from_array(&env, &sig);

    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &42,
        &false,
        &false,
        &1,
        &90,
        &1,
        &Some(attestation),
    );
    assert_eq!(result, Err(Ok(Error::InvalidAttestation)));
}

#[test]
fn test_submit_score_attestation_required_even_when_pubkey_set_after_first_submission() {
    let (env, client, _admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Accepted while no pubkey is configured.
    client.submit_score(&Vec::new(&env), &wallet, &pair, &10, &false, &false, &1, &50, &1, &None);

    // Admin opts in; subsequent calls without an attestation are rejected.
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &signing_key(1), true));
    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &20,
        &false,
        &false,
        &2,
        &50,
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::InvalidAttestation)));
}
