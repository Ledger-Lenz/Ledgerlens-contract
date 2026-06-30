//! Tests for #297: IQR-based outlier rejection in submit_consensus_score.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Bytes, BytesN, Env, Vec,
};

use crate::{
    LedgerLensScoreContract, LedgerLensScoreContractClient, ModelSubmission, ScoreAttestation,
};

const START_TS: u64 = 1_700_000_000;

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client)
}

fn dummy_attestation(env: &Env) -> ScoreAttestation {
    ScoreAttestation {
        commitment: BytesN::from_array(env, &[0u8; 32]),
        signature: BytesN::from_array(env, &[0u8; 65]),
    }
}

fn make_submission(env: &Env, model: &Address, score: u32) -> ModelSubmission {
    ModelSubmission {
        model_version: 1,
        model: model.clone(),
        score,
        confidence: 80,
        benford_flag: false,
        ml_flag: false,
        attestation: dummy_attestation(env),
    }
}

// ── normal scenario: all scores within IQR range → no rejections ─────────────

#[test]
fn test_normal_submissions_no_rejections() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let service = Address::generate(&env);

    let m1 = Address::generate(&env);
    let m2 = Address::generate(&env);
    let m3 = Address::generate(&env);
    let m4 = Address::generate(&env);

    // Scores tightly clustered: 50, 52, 54, 56 — IQR = 4, no outliers at 1.5×
    let mut subs: Vec<ModelSubmission> = Vec::new(&env);
    subs.push_back(make_submission(&env, &m1, 50));
    subs.push_back(make_submission(&env, &m2, 52));
    subs.push_back(make_submission(&env, &m3, 54));
    subs.push_back(make_submission(&env, &m4, 56));

    let mut signers: Vec<Address> = Vec::new(&env);
    signers.push_back(service.clone());

    client.submit_consensus_score(&signers, &wallet, &pair, &subs, &START_TS);

    // No rejections recorded.
    assert_eq!(client.get_signer_rejection_count(&m1), 0);
    assert_eq!(client.get_signer_rejection_count(&m2), 0);
    assert_eq!(client.get_signer_rejection_count(&m3), 0);
    assert_eq!(client.get_signer_rejection_count(&m4), 0);
}

// ── outlier injection: one score far from median is rejected ──────────────────

#[test]
fn test_outlier_signer_rejected() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let service = Address::generate(&env);

    let m1 = Address::generate(&env);
    let m2 = Address::generate(&env);
    let m3 = Address::generate(&env);
    let m4 = Address::generate(&env);
    let m_outlier = Address::generate(&env);

    // Scores: 48, 50, 52, 54, 99 (outlier)
    // Q1=50, Q3=54, IQR=4, threshold=1.5*4=6 → |99-52|=47 > 6 → outlier
    let mut subs: Vec<ModelSubmission> = Vec::new(&env);
    subs.push_back(make_submission(&env, &m1, 48));
    subs.push_back(make_submission(&env, &m2, 50));
    subs.push_back(make_submission(&env, &m3, 52));
    subs.push_back(make_submission(&env, &m4, 54));
    subs.push_back(make_submission(&env, &m_outlier, 99));

    let mut signers: Vec<Address> = Vec::new(&env);
    signers.push_back(service.clone());

    client.submit_consensus_score(&signers, &wallet, &pair, &subs, &START_TS);

    // Outlier's rejection count incremented.
    assert_eq!(client.get_signer_rejection_count(&m_outlier), 1);
    // Non-outliers not rejected.
    assert_eq!(client.get_signer_rejection_count(&m1), 0);
}

// ── configurable multiplier: larger multiplier allows bigger deviations ───────

#[test]
fn test_large_multiplier_allows_outlier() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    let service = Address::generate(&env);

    // Set multiplier to 2000 (20× IQR) — nothing will be rejected.
    client.set_iqr_rejection_multiplier(&Vec::new(&env), &2000);

    let m1 = Address::generate(&env);
    let m2 = Address::generate(&env);
    let m3 = Address::generate(&env);
    let m4 = Address::generate(&env);
    let m5 = Address::generate(&env);

    let mut subs: Vec<ModelSubmission> = Vec::new(&env);
    subs.push_back(make_submission(&env, &m1, 48));
    subs.push_back(make_submission(&env, &m2, 50));
    subs.push_back(make_submission(&env, &m3, 52));
    subs.push_back(make_submission(&env, &m4, 54));
    subs.push_back(make_submission(&env, &m5, 99)); // would normally be outlier

    let mut signers: Vec<Address> = Vec::new(&env);
    signers.push_back(service.clone());

    client.submit_consensus_score(&signers, &wallet, &pair, &subs, &START_TS);

    // With 20× multiplier, no one is rejected.
    assert_eq!(client.get_signer_rejection_count(&m5), 0);
}

// ── get_signer_rejection_count returns 0 for unknown signer ──────────────────

#[test]
fn test_rejection_count_zero_for_unknown_signer() {
    let (env, client) = setup();
    let unknown = Address::generate(&env);
    assert_eq!(client.get_signer_rejection_count(&unknown), 0);
}
