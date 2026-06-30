//! Instruction-count benchmark for lazy TTL extension in batch score submission.

use soroban_sdk::{
    testutils::{storage::Persistent as _, Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
};

use crate::{
    constants::MAX_BATCH_SIZE,
    storage,
    types::{RiskScore, ScoreSubmission},
    LedgerLensScoreContract, LedgerLensScoreContractClient,
};

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Symbol) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_700_000_000);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    let asset_pair = Symbol::new(&env, "XLM_USDC");
    (env, client, asset_pair)
}

fn build_batch(env: &Env, asset_pair: &Symbol, count: u32, score_base: u32) -> Vec<ScoreSubmission> {
    let mut batch = Vec::new(env);
    for i in 0..count {
        let wallet = Address::generate(env);
        batch.push_back(ScoreSubmission {
            wallet,
            asset_pair: asset_pair.clone(),
            score: score_base + i,
            benford_flag: false,
            ml_flag: false,
            timestamp: 1_700_000_000,
            confidence: 90,
            model_version: 1,
        });
    }
    batch
}

fn risk_score(sub: &ScoreSubmission) -> RiskScore {
    RiskScore {
        score: sub.score,
        benford_flag: sub.benford_flag,
        ml_flag: sub.ml_flag,
        timestamp: sub.timestamp,
        confidence: sub.confidence,
        model_version: sub.model_version,
    }
}

#[test]
fn test_batch_resubmit_lazy_ttl_reduces_instructions() {
    let (env, client, asset_pair) = setup();
    let batch = build_batch(&env, &asset_pair, MAX_BATCH_SIZE, 40);

    let contract_id = client.address.clone();
    env.as_contract(&contract_id, || {
        // Prewarm entries without a full batch submit (avoids multi-MB test snapshots).
        for i in 0..batch.len() {
            let sub = batch.get(i).unwrap();
            storage::set_score(&env, &sub.wallet, &asset_pair, &risk_score(&sub));
        }

        storage::reset_test_extend_count(&env);

        for i in 0..batch.len() {
            let sub = batch.get(i).unwrap();
            storage::set_score_eager_ttl(&env, &sub.wallet, &asset_pair, &risk_score(&sub));
        }
        let eager_extends = storage::test_extend_count(&env);

        storage::reset_test_extend_count(&env);

        for i in 0..batch.len() {
            let sub = batch.get(i).unwrap();
            storage::set_score(&env, &sub.wallet, &asset_pair, &risk_score(&sub));
        }
        let lazy_extends = storage::test_extend_count(&env);

        let reduction_pct =
            (eager_extends.saturating_sub(lazy_extends) as f64 / eager_extends as f64) * 100.0;

        assert!(
            reduction_pct >= 15.0,
            "expected >=15% fewer extend_ttl calls on pre-warmed {MAX_BATCH_SIZE}-entry resubmit; \
             eager={eager_extends}, lazy={lazy_extends}, reduction={reduction_pct:.1}%"
        );
    });
}

#[test]
fn test_batch_resubmit_lazy_ttl_preserves_scores() {
    let (env, client, asset_pair) = setup();
    // Small batch keeps Soroban test snapshots manageable while still exercising
    // the submit_scores_batch path end-to-end.
    let batch = build_batch(&env, &asset_pair, 3, 55);
    let _ = client.submit_scores_batch(&batch);

    env.ledger().with_mut(|l| l.timestamp += 3_601);

    let _ = client.submit_scores_batch(&batch);

    for i in 0..batch.len() {
        let sub = batch.get(i).unwrap();
        assert_eq!(client.get_score(&sub.wallet, &asset_pair).score, 55 + i);
    }
}

#[test]
fn test_lazy_ttl_skips_extend_when_entry_is_fresh() {
    let (env, client, asset_pair) = setup();
    let wallet = Address::generate(&env);
    let pair = asset_pair.clone();
    let contract_id = client.address.clone();

    env.as_contract(&contract_id, || {
        let score = RiskScore {
            score: 42,
            benford_flag: false,
            ml_flag: false,
            timestamp: 1_700_000_000,
            confidence: 90,
            model_version: 1,
        };
        storage::set_score(&env, &wallet, &pair, &score);

        let ttl_after_first = env.storage().persistent().get_ttl(&crate::types::DataKey::Score(
            wallet.clone(),
            pair.clone(),
        ));

        storage::set_score(&env, &wallet, &pair, &score);

        let ttl_after_second = env.storage().persistent().get_ttl(&crate::types::DataKey::Score(
            wallet.clone(),
            pair.clone(),
        ));

        assert_eq!(ttl_after_first, ttl_after_second);
    });
}
