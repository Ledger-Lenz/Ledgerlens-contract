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

// close_epoch before the contract has been initialized (no admin set yet)
// must fail with NotInitialized rather than panicking. Checked test_epoch.rs,
// test_admin_multisig.rs, and test_embargo.rs first: every existing
// close_epoch/open_epoch call goes through setup(), which always calls
// initialize(), so the pre-initialization path was never exercised.
#[test]
fn test_close_epoch_before_initialize_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &id);

    let result = client.try_close_epoch(&Vec::new(&env));
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

// get_current_epoch must keep returning the last opened epoch id after
// close_epoch — close_epoch only flips EpochOpen to false, it does not
// reset the stored epoch id back to 0. This transition (non-zero epoch,
// immediately after close, before the next open_epoch) isn't asserted by
// test_epoch_transitions above, which only checks get_current_epoch() == 0
// while the epoch id was already 0.
#[test]
fn test_get_current_epoch_persists_after_close_of_nonzero_epoch() {
    let (env, client) = setup();

    client.open_epoch(&Vec::new(&env), &7);
    assert_eq!(client.get_current_epoch(), 7);

    client.close_epoch(&Vec::new(&env));
    assert!(!client.is_epoch_open());
    assert_eq!(client.get_current_epoch(), 7);
}

// is_epoch_open has no NotInitialized guard (unlike open_epoch/close_epoch) —
// it reads straight from storage, which falls back to `true` via
// `unwrap_or(true)` when EpochOpen has never been written. Every other test
// in this file calls setup(), which always initializes the contract, so the
// pre-initialize / never-set-yet default was never asserted directly. This
// pins that default down instead of just asserting "doesn't panic".
#[test]
fn test_is_epoch_open_defaults_true_before_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &id);

    assert!(client.is_epoch_open());
}
