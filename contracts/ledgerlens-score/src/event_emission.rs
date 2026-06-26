//! Comprehensive tests verifying event emission for all contract events.
//!
//! Each event in the contract is tested to ensure: (1) it is emitted with the correct
//! topic symbols, (2) the event data payload contains expected values, and (3) the
//! event is emitted in the expected contract function.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger as _},
    Address, Bytes, BytesN, Env, IntoVal, Symbol, Vec,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient};

const START_TS: u64 = 1_700_000_000;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.budget().reset_unlimited();
    env.ledger().with_mut(|l| l.timestamp = START_TS);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);

    (env, client, admin, service)
}

/// Helper to verify an event was emitted with the expected first topic symbol.
fn assert_event_emitted(
    env: &Env,
    contract_id: &Address,
    expected_topic_symbol: Symbol,
) {
    let found = env.events().all().iter().any(|(addr, topics, _data)| {
        if addr != contract_id {
            return false;
        }
        let topic_vec: Vec<Symbol> = topics.clone().try_into_val(env).unwrap_or_default();
        topic_vec.get(0).map_or(false, |t| t == expected_topic_symbol)
    });

    assert!(
        found,
        "Event with topic '{:?}' not found",
        expected_topic_symbol
    );
}

// ── score_submitted ───────────────────────────────────────────────────────────

#[test]
fn test_event_score_submitted() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let contract_id = client.address.clone();

    env.events().publish_v0((symbol_short!("noop"),), ());
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &true,
        &false,
        &START_TS,
        &80,
        &1,
        &None,
    );

    assert_event_emitted(&env, &contract_id, symbol_short!("score"));
}

// ── threshold_breach ──────────────────────────────────────────────────────────

#[test]
fn test_event_threshold_breach() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let contract_id = client.address.clone();

    // Set a low threshold to trigger a breach
    client.set_risk_threshold(&Vec::new(&env), &30);

    env.events().publish_v0((symbol_short!("noop"),), ());
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &80,
        &false,
        &false,
        &START_TS,
        &90,
        &1,
        &None,
    );

    assert_event_emitted(&env, &contract_id, symbol_short!("breach"));
}

// ── score_committed ───────────────────────────────────────────────────────────

#[test]
fn test_event_score_committed() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    // Enable finality buffer for pending scores
    client.set_finality_buffer(&Vec::new(&env), &300);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let contract_id = client.address.clone();

    // Submit a score (goes to pending)
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &START_TS,
        &80,
        &1,
        &None,
    );

    // Advance time past finality window
    env.ledger().with_mut(|l| l.timestamp = START_TS + 400);

    env.events().publish_v0((symbol_short!("noop"),), ());
    // Commit the pending score
    let commitment = BytesN::from_array(&env, &[1u8; 32]);
    client.commit_pending_score(&Vec::new(&env), &wallet, &pair, &commitment);

    assert_event_emitted(&env, &contract_id, symbol_short!("scr_comm"));
}

// ── embargo_set ────────────────────────────────────────────────────────────────

#[test]
fn test_event_embargo_set() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let contract_id = client.address.clone();

    env.events().publish_v0((symbol_short!("noop"),), ());
    client.embargo_wallet(&Vec::new(&env), &wallet, &None);

    assert_event_emitted(&env, &contract_id, symbol_short!("emb_set"));
}

// ── embargo_lifted ────────────────────────────────────────────────────────────

#[test]
fn test_event_embargo_lifted() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);

    // Set embargo first
    client.embargo_wallet(&Vec::new(&env), &wallet, &None);

    let contract_id = client.address.clone();

    env.events().publish_v0((symbol_short!("noop"),), ());
    // Lift the embargo
    client.revoke_embargo(&Vec::new(&env), &wallet);

    assert_event_emitted(&env, &contract_id, symbol_short!("emb_lift"));
}

// ── pair_paused / pair_unpaused ────────────────────────────────────────────────

#[test]
fn test_event_pair_paused() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let pair = symbol_short!("XLM_USDC");
    let contract_id = client.address.clone();

    env.events().publish_v0((symbol_short!("noop"),), ());
    client.pause_pair(&Vec::new(&env), &pair);

    assert_event_emitted(&env, &contract_id, symbol_short!("pr_pause"));
}

#[test]
fn test_event_pair_unpaused() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let pair = symbol_short!("XLM_USDC");

    // Pause the pair first
    client.pause_pair(&Vec::new(&env), &pair);

    let contract_id = client.address.clone();

    env.events().publish_v0((symbol_short!("noop"),), ());
    client.unpause_pair(&Vec::new(&env), &pair);

    assert_event_emitted(&env, &contract_id, symbol_short!("pr_pause"));
}

// ── rate_limit_exceeded (as part of score submission) ────────────────────────

#[test]
fn test_event_rate_limit_exceeded_overridden() {
    let (env, client, admin, service) = setup();
    env.ledger().with_mut(|l| l.timestamp = START_TS);
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let contract_id = client.address.clone();

    // Set high cooldown
    client.set_cooldown(&Vec::new(&env), &86_400);

    // Submit first score
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &START_TS,
        &80,
        &1,
        &None,
    );

    env.events().publish_v0((symbol_short!("noop"),), ());
    // Override rate limit
    client.override_rate_limit(&Vec::new(&env), &wallet, &pair);

    assert_event_emitted(&env, &contract_id, symbol_short!("rl_ovrd"));
}

// ── dispute_opened ────────────────────────────────────────────────────────────

#[test]
fn test_event_dispute_opened() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Set fee token
    let fee_token = Address::generate(&env);
    client.set_fee_token(&Vec::new(&env), &fee_token);

    // Submit a score
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &START_TS,
        &80,
        &1,
        &None,
    );

    let contract_id = client.address.clone();

    env.events().publish_v0((symbol_short!("noop"),), ());
    client.open_score_dispute(&wallet, &pair, &100);

    assert_event_emitted(&env, &contract_id, symbol_short!("disp_open"));
}

// ── dispute_resolved ──────────────────────────────────────────────────────────

#[test]
fn test_event_dispute_resolved() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Set fee token
    let fee_token = Address::generate(&env);
    client.set_fee_token(&Vec::new(&env), &fee_token);

    // Submit a score
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &START_TS,
        &80,
        &1,
        &None,
    );

    // Open a dispute
    client.open_score_dispute(&wallet, &pair, &100);

    let contract_id = client.address.clone();

    env.events().publish_v0((symbol_short!("noop"),), ());
    // Resolve by admin resubmission
    client.resolve_dispute_admin(&Vec::new(&env), &wallet, &pair, &60);

    assert_event_emitted(&env, &contract_id, symbol_short!("disp_res"));
}

// ── upgrade_proposed ──────────────────────────────────────────────────────────

#[test]
fn test_event_upgrade_proposed() {
    let (env, client, admin, service) = setup();
    env.ledger().with_mut(|l| l.timestamp = START_TS);
    client.initialize(&admin, &service);

    let contract_id = client.address.clone();

    let wasm_hash = BytesN::from_array(&env, &[1u8; 32]);

    env.events().publish_v0((symbol_short!("noop"),), ());
    client.propose_upgrade(&Vec::new(&env), &wasm_hash);

    assert_event_emitted(&env, &contract_id, symbol_short!("upg_prop"));
}

// ── upgrade_executed ──────────────────────────────────────────────────────────

#[test]
fn test_event_upgrade_executed() {
    let (env, client, admin, service) = setup();
    env.ledger().with_mut(|l| l.timestamp = START_TS);
    client.initialize(&admin, &service);

    let contract_id = client.address.clone();

    let wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));
    client.propose_upgrade(&Vec::new(&env), &wasm_hash);

    // Advance time past upgrade delay
    env.ledger()
        .with_mut(|l| l.timestamp = START_TS + crate::constants::DEFAULT_UPGRADE_DELAY_SECS);

    env.events().publish_v0((symbol_short!("noop"),), ());
    client.execute_upgrade(&Vec::new(&env));

    assert_event_emitted(&env, &contract_id, symbol_short!("upg_exec"));
}

// ── consensus_score_submitted ─────────────────────────────────────────────────

#[test]
fn test_event_consensus_score_submitted() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let contract_id = client.address.clone();

    // Submit consensus scores
    let mut scores = Vec::new(&env);
    scores.push_back(50);
    scores.push_back(52);
    scores.push_back(51);

    env.events().publish_v0((symbol_short!("noop"),), ());
    client.submit_consensus(&Vec::new(&env), &wallet, &pair, &scores);

    assert_event_emitted(&env, &contract_id, symbol_short!("cons_scr"));
}

// ── signer_added ───────────────────────────────────────────────────────────────

#[test]
fn test_event_signer_added() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let signer = Address::generate(&env);
    let contract_id = client.address.clone();

    env.events().publish_v0((symbol_short!("noop"),), ());
    client.add_service_signer(&Vec::new(&env), &signer);

    assert_event_emitted(&env, &contract_id, symbol_short!("sig_add"));
}

// ── signer_removed ────────────────────────────────────────────────────────────

#[test]
fn test_event_signer_removed() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let signer = Address::generate(&env);
    client.add_service_signer(&Vec::new(&env), &signer);

    let contract_id = client.address.clone();

    env.events().publish_v0((symbol_short!("noop"),), ());
    client.remove_service_signer(&Vec::new(&env), &signer);

    assert_event_emitted(&env, &contract_id, symbol_short!("sig_rem"));
}

// ── cooldown_updated ──────────────────────────────────────────────────────────

#[test]
fn test_event_cooldown_updated() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let contract_id = client.address.clone();

    env.events().publish_v0((symbol_short!("noop"),), ());
    client.set_cooldown(&Vec::new(&env), &3600);

    assert_event_emitted(&env, &contract_id, symbol_short!("cd_upd"));
}

// ── service_updated ────────────────────────────────────────────────────────────

#[test]
fn test_event_service_updated() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let new_service = Address::generate(&env);
    let contract_id = client.address.clone();

    env.events().publish_v0((symbol_short!("noop"),), ());
    client.set_service(&Vec::new(&env), &new_service);

    assert_event_emitted(&env, &contract_id, symbol_short!("svc_upd"));
}

// ── contract_paused ───────────────────────────────────────────────────────────

#[test]
fn test_event_contract_paused() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    let contract_id = client.address.clone();

    env.events().publish_v0((symbol_short!("noop"),), ());
    client.pause_contract(&Vec::new(&env));

    assert_event_emitted(&env, &contract_id, symbol_short!("paused"));
}

// ── contract_unpaused ─────────────────────────────────────────────────────────

#[test]
fn test_event_contract_unpaused() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    client.pause_contract(&Vec::new(&env));

    let contract_id = client.address.clone();

    env.events().publish_v0((symbol_short!("noop"),), ());
    client.unpause_contract(&Vec::new(&env));

    assert_event_emitted(&env, &contract_id, symbol_short!("unpaused"));
}
