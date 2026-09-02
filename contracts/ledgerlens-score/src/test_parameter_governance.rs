//! Tests for the time-locked parameter change governance mechanism.

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Bytes, Env, Vec,
};

use crate::{
    constants::{
        DEFAULT_COOLDOWN_SECS, DEFAULT_UPGRADE_DELAY_SECS, MAX_PENDING_PARAMETER_PROPOSALS,
        MIN_COOLDOWN_SECS,
    },
    parameter_governance::param_key_cooldown,
    storage,
    types::ParameterProposalStatus,
    Error, LedgerLensScoreContract, LedgerLensScoreContractClient,
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

fn admin_signers(env: &Env, admin: &Address) -> Vec<Address> {
    Vec::from_array(env, [admin.clone()])
}

fn service_signers(env: &Env, service: &Address) -> Vec<Address> {
    Vec::from_array(env, [service.clone()])
}

fn encode_u64(env: &Env, value: u64) -> Bytes {
    Bytes::from_array(env, &value.to_be_bytes())
}

fn advance_to(env: &Env, ts: u64) {
    env.ledger().with_mut(|l| l.timestamp = ts);
}

#[test]
fn test_proposal_created_time_passes_executed() {
    let (env, client, admin, _service) = setup();
    let new_cooldown = MIN_COOLDOWN_SECS;
    let value = encode_u64(&env, new_cooldown);

    let proposal_id = client.propose_parameter_change(
        &admin_signers(&env, &admin),
        &param_key_cooldown(),
        &value,
    );

    assert_eq!(proposal_id, 1);
    let record = client.get_parameter_proposal(&proposal_id);
    assert_eq!(record.status, ParameterProposalStatus::Pending);
    assert_eq!(record.proposal.proposed_at, START_TS);
    assert_eq!(record.proposal.time_lock_secs, DEFAULT_UPGRADE_DELAY_SECS);

    advance_to(&env, START_TS + DEFAULT_UPGRADE_DELAY_SECS);
    client.execute_parameter_change(&admin_signers(&env, &admin), &proposal_id);

    assert_eq!(client.get_cooldown(), new_cooldown);
    let executed = client.get_parameter_proposal(&proposal_id);
    assert_eq!(executed.status, ParameterProposalStatus::Executed);
}

#[test]
fn test_get_parameter_proposal_nonexistent_id_returns_not_found() {
    let (_env, client, _admin, _service) = setup();

    let result = client.try_get_parameter_proposal(&999u64);
    assert_eq!(result, Err(Ok(Error::ParameterProposalNotFound)));
}

#[test]
fn test_vetoed_proposal_cannot_be_executed() {
    let (env, client, admin, service) = setup();
    let value = encode_u64(&env, MIN_COOLDOWN_SECS);

    let proposal_id = client.propose_parameter_change(
        &admin_signers(&env, &admin),
        &param_key_cooldown(),
        &value,
    );

    client.veto_parameter_change(&service_signers(&env, &service), &proposal_id);

    advance_to(&env, START_TS + DEFAULT_UPGRADE_DELAY_SECS);
    let result = client.try_execute_parameter_change(&admin_signers(&env, &admin), &proposal_id);
    assert_eq!(result, Err(Ok(Error::ParameterProposalVetoed)));
    assert_eq!(client.get_cooldown(), DEFAULT_COOLDOWN_SECS);
}

#[test]
fn test_execute_before_timelock_rejected() {
    let (env, client, admin, _service) = setup();
    let value = encode_u64(&env, MIN_COOLDOWN_SECS);

    let proposal_id = client.propose_parameter_change(
        &admin_signers(&env, &admin),
        &param_key_cooldown(),
        &value,
    );

    let result = client.try_execute_parameter_change(&admin_signers(&env, &admin), &proposal_id);
    assert_eq!(result, Err(Ok(Error::ParameterProposalNotReady)));

    advance_to(&env, START_TS + DEFAULT_UPGRADE_DELAY_SECS - 1);
    let result = client.try_execute_parameter_change(&admin_signers(&env, &admin), &proposal_id);
    assert_eq!(result, Err(Ok(Error::ParameterProposalNotReady)));
}

#[test]
fn test_maximum_pending_proposals_cap() {
    let (env, client, admin, _service) = setup();
    let value = encode_u64(&env, MIN_COOLDOWN_SECS);

    env.as_contract(&client.address, || {
        storage::test_seed_pending_parameter_proposals(
            &env,
            MAX_PENDING_PARAMETER_PROPOSALS,
            &admin,
            &param_key_cooldown(),
            &value,
        );
    });

    let result = client.try_propose_parameter_change(
        &admin_signers(&env, &admin),
        &param_key_cooldown(),
        &value,
    );
    assert_eq!(result, Err(Ok(Error::TooManyPendingParameterProposals)));
}

#[test]
fn test_veto_after_half_timelock_rejected() {
    let (env, client, admin, service) = setup();
    let value = encode_u64(&env, MIN_COOLDOWN_SECS);

    let proposal_id = client.propose_parameter_change(
        &admin_signers(&env, &admin),
        &param_key_cooldown(),
        &value,
    );

    let veto_deadline = START_TS + DEFAULT_UPGRADE_DELAY_SECS / 2;
    advance_to(&env, veto_deadline + 1);

    let result = client.try_veto_parameter_change(&service_signers(&env, &service), &proposal_id);
    assert_eq!(result, Err(Ok(Error::ParameterProposalVetoPeriodEnded)));
}

#[test]
fn test_expired_proposal_cannot_execute() {
    let (env, client, admin, _service) = setup();
    let value = encode_u64(&env, MIN_COOLDOWN_SECS);

    let proposal_id = client.propose_parameter_change(
        &admin_signers(&env, &admin),
        &param_key_cooldown(),
        &value,
    );

    let expiry = START_TS + DEFAULT_UPGRADE_DELAY_SECS * 2 + 1;
    advance_to(&env, expiry);

    let result = client.try_execute_parameter_change(&admin_signers(&env, &admin), &proposal_id);
    assert_eq!(result, Err(Ok(Error::ParameterProposalExpired)));

    env.as_contract(&client.address, || {
        storage::mark_parameter_proposal_status(
            &env,
            proposal_id,
            ParameterProposalStatus::Expired,
        );
    });

    let record = client.get_parameter_proposal(&proposal_id);
    assert_eq!(record.status, ParameterProposalStatus::Expired);
}

#[test]
fn test_executed_proposal_cannot_be_reexecuted() {
    let (env, client, admin, _service) = setup();
    let value = encode_u64(&env, MIN_COOLDOWN_SECS);

    let proposal_id = client.propose_parameter_change(
        &admin_signers(&env, &admin),
        &param_key_cooldown(),
        &value,
    );

    advance_to(&env, START_TS + DEFAULT_UPGRADE_DELAY_SECS);
    client.execute_parameter_change(&admin_signers(&env, &admin), &proposal_id);

    let result = client.try_execute_parameter_change(&admin_signers(&env, &admin), &proposal_id);
    assert_eq!(result, Err(Ok(Error::ParameterProposalAlreadyExecuted)));
}

#[test]
fn test_veto_before_half_timelock_succeeds() {
    let (env, client, admin, service) = setup();
    let value = encode_u64(&env, MIN_COOLDOWN_SECS);

    let proposal_id = client.propose_parameter_change(
        &admin_signers(&env, &admin),
        &param_key_cooldown(),
        &value,
    );

    client.veto_parameter_change(&service_signers(&env, &service), &proposal_id);

    let record = client.get_parameter_proposal(&proposal_id);
    assert_eq!(record.status, ParameterProposalStatus::Vetoed);
    assert!(client.get_pending_param_prop_ids().is_empty());
}

#[test]
fn test_executed_proposal_removed_from_pending_index() {
    let (env, client, admin, _service) = setup();
    let value = encode_u64(&env, MIN_COOLDOWN_SECS);

    let proposal_id = client.propose_parameter_change(
        &admin_signers(&env, &admin),
        &param_key_cooldown(),
        &value,
    );
    assert_eq!(client.get_pending_param_prop_ids(), Vec::from_array(&env, [proposal_id]));

    advance_to(&env, START_TS + DEFAULT_UPGRADE_DELAY_SECS);
    client.execute_parameter_change(&admin_signers(&env, &admin), &proposal_id);

    assert!(client.get_pending_param_prop_ids().is_empty());
}

/// `propose_parameter_change` must return `Error::InvalidParameterKey`
/// (aliased as `Error::InvalidThreshold` in errors.rs) when the caller
/// supplies a `param_key` that does not map to any known parameter.
/// The existing tests only exercise valid keys (`cooldown`); this test
/// covers the unknown-key rejection path in
/// `parameter_governance::validate_parameter_value`.
#[test]
fn test_propose_parameter_change_unknown_key_returns_invalid_parameter_key() {
    use soroban_sdk::symbol_short;

    let (env, client, admin, _service) = setup();
    let value = encode_u64(&env, 3600);

    // "unknown" is not one of the five recognised keys.
    let result = client.try_propose_parameter_change(
        &admin_signers(&env, &admin),
        &symbol_short!("unknown"),
        &value,
    );

    // InvalidParameterKey is an alias for Error::InvalidThreshold (discriminant 16).
    assert_eq!(result, Err(Ok(Error::InvalidThreshold)));
}

/// `veto_parameter_change` must return `Error::ParameterProposalVetoed` when
/// the same proposal is vetoed a second time (double-veto). The first call
/// marks the record `Vetoed`; the second call must detect that status and
/// return the error rather than panicking or silently succeeding.
/// The existing tests only veto once and then try to *execute*; this test
/// covers the double-veto path inside `veto_parameter_change` itself.
#[test]
fn test_veto_parameter_change_double_veto_returns_vetoed_error() {
    let (env, client, admin, service) = setup();
    let value = encode_u64(&env, MIN_COOLDOWN_SECS);

    let proposal_id = client.propose_parameter_change(
        &admin_signers(&env, &admin),
        &param_key_cooldown(),
        &value,
    );

    // First veto — must succeed.
    client.veto_parameter_change(&service_signers(&env, &service), &proposal_id);

    // Second veto on the same proposal — must return ParameterProposalVetoed.
    let result =
        client.try_veto_parameter_change(&service_signers(&env, &service), &proposal_id);
    assert_eq!(result, Err(Ok(Error::ParameterProposalVetoed)));
}

#[test]
fn test_expired_full_pending_set_is_pruned_before_accepting_new_proposal() {
    let (env, client, admin, _service) = setup();
    let value = encode_u64(&env, MIN_COOLDOWN_SECS);

    env.as_contract(&client.address, || {
        storage::test_seed_pending_parameter_proposals(
            &env,
            MAX_PENDING_PARAMETER_PROPOSALS,
            &admin,
            &param_key_cooldown(),
            &value,
        );
    });
    assert_eq!(client.get_pending_param_prop_ids().len(), MAX_PENDING_PARAMETER_PROPOSALS);

    advance_to(&env, START_TS + DEFAULT_UPGRADE_DELAY_SECS * 2 + 1);

    let proposal_id = client.propose_parameter_change(
        &admin_signers(&env, &admin),
        &param_key_cooldown(),
        &value,
    );

    assert_eq!(proposal_id, MAX_PENDING_PARAMETER_PROPOSALS as u64 + 1);
    assert_eq!(client.get_pending_param_prop_ids(), Vec::from_array(&env, [proposal_id]));
    for expired_id in 1..=MAX_PENDING_PARAMETER_PROPOSALS as u64 {
        let record = client.get_parameter_proposal(&expired_id);
        assert_eq!(record.status, ParameterProposalStatus::Expired);
    }
}

// ── get_pending_param_prop_ids edge-case tests ─────────────────────────────

#[test]
fn test_get_pending_param_prop_ids_uninitialized() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    // Calling before contract initialize() should safely return empty Vec
    let pending = client.get_pending_param_prop_ids();
    assert_eq!(pending, Vec::new(&env));
    assert!(pending.is_empty());
}

#[test]
fn test_get_pending_param_prop_ids_multiple_interleaved_lifecycle() {
    let (env, client, admin, service) = setup();
    let value1 = encode_u64(&env, MIN_COOLDOWN_SECS);
    let value2 = encode_u64(&env, MIN_COOLDOWN_SECS + 10);
    let value3 = encode_u64(&env, MIN_COOLDOWN_SECS + 20);

    // Create three proposals in sequence
    let p1 = client.propose_parameter_change(
        &admin_signers(&env, &admin),
        &param_key_cooldown(),
        &value1,
    );
    let p2 = client.propose_parameter_change(
        &admin_signers(&env, &admin),
        &param_key_cooldown(),
        &value2,
    );
    let p3 = client.propose_parameter_change(
        &admin_signers(&env, &admin),
        &param_key_cooldown(),
        &value3,
    );

    // Verify all three are pending in FIFO order
    assert_eq!(
        client.get_pending_param_prop_ids(),
        Vec::from_array(&env, [p1, p2, p3])
    );

    // Veto the middle proposal (p2)
    client.veto_parameter_change(&service_signers(&env, &service), &p2);

    // Verify p2 is removed and p1, p3 remain in pending list preserving order
    assert_eq!(
        client.get_pending_param_prop_ids(),
        Vec::from_array(&env, [p1, p3])
    );

    // Execute p1 after timelock
    advance_to(&env, START_TS + DEFAULT_UPGRADE_DELAY_SECS);
    client.execute_parameter_change(&admin_signers(&env, &admin), &p1);

    // Verify only p3 remains
    assert_eq!(
        client.get_pending_param_prop_ids(),
        Vec::from_array(&env, [p3])
    );

    // Execute p3
    client.execute_parameter_change(&admin_signers(&env, &admin), &p3);

    // Verify pending list is completely empty
    assert!(client.get_pending_param_prop_ids().is_empty());
}

#[test]
fn test_execute_nonexistent_proposal_returns_not_found() {
    // Attempt to execute a proposal_id that was never created.
    // The storage lookup returns None, so execute_parameter_change must
    // return Error::ParameterProposalNotFound (= Error::ScoreNotFound).
    // This is the only execute_parameter_change failure path not already
    // covered by the tests above.
    let (env, client, admin, _service) = setup();

    // Advance past any time-lock so the call is not blocked by NotReady —
    // the not-found guard is reached before the time checks.
    advance_to(&env, START_TS + DEFAULT_UPGRADE_DELAY_SECS);

    let bogus_id: u64 = 9_999;
    let result = client.try_execute_parameter_change(&admin_signers(&env, &admin), &bogus_id);
    assert_eq!(result, Err(Ok(Error::ParameterProposalNotFound)));
}
