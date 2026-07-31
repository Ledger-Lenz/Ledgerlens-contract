//! Tests for issue #700 — cryptographic key material length canonicalization.
//!
//! Verifies the full accept/reject matrix for `set_service_pubkey` and
//! `rotate_service_pubkey`:
//!
//! - Correct lengths with correct prefix bytes → accepted.
//! - Correct lengths with wrong prefix bytes   → `InvalidPubkeyLength`.
//! - Wrong lengths (boundary and adversarial)  → `InvalidPubkeyLength`.
//! - `validate_pubkey_format` unit tests as a standalone canonicalization gate.
//! - End-to-end attestation: a key stored with a semantically wrong prefix
//!   (previously accepted under length-only validation) now fails at set time.
//!
//! All tests use deterministic inputs and require no network access.

use soroban_sdk::{testutils::Address as _, Address, Bytes, Env, Vec};

use crate::{storage, Error, LedgerLensScoreContract, LedgerLensScoreContractClient};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client, admin, service)
}

/// Build a `Bytes` of the given `length` with `prefix` as byte 0 and all
/// remaining bytes set to `fill`.
fn make_pubkey(env: &Env, length: usize, prefix: u8, fill: u8) -> Bytes {
    let mut buf = vec![fill; length];
    if !buf.is_empty() {
        buf[0] = prefix;
    }
    Bytes::from_slice(env, &buf)
}

// ── validate_pubkey_format unit tests ────────────────────────────────────────
//
// These exercise the canonicalization helper directly (no contract round-trip)
// to document the exact accept/reject boundary as a spec-level truth table.

#[test]
fn validate_pubkey_format_accepts_33_byte_prefix_02() {
    let env = Env::default();
    let key = make_pubkey(&env, 33, 0x02, 0xAA);
    assert!(storage::validate_pubkey_format(&key));
}

#[test]
fn validate_pubkey_format_accepts_33_byte_prefix_03() {
    let env = Env::default();
    let key = make_pubkey(&env, 33, 0x03, 0xBB);
    assert!(storage::validate_pubkey_format(&key));
}

#[test]
fn validate_pubkey_format_accepts_65_byte_prefix_04() {
    let env = Env::default();
    let key = make_pubkey(&env, 65, 0x04, 0xCC);
    assert!(storage::validate_pubkey_format(&key));
}

#[test]
fn validate_pubkey_format_rejects_33_byte_prefix_00() {
    let env = Env::default();
    let key = make_pubkey(&env, 33, 0x00, 0xAA);
    assert!(!storage::validate_pubkey_format(&key));
}

#[test]
fn validate_pubkey_format_rejects_33_byte_prefix_01() {
    let env = Env::default();
    let key = make_pubkey(&env, 33, 0x01, 0xAA);
    assert!(!storage::validate_pubkey_format(&key));
}

#[test]
fn validate_pubkey_format_rejects_33_byte_prefix_04() {
    // 0x04 is only valid for 65-byte uncompressed keys.
    let env = Env::default();
    let key = make_pubkey(&env, 33, 0x04, 0xAA);
    assert!(!storage::validate_pubkey_format(&key));
}

#[test]
fn validate_pubkey_format_rejects_33_byte_prefix_ff() {
    let env = Env::default();
    let key = make_pubkey(&env, 33, 0xFF, 0xAA);
    assert!(!storage::validate_pubkey_format(&key));
}

#[test]
fn validate_pubkey_format_rejects_65_byte_prefix_00() {
    let env = Env::default();
    let key = make_pubkey(&env, 65, 0x00, 0xBB);
    assert!(!storage::validate_pubkey_format(&key));
}

#[test]
fn validate_pubkey_format_rejects_65_byte_prefix_02() {
    // 0x02/0x03 are only valid for 33-byte compressed keys.
    let env = Env::default();
    let key = make_pubkey(&env, 65, 0x02, 0xBB);
    assert!(!storage::validate_pubkey_format(&key));
}

#[test]
fn validate_pubkey_format_rejects_65_byte_prefix_03() {
    let env = Env::default();
    let key = make_pubkey(&env, 65, 0x03, 0xBB);
    assert!(!storage::validate_pubkey_format(&key));
}

#[test]
fn validate_pubkey_format_rejects_65_byte_prefix_ff() {
    let env = Env::default();
    let key = make_pubkey(&env, 65, 0xFF, 0xBB);
    assert!(!storage::validate_pubkey_format(&key));
}

// Wrong lengths
#[test]
fn validate_pubkey_format_rejects_empty() {
    let env = Env::default();
    let key = Bytes::new(&env);
    assert!(!storage::validate_pubkey_format(&key));
}

#[test]
fn validate_pubkey_format_rejects_length_1() {
    let env = Env::default();
    let key = make_pubkey(&env, 1, 0x02, 0x00);
    assert!(!storage::validate_pubkey_format(&key));
}

#[test]
fn validate_pubkey_format_rejects_length_32() {
    let env = Env::default();
    let key = make_pubkey(&env, 32, 0x02, 0xAA);
    assert!(!storage::validate_pubkey_format(&key));
}

#[test]
fn validate_pubkey_format_rejects_length_34() {
    let env = Env::default();
    let key = make_pubkey(&env, 34, 0x02, 0xAA);
    assert!(!storage::validate_pubkey_format(&key));
}

#[test]
fn validate_pubkey_format_rejects_length_64() {
    let env = Env::default();
    let key = make_pubkey(&env, 64, 0x04, 0xBB);
    assert!(!storage::validate_pubkey_format(&key));
}

#[test]
fn validate_pubkey_format_rejects_length_66() {
    let env = Env::default();
    let key = make_pubkey(&env, 66, 0x04, 0xBB);
    assert!(!storage::validate_pubkey_format(&key));
}

// ── set_service_pubkey: accepted inputs ───────────────────────────────────────

#[test]
fn set_service_pubkey_accepts_33_bytes_prefix_02() {
    let (env, client, _, _) = setup();
    let key = make_pubkey(&env, 33, 0x02, 0xAA);
    assert!(client.try_set_service_pubkey(&Vec::new(&env), &key).is_ok());
}

#[test]
fn set_service_pubkey_accepts_33_bytes_prefix_03() {
    let (env, client, _, _) = setup();
    let key = make_pubkey(&env, 33, 0x03, 0xAA);
    assert!(client.try_set_service_pubkey(&Vec::new(&env), &key).is_ok());
}

#[test]
fn set_service_pubkey_accepts_65_bytes_prefix_04() {
    let (env, client, _, _) = setup();
    let key = make_pubkey(&env, 65, 0x04, 0xCC);
    assert!(client.try_set_service_pubkey(&Vec::new(&env), &key).is_ok());
}

// ── set_service_pubkey: wrong lengths ─────────────────────────────────────────

#[test]
fn set_service_pubkey_rejects_empty() {
    let (env, client, _, _) = setup();
    let key = Bytes::new(&env);
    assert_eq!(
        client.try_set_service_pubkey(&Vec::new(&env), &key),
        Err(Ok(Error::InvalidPubkeyLength))
    );
}

#[test]
fn set_service_pubkey_rejects_length_1() {
    let (env, client, _, _) = setup();
    let key = make_pubkey(&env, 1, 0x02, 0x00);
    assert_eq!(
        client.try_set_service_pubkey(&Vec::new(&env), &key),
        Err(Ok(Error::InvalidPubkeyLength))
    );
}

#[test]
fn set_service_pubkey_rejects_length_32() {
    let (env, client, _, _) = setup();
    // Existing test already covers this; included here for the complete spec table.
    let key = make_pubkey(&env, 32, 0x02, 0xAA);
    assert_eq!(
        client.try_set_service_pubkey(&Vec::new(&env), &key),
        Err(Ok(Error::InvalidPubkeyLength))
    );
}

#[test]
fn set_service_pubkey_rejects_length_34() {
    let (env, client, _, _) = setup();
    let key = make_pubkey(&env, 34, 0x02, 0xAA);
    assert_eq!(
        client.try_set_service_pubkey(&Vec::new(&env), &key),
        Err(Ok(Error::InvalidPubkeyLength))
    );
}

#[test]
fn set_service_pubkey_rejects_length_64() {
    let (env, client, _, _) = setup();
    let key = make_pubkey(&env, 64, 0x04, 0xBB);
    assert_eq!(
        client.try_set_service_pubkey(&Vec::new(&env), &key),
        Err(Ok(Error::InvalidPubkeyLength))
    );
}

#[test]
fn set_service_pubkey_rejects_length_66() {
    let (env, client, _, _) = setup();
    let key = make_pubkey(&env, 66, 0x04, 0xBB);
    assert_eq!(
        client.try_set_service_pubkey(&Vec::new(&env), &key),
        Err(Ok(Error::InvalidPubkeyLength))
    );
}

// ── set_service_pubkey: correct length, wrong prefix byte ────────────────────

#[test]
fn set_service_pubkey_rejects_33_bytes_prefix_00() {
    // Previously this passed the length check and was silently stored.
    let (env, client, _, _) = setup();
    let key = make_pubkey(&env, 33, 0x00, 0xAA);
    assert_eq!(
        client.try_set_service_pubkey(&Vec::new(&env), &key),
        Err(Ok(Error::InvalidPubkeyLength))
    );
}

#[test]
fn set_service_pubkey_rejects_33_bytes_prefix_01() {
    let (env, client, _, _) = setup();
    let key = make_pubkey(&env, 33, 0x01, 0xAA);
    assert_eq!(
        client.try_set_service_pubkey(&Vec::new(&env), &key),
        Err(Ok(Error::InvalidPubkeyLength))
    );
}

#[test]
fn set_service_pubkey_rejects_33_bytes_prefix_04() {
    // 0x04 is only valid for 65-byte uncompressed keys; a 33-byte key with
    // 0x04 is structurally ambiguous and must be rejected.
    let (env, client, _, _) = setup();
    let key = make_pubkey(&env, 33, 0x04, 0xAA);
    assert_eq!(
        client.try_set_service_pubkey(&Vec::new(&env), &key),
        Err(Ok(Error::InvalidPubkeyLength))
    );
}

#[test]
fn set_service_pubkey_rejects_33_bytes_prefix_ff() {
    let (env, client, _, _) = setup();
    let key = make_pubkey(&env, 33, 0xFF, 0xAA);
    assert_eq!(
        client.try_set_service_pubkey(&Vec::new(&env), &key),
        Err(Ok(Error::InvalidPubkeyLength))
    );
}

#[test]
fn set_service_pubkey_rejects_65_bytes_prefix_00() {
    let (env, client, _, _) = setup();
    let key = make_pubkey(&env, 65, 0x00, 0xBB);
    assert_eq!(
        client.try_set_service_pubkey(&Vec::new(&env), &key),
        Err(Ok(Error::InvalidPubkeyLength))
    );
}

#[test]
fn set_service_pubkey_rejects_65_bytes_prefix_02() {
    let (env, client, _, _) = setup();
    let key = make_pubkey(&env, 65, 0x02, 0xBB);
    assert_eq!(
        client.try_set_service_pubkey(&Vec::new(&env), &key),
        Err(Ok(Error::InvalidPubkeyLength))
    );
}

#[test]
fn set_service_pubkey_rejects_65_bytes_prefix_03() {
    let (env, client, _, _) = setup();
    let key = make_pubkey(&env, 65, 0x03, 0xBB);
    assert_eq!(
        client.try_set_service_pubkey(&Vec::new(&env), &key),
        Err(Ok(Error::InvalidPubkeyLength))
    );
}

#[test]
fn set_service_pubkey_rejects_65_bytes_prefix_ff() {
    let (env, client, _, _) = setup();
    let key = make_pubkey(&env, 65, 0xFF, 0xBB);
    assert_eq!(
        client.try_set_service_pubkey(&Vec::new(&env), &key),
        Err(Ok(Error::InvalidPubkeyLength))
    );
}

// ── set_service_pubkey: adversarial fill patterns ─────────────────────────────

#[test]
fn set_service_pubkey_accepts_all_zero_payload_with_valid_prefix() {
    // Prefix is valid (0x02); remaining payload bytes are all zero.
    // This is accepted at key-set time (point-on-curve check is out of scope
    // on Soroban — see spec §5 and storage::validate_pubkey_format doc).
    // Such a key will never match any honestly-recovered secp256k1 point.
    let (env, client, _, _) = setup();
    let key = make_pubkey(&env, 33, 0x02, 0x00);
    assert!(client.try_set_service_pubkey(&Vec::new(&env), &key).is_ok());
}

#[test]
fn set_service_pubkey_accepts_all_ff_payload_with_valid_prefix() {
    // Same reasoning: prefix valid, payload all 0xFF; accepted, never matches.
    let (env, client, _, _) = setup();
    let key = make_pubkey(&env, 33, 0x02, 0xFF);
    assert!(client.try_set_service_pubkey(&Vec::new(&env), &key).is_ok());
}

#[test]
fn set_service_pubkey_accepts_all_ff_uncompressed_with_valid_prefix() {
    let (env, client, _, _) = setup();
    let key = make_pubkey(&env, 65, 0x04, 0xFF);
    assert!(client.try_set_service_pubkey(&Vec::new(&env), &key).is_ok());
}

// ── rotate_service_pubkey: same canonicalization gate ─────────────────────────

#[test]
fn rotate_service_pubkey_accepts_33_bytes_prefix_02() {
    let (env, client, _, _) = setup();
    // Set an initial key first.
    client.set_service_pubkey(&Vec::new(&env), &make_pubkey(&env, 33, 0x02, 0x10));
    let new_key = make_pubkey(&env, 33, 0x02, 0x20);
    assert!(client.try_rotate_service_pubkey(&Vec::new(&env), &new_key, &0u64).is_ok());
}

#[test]
fn rotate_service_pubkey_accepts_65_bytes_prefix_04() {
    let (env, client, _, _) = setup();
    client.set_service_pubkey(&Vec::new(&env), &make_pubkey(&env, 33, 0x02, 0x10));
    let new_key = make_pubkey(&env, 65, 0x04, 0x20);
    assert!(client.try_rotate_service_pubkey(&Vec::new(&env), &new_key, &0u64).is_ok());
}

#[test]
fn rotate_service_pubkey_rejects_wrong_length() {
    let (env, client, _, _) = setup();
    client.set_service_pubkey(&Vec::new(&env), &make_pubkey(&env, 33, 0x02, 0x10));
    let bad = make_pubkey(&env, 32, 0x02, 0xAA);
    assert_eq!(
        client.try_rotate_service_pubkey(&Vec::new(&env), &bad, &0u64),
        Err(Ok(Error::InvalidPubkeyLength))
    );
}

#[test]
fn rotate_service_pubkey_rejects_33_bytes_prefix_00() {
    let (env, client, _, _) = setup();
    client.set_service_pubkey(&Vec::new(&env), &make_pubkey(&env, 33, 0x02, 0x10));
    let bad = make_pubkey(&env, 33, 0x00, 0xAA);
    assert_eq!(
        client.try_rotate_service_pubkey(&Vec::new(&env), &bad, &0u64),
        Err(Ok(Error::InvalidPubkeyLength))
    );
}

#[test]
fn rotate_service_pubkey_rejects_65_bytes_prefix_02() {
    let (env, client, _, _) = setup();
    client.set_service_pubkey(&Vec::new(&env), &make_pubkey(&env, 33, 0x02, 0x10));
    let bad = make_pubkey(&env, 65, 0x02, 0xBB);
    assert_eq!(
        client.try_rotate_service_pubkey(&Vec::new(&env), &bad, &0u64),
        Err(Ok(Error::InvalidPubkeyLength))
    );
}

// ── Round-trip: stored key is exactly what was set ───────────────────────────

#[test]
fn get_service_pubkey_returns_stored_bytes_unchanged() {
    let (env, client, _, _) = setup();
    let key = make_pubkey(&env, 33, 0x03, 0xDE);
    client.set_service_pubkey(&Vec::new(&env), &key);
    assert_eq!(client.get_service_pubkey(), key);
}

#[test]
fn get_service_pubkey_uncompressed_returns_stored_bytes_unchanged() {
    let (env, client, _, _) = setup();
    let key = make_pubkey(&env, 65, 0x04, 0xAB);
    client.set_service_pubkey(&Vec::new(&env), &key);
    assert_eq!(client.get_service_pubkey(), key);
}

#[test]
fn rotate_service_pubkey_instant_updates_active_key() {
    let (env, client, _, _) = setup();
    let key_a = make_pubkey(&env, 33, 0x02, 0x10);
    let key_b = make_pubkey(&env, 33, 0x03, 0x20);
    client.set_service_pubkey(&Vec::new(&env), &key_a);
    client.rotate_service_pubkey(&Vec::new(&env), &key_b, &0u64);
    // After instant rotation the active slot holds key_b.
    assert_eq!(client.get_service_pubkey(), key_b);
    // No pending key after instant rotation.
    assert!(client.get_pending_service_pubkey().is_none());
}
