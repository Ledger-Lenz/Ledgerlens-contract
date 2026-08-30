#![cfg(test)]
//! Guards the event-topic names cited in
//! `docs/incident-severity-classification.md` against drift: if an event's
//! wire topic ever changes, the severity doc silently stops matching real
//! alerts. These tests assert the documented topic strings are exactly what
//! the contract emits for each classified action.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, Symbol, TryFromVal, Vec,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient};

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    (env, client, admin)
}

fn last_topic_symbol(env: &Env, contract_id: &Address) -> Symbol {
    let all_events = env.events().all();
    let (_, topics, _) = all_events
        .iter()
        .filter(|(addr, _, _)| addr == contract_id)
        .last()
        .expect("expected at least one contract event");
    Symbol::try_from_val(env, &topics.get(0).unwrap()).expect("first topic must be a Symbol")
}

#[test]
fn pause_emits_the_topic_the_severity_doc_maps_to_sev0() {
    let (env, client, admin) = setup();
    let contract_id = client.address.clone();
    let admin_signers = Vec::from_array(&env, [admin]);

    client.pause(&admin_signers);
    assert_eq!(last_topic_symbol(&env, &contract_id), symbol_short!("paused"));
}

#[test]
fn signer_removal_emits_the_topic_the_severity_doc_maps_to_sev1() {
    let (env, client, admin) = setup();
    let contract_id = client.address.clone();
    let admin_signers = Vec::from_array(&env, [admin]);
    let signer = Address::generate(&env);

    client.add_service_signer(&admin_signers, &signer);
    client.remove_service_signer(&admin_signers, &signer);
    assert_eq!(last_topic_symbol(&env, &contract_id), symbol_short!("sig_rem"));
}

#[test]
fn signer_addition_emits_the_topic_the_severity_doc_maps_to_sev1() {
    let (env, client, admin) = setup();
    let contract_id = client.address.clone();
    let admin_signers = Vec::from_array(&env, [admin]);
    let signer = Address::generate(&env);

    client.add_service_signer(&admin_signers, &signer);
    assert_eq!(last_topic_symbol(&env, &contract_id), symbol_short!("sig_add"));
}
