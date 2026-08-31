//! Tests for named administrative capability policies partitioned by
//! operation risk (issue #695). Each of `Policy::{ScorePolicy,
//! UpgradeGovernance, EmergencyPause, SignerAdmin}` gets its own optional
//! separate-approver policy (configured via `set_policy_approval`),
//! disjoint from routine admin quorum and from every other policy's
//! approver — see `LedgerLensScoreContract::require_policy_auth` in
//! `lib.rs`. `Policy::DataDeletion` continues to use the pre-existing
//! `set_deletion_approval_policy` (see `test_deletion_policy.rs`).

use soroban_sdk::{
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal, Vec,
};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient, Policy};

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

// ── Success path: default (disabled) policy is a no-op ────────────────────────

#[test]
fn test_pause_allowed_when_emergency_pause_policy_disabled() {
    let (env, client, _admin, _service) = setup();
    assert_eq!(client.try_pause(&Vec::new(&env)), Ok(Ok(())));
}

#[test]
fn test_get_policy_approval_defaults_to_disabled() {
    let (_env, client, _admin, _service) = setup();
    let cfg = client.get_policy_approval(&Policy::SignerAdmin);
    assert!(!cfg.enabled);
    assert_eq!(cfg.approver, None);
}

// ── Boundary: fail-closed configuration ────────────────────────────────────────

#[test]
fn test_enabling_policy_without_approver_is_rejected() {
    let (env, client, _admin, _service) = setup();
    let result =
        client.try_set_policy_approval(&Vec::new(&env), &Policy::EmergencyPause, &true, &None);
    assert_eq!(result, Err(Ok(Error::InvalidThreshold)));
}

#[test]
fn test_enabling_policy_with_admin_as_approver_is_rejected() {
    let (env, client, admin, _service) = setup();
    let result = client.try_set_policy_approval(
        &Vec::new(&env),
        &Policy::EmergencyPause,
        &true,
        &Some(admin),
    );
    assert_eq!(result, Err(Ok(Error::InvalidThreshold)));
}

#[test]
fn test_data_deletion_rejected_at_generic_policy_entry_point() {
    let (env, client, _admin, _service) = setup();
    let approver = Address::generate(&env);
    let result = client.try_set_policy_approval(
        &Vec::new(&env),
        &Policy::DataDeletion,
        &true,
        &Some(approver),
    );
    assert_eq!(result, Err(Ok(Error::InvalidPolicy)));
}

#[test]
fn test_get_policy_approval_reports_enabled_config() {
    let (env, client, _admin, _service) = setup();
    let approver = Address::generate(&env);
    client.set_policy_approval(
        &Vec::new(&env),
        &Policy::SignerAdmin,
        &true,
        &Some(approver.clone()),
    );
    let cfg = client.get_policy_approval(&Policy::SignerAdmin);
    assert!(cfg.enabled);
    assert_eq!(cfg.approver, Some(approver));
}

// ── Adversarial: enabled policy actually gates the mapped endpoint ─────────────

#[test]
fn test_pause_succeeds_when_correct_approver_authorizes() {
    let (env, client, admin, _service) = setup();
    let approver = Address::generate(&env);
    client.set_policy_approval(
        &Vec::new(&env),
        &Policy::EmergencyPause,
        &true,
        &Some(approver.clone()),
    );

    let admin_signers: Vec<Address> = Vec::new(&env);
    client
        .mock_auths(&[
            MockAuth {
                address: &admin,
                invoke: &MockAuthInvoke {
                    contract: &client.address,
                    fn_name: "pause",
                    args: (admin_signers.clone(),).into_val(&env),
                    sub_invokes: &[],
                },
            },
            MockAuth {
                address: &approver,
                invoke: &MockAuthInvoke {
                    contract: &client.address,
                    fn_name: "pause",
                    args: (admin_signers.clone(),).into_val(&env),
                    sub_invokes: &[],
                },
            },
        ])
        .pause(&admin_signers);
    assert!(client.is_paused());
}

#[test]
#[should_panic]
fn test_pause_without_approver_auth_panics_when_policy_enabled() {
    let (env, client, admin, _service) = setup();
    let approver = Address::generate(&env);
    client.set_policy_approval(&Vec::new(&env), &Policy::EmergencyPause, &true, &Some(approver));

    let admin_signers: Vec<Address> = Vec::new(&env);
    client
        .mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &client.address,
                fn_name: "pause",
                args: (admin_signers.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .pause(&admin_signers);
}

/// Cross-policy privilege reuse must fail (issue #695 acceptance
/// criterion): a *currently configured, real* approver for `SignerAdmin`
/// cannot stand in for `EmergencyPause`'s approver, even though both
/// endpoints share the same routine admin-quorum check underneath.
#[test]
#[should_panic]
fn test_cross_policy_approver_reuse_fails() {
    let (env, client, admin, _service) = setup();
    let pause_approver = Address::generate(&env);
    let signer_admin_approver = Address::generate(&env);
    client.set_policy_approval(
        &Vec::new(&env),
        &Policy::EmergencyPause,
        &true,
        &Some(pause_approver),
    );
    client.set_policy_approval(
        &Vec::new(&env),
        &Policy::SignerAdmin,
        &true,
        &Some(signer_admin_approver.clone()),
    );

    let admin_signers: Vec<Address> = Vec::new(&env);
    client
        .mock_auths(&[
            MockAuth {
                address: &admin,
                invoke: &MockAuthInvoke {
                    contract: &client.address,
                    fn_name: "pause",
                    args: (admin_signers.clone(),).into_val(&env),
                    sub_invokes: &[],
                },
            },
            MockAuth {
                address: &signer_admin_approver,
                invoke: &MockAuthInvoke {
                    contract: &client.address,
                    fn_name: "pause",
                    args: (admin_signers.clone(),).into_val(&env),
                    sub_invokes: &[],
                },
            },
        ])
        .pause(&admin_signers);
}

/// `ScorePolicy` and `UpgradeGovernance` are independently configurable —
/// setting one does not affect the other's stored approver.
#[test]
fn test_score_policy_and_upgrade_governance_are_independently_configurable() {
    let (env, client, _admin, _service) = setup();
    let score_approver = Address::generate(&env);
    let upgrade_approver = Address::generate(&env);
    client.set_policy_approval(
        &Vec::new(&env),
        &Policy::ScorePolicy,
        &true,
        &Some(score_approver.clone()),
    );
    client.set_policy_approval(
        &Vec::new(&env),
        &Policy::UpgradeGovernance,
        &true,
        &Some(upgrade_approver.clone()),
    );

    let cfg_score = client.get_policy_approval(&Policy::ScorePolicy);
    let cfg_upgrade = client.get_policy_approval(&Policy::UpgradeGovernance);
    assert_eq!(cfg_score.approver, Some(score_approver));
    assert_eq!(cfg_upgrade.approver, Some(upgrade_approver));
    assert_ne!(cfg_score.approver, cfg_upgrade.approver);
}
