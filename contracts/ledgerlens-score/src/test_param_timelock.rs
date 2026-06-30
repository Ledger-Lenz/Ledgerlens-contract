#![cfg(test)]

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient, ParamValue};

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

// ── set_param_change_delay / get_param_change_delay ──────────────────────────

#[test]
fn test_default_param_change_delay() {
    let (_env, client, _admin, _service) = setup();
    // Default is 24 hours.
    assert_eq!(client.get_param_change_delay(), 86_400);
}

#[test]
fn test_set_param_change_delay_proposes() {
    let (env, client, _admin, _service) = setup();
    // The meta-setter itself is time-locked.
    client.set_param_change_delay(&Vec::new(&env), &7_200);

    let proposal = client.get_pending_param_change(&symbol_short!("pc_delay"));
    assert_eq!(proposal.new_value, ParamValue::U64(7_200));
    // Delay not yet applied.
    assert_eq!(client.get_param_change_delay(), 86_400);
}

#[test]
fn test_set_param_change_delay_invalid_too_low() {
    let (env, client, _admin, _service) = setup();
    // Below MIN_PARAM_CHANGE_DELAY_SECS (3600).
    let result = client.try_set_param_change_delay(&Vec::new(&env), &3_599);
    assert_eq!(result, Err(Ok(Error::InvalidParamChangeDelay)));
}

#[test]
fn test_set_param_change_delay_invalid_too_high() {
    let (env, client, _admin, _service) = setup();
    // Above MAX_PARAM_CHANGE_DELAY_SECS (604800).
    let result = client.try_set_param_change_delay(&Vec::new(&env), &604_801);
    assert_eq!(result, Err(Ok(Error::InvalidParamChangeDelay)));
}

// ── apply_param_change — early rejection ─────────────────────────────────────

#[test]
fn test_apply_before_delay_fails() {
    let (env, client, _admin, _service) = setup();
    client.set_risk_threshold(&Vec::new(&env), &80);
    // Advance time but not past the full 24-hour delay.
    env.ledger().with_mut(|l| l.timestamp += 86_399);
    let result = client.try_apply_param_change(&symbol_short!("risk_thr"));
    assert_eq!(result, Err(Ok(Error::ParamChangeNotReady)));
    // Active threshold is still the default.
    assert_eq!(client.get_risk_threshold(), 75);
}

#[test]
fn test_apply_no_proposal_fails() {
    let (_env, client, _admin, _service) = setup();
    let result = client.try_apply_param_change(&symbol_short!("risk_thr"));
    assert_eq!(result, Err(Ok(Error::NoPendingParamChange)));
}

// ── apply_param_change — success after delay ─────────────────────────────────

#[test]
fn test_apply_risk_threshold_after_delay() {
    let (env, client, _admin, _service) = setup();
    client.set_risk_threshold(&Vec::new(&env), &80);
    env.ledger().with_mut(|l| l.timestamp += 86_400);
    client.apply_param_change(&symbol_short!("risk_thr"));
    assert_eq!(client.get_risk_threshold(), 80);
    // Proposal is cleared.
    let result = client.try_get_pending_param_change(&symbol_short!("risk_thr"));
    assert_eq!(result, Err(Ok(Error::NoPendingParamChange)));
}

#[test]
fn test_apply_cooldown_after_delay() {
    let (env, client, _admin, _service) = setup();
    client.set_cooldown(&Vec::new(&env), &120);
    env.ledger().with_mut(|l| l.timestamp += 86_400);
    client.apply_param_change(&symbol_short!("cooldown"));
    assert_eq!(client.get_cooldown(), 120);
}

#[test]
fn test_apply_staleness_window_after_delay() {
    let (env, client, _admin, _service) = setup();
    client.set_staleness_window(&Vec::new(&env), &3_600);
    env.ledger().with_mut(|l| l.timestamp += 86_400);
    client.apply_param_change(&symbol_short!("stale_w"));
    assert_eq!(client.get_staleness_window(), 3_600);
}

#[test]
fn test_apply_upgrade_delay_after_delay() {
    let (env, client, _admin, _service) = setup();
    // MIN_UPGRADE_DELAY_SECS = 172_800 (48h).
    client.set_upgrade_delay(&Vec::new(&env), &172_800);
    env.ledger().with_mut(|l| l.timestamp += 86_400);
    client.apply_param_change(&symbol_short!("upg_dly"));
    assert_eq!(client.get_upgrade_delay(), 172_800);
}

#[test]
fn test_apply_history_depth_after_delay() {
    let (env, client, _admin, _service) = setup();
    client.set_history_max_depth(&Vec::new(&env), &20);
    env.ledger().with_mut(|l| l.timestamp += 86_400);
    client.apply_param_change(&symbol_short!("hist_dep"));
    assert_eq!(client.get_history_max_depth(), 20);
}

#[test]
fn test_apply_param_change_delay_after_delay() {
    let (env, client, _admin, _service) = setup();
    client.set_param_change_delay(&Vec::new(&env), &7_200);
    env.ledger().with_mut(|l| l.timestamp += 86_400);
    client.apply_param_change(&symbol_short!("pc_delay"));
    assert_eq!(client.get_param_change_delay(), 7_200);
}

// ── apply_param_change callable by anyone ────────────────────────────────────

#[test]
fn test_apply_callable_by_non_admin() {
    let (env, client, _admin, _service) = setup();
    client.set_risk_threshold(&Vec::new(&env), &60);
    env.ledger().with_mut(|l| l.timestamp += 86_400);
    // apply_param_change has no admin auth — anyone can trigger it.
    client.apply_param_change(&symbol_short!("risk_thr"));
    assert_eq!(client.get_risk_threshold(), 60);
}

// ── cancel_param_change ───────────────────────────────────────────────────────

#[test]
fn test_cancel_param_change() {
    let (env, client, _admin, _service) = setup();
    client.set_risk_threshold(&Vec::new(&env), &80);
    client.cancel_param_change(&Vec::new(&env), &symbol_short!("risk_thr"));
    let result = client.try_get_pending_param_change(&symbol_short!("risk_thr"));
    assert_eq!(result, Err(Ok(Error::NoPendingParamChange)));
    // After cancel the live value is unchanged.
    assert_eq!(client.get_risk_threshold(), 75);
}

#[test]
fn test_cancel_no_proposal_fails() {
    let (env, client, _admin, _service) = setup();
    let result = client.try_cancel_param_change(&Vec::new(&env), &symbol_short!("risk_thr"));
    assert_eq!(result, Err(Ok(Error::NoPendingParamChange)));
}

#[test]
fn test_cancel_allows_new_proposal() {
    let (env, client, _admin, _service) = setup();
    client.set_risk_threshold(&Vec::new(&env), &80);
    client.cancel_param_change(&Vec::new(&env), &symbol_short!("risk_thr"));
    // Should succeed now that the slot is free.
    client.set_risk_threshold(&Vec::new(&env), &90);
    let proposal = client.get_pending_param_change(&symbol_short!("risk_thr"));
    assert_eq!(proposal.new_value, ParamValue::U32(90));
}

// ── duplicate proposal guard ──────────────────────────────────────────────────

#[test]
fn test_param_change_already_pending() {
    let (env, client, _admin, _service) = setup();
    client.set_risk_threshold(&Vec::new(&env), &80);
    let result = client.try_set_risk_threshold(&Vec::new(&env), &90);
    assert_eq!(result, Err(Ok(Error::ParamChangeAlreadyPending)));
}

// ── get_pending_param_change ──────────────────────────────────────────────────

#[test]
fn test_get_pending_param_change_fields() {
    let (env, client, _admin, _service) = setup();
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);
    client.set_risk_threshold(&Vec::new(&env), &80);
    let proposal = client.get_pending_param_change(&symbol_short!("risk_thr"));
    assert_eq!(proposal.new_value, ParamValue::U32(80));
    assert_eq!(proposal.proposed_at, 1_000_000);
    // apply_after = proposed_at + 24h
    assert_eq!(proposal.apply_after, 1_000_000 + 86_400);
}

// ── validation preserved at proposal time ────────────────────────────────────

#[test]
fn test_invalid_risk_threshold_rejected_at_proposal() {
    let (env, client, _admin, _service) = setup();
    let result = client.try_set_risk_threshold(&Vec::new(&env), &101);
    assert_eq!(result, Err(Ok(Error::InvalidScore)));
}

#[test]
fn test_invalid_cooldown_rejected_at_proposal() {
    let (env, client, _admin, _service) = setup();
    // Below MIN_COOLDOWN_SECS (60).
    let result = client.try_set_cooldown(&Vec::new(&env), &59);
    assert_eq!(result, Err(Ok(Error::InvalidCooldown)));
}

#[test]
fn test_zero_staleness_window_rejected_at_proposal() {
    let (env, client, _admin, _service) = setup();
    let result = client.try_set_staleness_window(&Vec::new(&env), &0);
    assert_eq!(result, Err(Ok(Error::InvalidStalenessWindow)));
}

#[test]
fn test_invalid_history_depth_rejected_at_proposal() {
    let (env, client, _admin, _service) = setup();
    let result = client.try_set_history_max_depth(&Vec::new(&env), &0);
    assert_eq!(result, Err(Ok(Error::InvalidHistoryDepth)));
}
