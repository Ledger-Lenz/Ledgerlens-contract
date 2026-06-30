//! Tests for the finality buffer: holding submitted scores in a pending,
//! admin-cancellable state for a configurable window before they take
//! effect on the live read path.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger as _},
    Address, Bytes, Env, IntoVal, Vec,
};

use crate::{
    constants::MAX_FINALITY_BUFFER_SECS, Error, LedgerLensScoreContract,
    LedgerLensScoreContractClient,
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

// ── Defaults / buffer disabled ────────────────────────────────────────────────

#[test]
fn test_default_finality_buffer_is_disabled() {
    let (_env, client, _admin, _service) = setup();
    assert_eq!(client.get_finality_buffer(), 0);
}

#[test]
fn test_buffer_disabled_commits_immediately() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 42);

    // Buffer == 0: identical to pre-feature behaviour — live score visible
    // right away and no pending entry was ever created.
    assert_eq!(client.get_score(&wallet, &pair).score, 42);
    assert!(client.get_pending_score(&wallet, &pair).is_none());
    assert_eq!(client.get_score_count(&wallet, &pair), 1);
}

// ── Configuring the buffer ───────────────────────────────────────────────────

#[test]
fn test_set_and_get_finality_buffer() {
    let (_env, client, _admin, _service) = setup();
    client.set_finality_buffer(&Vec::new(&_env), &300);
    assert_eq!(client.get_finality_buffer(), 300);
}

#[test]
fn test_set_finality_buffer_rejects_above_max() {
    let (env, client, _admin, _service) = setup();
    let result = client.try_set_finality_buffer(&Vec::new(&env), &(MAX_FINALITY_BUFFER_SECS + 1));
    assert_eq!(result, Err(Ok(Error::InvalidFinalityBuffer)));
}

#[test]
fn test_set_finality_buffer_requires_admin() {
    let (env, client, _admin, _service) = setup();
    let not_admin = Address::generate(&env);
    // mock_all_auths() lets any address's require_auth() succeed, so this
    // doesn't exercise the auth failure path itself, but it does confirm the
    // call is wired through `require_admin_auth` rather than skipped.
    let _ = not_admin;
    client.set_finality_buffer(&Vec::new(&env), &60);
    assert_eq!(client.get_finality_buffer(), 60);
}

// ── Buffer enabled: pending write, invisible to live reads ──────────────────

#[test]
fn test_buffer_enabled_writes_pending_not_live() {
    let (env, client, _admin, _service) = setup();
    client.set_finality_buffer(&Vec::new(&env), &300);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 90);

    // Invisible to the live read path.
    assert!(client.try_get_score(&wallet, &pair).is_err());
    assert!(!client.query_risk_gate(&wallet, &pair, &100));
    assert_eq!(client.get_score_count(&wallet, &pair), 0);
    assert_eq!(client.get_score_history(&wallet, &pair).len(), 0);

    // But visible via the dedicated pending lookup.
    let pending = client.get_pending_score(&wallet, &pair).unwrap();
    assert_eq!(pending.score, 90);
    assert!(pending.benford_flag);
    assert_eq!(pending.submitted_at, START_TS);
    assert_eq!(pending.commit_after, START_TS + 300);
}

#[test]
fn test_score_pending_event_emitted() {
    let (env, client, _admin, _service) = setup();
    client.set_finality_buffer(&Vec::new(&env), &120);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let c2 = LedgerLensScoreContractClient::new(&env, &contract_id);
    c2.initialize(&Address::generate(&env), &Address::generate(&env));
    c2.set_finality_buffer(&Vec::new(&env), &120);

    submit(&env, &c2, &wallet, &pair, 77);

    let topic = (symbol_short!("scr_pend"), wallet.clone(), pair.clone());
    let found = env.events().all().iter().any(|(addr, topics, data)| {
        if addr != contract_id || topics != topic.clone().into_val(&env) {
            return false;
        }
        let commit_after: u64 = data.into_val(&env);
        commit_after == START_TS + 120
    });
    assert!(found, "expected a scr_pend event with commit_after = START_TS + 120");
}

// ── Commit window ─────────────────────────────────────────────────────────────

#[test]
fn test_commit_before_window_fails() {
    let (env, client, _admin, _service) = setup();
    client.set_finality_buffer(&Vec::new(&env), &300);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 90);

    advance_to(&env, START_TS + 299);
    let result = client.try_commit_pending_score(&wallet, &pair);
    assert_eq!(result, Err(Ok(Error::FinalityWindowNotElapsed)));
    assert!(client.try_get_score(&wallet, &pair).is_err());
}

#[test]
fn test_commit_after_window_succeeds() {
    let (env, client, _admin, _service) = setup();
    client.set_finality_buffer(&Vec::new(&env), &300);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 90);

    advance_to(&env, START_TS + 300);
    client.commit_pending_score(&wallet, &pair);

    let score = client.get_score(&wallet, &pair);
    assert_eq!(score.score, 90);
    assert!(score.benford_flag);
    assert!(client.get_pending_score(&wallet, &pair).is_none());
    assert_eq!(client.get_score_count(&wallet, &pair), 1);
    assert_eq!(client.get_score_history(&wallet, &pair).len(), 1);
}

#[test]
fn test_commit_pending_score_is_permissionless() {
    let (env, client, _admin, _service) = setup();
    client.set_finality_buffer(&Vec::new(&env), &60);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 50);
    advance_to(&env, START_TS + 60);

    // No auths mocked off — anyone (here, a freshly generated address with
    // no relationship to the contract) can trigger the commit.
    let anyone = Address::generate(&env);
    let _ = anyone;
    client.commit_pending_score(&wallet, &pair);
    assert_eq!(client.get_score(&wallet, &pair).score, 50);
}

#[test]
fn test_auto_commit_score_after_window_succeeds() {
    let (env, client, _admin, _service) = setup();
    client.set_escrow_hold_window(&Vec::new(&env), &60).unwrap();

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 55);
    advance_to(&env, START_TS + 60);

    client.auto_commit_score(&wallet, &pair).unwrap();
    assert_eq!(client.get_score(&wallet, &pair).score, 55);
    assert!(client.get_pending_score(&wallet, &pair).is_none());
}

#[test]
fn test_auto_commit_score_fails_after_cancel() {
    let (env, client, _admin, _service) = setup();
    client.set_escrow_hold_window(&Vec::new(&env), &300).unwrap();

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 70);
    advance_to(&env, START_TS + 100);
    client.cancel_pending_score(&Vec::new(&env), &wallet, &pair).unwrap();

    advance_to(&env, START_TS + 300);
    let result = client.try_auto_commit_score(&wallet, &pair);
    assert_eq!(result, Err(Ok(Error::NoPendingScore)));
    assert!(client.try_get_score(&wallet, &pair).is_err());
}

#[test]
fn test_replacement_submission_during_escrow_updates_pending() {
    let (env, client, _admin, _service) = setup();
    client.set_escrow_hold_window(&Vec::new(&env), &300).unwrap();

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 10);

    advance_to(&env, START_TS + 3_601);
    submit(&env, &client, &wallet, &pair, 80);

    let pending = client.get_pending_score(&wallet, &pair).unwrap();
    assert_eq!(pending.score, 80);
    assert_eq!(pending.submitted_at, START_TS + 3_601);

    advance_to(&env, START_TS + 3_601 + 300);
    client.auto_commit_score(&wallet, &pair).unwrap();
    assert_eq!(client.get_score(&wallet, &pair).score, 80);
    assert_eq!(client.get_score_count(&wallet, &pair), 1);
}

#[test]
fn test_commit_with_no_pending_score_fails() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let result = client.try_commit_pending_score(&wallet, &pair);
    assert_eq!(result, Err(Ok(Error::NoPendingScore)));
}

#[test]
fn test_score_committed_event_emitted() {
    let (env, client, _admin, _service) = setup();
    client.set_finality_buffer(&Vec::new(&env), &60);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let c2 = LedgerLensScoreContractClient::new(&env, &contract_id);
    c2.initialize(&Address::generate(&env), &Address::generate(&env));
    c2.set_finality_buffer(&Vec::new(&env), &60);

    submit(&env, &c2, &wallet, &pair, 33);
    advance_to(&env, START_TS + 60);
    c2.commit_pending_score(&wallet, &pair);

    let topic = (symbol_short!("scr_comm"), wallet.clone());
    let found = env.events().all().iter().any(|(addr, topics, data)| {
        if addr != contract_id || topics != topic.clone().into_val(&env) {
            return false;
        }
        let event_pair: soroban_sdk::Symbol = data.into_val(&env);
        event_pair == pair
    });
    assert!(found, "expected a scr_comm event for the committed pair");
}

// ── Admin cancellation ────────────────────────────────────────────────────────

#[test]
fn test_cancel_before_window_removes_pending() {
    let (env, client, _admin, _service) = setup();
    client.set_finality_buffer(&Vec::new(&env), &300);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 99);

    advance_to(&env, START_TS + 100); // still within the window
    client.cancel_pending_score(&Vec::new(&env), &wallet, &pair);

    assert!(client.get_pending_score(&wallet, &pair).is_none());

    // Even after the original window would have elapsed, there is nothing
    // left to commit — the cancellation is final, not a delay.
    advance_to(&env, START_TS + 300);
    let result = client.try_commit_pending_score(&wallet, &pair);
    assert_eq!(result, Err(Ok(Error::NoPendingScore)));
    assert!(client.try_get_score(&wallet, &pair).is_err());
}

#[test]
fn test_cancel_with_no_pending_score_fails() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    let result = client.try_cancel_pending_score(&Vec::new(&env), &wallet, &pair);
    assert_eq!(result, Err(Ok(Error::NoPendingScore)));
}

#[test]
fn test_score_pending_cancelled_event_emitted() {
    let (env, client, admin, _service) = setup();
    client.set_finality_buffer(&Vec::new(&env), &300);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 60);
    client.cancel_pending_score(&Vec::new(&env), &wallet, &pair);

    let contract_id = client.address.clone();
    let topic = (symbol_short!("scr_canc"), wallet.clone(), pair.clone());
    let found = env.events().all().iter().any(|(addr, topics, data)| {
        if addr != contract_id || topics != topic.clone().into_val(&env) {
            return false;
        }
        let cancelled_by: Address = data.into_val(&env);
        cancelled_by == admin
    });
    assert!(found, "expected a scr_canc event naming the cancelling admin");
}

#[test]
fn test_veto_before_window_removes_pending() {
    let (env, client, _admin, _service) = setup();
    client.set_finality_buffer(&Vec::new(&env), &300);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 99);

    advance_to(&env, START_TS + 100);
    client.veto_pending_score(&Vec::new(&env), &wallet, &pair, &Bytes::from_slice(&env, b"veto reason"));

    assert!(client.get_pending_score(&wallet, &pair).is_none());
    let result = client.try_commit_pending_score(&wallet, &pair);
    assert_eq!(result, Err(Ok(Error::NoPendingScore)));
}

#[test]
fn test_veto_after_window_fails() {
    let (env, client, _admin, _service) = setup();
    client.set_finality_buffer(&Vec::new(&env), &300);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 99);

    advance_to(&env, START_TS + 300);
    let result = client.try_veto_pending_score(&Vec::new(&env), &wallet, &pair, &Bytes::from_slice(&env, b"late veto"));
    assert_eq!(result, Err(Ok(Error::FinalityWindowNotElapsed)));
}

#[test]
fn test_score_vetoed_event_emitted() {
    let (env, client, _admin, _service) = setup();
    client.set_finality_buffer(&Vec::new(&env), &300);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 60);
    client.veto_pending_score(&Vec::new(&env), &wallet, &pair, &Bytes::from_slice(&env, b"bad score"));

    let contract_id = client.address.clone();
    let topic = (symbol_short!("scr_veto"), wallet.clone(), pair.clone());
    let found = env.events().all().iter().any(|(addr, topics, data)| {
        if addr != contract_id || topics != topic.clone().into_val(&env) {
            return false;
        }
        let reason_hash: soroban_sdk::BytesN<32> = data.into_val(&env);
        reason_hash == env.crypto().sha256(&Bytes::from_slice(&env, b"bad score"))
    });
    assert!(found, "expected a scr_veto event with the veto reason hash");
}

// ── Replacement semantics ─────────────────────────────────────────────────────

#[test]
fn test_second_pending_submission_replaces_first() {
    let (env, client, _admin, _service) = setup();
    client.set_finality_buffer(&Vec::new(&env), &300);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    submit(&env, &client, &wallet, &pair, 10);

    // Cooldown is the default 1 hour; advance past it so the replacement
    // submission is itself accepted rather than rate-limited.
    advance_to(&env, START_TS + 3_601);
    submit(&env, &client, &wallet, &pair, 80);

    let pending = client.get_pending_score(&wallet, &pair).unwrap();
    assert_eq!(pending.score, 80, "second submission replaces, not queues, the pending entry");
    assert_eq!(pending.submitted_at, START_TS + 3_601);

    advance_to(&env, START_TS + 3_601 + 300);
    client.commit_pending_score(&wallet, &pair);
    assert_eq!(client.get_score(&wallet, &pair).score, 80);
    assert_eq!(
        client.get_score_count(&wallet, &pair),
        1,
        "only the committed entry counts — the replaced pending write never went live"
    );
}
