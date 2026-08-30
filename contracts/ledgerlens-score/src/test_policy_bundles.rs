use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, Env, Vec};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient};

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

// ── Policy bundles for risk threshold + cooldown (issue #788) ─────────────

#[test]
fn test_propose_rejects_invalid_threshold_no_partial_write() {
    let (env, client, _admin, _service) = setup();
    let signers = Vec::new(&env);

    let result = client.try_propose_policy_bundle(&signers, &101, &3_600);
    assert_eq!(result, Err(Ok(Error::InvalidScore)));

    // Neither the proposal nor either underlying field changed.
    assert!(client.try_apply_policy_bundle().is_err());
    assert_eq!(client.get_risk_threshold(), 75);
    assert_eq!(client.get_cooldown(), 3_600);
}

#[test]
fn test_propose_rejects_invalid_cooldown_no_partial_write() {
    let (env, client, _admin, _service) = setup();
    let signers = Vec::new(&env);

    let result = client.try_propose_policy_bundle(&signers, &80, &10);
    assert_eq!(result, Err(Ok(Error::InvalidCooldown)));

    assert!(client.try_apply_policy_bundle().is_err());
    assert_eq!(client.get_risk_threshold(), 75);
    assert_eq!(client.get_cooldown(), 3_600);
}

#[test]
fn test_apply_before_timelock_changes_neither_field() {
    let (env, client, _admin, _service) = setup();
    let signers = Vec::new(&env);

    client.propose_policy_bundle(&signers, &80, &7_200);

    // Too early: apply_after has not elapsed.
    let result = client.try_apply_policy_bundle();
    assert_eq!(result, Err(Ok(Error::UpgradeNotReady)));

    assert_eq!(client.get_risk_threshold(), 75);
    assert_eq!(client.get_cooldown(), 3_600);
}

#[test]
fn test_apply_after_timelock_activates_both_fields_atomically() {
    let (env, client, _admin, _service) = setup();
    let signers = Vec::new(&env);

    client.propose_policy_bundle(&signers, &80, &7_200);
    env.ledger().with_mut(|l| l.timestamp += 86_401);
    client.apply_policy_bundle();

    assert_eq!(client.get_risk_threshold(), 80);
    assert_eq!(client.get_cooldown(), 7_200);

    // Applying again with nothing pending fails cleanly.
    assert_eq!(client.try_apply_policy_bundle(), Err(Ok(Error::NoPendingUpgrade)));
}

#[test]
fn test_propose_while_pending_rejected() {
    let (env, client, _admin, _service) = setup();
    let signers = Vec::new(&env);

    client.propose_policy_bundle(&signers, &80, &7_200);
    let result = client.try_propose_policy_bundle(&signers, &90, &1_800);
    assert_eq!(result, Err(Ok(Error::ParamChangeAlreadyPending)));

    // The original proposal is untouched and still applies as proposed.
    env.ledger().with_mut(|l| l.timestamp += 86_401);
    client.apply_policy_bundle();
    assert_eq!(client.get_risk_threshold(), 80);
    assert_eq!(client.get_cooldown(), 7_200);
}
