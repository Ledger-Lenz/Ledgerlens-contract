//! Smoke test verifying that contract upgrade preserves score and configuration integrity.
//!
//! This test exercises the full upgrade lifecycle: proposing a no-op upgrade (uploading
//! the same WASM), advancing time past the delay, executing the upgrade, and verifying
//! that all stored scores and admin-configurable parameters remain unchanged.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Bytes, Env, Vec,
};

use crate::{
    constants::DEFAULT_UPGRADE_DELAY_SECS,
    LedgerLensScoreContract, LedgerLensScoreContractClient,
};

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

/// Upload the same WASM bytecode to create a no-op upgrade target.
fn upload_current_wasm(env: &Env) -> soroban_sdk::BytesN<32> {
    env.deployer().upload_contract_wasm(Bytes::new(env))
}

/// Advance the ledger timestamp to a specific value.
fn advance_to(env: &Env, ts: u64) {
    env.ledger().with_mut(|l| l.timestamp = ts);
}

#[test]
fn test_upgrade_preserves_score_integrity() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);

    // ── Submit initial scores across multiple wallets and pairs ─────────────────

    let wallet1 = Address::generate(&env);
    let wallet2 = Address::generate(&env);
    let wallet3 = Address::generate(&env);
    let pair1 = symbol_short!("XLM_USDC");
    let pair2 = symbol_short!("BTC_USDT");

    // Record submitted scores for later verification
    let scores = vec![
        (wallet1.clone(), pair1, 42, 80, 1, START_TS),
        (wallet1.clone(), pair2, 55, 75, 1, START_TS),
        (wallet1.clone(), pair1, 43, 85, 1, START_TS + 10),
        (wallet2.clone(), pair1, 27, 90, 1, START_TS + 20),
        (wallet2.clone(), pair2, 88, 60, 1, START_TS + 30),
        (wallet2.clone(), pair1, 28, 92, 1, START_TS + 40),
        (wallet3.clone(), pair1, 10, 70, 1, START_TS + 50),
        (wallet3.clone(), pair2, 95, 65, 1, START_TS + 60),
        (wallet3.clone(), pair1, 11, 75, 1, START_TS + 70),
        (wallet3.clone(), pair2, 96, 68, 1, START_TS + 80),
    ];

    for (wallet, pair, score, confidence, model_ver, ts) in &scores {
        client.submit_score(
            &Vec::new(&env),
            wallet,
            pair,
            score,
            &false,
            &false,
            ts,
            confidence,
            model_ver,
            &None,
        );
    }

    // ── Capture configuration state before upgrade ────────────────────────────

    let cooldown_before = client.get_cooldown();
    let history_depth_before = client.get_history_max_depth();
    let decay_before = client.get_decay_rate();
    let threshold_before = client.get_risk_threshold();

    // ── Propose no-op upgrade ──────────────────────────────────────────────────

    let wasm_hash = upload_current_wasm(&env);
    client.propose_upgrade(&Vec::new(&env), &wasm_hash);

    // Verify proposal was stored
    let proposal = client.get_pending_upgrade().expect("proposal should exist");
    assert_eq!(proposal.new_wasm_hash, wasm_hash);
    assert_eq!(proposal.executable_after, START_TS + DEFAULT_UPGRADE_DELAY_SECS);

    // ── Advance time past the upgrade delay ────────────────────────────────────

    advance_to(&env, START_TS + DEFAULT_UPGRADE_DELAY_SECS);

    // Execute the upgrade
    client.execute_upgrade(&Vec::new(&env));

    // ── Verify all scores are intact ───────────────────────────────────────────

    for (wallet, pair, expected_score, expected_conf, expected_model, _ts) in scores.iter() {
        let retrieved = client
            .get_score(wallet, pair)
            .expect("score should be retrievable after upgrade");

        assert_eq!(
            retrieved.score, *expected_score,
            "Score mismatch for {:?} / {:?}",
            wallet, pair
        );
        assert_eq!(
            retrieved.confidence, *expected_conf,
            "Confidence mismatch for {:?} / {:?}",
            wallet, pair
        );
        assert_eq!(
            retrieved.model_version, *expected_model,
            "Model version mismatch for {:?} / {:?}",
            wallet, pair
        );
    }

    // ── Verify configuration parameters are unchanged ────────────────────────

    assert_eq!(
        client.get_cooldown(),
        cooldown_before,
        "Cooldown should be unchanged after upgrade"
    );
    assert_eq!(
        client.get_history_max_depth(),
        history_depth_before,
        "History depth should be unchanged after upgrade"
    );
    assert_eq!(
        client.get_decay_rate(),
        decay_before,
        "Decay rate should be unchanged after upgrade"
    );
    assert_eq!(
        client.get_risk_threshold(),
        threshold_before,
        "Risk threshold should be unchanged after upgrade"
    );

    // Verify the admin and service are still the same
    assert_eq!(
        client.get_admin(),
        admin,
        "Admin should be unchanged after upgrade"
    );
    assert_eq!(
        client.get_service(),
        service,
        "Service should be unchanged after upgrade"
    );
}
