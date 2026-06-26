//! Tests for #301: score epoch sealing.

use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient};

const START_TS: u64 = 1_700_000_000;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client)
}

fn submit(client: &LedgerLensScoreContractClient, env: &Env, wallet: &Address) -> Result<(), Error> {
    client
        .try_submit_score(
            &Vec::new(env),
            wallet,
            &symbol_short!("XLM_USDC"),
            &50,
            &false,
            &false,
            &START_TS,
            &90,
            &1,
            &None,
        )
        .map_err(|e| e.unwrap())
}

// Default state: epoch is open (submissions accepted before any epoch management).
#[test]
fn test_submit_accepted_by_default() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    assert_eq!(submit(&client, &env, &wallet), Ok(()));
}

// close_epoch -> is_epoch_open == false, submit rejected.
#[test]
fn test_close_epoch_blocks_submission() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);

    client.close_epoch(&Vec::new(&env));
    assert!(!client.is_epoch_open());
    assert_eq!(submit(&client, &env, &wallet), Err(Error::EpochClosed));
}

// open_epoch after close -> is_epoch_open == true, submit succeeds.
#[test]
fn test_open_epoch_re_allows_submission() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);

    client.close_epoch(&Vec::new(&env));
    client.open_epoch(&Vec::new(&env), &1);
    assert!(client.is_epoch_open());
    assert_eq!(client.get_current_epoch(), 1);
    assert_eq!(submit(&client, &env, &wallet), Ok(()));
}

// Transition: close -> reject -> open epoch 1 -> submit -> close -> reject -> open epoch 2.
#[test]
fn test_epoch_transitions() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);

    // Default open — submit succeeds.
    assert_eq!(submit(&client, &env, &wallet), Ok(()));

    // Close epoch -> blocked.
    client.close_epoch(&Vec::new(&env));
    assert_eq!(client.get_current_epoch(), 0);
    let wallet2 = Address::generate(&env);
    assert_eq!(submit(&client, &env, &wallet2), Err(Error::EpochClosed));

    // Open epoch 1 -> allowed again.
    client.open_epoch(&Vec::new(&env), &1);
    assert_eq!(client.get_current_epoch(), 1);
    env.ledger().with_mut(|li| li.timestamp = START_TS + 4000);
    assert_eq!(submit(&client, &env, &wallet2), Ok(()));

    // Close again, open epoch 2.
    client.close_epoch(&Vec::new(&env));
    client.open_epoch(&Vec::new(&env), &2);
    assert_eq!(client.get_current_epoch(), 2);
}
