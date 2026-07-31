#[cfg(test)]
mod tests {
    use ledgerlens_score::{
        LedgerLensScoreContract, LedgerLensScoreContractClient, ScoreSubmission,
    };
    use replay::{compare_config_manifests, parse_manifest_json, recommended_manifest_template};
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{Address, Env, Symbol, Vec as SVec};

    fn init_contract(env: &Env) -> (LedgerLensScoreContractClient<'_>, Address, Address) {
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
        assert!(score.score <= 100, "score must be in [0, 100]");
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
                score: (i * 20) as u32,
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

        for entry_result in result.results.iter() {
            assert!(entry_result.accepted);
            assert_eq!(entry_result.rejection_code, 0);
        }
    }

    #[test]
    fn test_replay_respects_rate_limit() {
        use soroban_sdk::testutils::Ledger as _;
        let env = Env::default();
        let (client, _admin, _service) = init_contract(&env);

        let wallet = Address::generate(&env);
        let pair = Symbol::new(&env, "XLM_USDC");
        let ts = 1_000_000u64;

        env.ledger().with_mut(|l| l.timestamp = ts);

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

        env.ledger().with_mut(|l| l.timestamp = ts + 100);

        let mut batch2: SVec<ScoreSubmission> = SVec::new(&env);
        batch2.push_back(ScoreSubmission {
            wallet: wallet.clone(),
            asset_pair: pair.clone(),
            score: 60u32,
            benford_flag: false,
            ml_flag: false,
            timestamp: ts + 100,
            confidence: 80u32,
            model_version: 1u32,
        });
        let result2 = client.submit_scores_batch(&batch2);
        assert_eq!(result2.rejected_count, 1);
    }

    #[test]
    fn test_replay_deterministic() {
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

    // ── Adversarial test scenarios ──────────────────────

    #[test]
    fn test_adversarial_score_101_rejected() {
        let env = Env::default();
        let (client, _admin, _service) = init_contract(&env);

        let wallet = Address::generate(&env);
        let pair = Symbol::new(&env, "XLM_USDC");

        let mut batch: SVec<ScoreSubmission> = SVec::new(&env);
        batch.push_back(ScoreSubmission {
            wallet: wallet.clone(),
            asset_pair: pair.clone(),
            score: 101,
            benford_flag: false,
            ml_flag: false,
            timestamp: env.ledger().timestamp(),
            confidence: 80,
            model_version: 1,
        });

        let result = client.submit_scores_batch(&batch);
        assert_eq!(result.accepted_count, 0);
        assert_eq!(result.rejected_count, 1);
        assert_eq!(
            result.results.get(0).unwrap().rejection_code,
            ledgerlens_score::Error::InvalidScore as u32
        );
    }

    #[test]
    fn test_adversarial_timestamp_zero_rejected() {
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
            timestamp: 0,
            confidence: 80,
            model_version: 1,
        });

        let result = client.submit_scores_batch(&batch);
        assert_eq!(result.accepted_count, 0);
        assert_eq!(result.rejected_count, 1);
        assert_eq!(
            result.results.get(0).unwrap().rejection_code,
            ledgerlens_score::Error::InvalidTimestamp as u32
        );
    }

    #[test]
    fn test_adversarial_repeated_submission_rate_limited() {
        use soroban_sdk::testutils::Ledger as _;
        let env = Env::default();
        let (client, _admin, _service) = init_contract(&env);

        let wallet = Address::generate(&env);
        let pair = Symbol::new(&env, "XLM_USDC");
        let ts = 1_000_000u64;

        env.ledger().with_mut(|l| l.timestamp = ts);

        let mut batch1: SVec<ScoreSubmission> = SVec::new(&env);
        batch1.push_back(ScoreSubmission {
            wallet: wallet.clone(),
            asset_pair: pair.clone(),
            score: 50,
            benford_flag: false,
            ml_flag: false,
            timestamp: ts,
            confidence: 80,
            model_version: 1,
        });
        let result1 = client.submit_scores_batch(&batch1);
        assert_eq!(result1.accepted_count, 1);

        env.ledger().with_mut(|l| l.timestamp = ts + 100);

        let mut batch2: SVec<ScoreSubmission> = SVec::new(&env);
        batch2.push_back(ScoreSubmission {
            wallet: wallet.clone(),
            asset_pair: pair.clone(),
            score: 55,
            benford_flag: false,
            ml_flag: false,
            timestamp: ts + 100,
            confidence: 80,
            model_version: 1,
        });
        let result2 = client.submit_scores_batch(&batch2);
        assert_eq!(result2.rejected_count, 1);
        assert_eq!(
            result2.results.get(0).unwrap().rejection_code,
            ledgerlens_score::Error::RateLimitExceeded as u32
        );
    }

    #[test]
    fn test_adversarial_paused_pair_rejected() {
        let env = Env::default();
        let (client, _admin, _service) = init_contract(&env);

        let pair = Symbol::new(&env, "XLM_USDC");
        client.set_pair_paused(&pair, &true);

        let wallet = Address::generate(&env);

        let mut batch: SVec<ScoreSubmission> = SVec::new(&env);
        batch.push_back(ScoreSubmission {
            wallet: wallet.clone(),
            asset_pair: pair.clone(),
            score: 50,
            benford_flag: false,
            ml_flag: false,
            timestamp: env.ledger().timestamp(),
            confidence: 80,
            model_version: 1,
        });

        let result = client.submit_scores_batch(&batch);
        assert_eq!(result.accepted_count, 0);
        assert_eq!(result.rejected_count, 1);
        assert_eq!(
            result.results.get(0).unwrap().rejection_code,
            ledgerlens_score::Error::ContractPaused as u32
        );

        client.set_pair_paused(&pair, &false);
    }

    #[test]
    fn test_adversarial_replay_same_wallet_pair_cooldown() {
        use soroban_sdk::testutils::Ledger as _;
        let env = Env::default();
        let (client, _admin, _service) = init_contract(&env);

        let wallet = Address::generate(&env);
        let pair = Symbol::new(&env, "XLM_USDC");
        let ts = 1_000_000u64;

        env.ledger().with_mut(|l| l.timestamp = ts);

        let mut batch1: SVec<ScoreSubmission> = SVec::new(&env);
        batch1.push_back(ScoreSubmission {
            wallet: wallet.clone(),
            asset_pair: pair.clone(),
            score: 50,
            benford_flag: false,
            ml_flag: false,
            timestamp: ts,
            confidence: 80,
            model_version: 1,
        });
        let result1 = client.submit_scores_batch(&batch1);
        assert_eq!(result1.accepted_count, 1);

        env.ledger().with_mut(|l| l.timestamp = ts + 100);

        let mut batch2: SVec<ScoreSubmission> = SVec::new(&env);
        batch2.push_back(ScoreSubmission {
            wallet: wallet.clone(),
            asset_pair: pair.clone(),
            score: 55,
            benford_flag: false,
            ml_flag: false,
            timestamp: ts + 100,
            confidence: 80,
            model_version: 1,
        });
        let result2 = client.submit_scores_batch(&batch2);
        assert_eq!(result2.rejected_count, 1);
    }

    #[cfg(test)]
    mod schema_tests {
        use replay::schema::{self, ReplayEntryV1, ReplayFileHeader, ReplayMetadata};

        #[test]
        fn test_schema_version_validation() {
            assert!(schema::validate_schema_version(1).is_ok());
            assert!(schema::validate_schema_version(999).is_err());
        }

        #[test]
        fn test_current_schema_version() {
            assert_eq!(schema::current_version(), 1);
        }

        #[test]
        fn test_supported_versions() {
            let supported = schema::supported_versions();
            assert!(supported.contains(&1));
        }

        #[test]
        fn test_replay_file_header_with_metadata() {
            let header = ReplayFileHeader {
                schema_version: 1,
                metadata: Some(ReplayMetadata {
                    description: Some("Test data".to_string()),
                    created_at: Some(1700000000),
                    host_version: Some("21.0.0".to_string()),
                    custom: None,
                }),
            };
            let json = serde_json::to_string(&header).unwrap();
            let parsed: ReplayFileHeader = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed.schema_version, 1);
            assert_eq!(parsed.metadata.unwrap().description.unwrap(), "Test data");
        }

        #[test]
        fn test_replay_entry_roundtrip() {
            let entry = ReplayEntryV1 {
                wallet: "test_wallet".to_string(),
                asset_pair: "XLM_USDC".to_string(),
                trades: Some(vec![
                    replay::schema::TradeRecord { price: 1.5, quantity: None, timestamp: None },
                    replay::schema::TradeRecord { price: 1.6, quantity: None, timestamp: None },
                ]),
            };
            let json = serde_json::to_string(&entry).unwrap();
            let parsed: ReplayEntryV1 = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed.wallet, "test_wallet");
            assert_eq!(parsed.trades.unwrap().len(), 2);
        }
    }

    #[cfg(test)]
    mod determinism_tests {
        use replay::determinism::{self, HostVersionResult, ExecutionMetadata};
        use std::collections::HashMap;

        fn create_test_result(
            host_version: &str,
            accepted: u32,
            rejected: u32,
            state: HashMap<String, String>,
        ) -> HostVersionResult {
            HostVersionResult {
                host_version: host_version.to_string(),
                accepted_count: accepted,
                rejected_count: rejected,
                state_snapshot: state,
                events: vec![],
                error_code: None,
                metadata: ExecutionMetadata {
                    gas_consumed: Some(1000),
                    execution_time_ms: Some(100),
                    peak_memory_bytes: Some(1024),
                    custom: HashMap::new(),
                },
            }
        }

        #[test]
        fn test_identical_results_deterministic() {
            let mut state = HashMap::new();
            state.insert("score_wallet_1_XLM_USDC".to_string(), "50".to_string());

            let result_a = create_test_result("21.0.0", 5, 0, state.clone());
            let result_b = create_test_result("21.0.0", 5, 0, state);

            let comparison = determinism::compare_results(&result_a, &result_b);
            assert!(comparison.is_deterministic);
            assert!(comparison.divergences.is_empty());
        }

        #[test]
        fn test_acceptance_count_divergence() {
            let state = HashMap::new();
            let result_a = create_test_result("21.0.0", 5, 0, state.clone());
            let result_b = create_test_result("21.1.0", 4, 1, state);

            let comparison = determinism::compare_results(&result_a, &result_b);
            assert!(!comparison.is_deterministic);
            assert!(!comparison.divergences.is_empty());
        }

        #[test]
        fn test_state_value_divergence() {
            let mut state_a = HashMap::new();
            state_a.insert("key1".to_string(), "value_a".to_string());

            let mut state_b = HashMap::new();
            state_b.insert("key1".to_string(), "value_b".to_string());

            let result_a = create_test_result("21.0.0", 5, 0, state_a);
            let result_b = create_test_result("21.1.0", 5, 0, state_b);

            let comparison = determinism::compare_results(&result_a, &result_b);
            assert!(!comparison.is_deterministic);
            assert!(!comparison.state_differences.is_empty());
        }

        #[test]
        fn test_event_divergence() {
            let state = HashMap::new();
            let mut result_a = create_test_result("21.0.0", 5, 0, state.clone());
            result_a.events = vec!["ScoreUpdated".to_string()];

            let mut result_b = create_test_result("21.1.0", 5, 0, state);
            result_b.events = vec!["ScoreUpdatedV2".to_string()];

            let comparison = determinism::compare_results(&result_a, &result_b);
            assert!(!comparison.is_deterministic);
            assert!(!comparison.event_differences.is_empty());
        }

        #[test]
        fn test_error_code_divergence() {
            let state = HashMap::new();
            let mut result_a = create_test_result("21.0.0", 5, 0, state.clone());
            result_a.error_code = Some(1);

            let mut result_b = create_test_result("21.1.0", 5, 0, state);
            result_b.error_code = Some(2);

            let comparison = determinism::compare_results(&result_a, &result_b);
            assert!(!comparison.is_deterministic);
        }
    }

    #[cfg(test)]
    mod wasm_analysis_tests {
        use replay::wasm_analysis::{self, SizeCategory, RegressionSeverity, WasmBinaryAnalysis};

        #[test]
        fn test_create_wasm_analysis() {
            let analysis = WasmBinaryAnalysis::new(1000000);
            assert_eq!(analysis.total_size, 1000000);
            assert!(analysis.modules.is_empty());
        }

        #[test]
        fn test_add_module_to_analysis() {
            let mut analysis = WasmBinaryAnalysis::new(1000000);
            analysis.add_module("score", 500000, SizeCategory::Core);
            assert_eq!(analysis.modules.len(), 1);
            assert_eq!(analysis.modules[0].bytes, 500000);
            assert_eq!(analysis.modules[0].percentage, 50.0);
        }

        #[test]
        fn test_add_feature_to_analysis() {
            let mut analysis = WasmBinaryAnalysis::new(1000000);
            analysis.add_feature("validation", 200000, SizeCategory::Feature);
            assert_eq!(analysis.features.len(), 1);
            assert_eq!(analysis.features[0].bytes, 200000);
        }

        #[test]
        fn test_modules_sorted_by_size() {
            let mut analysis = WasmBinaryAnalysis::new(1000000);
            analysis.add_module("small", 100000, SizeCategory::Test);
            analysis.add_module("large", 500000, SizeCategory::Core);
            analysis.add_module("medium", 300000, SizeCategory::Feature);

            let sorted = analysis.modules_by_size();
            assert_eq!(sorted[0].bytes, 500000);
            assert_eq!(sorted[1].bytes, 300000);
            assert_eq!(sorted[2].bytes, 100000);
        }

        #[test]
        fn test_detect_size_regression() {
            let mut prev = WasmBinaryAnalysis::new(1000000);
            prev.add_module("score", 500000, SizeCategory::Core);

            let mut curr = WasmBinaryAnalysis::new(1100000);
            curr.add_module("score", 550000, SizeCategory::Core); // 10% increase

            let comparison = wasm_analysis::compare_binaries(prev, curr);
            assert!(!comparison.regressions.is_empty());
            assert_eq!(comparison.regressions[0].name, "score");
            assert_eq!(comparison.regressions[0].increase_percent, 10.0);
        }

        #[test]
        fn test_regression_severity() {
            assert_eq!(RegressionSeverity::from_percentage(0.5), RegressionSeverity::Negligible);
            assert_eq!(RegressionSeverity::from_percentage(3.0), RegressionSeverity::Minor);
            assert_eq!(RegressionSeverity::from_percentage(7.0), RegressionSeverity::Moderate);
            assert_eq!(RegressionSeverity::from_percentage(15.0), RegressionSeverity::Severe);
        }

        #[test]
        fn test_requires_review_flag() {
            let mut prev = WasmBinaryAnalysis::new(1000000);
            prev.add_module("score", 500000, SizeCategory::Core);

            let mut curr = WasmBinaryAnalysis::new(1500000);
            curr.add_module("score", 750000, SizeCategory::Core); // 50% increase

            let comparison = wasm_analysis::compare_binaries(prev, curr);
            assert!(comparison.requires_review);
            assert_eq!(comparison.regressions[0].severity, RegressionSeverity::Severe);
        }

        #[test]
        fn test_improvement_not_regression() {
            let mut prev = WasmBinaryAnalysis::new(1000000);
            prev.add_module("score", 500000, SizeCategory::Core);

            let mut curr = WasmBinaryAnalysis::new(900000);
            curr.add_module("score", 450000, SizeCategory::Core); // 10% decrease

            let comparison = wasm_analysis::compare_binaries(prev, curr);
            assert!(comparison.regressions.is_empty()); // Improvement, not regression
        }
    }
}
