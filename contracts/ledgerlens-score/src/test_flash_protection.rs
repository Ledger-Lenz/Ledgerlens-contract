//! Tests for #300: anti-flash-loan protection (same-ledger gate-read + submit detection).

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, Symbol, Vec,
};

use crate::{Error, FlashProtectionMode, LedgerLensScoreContract, LedgerLensScoreContractClient};

const START_TS: u64 = 1_700_000_000;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    // Epoch is open by default; no need to call open_epoch.
    (env, client)
}

fn submit(
    client: &LedgerLensScoreContractClient,
    env: &Env,
    wallet: &Address,
) -> Result<(), Error> {
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

// Same-ledger query + submit in Log mode: submission succeeds, event emitted.
#[test]
fn test_same_ledger_log_mode_allows_submission_and_emits_event() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);

    // gate read and submit happen in the same ledger (sequence = default 0).
    client.query_risk_gate(&wallet, &symbol_short!("XLM_USDC"), &75);
    assert_eq!(submit(&client, &env, &wallet), Ok(()));

    // Verify the flash_sub event was emitted.
    let events = env.events().all();
    let flash_topic = Symbol::new(&env, "flash_sub");
    let found = events.iter().any(|(_, topics, _)| {
        let topics: soroban_sdk::Vec<soroban_sdk::Val> =
            soroban_sdk::Vec::try_from_val(&env, &topics).unwrap_or_else(|_| soroban_sdk::Vec::new(&env));
        topics.len() > 0 && topics.get(0).map(|v| v == flash_topic.into_val(&env)).unwrap_or(false)
    });
    assert!(found, "expected flash_sub event");
}

// Cross-ledger gate read + submit: no event, submission succeeds normally.
#[test]
fn test_cross_ledger_no_event() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);

    // Gate read at ledger sequence 0.
    client.query_risk_gate(&wallet, &symbol_short!("XLM_USDC"), &75);

    // Advance ledger sequence so GateReadLedger TTL expires.
    env.ledger().with_mut(|li| {
        li.sequence_number += 2;
        li.timestamp = START_TS + 10;
    });

    assert_eq!(submit(&client, &env, &wallet), Ok(()));

    // No flash_sub event should have been emitted.
    let events = env.events().all();
    let flash_topic = Symbol::new(&env, "flash_sub");
    let found = events.iter().any(|(_, topics, _)| {
        let topics: soroban_sdk::Vec<soroban_sdk::Val> =
            soroban_sdk::Vec::try_from_val(&env, &topics).unwrap_or_else(|_| soroban_sdk::Vec::new(&env));
        topics.len() > 0 && topics.get(0).map(|v| v == flash_topic.into_val(&env)).unwrap_or(false)
    });
    assert!(!found, "unexpected flash_sub event for cross-ledger scenario");
}

// Same-ledger gate read + submit in Reject mode: submission rejected.
#[test]
fn test_same_ledger_reject_mode_blocks_submission() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);

    client.set_flash_protection_mode(&Vec::new(&env), &FlashProtectionMode::Reject);
    assert_eq!(client.get_flash_protection_mode(), FlashProtectionMode::Reject);

    client.query_risk_gate(&wallet, &symbol_short!("XLM_USDC"), &75);
    assert_eq!(submit(&client, &env, &wallet), Err(Error::EpochClosed));
}

// No gate read: submission proceeds normally regardless of mode.
#[test]
fn test_no_gate_read_no_flash_detection() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);

    client.set_flash_protection_mode(&Vec::new(&env), &FlashProtectionMode::Reject);
    // No query_risk_gate call — should not be flagged.
    assert_eq!(submit(&client, &env, &wallet), Ok(()));
}
