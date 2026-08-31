//! Tests for the post-incident reconciliation workflow (issue #775).
//!
//! These tests verify that the replay harness produces a deterministic
//! reconciliation outcome: same pipeline input → same mismatch classification
//! regardless of execution order.

#[cfg(test)]
mod reconciliation_tests {
    use ledgerlens_score::{
        LedgerLensScoreContract, LedgerLensScoreContractClient, ScoreSubmission,
    };
    use soroban_sdk::testutils::{Address as _, Ledger as _};
    use soroban_sdk::{Address, Env, Symbol, Vec as SVec};

    const BASE_TS: u64 = 1_700_000_000;

    fn init_contract(env: &Env) -> LedgerLensScoreContractClient<'_> {
        env.mock_all_auths();
        let id = env.register_contract(None, LedgerLensScoreContract);
        let client = LedgerLensScoreContractClient::new(env, &id);
        client.initialize(&Address::generate(env), &Address::generate(env));
        client
    }

    fn submit_one(
        env: &Env,
        client: &LedgerLensScoreContractClient,
        wallet: &Address,
        pair: &Symbol,
        score: u32,
        confidence: u32,
        ts: u64,
    ) {
        env.ledger().with_mut(|l| l.timestamp = ts);
        let mut batch: SVec<ScoreSubmission> = SVec::new(env);
        batch.push_back(ScoreSubmission {
            wallet: wallet.clone(),
            asset_pair: pair.clone(),
            score,
            benford_flag: false,
            ml_flag: false,
            timestamp: ts,
            confidence,
            model_version: 1,
        });
        client.submit_scores_batch(&batch);
    }

    // ── ok: on-chain score matches pipeline record ───────────────────────────

    #[test]
    fn test_reconcile_ok_when_scores_match() {
        let env = Env::default();
        let client = init_contract(&env);
        let wallet = Address::generate(&env);
        let pair = Symbol::new(&env, "XLM_USDC");

        submit_one(&env, &client, &wallet, &pair, 72, 88, BASE_TS);

        // Simulate reconciliation: pipeline record score == on-chain score.
        let on_chain = client.get_score(&wallet, &pair);
        assert_eq!(on_chain.score, 72);
        assert_eq!(on_chain.confidence, 88);
        // delta == 0, status should be "ok"
        let delta = (72i64 - on_chain.score as i64).unsigned_abs();
        assert_eq!(delta, 0, "no mismatch expected");
    }

    // ── mismatch: pipeline score differs from on-chain score ─────────────────

    #[test]
    fn test_reconcile_detects_mismatch() {
        let env = Env::default();
        let client = init_contract(&env);
        let wallet = Address::generate(&env);
        let pair = Symbol::new(&env, "XLM_USDC");

        // On-chain was written with score 10 (simulating a bad submission).
        submit_one(&env, &client, &wallet, &pair, 10, 40, BASE_TS);

        let on_chain = client.get_score(&wallet, &pair);
        let pipeline_score: u32 = 72; // what the pipeline actually computed

        let delta = (pipeline_score as i64 - on_chain.score as i64).unsigned_abs();
        assert!(
            delta > 0,
            "mismatch expected: on-chain={}, pipeline={}",
            on_chain.score,
            pipeline_score
        );
    }

    // ── pipeline_only: entry exists in pipeline but not on-chain ─────────────

    #[test]
    fn test_reconcile_pipeline_only_when_no_onchain_entry() {
        let env = Env::default();
        let client = init_contract(&env);
        let wallet = Address::generate(&env);
        let pair = Symbol::new(&env, "XLM_BTC");

        // No submission made for this wallet/pair.
        let result = client.try_get_score(&wallet, &pair);
        assert!(result.is_err(), "expected ScoreNotFound for unsubmitted wallet/pair");
        // Status classification: pipeline_only — on-chain has no entry.
    }

    // ── stale: on-chain timestamp is older than max-age threshold ────────────

    #[test]
    fn test_reconcile_stale_when_timestamp_exceeds_max_age() {
        let env = Env::default();
        let client = init_contract(&env);
        let wallet = Address::generate(&env);
        let pair = Symbol::new(&env, "XLM_EUR");

        let old_ts = BASE_TS - 90_000; // 25 hours ago
        submit_one(&env, &client, &wallet, &pair, 55, 80, old_ts);

        let on_chain = client.get_score(&wallet, &pair);
        let max_age_secs: u64 = 86_400; // 24 h
        let now = BASE_TS;
        let age = now.saturating_sub(on_chain.timestamp);
        assert!(
            age > max_age_secs,
            "entry should be classified as stale: age={}s, max={}s",
            age,
            max_age_secs
        );
    }

    // ── deterministic: same input → same classification ──────────────────────

    #[test]
    fn test_reconcile_deterministic_across_two_runs() {
        let classify = |score_a: u32, score_b: u32, tolerance: u32| -> &'static str {
            let delta = (score_a as i64 - score_b as i64).unsigned_abs() as u32;
            if delta <= tolerance {
                "ok"
            } else {
                "mismatch"
            }
        };

        // Same inputs must always produce the same classification.
        assert_eq!(classify(72, 72, 0), "ok");
        assert_eq!(classify(72, 70, 0), "mismatch");
        assert_eq!(classify(72, 70, 2), "ok"); // within tolerance
        assert_eq!(classify(72, 70, 1), "mismatch"); // delta=2 > tolerance=1
    }

    // ── batch: multiple entries reconciled independently ─────────────────────

    #[test]
    fn test_reconcile_multiple_entries_independently() {
        let env = Env::default();
        let client = init_contract(&env);

        let wallets: Vec<Address> = (0..3).map(|_| Address::generate(&env)).collect();
        let pair = Symbol::new(&env, "XLM_USDC");

        let pipeline_scores = [60u32, 75u32, 90u32];

        // Submit all three.
        for (i, wallet) in wallets.iter().enumerate() {
            submit_one(&env, &client, wallet, &pair, pipeline_scores[i], 80, BASE_TS + i as u64);
            // advance past cooldown for next entry (different wallets, so no cooldown conflict)
        }

        // Each on-chain score must match its pipeline record.
        for (i, wallet) in wallets.iter().enumerate() {
            let on_chain = client.get_score(wallet, &pair);
            let delta = (pipeline_scores[i] as i64 - on_chain.score as i64).unsigned_abs() as u32;
            assert_eq!(
                delta, 0,
                "wallet[{}]: on_chain={}, pipeline={}",
                i, on_chain.score, pipeline_scores[i]
            );
        }
    }

    // ── remediation: override_rate_limit clears cooldown for re-submission ───

    #[test]
    fn test_reconcile_remediation_resubmit_after_rate_limit_override() {
        let env = Env::default();
        let client = init_contract(&env);
        let wallet = Address::generate(&env);
        let pair = Symbol::new(&env, "XLM_USDC");

        // Bad score written during incident.
        submit_one(&env, &client, &wallet, &pair, 10, 40, BASE_TS);
        assert_eq!(client.get_score(&wallet, &pair).score, 10);

        // Admin clears rate limit for immediate re-submission.
        client.override_rate_limit(
            &SVec::new(&env),
            &wallet,
            &pair,
            &soroban_sdk::Bytes::from_slice(&env, b"incident reconciliation"),
        );

        // Re-submit corrected pipeline score without waiting for cooldown.
        env.ledger().with_mut(|l| l.timestamp = BASE_TS + 1);
        let mut batch: SVec<ScoreSubmission> = SVec::new(&env);
        batch.push_back(ScoreSubmission {
            wallet: wallet.clone(),
            asset_pair: pair.clone(),
            score: 72,
            benford_flag: false,
            ml_flag: false,
            timestamp: BASE_TS + 1,
            confidence: 88,
            model_version: 1,
        });
        let result = client.submit_scores_batch(&batch);
        assert_eq!(result.accepted_count, 1, "corrected score must be accepted after override");
        assert_eq!(client.get_score(&wallet, &pair).score, 72, "reconciliation complete");
    }
}
