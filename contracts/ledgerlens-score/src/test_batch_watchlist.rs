use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient};

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

#[test]
fn test_batch_add_to_watchlist_adds_all() {
    let (env, client, _) = setup();
    let w1 = Address::generate(&env);
    let w2 = Address::generate(&env);
    let mut wallets = Vec::new(&env);
    wallets.push_back(w1.clone());
    wallets.push_back(w2.clone());
    client.batch_add_to_watchlist(&Vec::new(&env), &wallets).unwrap();
    assert!(client.is_watchlisted(&w1));
    assert!(client.is_watchlisted(&w2));
}

#[test]
fn test_batch_add_to_watchlist_skips_duplicates() {
    let (env, client, _) = setup();
    let w = Address::generate(&env);
    let mut wallets = Vec::new(&env);
    wallets.push_back(w.clone());
    // First add
    client.batch_add_to_watchlist(&Vec::new(&env), &wallets).unwrap();
    // Second add — must not error
    client.batch_add_to_watchlist(&Vec::new(&env), &wallets).unwrap();
    assert!(client.is_watchlisted(&w));
}

#[test]
fn test_batch_add_to_watchlist_too_large_rejected() {
    let (env, client, _) = setup();
    let mut wallets = Vec::new(&env);
    for _ in 0..=crate::constants::MAX_BATCH_SIZE {
        wallets.push_back(Address::generate(&env));
    }
    assert_eq!(
        client.try_batch_add_to_watchlist(&Vec::new(&env), &wallets),
        Err(Ok(Error::BatchTooLarge))
    );
}

#[test]
fn test_batch_add_to_watchlist_not_initialized_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let mut wallets = Vec::new(&env);
    wallets.push_back(Address::generate(&env));
    assert_eq!(
        client.try_batch_add_to_watchlist(&Vec::new(&env), &wallets),
        Err(Ok(Error::NotInitialized))
    );
}

#[test]
#[should_panic]
fn test_batch_add_to_watchlist_non_admin_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    env.set_auths(&[]);
    let mut wallets = Vec::new(&env);
    wallets.push_back(Address::generate(&env));
    client.batch_add_to_watchlist(&Vec::new(&env), &wallets).unwrap();
}

#[test]
fn test_batch_remove_from_watchlist_removes_all() {
    let (env, client, _) = setup();
    let w1 = Address::generate(&env);
    let w2 = Address::generate(&env);
    let mut wallets = Vec::new(&env);
    wallets.push_back(w1.clone());
    wallets.push_back(w2.clone());
    client.batch_add_to_watchlist(&Vec::new(&env), &wallets).unwrap();
    client.batch_remove_from_watchlist(&Vec::new(&env), &wallets).unwrap();
    assert!(!client.is_watchlisted(&w1));
    assert!(!client.is_watchlisted(&w2));
}

#[test]
fn test_batch_remove_from_watchlist_skips_unlisted() {
    let (env, client, _) = setup();
    let w = Address::generate(&env);
    let mut wallets = Vec::new(&env);
    wallets.push_back(w.clone());
    // Never added — must not error
    client.batch_remove_from_watchlist(&Vec::new(&env), &wallets).unwrap();
    assert!(!client.is_watchlisted(&w));
}

#[test]
fn test_batch_remove_from_watchlist_too_large_rejected() {
    let (env, client, _) = setup();
    let mut wallets = Vec::new(&env);
    for _ in 0..=crate::constants::MAX_BATCH_SIZE {
        wallets.push_back(Address::generate(&env));
    }
    assert_eq!(
        client.try_batch_remove_from_watchlist(&Vec::new(&env), &wallets),
        Err(Ok(Error::BatchTooLarge))
    );
}

#[test]
fn test_batch_remove_from_watchlist_not_initialized_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let mut wallets = Vec::new(&env);
    wallets.push_back(Address::generate(&env));
    assert_eq!(
        client.try_batch_remove_from_watchlist(&Vec::new(&env), &wallets),
        Err(Ok(Error::NotInitialized))
    );
}
