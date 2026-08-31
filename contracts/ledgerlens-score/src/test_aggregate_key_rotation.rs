//! Tests for the aggregate (threshold-signature) service pubkey dual-key
//! overlap window (issue #697): `rotate_aggregate_service_pubkey` /
//! `get_pending_aggregate_pubkey`, verified through
//! `verify_threshold_attestation`. Mirrors the single-signer coverage in
//! `test_dual_key_pubkey.rs` (issue #295) for the `ThresholdAttestation`
//! path, which previously had no rotation-overlap support at all — only
//! `verify_signature` (the single-key `ScoreAttestation` path) did.

use k256::ecdsa::SigningKey;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Bytes, BytesN, Env, Symbol, Vec,
};

use crate::{
    Error, LedgerLensScoreContract, LedgerLensScoreContractClient, MaybeScoreAttestation,
    MaybeThresholdAttestation, ScoreAttestationInput, ThresholdAttestation,
};

const START_TS: u64 = 1_700_000_000;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);
    let id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client, admin, service)
}

fn signing_key(seed: u8) -> SigningKey {
    let mut b = [0u8; 32];
    b[31] = seed;
    b[0] = 1;
    SigningKey::from_bytes((&b).into()).unwrap()
}

fn pubkey_bytes(env: &Env, key: &SigningKey) -> Bytes {
    let pt = key.verifying_key().to_encoded_point(true);
    Bytes::from_slice(env, pt.as_bytes())
}

fn threshold_attest(
    env: &Env,
    contract_id: &Address,
    contract_version: u32,
    key: &SigningKey,
    wallet: &Address,
    pair: &Symbol,
) -> ThresholdAttestation {
    let contract_id_zero = BytesN::from_array(env, &[0u8; 32]);
    let digest = env.as_contract(contract_id, || {
        LedgerLensScoreContract::compute_commitment(
            env,
            wallet,
            pair,
            50,
            false,
            false,
            START_TS,
            90,
            1,
            &contract_id_zero,
            contract_version,
        )
        .unwrap()
        .to_bytes()
        .to_array()
    });
    let (sig, recid) = key.sign_prehash_recoverable(&digest).unwrap();
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&sig.to_bytes());
    sig_bytes[64] = recid.to_byte();
    ThresholdAttestation {
        commitment: BytesN::from_array(env, &digest),
        threshold_sig: BytesN::from_array(env, &sig_bytes),
        participating_signers: Vec::new(env),
        contract_id: contract_id_zero,
        contract_version,
    }
}

fn submit(
    client: &LedgerLensScoreContractClient,
    env: &Env,
    wallet: &Address,
    pair: &Symbol,
    ta: ThresholdAttestation,
) -> Result<(), Error> {
    match client.try_submit_score(
        &Vec::new(env),
        wallet,
        pair,
        &50,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &Some(ScoreAttestationInput {
            attestation: MaybeScoreAttestation::None,
            threshold_attestation: MaybeThresholdAttestation::Some(ta),
            commitment: None,
        }),
    ) {
        Ok(Ok(())) => Ok(()),
        Err(Ok(e)) => Err(e),
        Ok(Err(_)) | Err(Err(_)) => panic!("unexpected host conversion or invocation error"),
    }
}

// ── Instant rotation (overlap = 0) ────────────────────────────────────────────

#[test]
fn test_instant_aggregate_rotation_promotes_key_immediately() {
    let (env, client, _admin, _service) = setup();
    let old_key = signing_key(1);
    let new_key = signing_key(2);

    client.set_aggregate_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &old_key));
    client.rotate_aggregate_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &new_key), &0u64);

    assert!(client.get_pending_aggregate_pubkey().is_none());
    assert_eq!(client.get_aggregate_service_pubkey(), pubkey_bytes(&env, &new_key));
}

#[test]
fn test_old_aggregate_key_rejected_after_instant_rotation() {
    let (env, client, _admin, _service) = setup();
    let contract_id = client.address.clone();
    let old_key = signing_key(1);
    let new_key = signing_key(2);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let version = client.get_contract_version();

    client.set_aggregate_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &old_key));
    client.rotate_aggregate_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &new_key), &0u64);

    let ta = threshold_attest(&env, &contract_id, version, &old_key, &wallet, &pair);
    assert_eq!(submit(&client, &env, &wallet, &pair, ta), Err(Error::InvalidAttestation));
}

// ── Overlap window: both keys accepted ───────────────────────────────────────

#[test]
fn test_new_aggregate_key_accepted_during_overlap() {
    let (env, client, _admin, _service) = setup();
    let contract_id = client.address.clone();
    let old_key = signing_key(1);
    let new_key = signing_key(2);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let version = client.get_contract_version();

    client.set_aggregate_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &old_key));
    client.rotate_aggregate_service_pubkey(
        &Vec::new(&env),
        &pubkey_bytes(&env, &new_key),
        &3600u64,
    );

    let ta = threshold_attest(&env, &contract_id, version, &new_key, &wallet, &pair);
    assert!(submit(&client, &env, &wallet, &pair, ta).is_ok());
}

#[test]
fn test_old_aggregate_key_accepted_during_overlap() {
    let (env, client, _admin, _service) = setup();
    let contract_id = client.address.clone();
    let old_key = signing_key(1);
    let new_key = signing_key(2);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let version = client.get_contract_version();

    client.set_aggregate_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &old_key));
    client.rotate_aggregate_service_pubkey(
        &Vec::new(&env),
        &pubkey_bytes(&env, &new_key),
        &3600u64,
    );

    let ta = threshold_attest(&env, &contract_id, version, &old_key, &wallet, &pair);
    assert!(submit(&client, &env, &wallet, &pair, ta).is_ok());
}

#[test]
fn test_get_pending_aggregate_pubkey_during_overlap() {
    let (env, client, _admin, _service) = setup();
    let old_key = signing_key(1);
    let new_key = signing_key(2);
    let overlap = 3600u64;

    client.set_aggregate_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &old_key));
    client.rotate_aggregate_service_pubkey(
        &Vec::new(&env),
        &pubkey_bytes(&env, &new_key),
        &overlap,
    );

    let pending = client.get_pending_aggregate_pubkey();
    assert!(pending.is_some());
    let (pk, expiry) = pending.unwrap();
    assert_eq!(pk, pubkey_bytes(&env, &new_key));
    assert_eq!(expiry, START_TS + overlap);
}

// ── Post-overlap: old key rejected, new key promoted ─────────────────────────

#[test]
fn test_old_aggregate_key_rejected_after_overlap_expires() {
    let (env, client, _admin, _service) = setup();
    let contract_id = client.address.clone();
    let old_key = signing_key(1);
    let new_key = signing_key(2);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let overlap = 1000u64;
    let version = client.get_contract_version();

    client.set_aggregate_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &old_key));
    client.rotate_aggregate_service_pubkey(
        &Vec::new(&env),
        &pubkey_bytes(&env, &new_key),
        &overlap,
    );

    // Advance time past the overlap window — the pending key must now be
    // the only one that verifies; the retired key is fully expired.
    env.ledger().with_mut(|l| l.timestamp = START_TS + overlap + 1);

    let ta = threshold_attest(&env, &contract_id, version, &old_key, &wallet, &pair);
    assert_eq!(submit(&client, &env, &wallet, &pair, ta), Err(Error::InvalidAttestation));
}

#[test]
fn test_new_aggregate_key_accepted_after_overlap_expires() {
    let (env, client, _admin, _service) = setup();
    let contract_id = client.address.clone();
    let old_key = signing_key(1);
    let new_key = signing_key(2);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let overlap = 1000u64;
    let version = client.get_contract_version();

    client.set_aggregate_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &old_key));
    client.rotate_aggregate_service_pubkey(
        &Vec::new(&env),
        &pubkey_bytes(&env, &new_key),
        &overlap,
    );

    env.ledger().with_mut(|l| l.timestamp = START_TS + overlap + 1);

    let ta = threshold_attest(&env, &contract_id, version, &new_key, &wallet, &pair);
    assert!(submit(&client, &env, &wallet, &pair, ta).is_ok());
}

#[test]
fn test_aggregate_pending_key_auto_promoted_after_expiry() {
    let (env, client, _admin, _service) = setup();
    let old_key = signing_key(1);
    let new_key = signing_key(2);
    let overlap = 1000u64;

    client.set_aggregate_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &old_key));
    client.rotate_aggregate_service_pubkey(
        &Vec::new(&env),
        &pubkey_bytes(&env, &new_key),
        &overlap,
    );

    env.ledger().with_mut(|l| l.timestamp = START_TS + overlap + 1);

    // Any verification attempt (even a failing one) resolves the overlap
    // state first, promoting the pending key to active.
    let contract_id = client.address.clone();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let version = client.get_contract_version();
    let ta = threshold_attest(&env, &contract_id, version, &new_key, &wallet, &pair);
    let _ = submit(&client, &env, &wallet, &pair, ta);

    assert!(client.get_pending_aggregate_pubkey().is_none());
    assert_eq!(client.get_aggregate_service_pubkey(), pubkey_bytes(&env, &new_key));
}

// ── Invalid pubkey length rejected ───────────────────────────────────────────

#[test]
fn test_rotate_aggregate_service_pubkey_rejects_invalid_length() {
    let (env, client, _admin, _service) = setup();
    let bad = Bytes::from_array(&env, &[0u8; 32]);
    let result = client.try_rotate_aggregate_service_pubkey(&Vec::new(&env), &bad, &0u64);
    assert_eq!(result, Err(Ok(Error::InvalidPubkeyLength)));
}
