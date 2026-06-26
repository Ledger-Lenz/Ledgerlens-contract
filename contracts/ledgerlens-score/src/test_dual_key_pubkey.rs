//! Tests for dual-key pubkey overlap window (#295).
//! Covers: overlap acceptance, post-overlap old key rejection,
//! instant rotation (overlap_secs = 0), and get_pending_service_pubkey.

use k256::ecdsa::SigningKey;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Bytes, BytesN, Env, Symbol, Vec,
};

use crate::{
    Error, LedgerLensScoreContract, LedgerLensScoreContractClient, ScoreAttestation,
    ScoreAttestationInput,
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

fn sign(env: &Env, contract_id: &Address, key: &SigningKey, wallet: &Address, pair: &Symbol) -> ScoreAttestation {
    let digest = env.as_contract(contract_id, || {
        LedgerLensScoreContract::compute_commitment(
            env, wallet, pair, 50, false, false, START_TS, 90, 1,
        )
        .unwrap()
        .to_bytes()
        .to_array()
    });
    let (sig, recid) = key.sign_prehash_recoverable(&digest).unwrap();
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&sig.to_bytes());
    sig_bytes[64] = recid.to_byte();
    ScoreAttestation {
        commitment: BytesN::from_array(env, &digest),
        signature: BytesN::from_array(env, &sig_bytes),
    }
}

fn submit(
    client: &LedgerLensScoreContractClient,
    env: &Env,
    wallet: &Address,
    pair: &Symbol,
    att: ScoreAttestation,
) -> Result<(), crate::Error> {
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
        &Some(ScoreAttestationInput::Single(att)),
    ) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) => Err(crate::Error::InvalidAttestation),
        Err(e) => Err(e.unwrap()),
    }
}

// ── Instant rotation (overlap = 0) ────────────────────────────────────────────

#[test]
fn test_instant_rotation_promotes_key_immediately() {
    let (env, client, _admin, _) = setup();
    let old_key = signing_key(1);
    let new_key = signing_key(2);
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    // Use a fresh client on the same env to share the contract.
    let (env, client, _admin, _) = setup();
    let contract_id = client.address.clone();

    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &old_key));
    // Instant rotation: overlap = 0
    client.rotate_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &new_key), &0u64);

    // No pending key after instant rotation.
    assert!(client.get_pending_service_pubkey().is_none());
    // Active key is now new_key.
    assert_eq!(client.get_service_pubkey(), pubkey_bytes(&env, &new_key));
}

#[test]
fn test_old_key_rejected_after_instant_rotation() {
    let (env, client, _admin, _) = setup();
    let contract_id = client.address.clone();
    let old_key = signing_key(1);
    let new_key = signing_key(2);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &old_key));
    client.rotate_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &new_key), &0u64);

    let att = sign(&env, &contract_id, &old_key, &wallet, &pair);
    let result = submit(&client, &env, &wallet, &pair, att);
    assert_eq!(result, Err(Error::InvalidAttestation));
}

// ── Overlap window: both keys accepted ───────────────────────────────────────

#[test]
fn test_new_key_accepted_during_overlap() {
    let (env, client, _admin, _) = setup();
    let contract_id = client.address.clone();
    let old_key = signing_key(1);
    let new_key = signing_key(2);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &old_key));
    client.rotate_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &new_key), &3600u64);

    // New key is the pending key — sign with it.
    let att = sign(&env, &contract_id, &new_key, &wallet, &pair);
    assert!(submit(&client, &env, &wallet, &pair, att).is_ok());
}

#[test]
fn test_old_key_accepted_during_overlap() {
    let (env, client, _admin, _) = setup();
    let contract_id = client.address.clone();
    let old_key = signing_key(1);
    let new_key = signing_key(2);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &old_key));
    client.rotate_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &new_key), &3600u64);

    // Old key is still the active key during overlap.
    let att = sign(&env, &contract_id, &old_key, &wallet, &pair);
    assert!(submit(&client, &env, &wallet, &pair, att).is_ok());
}

// ── get_pending_service_pubkey during overlap ─────────────────────────────────

#[test]
fn test_get_pending_service_pubkey_during_overlap() {
    let (env, client, _admin, _) = setup();
    let old_key = signing_key(1);
    let new_key = signing_key(2);
    let overlap = 3600u64;

    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &old_key));
    client.rotate_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &new_key), &overlap);

    let pending = client.get_pending_service_pubkey();
    assert!(pending.is_some());
    let (pk, expiry) = pending.unwrap();
    assert_eq!(pk, pubkey_bytes(&env, &new_key));
    assert_eq!(expiry, START_TS + overlap);
}

// ── Post-overlap: old key rejected, new key promoted ─────────────────────────

#[test]
fn test_old_key_rejected_after_overlap_expires() {
    let (env, client, _admin, _) = setup();
    let contract_id = client.address.clone();
    let old_key = signing_key(1);
    let new_key = signing_key(2);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let overlap = 1000u64;

    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &old_key));
    client.rotate_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &new_key), &overlap);

    // Advance time past the overlap window.
    env.ledger().with_mut(|l| l.timestamp = START_TS + overlap + 1);

    let att = sign(&env, &contract_id, &old_key, &wallet, &pair);
    let result = submit(&client, &env, &wallet, &pair, att);
    assert_eq!(result, Err(Error::InvalidAttestation));
}

#[test]
fn test_new_key_accepted_after_overlap_expires() {
    let (env, client, _admin, _) = setup();
    let contract_id = client.address.clone();
    let old_key = signing_key(1);
    let new_key = signing_key(2);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let overlap = 1000u64;

    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &old_key));
    client.rotate_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &new_key), &overlap);

    // Advance time past the overlap window.
    env.ledger().with_mut(|l| l.timestamp = START_TS + overlap + 1);

    let att = sign(&env, &contract_id, &new_key, &wallet, &pair);
    assert!(submit(&client, &env, &wallet, &pair, att).is_ok());
}

// ── Invalid pubkey length rejected ───────────────────────────────────────────

#[test]
fn test_rotate_service_pubkey_rejects_invalid_length() {
    let (env, client, _admin, _) = setup();
    let bad = Bytes::from_array(&env, &[0u8; 32]);
    let result = client.try_rotate_service_pubkey(&Vec::new(&env), &bad, &0u64);
    assert_eq!(result, Err(Ok(Error::InvalidPubkeyLength)));
}
