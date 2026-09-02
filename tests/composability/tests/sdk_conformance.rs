use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
use mock_amm::{FailPolicy as AmmFailPolicy, MockAmm, MockAmmClient, MockAmmError};
use serde::Deserialize;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

#[derive(Debug, Deserialize)]
struct FixtureData {
    default_config: DefaultConfig,
    cases: std::vec::Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct DefaultConfig {
    gate_threshold: u32,
    min_confidence: u32,
    fail_policy: String,
    max_staleness_secs: u64,
    required_oracle_version: u32,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    score: Option<u32>,
    confidence: Option<u32>,
    age_secs: Option<u64>,
    oracle: Option<String>,
    fail_policy: Option<String>,
    required_oracle_version: Option<serde_json::Value>,
    expected: String,
}

#[test]
fn sdk_conformance() {
    let fixture_str = include_str!("../sdk_conformance_fixtures.json");
    let fixture_data: FixtureData = serde_json::from_str(fixture_str).expect("Failed to parse JSON");

    for case in fixture_data.cases {
        let env = Env::default();
        env.mock_all_auths();

        let ledgerlens_id = env.register_contract(None, LedgerLensScoreContract);
        let ledgerlens = LedgerLensScoreContractClient::new(&env, &ledgerlens_id);
        let admin = Address::generate(&env);
        let service = Address::generate(&env);
        ledgerlens.initialize(&admin, &service);

        let amm_id = env.register_contract(None, MockAmm);
        let amm = MockAmmClient::new(&env, &amm_id);

        let gate_threshold = fixture_data.default_config.gate_threshold;
        let min_confidence = fixture_data.default_config.min_confidence;

        let fail_policy_str = case
            .fail_policy
            .as_deref()
            .unwrap_or(&fixture_data.default_config.fail_policy);
        let fail_policy = if fail_policy_str == "fail_closed" {
            AmmFailPolicy::FailClosed
        } else {
            AmmFailPolicy::FailOpen
        };

        let max_staleness_secs = fixture_data.default_config.max_staleness_secs;

        let required_oracle_version = match &case.required_oracle_version {
            Some(v) => {
                if v.is_number() {
                    v.as_u64().unwrap() as u32
                } else if v.is_string() && v.as_str().unwrap() == "current_plus_one" {
                    ledgerlens.get_contract_version() + 1
                } else {
                    panic!("Unknown required_oracle_version format in case {}", case.name)
                }
            }
            None => fixture_data.default_config.required_oracle_version,
        };

        amm.initialize(&admin, &ledgerlens_id, &gate_threshold);
        amm.set_liquidity_gate_config(
            &admin,
            &gate_threshold,
            &min_confidence,
            &fail_policy,
            &max_staleness_secs,
            &required_oracle_version,
        );

        let wallet = Address::generate(&env);

        if let Some(oracle_status) = &case.oracle {
            if oracle_status == "unavailable" {
                let bad_oracle = Address::generate(&env);
                amm.set_risk_oracle(&admin, &bad_oracle);
            }
        }

        let score = if case.name == "reject_unknown_wallet" {
            None
        } else {
            Some(case.score.unwrap_or(10))
        };
        let confidence = if case.name == "reject_unknown_wallet" {
            None
        } else {
            Some(case.confidence.unwrap_or(90))
        };
        let age_secs = if case.name == "reject_unknown_wallet" {
            None
        } else {
            Some(case.age_secs.unwrap_or(0))
        };

        if let (Some(s), Some(c), Some(a)) = (score, confidence, age_secs) {
            env.ledger().with_mut(|l| l.timestamp += 3_601);
            let submission_time = env.ledger().timestamp();

            ledgerlens.submit_score(
                &Vec::new(&env),
                &wallet,
                &symbol_short!("XLM_USDC"),
                &s,
                &false,
                &false,
                &submission_time,
                &c,
                &1,
                &None,
            );

            env.ledger().with_mut(|l| l.timestamp += a);
        }

        let result = amm.try_provide_liquidity_gated(&wallet, &1_000);

        let mapped_result = match result {
            Ok(Ok(())) => "allow",
            Err(Ok(MockAmmError::HighRiskWallet)) => "reject_high_risk",
            Err(Ok(MockAmmError::LowConfidence)) => "reject_low_confidence",
            Err(Ok(MockAmmError::StaleScore)) => "reject_stale",
            Err(Ok(MockAmmError::OracleUnavailable)) => "oracle_unavailable",
            Err(Ok(MockAmmError::UnsupportedVersion)) => "unsupported_version",
            _ => panic!("Unexpected result in case '{}': {:?}", case.name, result),
        };

        assert_eq!(
            mapped_result, case.expected,
            "Case '{}' mismatched. Expected '{}', got '{}'",
            case.name, case.expected, mapped_result
        );
    }
}
