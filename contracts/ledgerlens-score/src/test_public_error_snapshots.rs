use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Bytes, Env, Vec,
};

use crate::{
    test_builders::ContractStateBuilder, Error, LedgerLensScoreContract, LedgerLensScoreContractClient,
    ModelSubmission, ScoreAttestation,
};

#[derive(Debug, PartialEq, Eq)]
struct ErrorSnapshot {
    context: &'static str,
    error: Error,
    paused: bool,
    pending_upgrade: bool,
    score_exists: bool,
}

fn snapshot_for(
    client: &LedgerLensScoreContractClient,
    context: &'static str,
    error: Error,
    wallet: &Address,
) -> ErrorSnapshot {
    ErrorSnapshot {
        context,
        error,
        paused: client.is_paused(),
        pending_upgrade: client.get_pending_upgrade().is_ok(),
        score_exists: client.get_score_exists(wallet, &symbol_short!("XLM_USDC")),
    }
}

#[test]
fn snapshot_contract_paused_blocks_submission_without_state_mutation() {
    let state = ContractStateBuilder::new().paused(true).build();
    let result = state.client.try_submit_score(
        &Vec::new(&state.env),
        &state.wallet,
        &state.pair,
        &42,
        &false,
        &false,
        &100_000,
        &90,
        &1,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::ContractPaused)));
    assert_eq!(
        snapshot_for(&state.client, "submit_score when globally paused", Error::ContractPaused, &state.wallet),
        ErrorSnapshot {
            context: "submit_score when globally paused",
            error: Error::ContractPaused,
            paused: true,
            pending_upgrade: false,
            score_exists: false,
        }
    );
}

#[test]
fn snapshot_consensus_input_empty_uses_dedicated_error_code() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|ledger| ledger.timestamp = 100_000);
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let empty = Vec::<ModelSubmission>::new(&env);
    let result = client.try_submit_consensus_score(&Vec::new(&env), &wallet, &pair, &empty, &100_000);

    assert_eq!(result, Err(Ok(Error::ConsensusInputEmpty)));
    assert_eq!(
        snapshot_for(&client, "submit_consensus_score with empty submissions", Error::ConsensusInputEmpty, &wallet),
        ErrorSnapshot {
            context: "submit_consensus_score with empty submissions",
            error: Error::ConsensusInputEmpty,
            paused: false,
            pending_upgrade: false,
            score_exists: false,
        }
    );
}

#[test]
fn snapshot_no_pending_upgrade_leaves_governance_state_unchanged() {
    let state = ContractStateBuilder::new().with_admin_multisig(2, 2).build();
    let mut admin_signers = Vec::new(&state.env);
    for i in 0..state.admin_signers.len() {
        admin_signers.push_back(state.admin_signers.get(i).unwrap());
    }

    let result = state.client.try_execute_upgrade(&admin_signers);
    assert_eq!(result, Err(Ok(Error::NoPendingUpgrade)));
    assert_eq!(
        snapshot_for(&state.client, "execute_upgrade without proposal", Error::NoPendingUpgrade, &state.wallet),
        ErrorSnapshot {
            context: "execute_upgrade without proposal",
            error: Error::NoPendingUpgrade,
            paused: false,
            pending_upgrade: false,
            score_exists: false,
        }
    );
}

#[test]
fn snapshot_reveal_window_elapsed_preserves_live_score_state() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|ledger| ledger.timestamp = 100_000);
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    let challenger = Address::generate(&env);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let token = Address::generate(&env);
    client.set_fee_token(&token);
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &25,
        &false,
        &false,
        &100_000,
        &90,
        &1,
        &None,
    );
    client.set_reveal_window(&1u64);

    let bond: i128 = 10;
    let salt = Bytes::from_array(&env, &[7u8; 16]);
    let mut commitment = Bytes::new(&env);
    commitment.extend_from_slice(&bond.to_le_bytes());
    commitment.extend_from_slice(&[7u8; 16]);
    client.commit_dispute_bond(&challenger, &wallet, &pair, &commitment);
    env.ledger().with_mut(|ledger| ledger.timestamp = 100_002);

    let result = client.try_open_score_dispute(&challenger, &wallet, &pair, &bond, &salt);
    assert_eq!(result, Err(Ok(Error::RevealWindowExpired)));
    assert_eq!(
        snapshot_for(&client, "open_score_dispute after reveal window", Error::RevealWindowExpired, &wallet),
        ErrorSnapshot {
            context: "open_score_dispute after reveal window",
            error: Error::RevealWindowExpired,
            paused: false,
            pending_upgrade: false,
            score_exists: true,
        }
    );
}

#[test]
fn snapshot_invalid_attestation_does_not_create_score() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    client.set_service_pubkey(&Bytes::from_array(&env, &[3u8; 32]));

    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let attestation = ScoreAttestation {
        commitment: soroban_sdk::BytesN::from_array(&env, &[0u8; 32]),
        signature: soroban_sdk::BytesN::from_array(&env, &[0u8; 65]),
        contract_id: soroban_sdk::BytesN::from_array(&env, &[0u8; 32]),
        contract_version: 4,
        nonce: 1,
    };

    let result = client.try_submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &50,
        &false,
        &false,
        &100_000,
        &90,
        &1,
        &Some(attestation),
    );

    assert_eq!(result, Err(Ok(Error::InvalidAttestation)));
    assert_eq!(
        snapshot_for(&client, "submit_score with invalid attestation", Error::InvalidAttestation, &wallet),
        ErrorSnapshot {
            context: "submit_score with invalid attestation",
            error: Error::InvalidAttestation,
            paused: false,
            pending_upgrade: false,
            score_exists: false,
        }
    );
}
