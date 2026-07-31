mod schema;

use anyhow::{Context, Result};
use replay::{compare_config_manifests, parse_manifest_json, recommended_manifest_template};
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::{env as std_env, fs};

use soroban_sdk::testutils::Address as _;
use soroban_sdk::testutils::Ledger as _;
use soroban_sdk::{Address, Env, Symbol, Vec as SVec};

use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient, ScoreSubmission};
use schema::{ReplayEntryV1, ReplayFileHeader, ReplayMetadata};

#[derive(Debug, Deserialize)]
struct SnapshotEntry {
    wallet: String,
    asset_pair: String,
    trades: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
struct FailureEntry {
    scenario: String,
    wallet: String,
    asset_pair: String,
    score: Option<u32>,
    timestamp: Option<u64>,
    confidence: Option<u32>,
    model_version: Option<u32>,
}

fn parse_price_average(trades: &Option<Vec<serde_json::Value>>) -> Option<f64> {
    trades.as_ref().and_then(|t| {
        let mut sum = 0.0f64;
        let mut cnt = 0usize;
        for v in t.iter() {
            if let Some(p) = v.get("price").and_then(|p| p.as_f64()) {
                sum += p;
                cnt += 1;
            }
        }
        if cnt == 0 {
            None
        } else {
            Some(sum / cnt as f64)
        }
    })
}

fn process_snapshot(
    path: &str,
    env: &Env,
    client: &LedgerLensScoreContractClient,
) -> Result<usize> {
    let f = File::open(path).context("opening snapshot file")?;
    let reader = BufReader::new(f);
    let mut count = 0usize;
    let mut addr_map: HashMap<String, Address> = HashMap::new();
    let mut schema_version: Option<u32> = None;

    for line in reader.lines() {
        let l = line?;
        if l.trim().is_empty() {
            continue;
        }

        // Try to parse as header on first line
        if count == 0 {
            if let Ok(header) = serde_json::from_str::<ReplayFileHeader>(&l) {
                // This is a schema header line
                schema::validate_schema_version(header.schema_version)
                    .map_err(|e| anyhow::anyhow!("Schema validation failed: {}", e))?;
                schema_version = Some(header.schema_version);
                println!(
                    "Loaded replay with schema version {} (supported: {})",
                    header.schema_version,
                    schema::supported_versions()
                        .iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                if let Some(ref metadata) = header.metadata {
                    if let Some(ref desc) = metadata.description {
                        println!("Description: {}", desc);
                    }
                    if let Some(ref host_ver) = metadata.host_version {
                        println!("Host version: {}", host_ver);
                    }
                }
                continue;
            }
        }

        // Parse entry with backward compatibility
        let entry: SnapshotEntry = serde_json::from_str(&l).context("parsing ndjson line")?;
        let wallet_addr =
            addr_map.entry(entry.wallet.clone()).or_insert_with(|| Address::generate(env)).clone();
        let pair_sym = Symbol::new(env, &entry.asset_pair);

        let score = parse_price_average(&entry.trades)
            .map(|avg| {
                let s = (avg * 10.0).round() as i64;
                s.clamp(0, 100) as u32
            })
            .unwrap_or(50u32);

        let mut batch: SVec<ScoreSubmission> = SVec::new(env);
        batch.push_back(ScoreSubmission {
            wallet: wallet_addr.clone(),
            asset_pair: pair_sym.clone(),
            score,
            benford_flag: false,
            ml_flag: false,
            timestamp: env.ledger().timestamp().saturating_add(count as u64),
            confidence: 80u32,
            model_version: 1u32,
        });

        let result = client.submit_scores_batch(&batch);
        println!(
            "submitted wallet={}, pair={} -> accepted_count={} rejected_count={}",
            entry.wallet, entry.asset_pair, result.accepted_count, result.rejected_count
        );
        count += 1;
    }

    if let Some(ver) = schema_version {
        println!("Processed {} entries with schema version {}", count, ver);
    } else {
        println!("Processed {} entries (no explicit schema version found - using implicit v1)", count);
    }

    Ok(count)
}

fn process_failure_scenario(
    scenario: &str,
    path: &str,
    env: &Env,
    client: &LedgerLensScoreContractClient,
    _admin: &Address,
) -> Result<usize> {
    let f = File::open(path).context("opening failure scenario file")?;
    let reader = BufReader::new(f);
    let mut count = 0usize;
    let mut addr_map: HashMap<String, Address> = HashMap::new();

    for line in reader.lines() {
        let l = line?;
        if l.trim().is_empty() {
            continue;
        }
        let entry: FailureEntry = serde_json::from_str(&l).context("parsing failure entry")?;
        if entry.scenario != scenario {
            continue;
        }

        let wallet_addr =
            addr_map.entry(entry.wallet.clone()).or_insert_with(|| Address::generate(env)).clone();
        let pair_sym = Symbol::new(env, &entry.asset_pair);
        let score = entry.score.unwrap_or(50);
        let timestamp = entry.timestamp.unwrap_or(env.ledger().timestamp());
        let confidence = entry.confidence.unwrap_or(80);
        let model_version = entry.model_version.unwrap_or(1);

        let mut batch: SVec<ScoreSubmission> = SVec::new(env);
        batch.push_back(ScoreSubmission {
            wallet: wallet_addr.clone(),
            asset_pair: pair_sym.clone(),
            score,
            benford_flag: false,
            ml_flag: false,
            timestamp,
            confidence,
            model_version,
        });

        let result = client.submit_scores_batch(&batch);
        println!(
            "scenario={} wallet={}, pair={} -> accepted_count={} rejected_count={}",
            scenario, entry.wallet, entry.asset_pair, result.accepted_count, result.rejected_count
        );
        count += 1;
    }
    Ok(count)
}

fn run_failure_injection(env: &Env, client: &LedgerLensScoreContractClient, admin: &Address) {
    println!("Running failure-injection scenarios...");

    let scenarios = vec![
        ("partial-signer-loss", "testdata/failure_partial_signer_loss.ndjson"),
        ("stale-data", "testdata/failure_stale_data.ndjson"),
        ("replay-attack", "testdata/failure_replay_attack.ndjson"),
        ("zero-value", "testdata/failure_zero_value.ndjson"),
        ("max-value", "testdata/failure_max_value.ndjson"),
        ("unauthorized-caller", "testdata/failure_unauthorized_caller.ndjson"),
        ("interrupted-retry", "testdata/failure_interrupted_retry.ndjson"),
    ];

    for (scenario_name, scenario_file) in scenarios {
        println!("--- Scenario: {} ---", scenario_name);
        match process_failure_scenario(scenario_name, scenario_file, env, client, admin) {
            Ok(n) => println!("  Scenario '{}' processed {} entries", scenario_name, n),
            Err(e) => println!("  Scenario '{}' error: {}", scenario_name, e),
        }
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("replay");

    match mode {
        "replay" => {
            let path = args.get(2).map(|s| s.as_str()).unwrap_or("testdata/reference.ndjson");
            println!("Replay — reading {}", path);

            let env = Env::default();
            env.mock_all_auths();
            let contract_id = env.register_contract(None, LedgerLensScoreContract);
            let client = LedgerLensScoreContractClient::new(&env, &contract_id);
            let admin = Address::generate(&env);
            let service = Address::generate(&env);
            client.initialize(&admin, &service);

            match process_snapshot(path, &env, &client) {
                Ok(n) => println!("processed {} entries", n),
                Err(e) => println!("error processing snapshot: {:#}", e),
            }
        }
        "failure-inject" => {
            let path =
                args.get(2).map(|s| s.as_str()).unwrap_or("testdata/failure_scenarios.ndjson");
            println!("Failure Injection — mode=failure-injection reading {}", path);

            let env = Env::default();
            env.mock_all_auths();
            let contract_id = env.register_contract(None, LedgerLensScoreContract);
            let client = LedgerLensScoreContractClient::new(&env, &contract_id);
            let admin = Address::generate(&env);
            let service = Address::generate(&env);
            client.initialize(&admin, &service);

            for i in 0..50 {
                env.ledger().with_mut(|l| l.timestamp = 1_000_000 + i as u64);
            }

            run_failure_injection(&env, &client, &admin);
        }
        _ => {
            eprintln!("Usage: replay <mode> [path]");
            eprintln!("  Modes:");
            eprintln!("    replay [path]               - Replay NDJSON snapshot (default: testdata/reference.ndjson)");
            eprintln!("    failure-inject [path]       - Run failure-injection scenarios");
            std::process::exit(1);
        }
    }
    Ok(())
}
