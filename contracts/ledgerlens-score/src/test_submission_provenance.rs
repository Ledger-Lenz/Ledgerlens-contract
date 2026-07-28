/// #688 — Submission provenance snapshot tests.
///
/// Verifies that `get_submission_provenance` returns accurate policy and
/// signer context recorded at the moment of each accepted submission.
#[cfg(test)]
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger},
    Address, Env, Vec,
};

#[cfg(test)]
use crate::LedgerLensScoreContract;

#[cfg(test)]
fn setup() -> (
    Env,
    crate::LedgerLensScoreContractClient<'static>,
    Address,
    Address,
    Address,
    soroban_sdk::Symbol,
) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = crate::LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    env.ledger().with_mut(|l| l.timestamp = 2_000_000);
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    (env, client, admin, service, wallet, pair)
}

/// Before any submission, get_submission_provenance returns ScoreNotFound.
#[test]
fn test_no_provenance_before_submission() {
    let (env, client, _admin, _service, wallet, pair) = setup();
    let result = client.try_get_submission_provenance(&wallet, &pair);
    assert!(result.is_err() || result.unwrap().is_err());
}

/// After submit_score, provenance captures model_version and validation_branch.
#[test]
fn test_provenance_after_submit_score() {
    let (env, client, _admin, _service, wallet, pair) = setup();
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &42,
        &false,
        &false,
        &1,
        &90,
        &5, // model_version = 5
        &None,
    );
    let prov = client.get_submission_provenance(&wallet, &pair).unwrap();
    assert_eq!(prov.model_version, 5, "model_version must match submitted value");
    assert_eq!(
        prov.validation_branch,
        symbol_short!("single"),
        "single-service path must record 'single'"
    );
    assert_eq!(prov.submitted_at, 2_000_000, "submitted_at must match ledger timestamp");
    assert!(!prov.score_floor_enabled, "floor not enabled by default");
}

/// Provenance epoch_id matches the current epoch.
#[test]
fn test_provenance_epoch_id() {
    let (env, client, _admin, _service, wallet, pair) = setup();
    // Open epoch 7
    client.open_epoch(&Vec::new(&env), &7);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &50, &false, &false, &1, &80, &1, &None);
    let prov = client.get_submission_provenance(&wallet, &pair).unwrap();
    assert_eq!(prov.epoch_id, 7, "epoch_id must reflect the open epoch at acceptance");
}

/// Provenance cooldown_secs reflects the global cooldown at acceptance time.
#[test]
fn test_provenance_cooldown_secs() {
    let (env, client, _admin, _service, wallet, pair) = setup();
    // Default cooldown is 3600 seconds
    client.submit_score(&Vec::new(&env), &wallet, &pair, &30, &false, &false, &1, &70, &1, &None);
    let prov = client.get_submission_provenance(&wallet, &pair).unwrap();
    assert_eq!(prov.cooldown_secs, 3600, "default cooldown must be 3600 seconds");
}

/// Provenance score_floor_enabled is true when floor policy is active.
#[test]
fn test_provenance_score_floor_fields() {
    let (env, client, admin, _service, wallet, pair) = setup();
    // Enable score floor with HWM=80, floor=20
    client.set_score_floor_policy(&Vec::new(&env), &true, &80, &20);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &85, &false, &false, &1, &90, &1, &None);
    let prov = client.get_submission_provenance(&wallet, &pair).unwrap();
    assert!(prov.score_floor_enabled, "score_floor_enabled must be true");
    assert_eq!(prov.score_floor_high_water_mark, 80);
    assert_eq!(prov.score_floor_value, 20);
}

/// Provenance updates on each subsequent submission.
#[test]
fn test_provenance_updates_on_resubmit() {
    let (env, client, _admin, _service, wallet, pair) = setup();
    // First submission
    client.submit_score(&Vec::new(&env), &wallet, &pair, &10, &false, &false, &1, &80, &1, &None);
    let prov1 = client.get_submission_provenance(&wallet, &pair).unwrap();
    assert_eq!(prov1.model_version, 1);

    // Advance past cooldown and resubmit with model_version=2
    env.ledger().with_mut(|l| l.timestamp += 3_601);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &20, &false, &false, &2, &85, &2, &None);
    let prov2 = client.get_submission_provenance(&wallet, &pair).unwrap();
    assert_eq!(prov2.model_version, 2, "provenance must update to latest submission");
    assert!(prov2.submitted_at > prov1.submitted_at, "submitted_at must advance");
}

/// Batch submissions record validation_branch = "batch".
#[test]
fn test_provenance_batch_validation_branch() {
    use crate::ScoreSubmission;

    let (env, client, _admin, _service, _wallet, pair) = setup();
    let wallet = Address::generate(&env);

    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: wallet.clone(),
        asset_pair: pair.clone(),
        score: 55,
        benford_flag: false,
        ml_flag: false,
        timestamp: 1,
        confidence: 80,
        model_version: 3,
    });

    let result = client.submit_scores_batch(&batch);
    assert_eq!(result.accepted_count, 1);

    let prov = client.get_submission_provenance(&wallet, &pair).unwrap();
    assert_eq!(prov.model_version, 3);
    assert_eq!(
        prov.validation_branch,
        symbol_short!("batch"),
        "batch path must record 'batch' as the validation_branch"
    );
}

/// Different wallets/pairs each get independent provenance records.
#[test]
fn test_provenance_independent_per_wallet_pair() {
    use crate::ScoreSubmission;

    let (env, client, _admin, _service, _wallet, _pair) = setup();
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("XLM_BTC");
    let wallet_a = Address::generate(&env);
    let wallet_b = Address::generate(&env);

    client.submit_score(
        &Vec::new(&env),
        &wallet_a,
        &pair_a,
        &10,
        &false,
        &false,
        &1,
        &80,
        &1,
        &None,
    );
    client.submit_score(
        &Vec::new(&env),
        &wallet_b,
        &pair_b,
        &90,
        &true,
        &true,
        &1,
        &95,
        &2,
        &None,
    );

    let prov_a = client.get_submission_provenance(&wallet_a, &pair_a).unwrap();
    let prov_b = client.get_submission_provenance(&wallet_b, &pair_b).unwrap();

    assert_eq!(prov_a.model_version, 1);
    assert_eq!(prov_b.model_version, 2);
    // Wallet_b's provenance must not bleed into wallet_a's
    assert_ne!(prov_a.model_version, prov_b.model_version);
}
