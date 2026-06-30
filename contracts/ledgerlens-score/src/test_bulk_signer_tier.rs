use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

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

fn add_signer(env: &Env, client: &LedgerLensScoreContractClient, signer: &Address) {
    client.add_service_signer(&Vec::new(env), signer);
}

#[test]
fn test_bulk_set_signer_tier_updates_all() {
    let (env, client, _, _) = setup();
    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    add_signer(&env, &client, &s1);
    add_signer(&env, &client, &s2);
    let mut entries = Vec::new(&env);
    entries.push_back((s1.clone(), 10u32, 80u32));
    entries.push_back((s2.clone(), 20u32, 90u32));
    client.bulk_set_signer_tier(&Vec::new(&env), &entries).unwrap();
}

#[test]
fn test_bulk_set_signer_tier_min_gt_max_rejected() {
    let (env, client, _, _) = setup();
    let s = Address::generate(&env);
    add_signer(&env, &client, &s);
    let mut entries = Vec::new(&env);
    entries.push_back((s, 80u32, 10u32)); // min > max
    assert_eq!(
        client.try_bulk_set_signer_tier(&Vec::new(&env), &entries),
        Err(Ok(Error::InvalidThreshold))
    );
}

#[test]
fn test_bulk_set_signer_tier_not_in_service_set_rejected() {
    let (env, client, _, _) = setup();
    let s = Address::generate(&env); // never added to service set
    let mut entries = Vec::new(&env);
    entries.push_back((s, 10u32, 80u32));
    assert_eq!(
        client.try_bulk_set_signer_tier(&Vec::new(&env), &entries),
        Err(Ok(Error::SignerNotInSet))
    );
}

#[test]
fn test_bulk_set_signer_tier_too_large_rejected() {
    let (env, client, _, _) = setup();
    let mut entries = Vec::new(&env);
    for _ in 0..=crate::constants::MAX_BATCH_SIZE {
        entries.push_back((Address::generate(&env), 0u32, 100u32));
    }
    assert_eq!(
        client.try_bulk_set_signer_tier(&Vec::new(&env), &entries),
        Err(Ok(Error::BatchTooLarge))
    );
}

#[test]
fn test_bulk_set_signer_tier_not_initialized_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let mut entries: Vec<(Address, u32, u32)> = Vec::new(&env);
    entries.push_back((Address::generate(&env), 0u32, 100u32));
    assert_eq!(
        client.try_bulk_set_signer_tier(&Vec::new(&env), &entries),
        Err(Ok(Error::NotInitialized))
    );
}

#[test]
#[should_panic]
fn test_bulk_set_signer_tier_non_admin_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    let s = Address::generate(&env);
    add_signer(&env, &client, &s);
    env.set_auths(&[]);
    let mut entries = Vec::new(&env);
    entries.push_back((s, 0u32, 100u32));
    client.bulk_set_signer_tier(&Vec::new(&env), &entries).unwrap();
}
