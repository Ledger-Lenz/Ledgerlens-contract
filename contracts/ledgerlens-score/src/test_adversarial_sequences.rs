use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    BytesN, Vec,
};

use crate::{test_builders::ContractStateBuilder, Error, LedgerLensScoreContractClient};

#[derive(Clone, Copy, Debug)]
enum Op {
    Pause,
    Unpause,
    Submit { timestamp: u64, score: u32 },
    ProposeUpgrade,
    ExecuteUpgrade,
    RemoveServiceSigner,
}

fn assert_invariants(
    client: &LedgerLensScoreContractClient,
    paused: bool,
    service_threshold: u32,
    expected_service_signers: u32,
    pending_upgrade: bool,
) {
    assert_eq!(client.is_paused(), paused);
    assert_eq!(client.get_service_threshold(), service_threshold);
    assert_eq!(client.get_service_signers().len(), expected_service_signers);
    assert_eq!(client.get_pending_upgrade().is_ok(), pending_upgrade);
}

fn run_prefix(ops: &[Op]) -> Result<(), (usize, Error)> {
    let state = ContractStateBuilder::new()
        .with_service_multisig(2, 2)
        .with_admin_multisig(2, 2)
        .build();

    let mut paused = false;
    let mut pending_upgrade = false;
    let mut expected_service_signers = state.client.get_service_signers().len();
    let mut service_threshold = 2;

    let mut admin_approvers = Vec::new(&state.env);
    for i in 0..state.admin_signers.len() {
        admin_approvers.push_back(state.admin_signers.get(i).unwrap());
    }

    let mut service_submitters = Vec::new(&state.env);
    for i in 0..state.service_signers.len() {
        service_submitters.push_back(state.service_signers.get(i).unwrap());
    }

    for (idx, op) in ops.iter().enumerate() {
        let result = match op {
            Op::Pause => {
                paused = true;
                state.client.try_pause(&admin_approvers)
            }
            Op::Unpause => {
                paused = false;
                state.client.try_unpause(&admin_approvers)
            }
            Op::Submit { timestamp, score } => {
                state.env.ledger().with_mut(|ledger| ledger.timestamp = *timestamp);
                state.client.try_submit_score(
                    &service_submitters,
                    &state.wallet,
                    &state.pair,
                    score,
                    &false,
                    &false,
                    timestamp,
                    &90,
                    &1,
                    &None,
                )
            }
            Op::ProposeUpgrade => {
                pending_upgrade = true;
                let hash = BytesN::from_array(&state.env, &[7u8; 32]);
                state.client.try_propose_upgrade(&admin_approvers, &hash)
            }
            Op::ExecuteUpgrade => {
                state.env.ledger().with_mut(|ledger| ledger.timestamp += 172_800);
                pending_upgrade = false;
                state.client.try_execute_upgrade(&admin_approvers)
            }
            Op::RemoveServiceSigner => {
                let signer = state.service_signers.get(0).unwrap();
                expected_service_signers -= 1;
                if service_threshold > expected_service_signers {
                    service_threshold = expected_service_signers;
                }
                state.client.try_remove_service_signer(&admin_approvers, &signer)
            }
        };

        match result {
            Ok(Ok(())) => {
                assert_invariants(
                    &state.client,
                    paused,
                    service_threshold,
                    expected_service_signers,
                    pending_upgrade,
                );
            }
            Err(Ok(err)) => {
                assert_invariants(
                    &state.client,
                    state.client.is_paused(),
                    service_threshold,
                    state.client.get_service_signers().len(),
                    state.client.get_pending_upgrade().is_ok(),
                );
                return Err((idx, err));
            }
            other => panic!("unexpected host result at step {idx}: {other:?}"),
        }
    }

    Ok(())
}

#[test]
fn builders_make_defaults_and_complex_state_explicit() {
    let state = ContractStateBuilder::new()
        .with_service_multisig(3, 2)
        .with_admin_multisig(2, 2)
        .with_finality_buffer(300)
        .build();

    assert_eq!(state.client.get_service_threshold(), 2);
    assert_eq!(state.client.get_service_signers().len(), 3);
    assert_eq!(state.client.get_admin_set().len(), 2);
    assert_eq!(state.client.get_admin_threshold(), 2);
    assert_eq!(state.client.get_finality_buffer(), 300);
    assert!(!state.client.is_paused());
}

#[test]
fn builders_replay_deterministic_history() {
    const HISTORY: &[(u64, u32, u32)] = &[(100_000, 15, 91), (103_700, 25, 92)];
    let state = ContractStateBuilder::new().with_score_history(HISTORY).build();

    let history = state.client.get_score_history(&state.wallet, &state.pair);
    assert_eq!(history.len(), 2);
    assert_eq!(history.get(0).unwrap().score, 15);
    assert_eq!(history.get(1).unwrap().score, 25);
}

#[test]
fn adversarial_sequence_preserves_invariants_after_each_successful_step() {
    let ops = [
        Op::Submit { timestamp: 103_601, score: 41 },
        Op::Pause,
        Op::Unpause,
        Op::ProposeUpgrade,
        Op::RemoveServiceSigner,
    ];

    assert_eq!(run_prefix(&ops), Ok(()));
}

#[test]
fn adversarial_sequence_reports_shortest_failing_prefix() {
    let ops = [
        Op::Pause,
        Op::Submit { timestamp: 103_601, score: 50 },
        Op::Unpause,
        Op::ExecuteUpgrade,
    ];

    let failure = run_prefix(&ops).unwrap_err();
    assert_eq!(failure.0, 1);
    assert_eq!(failure.1, Error::ContractPaused);
    assert_eq!(run_prefix(&ops[..=failure.0]), Err((1, Error::ContractPaused)));
}

#[test]
fn adversarial_sequence_catches_upgrade_without_proposal_as_minimal_failure() {
    let ops = [Op::ExecuteUpgrade];

    let failure = run_prefix(&ops).unwrap_err();
    assert_eq!(failure, (0, Error::NoPendingUpgrade));
}
