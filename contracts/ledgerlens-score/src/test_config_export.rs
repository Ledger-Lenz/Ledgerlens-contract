#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Bytes, Env, Vec,
};

use crate::{
    parameter_governance::param_key_cooldown, LedgerLensScoreContract, LedgerLensScoreContractClient,
};

const START_TS: u64 = 1_700_000_000;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    let approver = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client, admin, service, approver)
}

fn encode_u64(env: &Env, value: u64) -> Bytes {
    Bytes::from_array(env, &value.to_be_bytes())
}

#[test]
fn test_export_configuration_is_deterministic_for_same_state() {
    let (env, client, _admin, _service, approver) = setup();
    client.set_global_min_confidence(&70);
    client.set_deletion_approval_policy(&Vec::new(&env), &true, &Some(approver));

    let first = client.export_configuration();
    let second = client.export_configuration();

    assert_eq!(first, second);
    assert_eq!(first.schema_version, 1);
    assert_eq!(first.omitted_secret_rationale.len(), 2);
}

#[test]
fn test_export_configuration_hash_changes_when_active_config_changes() {
    let (_env, client, _admin, _service, _approver) = setup();
    let before = client.export_configuration();

    client.set_global_min_confidence(&55);
    let after = client.export_configuration();

    assert_ne!(before.active_hash, after.active_hash);
    assert_ne!(before.export_hash, after.export_hash);
}

#[test]
fn test_export_configuration_includes_pending_parameter_change() {
    let (env, client, admin, _service, _approver) = setup();
    let proposal_id = client.propose_parameter_change(
        &Vec::from_array(&env, [admin]),
        &param_key_cooldown(),
        &encode_u64(&env, 120),
    );

    let exported = client.export_configuration();
    let mut found = false;
    for i in 0..exported.pending_values.len() {
        let entry = exported.pending_values.get(i).unwrap();
        if entry.proposal_id == proposal_id && entry.key == param_key_cooldown() {
            found = true;
            break;
        }
    }
    assert!(found);
}
