mod schema;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use soroban_sdk::testutils::Address as _;
use soroban_sdk::testutils::Ledger as _;
use soroban_sdk::{Address, Env, Symbol, Vec as SVec};

use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient, ScoreSubmission};
use schema::ReplayFileHeader;

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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct ConfigSnapshot {
    admin: String,
    service: String,
    cooldown_secs: u64,
    default_score: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct TransactionEvidence {
    sequence: u64,
    wallet: String,
    asset_pair: String,
    score: u32,
    timestamp: u64,
    accepted: bool,
    rejection_code: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct EventEvidence {
    sequence: u64,
    kind: String,
    wallet: String,
    asset_pair: String,
    message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct BundleHashes {
    transactions_hash: String,
    events_hash: String,
    config_hash: String,
    issue_refs_hash: String,
    bundle_hash: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct IncidentEvidenceBundle {
    bundle_version: u32,
    transactions: Vec<TransactionEvidence>,
    events: Vec<EventEvidence>,
    config_snapshot: ConfigSnapshot,
    issue_references: Vec<String>,
    hashes: BundleHashes,
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

fn manifest_fixture_path(relative: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative).to_string_lossy().into_owned()
}

fn hash_json<T: Serialize>(value: &T) -> String {
    let bytes = serde_json::to_vec(value).expect("evidence bundle values must serialize");
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn build_evidence_bundle(
    transactions: Vec<TransactionEvidence>,
    events: Vec<EventEvidence>,
    config_snapshot: ConfigSnapshot,
    issue_references: &[String],
) -> IncidentEvidenceBundle {
    let mut normalized_refs = issue_references.to_vec();
    normalized_refs.sort();
    normalized_refs.dedup();

    let transactions_hash = hash_json(&transactions);
    let events_hash = hash_json(&events);
    let config_hash = hash_json(&config_snapshot);
    let issue_refs_hash = hash_json(&normalized_refs);
    let bundle_hash = hash_json(&json!({
        "bundle_version": 1u32,
        "transactions": transactions,
        "events": events,
        "config_snapshot": config_snapshot,
        "issue_references": normalized_refs
    }));

    IncidentEvidenceBundle {
        bundle_version: 1,
        transactions,
        events,
        config_snapshot,
        issue_references: normalized_refs,
        hashes: BundleHashes {
            transactions_hash,
            events_hash,
            config_hash,
            issue_refs_hash,
            bundle_hash,
        },
    }
}

fn parse_issue_references(args: &[String]) -> Vec<String> {
    let mut refs = Vec::new();
    let mut idx = 0usize;
    while idx < args.len() {
        let arg = &args[idx];
        if arg == "--issue-ref" {
            if let Some(value) = args.get(idx + 1) {
                refs.push(value.clone());
                idx += 2;
                continue;
            }
        } else if let Some(rest) = arg.strip_prefix("--issue-ref=") {
            refs.push(rest.to_string());
            idx += 1;
            continue;
        }
        idx += 1;
    }
    refs.sort();
    refs.dedup();
    refs
}

fn process_snapshot(
    path: &str,
    env: &Env,
    client: &LedgerLensScoreContractClient,
    config_snapshot: &ConfigSnapshot,
    issue_references: &[String],
) -> Result<(usize, IncidentEvidenceBundle)> {
    let f = File::open(path).context("opening snapshot file")?;
    let reader = BufReader::new(f);
    let mut count = 0usize;
    let mut addr_map: HashMap<String, Address> = HashMap::new();
    let mut schema_version: Option<u32> = None;
    let mut transactions = Vec::new();
    let mut events = Vec::new();

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
                    "Loaded replay with schema version {} (current: {}, supported: {})",
                    header.schema_version,
                    schema::current_version(),
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
            .unwrap_or(config_snapshot.default_score);

        let timestamp = env.ledger().timestamp().saturating_add(count as u64);
        let mut batch: SVec<ScoreSubmission> = SVec::new(env);
        batch.push_back(ScoreSubmission {
            wallet: wallet_addr.clone(),
            asset_pair: pair_sym.clone(),
            score,
            benford_flag: false,
            ml_flag: false,
            timestamp,
            confidence: 80u32,
            model_version: 1u32,
        });

        let result = client.submit_scores_batch(&batch);
        let tx_sequence = count as u64 + 1;
        let accepted = result.accepted_count > 0;
        let rejection_code = if accepted { None } else { Some(result.rejected_count) };

        transactions.push(TransactionEvidence {
            sequence: tx_sequence,
            wallet: entry.wallet.clone(),
            asset_pair: entry.asset_pair.clone(),
            score,
            timestamp,
            accepted,
            rejection_code,
        });
        events.push(EventEvidence {
            sequence: tx_sequence,
            kind: "batch_submission".to_string(),
            wallet: entry.wallet.clone(),
            asset_pair: entry.asset_pair.clone(),
            message: format!(
                "submitted score {} for {} (accepted_count={}, rejected_count={})",
                score, entry.asset_pair, result.accepted_count, result.rejected_count
            ),
        });
        count += 1;
    }

    if let Some(ver) = schema_version {
        println!("Processed {} entries with schema version {}", count, ver);
    } else {
        println!(
            "Processed {} entries (no explicit schema version found - using implicit v1)",
            count
        );
    }

    let bundle =
        build_evidence_bundle(transactions, events, config_snapshot.clone(), issue_references);
    Ok((count, bundle))
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
        let scenario_path = manifest_fixture_path(scenario_file);
        match process_failure_scenario(scenario_name, &scenario_path, env, client, admin) {
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
            let path = args
                .get(2)
                .cloned()
                .unwrap_or_else(|| manifest_fixture_path("testdata/reference.ndjson"));
            println!("Replay — reading {}", path);

            let env = Env::default();
            env.mock_all_auths();
            let contract_id = env.register_contract(None, LedgerLensScoreContract);
            let client = LedgerLensScoreContractClient::new(&env, &contract_id);
            let admin = Address::generate(&env);
            let service = Address::generate(&env);
            client.initialize(&admin, &service);

            let config_snapshot = ConfigSnapshot {
                admin: "initialized-admin".to_string(),
                service: "initialized-service".to_string(),
                cooldown_secs: 3600,
                default_score: 50,
            };
            let issue_references = parse_issue_references(args.get(3..).unwrap_or_default());

            let (n, bundle) =
                process_snapshot(&path, &env, &client, &config_snapshot, &issue_references)?;
            println!("processed {} entries", n);
            println!("evidence_bundle={}", serde_json::to_string_pretty(&bundle)?);
        }
        "failure-inject" => {
            let path = args
                .get(2)
                .cloned()
                .unwrap_or_else(|| manifest_fixture_path("testdata/failure_scenarios.ndjson"));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config() -> ConfigSnapshot {
        ConfigSnapshot {
            admin: "admin".to_string(),
            service: "service".to_string(),
            cooldown_secs: 3600,
            default_score: 50,
        }
    }

    #[test]
    fn builds_a_deterministic_bundle_for_the_success_path() {
        let transactions = vec![
            TransactionEvidence {
                sequence: 1,
                wallet: "wallet-a".to_string(),
                asset_pair: "XLM_USDC".to_string(),
                score: 70,
                timestamp: 1,
                accepted: true,
                rejection_code: None,
            },
            TransactionEvidence {
                sequence: 2,
                wallet: "wallet-b".to_string(),
                asset_pair: "USD_USDC".to_string(),
                score: 40,
                timestamp: 1,
                accepted: false,
                rejection_code: Some(7),
            },
        ];
        let events = vec![EventEvidence {
            sequence: 1,
            kind: "batch_submission".to_string(),
            wallet: "wallet-a".to_string(),
            asset_pair: "XLM_USDC".to_string(),
            message: "submitted score 70 for XLM_USDC".to_string(),
        }];
        let issue_refs = vec!["ISSUE-2".to_string(), "ISSUE-1".to_string()];

        let bundle = build_evidence_bundle(
            transactions.clone(),
            events.clone(),
            sample_config(),
            &issue_refs,
        );

        assert_eq!(bundle.bundle_version, 1);
        assert_eq!(bundle.issue_references, vec!["ISSUE-1", "ISSUE-2"]);
        assert_eq!(bundle.hashes.transactions_hash, hash_json(&transactions));
        assert_eq!(bundle.hashes.events_hash, hash_json(&events));
        assert_eq!(
            bundle.hashes.bundle_hash,
            hash_json(&json!({
                "bundle_version": 1u32,
                "transactions": transactions,
                "events": events,
                "config_snapshot": sample_config(),
                "issue_references": vec!["ISSUE-1", "ISSUE-2"]
            }))
        );
    }

    #[test]
    fn empty_inputs_still_produce_stable_hashes() {
        let bundle = build_evidence_bundle(Vec::new(), Vec::new(), sample_config(), &[]);

        assert!(bundle.transactions.is_empty());
        assert!(bundle.events.is_empty());
        assert_eq!(bundle.hashes.transactions_hash, hash_json(&Vec::<TransactionEvidence>::new()));
        assert_eq!(bundle.hashes.events_hash, hash_json(&Vec::<EventEvidence>::new()));
        assert_eq!(bundle.hashes.issue_refs_hash, hash_json(&Vec::<String>::new()));
    }

    #[test]
    fn duplicate_and_unsorted_issue_references_are_normalized() {
        let issue_refs = vec!["ISSUE-2".to_string(), "ISSUE-1".to_string(), "ISSUE-2".to_string()];
        let bundle = build_evidence_bundle(Vec::new(), Vec::new(), sample_config(), &issue_refs);

        assert_eq!(bundle.issue_references, vec!["ISSUE-1", "ISSUE-2"]);
        assert_eq!(bundle.hashes.issue_refs_hash, hash_json(&vec!["ISSUE-1", "ISSUE-2"]));
    }
}
