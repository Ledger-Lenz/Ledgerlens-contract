//! Tests for operator alert acknowledgement records (issue #630).
//!
//! The feature introduces `acknowledge_alert` (admin-auth write) and
//! `get_alert_acknowledgement` (unauthenticated read) alongside two
//! public types: `AlertType` and `AlertAckRecord`.
//!
//! Coverage:
//!   Positive  — happy-path ack + read-back for each AlertType variant.
//!   Negative  — uninitialized contract, non-admin caller.
//!   Boundary  — zero note_hash, Momentum keyed by wallet/pair, re-ack overwrites.
//!   Event     — `alrt_ack` event carries the correct topic and data.
//!   Read-only — `get_alert_acknowledgement` needs no auth.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger as _},
    Address, BytesN, Env, IntoVal, Vec,
};

use crate::{
    AlertAckRecord, AlertType, Error, LedgerLensScoreContract, LedgerLensScoreContractClient,
};

// ── Helpers ──────────────────────────────────────────────────────────────────

const START_TS: u64 = 1_700_000_000;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    (env, client, admin, service)
}

fn zero_hash(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
}

fn nonzero_hash(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[
        0xde, 0xad, 0xbe, 0xef, 0x01, 0x02, 0x03, 0x04,
        0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c,
        0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14,
        0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c,
    ])
}

// ── Positive (happy path) ────────────────────────────────────────────────────

#[test]
fn test_ack_service_silence_stores_record() {
    let (env, client, admin, _service) = setup();
    let note = nonzero_hash(&env);

    client.acknowledge_alert(&Vec::new(&env), &AlertType::ServiceSilence, &note);

    let record = client.get_alert_acknowledgement(&AlertType::ServiceSilence).expect("record missing");
    assert_eq!(record.operator, admin, "operator must be the admin address");
    assert_eq!(record.acknowledged_at, START_TS, "timestamp must match ledger timestamp");
    assert_eq!(record.note_hash, note, "note_hash must round-trip");
}

#[test]
fn test_ack_momentum_stores_record_keyed_by_wallet_pair() {
    let (env, client, admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let note = nonzero_hash(&env);

    client.acknowledge_alert(
        &Vec::new(&env),
        &AlertType::Momentum(wallet.clone(), pair.clone()),
        &note,
    );

    let record = client
        .get_alert_acknowledgement(&AlertType::Momentum(wallet.clone(), pair.clone()))
        .expect("record missing");
    assert_eq!(record.operator, admin);
    assert_eq!(record.acknowledged_at, START_TS);
    assert_eq!(record.note_hash, note);
}

#[test]
fn test_ack_zero_note_hash_is_valid() {
    // The zero hash is explicitly a valid value — operator deliberateness is
    // the signal, not the presence of a runbook note.
    let (env, client, _admin, _service) = setup();
    let note = zero_hash(&env);

    client.acknowledge_alert(&Vec::new(&env), &AlertType::ServiceSilence, &note);

    let record = client
        .get_alert_acknowledgement(&AlertType::ServiceSilence)
        .expect("record missing even with zero note_hash");
    assert_eq!(record.note_hash, note);
}

// ── Read-only (no auth required on getter) ────────────────────────────────────

#[test]
fn test_get_alert_acknowledgement_returns_none_before_first_ack() {
    let (_env, client, _admin, _service) = setup();
    assert!(
        client.get_alert_acknowledgement(&AlertType::ServiceSilence).is_none(),
        "must return None before any ack"
    );
}

#[test]
fn test_get_alert_acknowledgement_for_momentum_returns_none_before_ack() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    assert!(
        client
            .get_alert_acknowledgement(&AlertType::Momentum(wallet.clone(), pair.clone()))
            .is_none()
    );
}

// ── Idempotency / re-acknowledge ────────────────────────────────────────────

#[test]
fn test_re_acknowledge_overwrites_previous_record() {
    let (env, client, admin, _service) = setup();
    let first_note = nonzero_hash(&env);

    client.acknowledge_alert(&Vec::new(&env), &AlertType::ServiceSilence, &first_note);

    // Advance time and re-ack with a different note.
    env.ledger().with_mut(|l| l.timestamp = START_TS + 999);
    let second_note = zero_hash(&env);
    client.acknowledge_alert(&Vec::new(&env), &AlertType::ServiceSilence, &second_note);

    let record = client
        .get_alert_acknowledgement(&AlertType::ServiceSilence)
        .expect("record missing after re-ack");

    assert_eq!(record.operator, admin, "operator must still be admin");
    assert_eq!(record.acknowledged_at, START_TS + 999, "timestamp must reflect re-ack time");
    assert_eq!(record.note_hash, second_note, "note_hash must reflect re-ack note");
}

// ── Isolation across AlertType variants ──────────────────────────────────────

#[test]
fn test_ack_of_one_type_does_not_affect_other() {
    let (env, client, _admin, _service) = setup();
    let note = nonzero_hash(&env);

    client.acknowledge_alert(&Vec::new(&env), &AlertType::ServiceSilence, &note);

    // Momentum for a specific pair has not been acked — must stay None.
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    assert!(
        client
            .get_alert_acknowledgement(&AlertType::Momentum(wallet.clone(), pair.clone()))
            .is_none(),
        "acking ServiceSilence must not create a Momentum record"
    );
}

#[test]
fn test_different_momentum_pairs_are_independent() {
    let (env, client, _admin, _service) = setup();
    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let note_a = nonzero_hash(&env);

    client.acknowledge_alert(
        &Vec::new(&env),
        &AlertType::Momentum(wallet_a.clone(), pair.clone()),
        &note_a,
    );

    assert!(
        client
            .get_alert_acknowledgement(&AlertType::Momentum(wallet_b.clone(), pair.clone()))
            .is_none(),
        "ack for wallet_a must not create a record for wallet_b"
    );
}

// ── Authorization ────────────────────────────────────────────────────────────

#[test]
fn test_acknowledge_alert_requires_initialization() {
    // Contract has not been initialized — must return NotInitialized.
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let result = client.try_acknowledge_alert(
        &Vec::new(&env),
        &AlertType::ServiceSilence,
        &zero_hash(&env),
    );
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_acknowledge_alert_rejects_unauthorized_caller() {
    let env = Env::default();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);

    client.initialize(&admin, &service);

    let result = client.try_acknowledge_alert(
        &Vec::new(&env),
        &AlertType::ServiceSilence,
        &zero_hash(&env),
    );
    assert!(result.is_err(), "non-admin must not be able to acknowledge an alert");
}

// ── Event emission ───────────────────────────────────────────────────────────

#[test]
fn test_acknowledge_alert_emits_alrt_ack_event() {
    let (env, client, admin, _service) = setup();
    let note = nonzero_hash(&env);

    client.acknowledge_alert(&Vec::new(&env), &AlertType::ServiceSilence, &note);

    let expected_record = AlertAckRecord {
        operator: admin.clone(),
        acknowledged_at: START_TS,
        note_hash: note.clone(),
    };

    let topic = (symbol_short!("alrt_ack"), 1u32, AlertType::ServiceSilence);
    let found = env.events().all().iter().any(|(addr, topics, data)| {
        if addr != client.address || topics != topic.clone().into_val(&env) {
            return false;
        }
        let record: AlertAckRecord = data.into_val(&env);
        record == expected_record
    });
    assert!(found, "alrt_ack event not emitted or payload mismatch");
}

#[test]
fn test_acknowledge_alert_emits_separate_events_per_re_ack() {
    let (env, client, _admin, _service) = setup();
    let note1 = nonzero_hash(&env);
    let note2 = zero_hash(&env);

    client.acknowledge_alert(&Vec::new(&env), &AlertType::ServiceSilence, &note1);
    client.acknowledge_alert(&Vec::new(&env), &AlertType::ServiceSilence, &note2);

    let topic = (symbol_short!("alrt_ack"), 1u32, AlertType::ServiceSilence);
    let count = env.events().all().iter().filter(|(addr, topics, _)| {
        *addr == client.address && *topics == topic.clone().into_val(&env)
    }).count();
    assert_eq!(count, 2, "each call must emit its own event");
}
