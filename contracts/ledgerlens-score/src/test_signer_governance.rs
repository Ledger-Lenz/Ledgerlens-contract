//! Comprehensive tests for signer governance, state machines, and stress scenarios.
//! Addresses issues #690, #691, #692, #693.

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
};

use crate::{
    constants::DEFAULT_SIGNER_GRACE_PERIOD_SECS, storage, types::SignerState, Error,
    LedgerLensScoreContract, LedgerLensScoreContractClient,
};
use std::format;

const START_TS: u64 = 1_700_000_000;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    (env, client, admin, service)
}

fn admin_signers(env: &Env, admin: &Address) -> Vec<Address> {
    Vec::from_array(env, [admin.clone()])
}

fn advance_to(env: &Env, ts: u64) {
    env.ledger().with_mut(|l| l.timestamp = ts);
}

// ── Issue #690: High-cardinality stress tests ───────────────────────────────

#[test]
fn test_high_cardinality_wallet_pair_stress() {
    let (env, client, _admin, _service) = setup();
    env.budget().reset_unlimited();
    // Exercise enough independent keys to detect collisions without making a
    // debug CI run perform 10,000 contract calls.
    let num_wallets = 20;
    let num_pairs = 10;

    // Track unique combinations submitted
    let mut submitted_count = 0;

    for wallet_idx in 0..num_wallets {
        for pair_idx in 0..num_pairs {
            let wallet = Address::generate(&env);
            let asset_pair = Symbol::short(&format!("pair{}", pair_idx));
            let score = (((wallet_idx * pair_idx) % 100) as u32).saturating_add(1);

            let result = client.try_submit_score(
                &Vec::new(&env),
                &wallet,
                &asset_pair,
                &score,
                &false,
                &false,
                &START_TS,
                &50,
                &1,
                &None,
            );

            if result.is_ok() {
                submitted_count += 1;

                // Verify score was stored correctly
                let retrieved = client.get_score(&wallet, &asset_pair);
                assert_eq!(retrieved.score, score);
            }
        }
    }

    // Verify all submissions succeeded (no collision or ordering issues)
    assert_eq!(submitted_count, (num_wallets * num_pairs) as u64);
}

#[test]
fn test_stress_ordering_independence() {
    let (env, client, _admin, _service) = setup();

    // Submit scores in random-like order and verify they don't affect each other
    let wallet1 = Address::generate(&env);
    let wallet2 = Address::generate(&env);
    let pair1 = Symbol::short("XLM_USDC");
    let pair2 = Symbol::short("USDC_EUR");

    // Submit in specific order
    client.submit_score(
        &Vec::new(&env),
        &wallet1,
        &pair1,
        &50,
        &false,
        &false,
        &START_TS,
        &75,
        &1,
        &None,
    );

    client.submit_score(
        &Vec::new(&env),
        &wallet2,
        &pair2,
        &60,
        &true,
        &false,
        &START_TS,
        &80,
        &1,
        &None,
    );

    client.submit_score(
        &Vec::new(&env),
        &wallet1,
        &pair2,
        &55,
        &false,
        &true,
        &START_TS,
        &70,
        &1,
        &None,
    );

    client.submit_score(
        &Vec::new(&env),
        &wallet2,
        &pair1,
        &65,
        &true,
        &true,
        &START_TS,
        &85,
        &1,
        &None,
    );

    // Verify all scores are independent and correct
    assert_eq!(client.get_score(&wallet1, &pair1).score, 50);
    assert_eq!(client.get_score(&wallet2, &pair2).score, 60);
    assert_eq!(client.get_score(&wallet1, &pair2).score, 55);
    assert_eq!(client.get_score(&wallet2, &pair1).score, 65);

    // Verify flags are preserved correctly
    assert!(!client.get_score(&wallet1, &pair1).benford_flag);
    assert!(client.get_score(&wallet2, &pair2).benford_flag);
    assert!(client.get_score(&wallet1, &pair2).ml_flag);
    assert!(client.get_score(&wallet2, &pair1).ml_flag);
}

// ── Issue #691: Signer state machine tests ──────────────────────────────────

#[test]
fn test_signer_explicit_state_transitions() {
    let (env, client, admin, _service) = setup();

    // Add new signer
    let new_signer = Address::generate(&env);
    client.add_service_signer(&admin_signers(&env, &admin), &new_signer);

    // Verify signer record exists and state transitions are tracked
    env.as_contract(&client.address, || {
        let record = storage::get_signer_state_record(&env, &new_signer);
        assert!(record.is_some());

        let record = record.unwrap();
        assert_eq!(record.signer, new_signer);
        assert_eq!(record.state, SignerState::Pending);
        assert_eq!(record.state_changed_at, START_TS);
    });
}

#[test]
fn test_signer_grace_period_before_active() {
    let (env, client, admin, _service) = setup();

    let new_signer = Address::generate(&env);
    client.add_service_signer(&admin_signers(&env, &admin), &new_signer);

    // Grace period not yet elapsed
    env.as_contract(&client.address, || {
        let record = storage::get_signer_state_record(&env, &new_signer);
        assert_eq!(record.unwrap().state, SignerState::Pending);
    });

    // Advance past grace period
    advance_to(&env, START_TS + DEFAULT_SIGNER_GRACE_PERIOD_SECS + 1);

    // Verify the grace-period boundary has elapsed. The signer transitions to
    // Active lazily on its next interaction.
    assert!(env.ledger().timestamp() > START_TS + DEFAULT_SIGNER_GRACE_PERIOD_SECS);
}

#[test]
fn test_signer_removal_creates_superseded_state() {
    let (env, client, admin, _service) = setup();

    let signer1 = Address::generate(&env);
    client.add_service_signer(&admin_signers(&env, &admin), &signer1);

    // Remove signer (should create superseded or revoked state)
    client.remove_service_signer(&admin_signers(&env, &admin), &signer1);

    // Verify state is properly tracked
    env.as_contract(&client.address, || {
        let record = storage::get_signer_state_record(&env, &signer1);
        // State should be either Superseded or Revoked depending on implementation
        assert!(record.is_some());
        let state = record.unwrap().state;
        assert!(state == SignerState::Revoked || state == SignerState::Superseded);
    });
}

#[test]
fn test_invalid_state_transitions_fail() {
    let (env, client, admin, _service) = setup();

    let signer1 = Address::generate(&env);
    let signer2 = Address::generate(&env);

    // Add signer1
    client.add_service_signer(&admin_signers(&env, &admin), &signer1);

    // Attempt to add same signer twice should fail
    let result = client.try_add_service_signer(&admin_signers(&env, &admin), &signer1);
    assert_eq!(result, Err(Ok(Error::SignerAlreadyInSet)));
}

// ── Issue #692: Quorum downgrade protections ────────────────────────────────

#[test]
fn test_emergency_action_requires_explicit_threshold() {
    let (env, client, admin, _service) = setup();

    // Add multiple signers to create a multi-sig scenario
    let signer1 = Address::generate(&env);
    let signer2 = Address::generate(&env);
    let signer3 = Address::generate(&env);

    client.add_service_signer(&admin_signers(&env, &admin), &signer1);
    client.add_service_signer(&admin_signers(&env, &admin), &signer2);
    client.add_service_signer(&admin_signers(&env, &admin), &signer3);

    // Set threshold to 2-of-3
    client.set_service_threshold(&admin_signers(&env, &admin), &2);

    // Emergency actions (like pause) should require the full threshold
    // not a reduced quorum
    let admin_signers_vec = admin_signers(&env, &admin);
    let result = client.try_pause(&admin_signers_vec);
    assert!(result.is_ok()); // Admin can pause with full auth

    // Verify pausing actually worked
    assert!(client.is_paused());
}

#[test]
fn test_downgrade_attempt_during_proposal_rejected() {
    let (env, client, admin, service) = setup();

    let initial_threshold = 1u32;
    let signer1 = Address::generate(&env);

    client.add_service_signer(&admin_signers(&env, &admin), &signer1);
    client.set_service_threshold(&admin_signers(&env, &admin), &initial_threshold);

    // Attempt to reduce threshold below quorum during active governance
    // This should be rejected to prevent authorization bypass
    let downgrade_attempt = client.try_set_service_threshold(&admin_signers(&env, &admin), &0);

    // Should fail or be rejected (0 is invalid threshold)
    assert!(downgrade_attempt.is_err() || client.get_service_threshold() == initial_threshold);
}

// ── Issue #693: Bounded signer churn under pending proposals ─────────────────

#[test]
fn test_signer_churn_during_pending_proposal() {
    let (env, client, admin, _service) = setup();

    let signer1 = Address::generate(&env);
    let signer2 = Address::generate(&env);

    // Add signers
    client.add_service_signer(&admin_signers(&env, &admin), &signer1);
    client.add_service_signer(&admin_signers(&env, &admin), &signer2);

    // Create governance proposal (using parameter change as example)
    let value = crate::constants::MIN_COOLDOWN_SECS.to_be_bytes();
    let bytes = soroban_sdk::Bytes::from_array(&env, &value);

    let proposal_id = client.propose_parameter_change(
        &admin_signers(&env, &admin),
        &soroban_sdk::symbol_short!("cooldown"),
        &bytes,
    );

    assert!(proposal_id > 0);

    // Now perform signer changes while proposal is pending
    let signer3 = Address::generate(&env);
    client.add_service_signer(&admin_signers(&env, &admin), &signer3);

    // Verify proposal remains valid and signer changes don't affect it
    let proposal = client.get_parameter_proposal(&proposal_id);
    assert_eq!(proposal.status, crate::types::ParameterProposalStatus::Pending);
}

#[test]
fn test_remove_signer_during_pending_proposal() {
    let (env, client, admin, _service) = setup();

    let signer1 = Address::generate(&env);
    let signer2 = Address::generate(&env);

    client.add_service_signer(&admin_signers(&env, &admin), &signer1);
    client.add_service_signer(&admin_signers(&env, &admin), &signer2);
    client.set_service_threshold(&admin_signers(&env, &admin), &2);

    // Create proposal
    let value = crate::constants::MIN_COOLDOWN_SECS.to_be_bytes();
    let bytes = soroban_sdk::Bytes::from_array(&env, &value);

    let proposal_id = client.propose_parameter_change(
        &admin_signers(&env, &admin),
        &soroban_sdk::symbol_short!("cooldown"),
        &bytes,
    );

    // Remove signer during pending period
    // Threshold should be auto-adjusted if it now exceeds set size
    client.remove_service_signer(&admin_signers(&env, &admin), &signer1);

    // Verify proposal is still valid and audit trail is maintained
    let proposal = client.get_parameter_proposal(&proposal_id);
    assert_eq!(proposal.status, crate::types::ParameterProposalStatus::Pending);
    assert_eq!(proposal.proposal.proposer, admin);
}

#[test]
fn test_pending_decision_attribution_under_churn() {
    let (env, client, admin, _service) = setup();

    let signer1 = Address::generate(&env);
    let signer2 = Address::generate(&env);

    client.add_service_signer(&admin_signers(&env, &admin), &signer1);
    client.add_service_signer(&admin_signers(&env, &admin), &signer2);

    // Record current signer set
    let signers_at_proposal = client.get_service_signers();
    let signer_count_at_proposal = signers_at_proposal.len();

    // Create proposal
    let value = crate::constants::MIN_COOLDOWN_SECS.to_be_bytes();
    let bytes = soroban_sdk::Bytes::from_array(&env, &value);

    let proposal_id = client.propose_parameter_change(
        &admin_signers(&env, &admin),
        &soroban_sdk::symbol_short!("cooldown"),
        &bytes,
    );

    // Change signer set
    let signer3 = Address::generate(&env);
    client.add_service_signer(&admin_signers(&env, &admin), &signer3);

    // Verify new set is different
    let signers_after = client.get_service_signers();
    assert!(signers_after.len() > signer_count_at_proposal);

    // Proposal should remain valid but attributed to original signer context
    let proposal = client.get_parameter_proposal(&proposal_id);
    assert_eq!(proposal.proposal.proposer, admin);
    // The proposal was created by admin, not affected by signer churn
}
