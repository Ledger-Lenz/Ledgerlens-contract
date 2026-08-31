#![cfg(test)]
//! Verifies the containment/rotation sequence documented in
//! `docs/runbooks/signer-compromise.md` actually produces the contract
//! states the runbook claims at each decision point.

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient};

const BASE_TS: u64 = 1_700_000_000;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = BASE_TS);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    (env, client, admin)
}

#[test]
fn step_2_pause_stops_the_contract() {
    let (env, client, admin) = setup();
    let admin_signers = Vec::from_array(&env, [admin.clone()]);

    assert!(!client.is_paused(), "contract must start unpaused");
    client.pause(&admin_signers);
    assert!(client.is_paused(), "runbook step 2: pause must contain the contract");
}

#[test]
fn step_3_remove_service_signer_drops_it_from_the_set() {
    let (env, client, admin) = setup();
    let admin_signers = Vec::from_array(&env, [admin.clone()]);
    let compromised = Address::generate(&env);

    client.add_service_signer(&admin_signers, &compromised);
    assert!(client.get_service_signers().contains(&compromised));

    client.remove_service_signer(&admin_signers, &compromised);
    assert!(
        !client.get_service_signers().contains(&compromised),
        "runbook step 3: compromised signer must be removed from the service set"
    );
}

#[test]
fn step_6_recovery_requires_quorum_before_unpause() {
    let (env, client, admin) = setup();
    let admin_signers = Vec::from_array(&env, [admin.clone()]);
    let signer_a = Address::generate(&env);
    let signer_b = Address::generate(&env);

    client.add_service_signer(&admin_signers, &signer_a);
    client.add_service_signer(&admin_signers, &signer_b);
    client.set_service_threshold(&admin_signers, &2);
    client.pause(&admin_signers);

    // Simulate removing a compromised signer that drops the set below quorum;
    // the contract auto-reduces the threshold rather than leaving it
    // unsatisfiable, matching runbook step 6's recovery check
    // (`get_service_signer_count() >= get_service_threshold()`).
    client.remove_service_signer(&admin_signers, &signer_a);
    assert!(
        client.get_service_signer_count() >= client.get_service_threshold(),
        "runbook step 6: signer count must satisfy threshold before unpausing"
    );

    client.unpause(&admin_signers);
    assert!(!client.is_paused(), "runbook step 6: unpause must succeed once quorum holds");
}
