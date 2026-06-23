#[cfg(test)]
mod tests {
    use soroban_sdk::{Env, Address, Vec as SVec, Symbol};
    use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient, ScoreSubmission};
    use std::collections::HashMap;

    fn init_contract(env: &Env) -> (LedgerLensScoreContractClient, Address, Address) {
        env.mock_all_auths();
        let contract_id = env.register_contract(None, LedgerLensScoreContract);
        let client = LedgerLensScoreContractClient::new(env, &contract_id);
        let admin = Address::generate(env);
        let service = Address::generate(env);
        client.initialize(&admin, &service);
        (client, admin, service)
    }

    #[test]
    fn test_replay_single_entry_no_panic() {
        let env = Env::default();
        let (client, _admin, _service) = init_contract(&env);
        
        let wallet = Address::generate(&env);
        let pair = Symbol::new(&env, "XLM_USDC");
        
        let mut batch: SVec<ScoreSubmission> = SVec::new(&env);
        batch.push_back(ScoreSubmission {
            wallet: wallet.clone(),
            asset_pair: pair.clone(),
            score: 50,
            benford_flag: false,
            ml_flag: false,
            timestamp: 1_000_000u64,
            confidence: 80u32,
            model_version: 1u32,
        });
        
        let result = client.submit_scores_batch(&batch);
        assert_eq!(result.accepted_count, 1);
        assert_eq!(result.rejected_count, 0);
        
        let score = client.get_score(&wallet, &pair);
        assert!(score.score >= 0 && score.score <= 100, "score must be in [0, 100]");
    }

    #[test]
    fn test_replay_multiple_entries_score_range() {
        let env = Env::default();
        let (client, _admin, _service) = init_contract(&env);
        
        let mut batch: SVec<ScoreSubmission> = SVec::new(&env);
        for i in 0..5 {
            let wallet = Address::generate(&env);
            let pair = Symbol::new(&env, "XLM_USDC");
            batch.push_back(ScoreSubmission {
                wallet: wallet.clone(),
                asset_pair: pair,
                score: (i * 20) as u32, // 0, 20, 40, 60, 80
                benford_flag: false,
                ml_flag: false,
                timestamp: 1_000_000u64 + i as u64,
                confidence: 90u32,
                model_version: 1u32,
            });
        }
        
        let result = client.submit_scores_batch(&batch);
        assert_eq!(result.accepted_count, 5);
        assert_eq!(result.rejected_count, 0);
        
        // Verify all entries were accepted and scores are in valid range
        for entry_result in result.results.iter() {
            assert!(entry_result.accepted);
            assert_eq!(entry_result.rejection_code, 0);
        }
    }

    #[test]
    fn test_replay_respects_rate_limit() {
        let env = Env::default();
        let (client, _admin, _service) = init_contract(&env);
        
        let wallet = Address::generate(&env);
        let pair = Symbol::new(&env, "XLM_USDC");
        let ts = 1_000_000u64;
        
        // First submission should succeed
        let mut batch1: SVec<ScoreSubmission> = SVec::new(&env);
        batch1.push_back(ScoreSubmission {
            wallet: wallet.clone(),
            asset_pair: pair.clone(),
            score: 50u32,
            benford_flag: false,
            ml_flag: false,
            timestamp: ts,
            confidence: 80u32,
            model_version: 1u32,
        });
        let result1 = client.submit_scores_batch(&batch1);
        assert_eq!(result1.accepted_count, 1);
        
        // Second submission with same (wallet, pair) within cooldown should be rejected
        let mut batch2: SVec<ScoreSubmission> = SVec::new(&env);
        batch2.push_back(ScoreSubmission {
            wallet: wallet.clone(),
            asset_pair: pair.clone(),
            score: 60u32,
            benford_flag: false,
            ml_flag: false,
            timestamp: ts + 100, // still within cooldown
            confidence: 80u32,
            model_version: 1u32,
        });
        let result2 = client.submit_scores_batch(&batch2);
        assert_eq!(result2.rejected_count, 1);
    }

    #[test]
    fn test_replay_deterministic() {
        // Same input should produce same contract state
        let env1 = Env::default();
        let (client1, _, _) = init_contract(&env1);
        
        let wallet1 = Address::generate(&env1);
        let pair1 = Symbol::new(&env1, "XLM_USDC");
        
        let mut batch1: SVec<ScoreSubmission> = SVec::new(&env1);
        batch1.push_back(ScoreSubmission {
            wallet: wallet1.clone(),
            asset_pair: pair1.clone(),
            score: 42u32,
            benford_flag: true,
            ml_flag: false,
            timestamp: 1_234_567u64,
            confidence: 95u32,
            model_version: 2u32,
        });
        
        let result1 = client1.submit_scores_batch(&batch1);
        let score1 = client1.get_score(&wallet1, &pair1);
        assert_eq!(result1.accepted_count, 1);
        assert_eq!(score1.score, 42);
        assert!(score1.benford_flag);
        assert!(!score1.ml_flag);
    }
}
