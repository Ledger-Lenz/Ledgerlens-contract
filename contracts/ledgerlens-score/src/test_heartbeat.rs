//! Tests for the off-chain service heartbeat monitor: a global liveness
//! signal (`LastServiceActivityAt`) updated on every accepted submission or
//! `ping_heartbeat`, queryable via `is_service_alive`, with a one-shot
//! `ServiceSilenceAlertEvent` / `ServiceResumedEvent` pair marking the start
//! and end of a silence window.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger as _},
    Address, Env, IntoVal, Vec,
};

use crate::{
    events::{ServiceResumedEvent, ServiceSilenceAlertEvent},
    LedgerLensScoreContract, LedgerLensScoreContractClient,
};

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

fn submit(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    wallet: &Address,
    pair: &soroban_sdk::Symbol,
    score: u32,
) {
    client.submit_score(
        &Vec::new(env),
        wallet,
        pair,
        &score,
        &true,
        &false,
        &env.ledger().timestamp(),
        &90,
        &1,
        &None,
    );
}

fn advance_to(env: &Env, ts: u64) {
    env.ledger().with_mut(|l| l.timestamp = ts);
}

// ── Defaults / never-active ─────────────────────────────────────────────────

#[test]
fn test_no_submissions_alive_by_default() {
    let (_env, client, _admin, _service) = setup();
    assert_eq!(client.get_last_service_activity(), 0);
    assert!(client.is_service_alive(), "never-active service must be reported alive");
}

#[test]
fn test_default_heartbeat_threshold() {
    let (_env, client, _admin, _service) = setup();
    assert_eq!(client.get_heartbeat_alert_threshold(), 3_600);
}

// ── Activity recording ──────────────────────────────────────────────────────

#[test]
fn test_submission_updates_last_activity() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 42);

    assert_eq!(client.get_last_service_activity(), START_TS);
    assert!(client.is_service_alive());
}

#[test]
fn test_batch_submission_updates_last_activity() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let mut batch: Vec<crate::ScoreSubmission> = Vec::new(&env);
    batch.push_back(crate::ScoreSubmission {
        wallet: wallet.clone(),
        asset_pair: pair.clone(),
        score: 50,
        benford_flag: false,
        ml_flag: false,
        timestamp: START_TS,
        confidence: 80,
        model_version: 1,
    });
    client.submit_scores_batch(&batch);

    assert_eq!(client.get_last_service_activity(), START_TS);
}

#[test]
fn test_ping_heartbeat_updates_last_activity_without_a_score() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.ping_heartbeat();

    assert_eq!(client.get_last_service_activity(), START_TS);
    // No score was ever submitted — the ping alone must not write one.
    assert!(client.try_get_score(&wallet, &pair).is_err());
}

#[test]
fn test_ping_heartbeat_requires_initialization() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let result = client.try_ping_heartbeat();
    assert_eq!(result, Err(Ok(crate::Error::NotInitialized)));
}

// ── Threshold configuration ──────────────────────────────────────────────────

#[test]
fn test_set_and_get_heartbeat_alert_threshold() {
    let (env, client, _admin, _service) = setup();
    client.set_heartbeat_alert_threshold(&Vec::new(&env), &7_200);
    assert_eq!(client.get_heartbeat_alert_threshold(), 7_200);
}

// ── Silence alert logic ──────────────────────────────────────────────────────

#[test]
fn test_is_service_alive_within_threshold() {
    let (env, client, _admin, _service) = setup();
    client.set_heartbeat_alert_threshold(&Vec::new(&env), &100);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 10);

    advance_to(&env, START_TS + 100); // exactly at the threshold — still alive
    assert!(client.is_service_alive());
}

#[test]
fn test_is_service_alive_past_threshold() {
    let (env, client, _admin, _service) = setup();
    client.set_heartbeat_alert_threshold(&Vec::new(&env), &100);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 10);

    advance_to(&env, START_TS + 101);
    assert!(!client.is_service_alive());
}

#[test]
fn test_get_score_emits_silence_alert_once_past_threshold() {
    let (env, client, _admin, _service) = setup();
    client.set_heartbeat_alert_threshold(&Vec::new(&env), &100);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 10);

    // Still within the window: no alert.
    advance_to(&env, START_TS + 50);
    let _ = client.try_get_score(&wallet, &pair);
    assert!(!silence_alert_emitted(&env, client.address.clone()));

    // Past the window: the read path emits the alert exactly once.
    advance_to(&env, START_TS + 101);
    let _ = client.try_get_score(&wallet, &pair);
    assert!(silence_alert_emitted(&env, client.address.clone()));

    let event = find_silence_alert(&env, client.address.clone()).unwrap();
    assert_eq!(event.last_active_at, START_TS);
    assert_eq!(event.silent_secs, 101);
    assert_eq!(event.threshold_secs, 100);
}

#[test]
fn test_silence_alert_fires_only_once_per_window() {
    let (env, client, _admin, _service) = setup();
    client.set_heartbeat_alert_threshold(&Vec::new(&env), &100);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 10);

    advance_to(&env, START_TS + 101);
    let _ = client.try_get_score(&wallet, &pair);
    let first_count = count_silence_alerts(&env, client.address.clone());
    assert_eq!(first_count, 1);

    // Calling get_score again, still silent, must not emit a second alert.
    advance_to(&env, START_TS + 500);
    let _ = client.try_get_score(&wallet, &pair);
    let second_count = count_silence_alerts(&env, client.address.clone());
    assert_eq!(second_count, 1, "silence alert must fire only once per silence window");
}

#[test]
fn test_never_active_service_never_alerts() {
    let (env, client, _admin, _service) = setup();
    client.set_heartbeat_alert_threshold(&Vec::new(&env), &100);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    advance_to(&env, START_TS + 1_000_000);
    let _ = client.try_get_score(&wallet, &pair);

    assert!(
        !silence_alert_emitted(&env, client.address.clone()),
        "a service that has never been active is not 'silent' — see is_service_alive"
    );
}

// ── Resumption after a silence window ───────────────────────────────────────

#[test]
fn test_submission_after_silence_emits_resumed_event_and_clears_alert() {
    let (env, client, _admin, _service) = setup();
    client.set_heartbeat_alert_threshold(&Vec::new(&env), &100);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 10);

    // Drive the contract into the alerted-silence state. The gap must also
    // clear the default 1-hour submission cooldown so the resuming
    // submission below is itself accepted rather than rate-limited.
    advance_to(&env, START_TS + 4_000);
    let _ = client.try_get_score(&wallet, &pair);
    assert!(silence_alert_emitted(&env, client.address.clone()));

    // The next accepted submission resumes service: emits ServiceResumedEvent
    // with the gap since the last recorded activity, and clears the flag.
    submit(&env, &client, &wallet, &pair, 20);

    let resumed = find_resumed_event(&env, client.address.clone()).unwrap();
    assert_eq!(resumed.last_active_at, START_TS);
    assert_eq!(resumed.gap_secs, 4_000);

    assert_eq!(client.get_last_service_activity(), START_TS + 4_000);
    assert!(client.is_service_alive());

    // Now that the flag is cleared, an immediate get_score does not re-alert.
    let _ = client.try_get_score(&wallet, &pair);
    assert_eq!(
        count_silence_alerts(&env, client.address.clone()),
        1,
        "only the original silence window's alert should exist"
    );
}

#[test]
fn test_ping_heartbeat_resumes_after_silence() {
    let (env, client, _admin, _service) = setup();
    client.set_heartbeat_alert_threshold(&Vec::new(&env), &100);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 10);

    advance_to(&env, START_TS + 300);
    let _ = client.try_get_score(&wallet, &pair);
    assert!(silence_alert_emitted(&env, client.address.clone()));

    client.ping_heartbeat();

    let resumed = find_resumed_event(&env, client.address.clone()).unwrap();
    assert_eq!(resumed.last_active_at, START_TS);
    assert_eq!(resumed.gap_secs, 300);
    assert_eq!(client.get_last_service_activity(), START_TS + 300);
    assert!(client.is_service_alive());
}

// ── Event-inspection helpers ─────────────────────────────────────────────────

fn find_silence_alert(env: &Env, contract_id: Address) -> Option<ServiceSilenceAlertEvent> {
    let topic = (symbol_short!("svc_sil"),);
    env.events().all().iter().find_map(|(addr, topics, data)| {
        if addr != contract_id || topics != topic.clone().into_val(env) {
            return None;
        }
        Some(data.into_val(env))
    })
}

fn count_silence_alerts(env: &Env, contract_id: Address) -> usize {
    let topic = (symbol_short!("svc_sil"),);
    env.events()
        .all()
        .iter()
        .filter(|(addr, topics, _)| *addr == contract_id && *topics == topic.clone().into_val(env))
        .count()
}

fn silence_alert_emitted(env: &Env, contract_id: Address) -> bool {
    find_silence_alert(env, contract_id).is_some()
}

fn find_resumed_event(env: &Env, contract_id: Address) -> Option<ServiceResumedEvent> {
    let topic = (symbol_short!("svc_res"),);
    env.events().all().iter().find_map(|(addr, topics, data)| {
        if addr != contract_id || topics != topic.clone().into_val(env) {
            return None;
        }
        Some(data.into_val(env))
    })
}
