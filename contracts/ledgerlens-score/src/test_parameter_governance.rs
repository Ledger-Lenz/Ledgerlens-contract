//! Tests for the time-locked parameter change governance mechanism.

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Bytes, Env, Vec,
};

use crate::{
    constants::{DEFAULT_COOLDOWN_SECS, DEFAULT_UPGRADE_DELAY_SECS, MAX_PENDING_PARAMETER_PROPOSALS,
                MIN_COOLDOWN_SECS},
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

    let result =
        client.try_execute_parameter_change(&admin_signers(&env, &admin), &proposal_id);
    assert_eq!(result, Err(Ok(Error::ParameterProposalNotReady)));

    advance_to(&env, START_TS + DEFAULT_UPGRADE_DELAY_SECS - 1);
    let result =
        client.try_execute_parameter_change(&admin_signers(&env, &admin), &proposal_id);
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

    let result =
        client.try_veto_parameter_change(&service_signers(&env, &service), &proposal_id);
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

    let result =
        client.try_execute_parameter_change(&admin_signers(&env, &admin), &proposal_id);
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

    let result =
        client.try_execute_parameter_change(&admin_signers(&env, &admin), &proposal_id);
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
