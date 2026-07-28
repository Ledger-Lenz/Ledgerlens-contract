//! # LedgerLens Post-Incident Recovery & Reconciliation Tool
//!
//! Off-chain tooling for state snapshot, reconciliation, backup, key-rotation
//! verification, and post-action verification workflows.
//!
//! ## Commands
//!
//! * `snapshot` — Save a state snapshot to a JSON file.
//! * `export` — Export score entries for off-chain backup.
//! * `reconcile` — Compare two snapshot files and produce a diff report.
//! * `verify` — Verify a saved snapshot against current state metadata.
//! * `report` — Generate a post-action verification report.
//! * `verify-rotation` — Verify a key-rotation operation produced the expected
//!   signer set configuration.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

// ── Data types ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StateSnapshot {
    score_root: String,
    config_root: String,
    auth_root: String,
    entry_count: u32,
    ledger_seq: u32,
    timestamp: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ExportableScoreEntry {
    wallet: String,
    asset_pair: String,
    score: u32,
    benford_flag: bool,
    ml_flag: bool,
    timestamp: u64,
    confidence: u32,
    model_version: u32,
    benford_score: u32,
    ml_score: u32,
    network_score: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ReconciliationReport {
    snapshot_a_path: String,
    snapshot_b_path: String,
    score_roots_match: bool,
    config_roots_match: bool,
    auth_roots_match: bool,
    entry_counts_match: bool,
    all_match: bool,
    details: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PostActionReport {
    snapshot: StateSnapshot,
    action_type: String,
    action_timestamp: String,
    action_description: String,
    pre_action_entry_count: u32,
    post_action_entry_count: Option<u32>,
    checksum_verified: bool,
    verification_notes: Vec<String>,
}

/// Describes a key-rotation operation and its expected outcome.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct RotationVerificationRequest {
    /// Expected number of service signers after rotation.
    expected_service_signer_count: u32,
    /// Expected number of admin signers after rotation.
    expected_admin_signer_count: u32,
    /// Expected service threshold, or 0 if unchanged.
    expected_service_threshold: u32,
    /// Expected admin threshold, or 0 if unchanged.
    expected_admin_threshold: u32,
    /// Whether a pubkey rotation was performed.
    pubkey_rotated: bool,
    /// Whether an overlap window was used (vs instant rotation).
    overlap_used: bool,
}

/// Result of verifying a key-rotation operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct RotationVerificationReport {
    request: RotationVerificationRequest,
    service_signer_count_match: bool,
    admin_signer_count_match: bool,
    service_threshold_match: bool,
    admin_threshold_match: bool,
    all_match: bool,
    notes: Vec<String>,
}

// ── CLI ─────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "recovery", about = "LedgerLens post-incident recovery & key-rotation tooling")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Save a snapshot description to a JSON file.
    Snapshot {
        #[arg(short, long, default_value = "snapshot.json")]
        output: PathBuf,
        #[arg(short = 'r', long)]
        score_root: String,
        #[arg(short = 'c', long)]
        config_root: String,
        #[arg(short = 'a', long)]
        auth_root: String,
        #[arg(short = 'n', long)]
        entry_count: u32,
        #[arg(short = 's', long)]
        ledger_seq: u32,
        #[arg(short = 't', long)]
        timestamp: u64,
    },

    /// Export score entries to a JSON lines file.
    Export {
        #[arg(short, long, default_value = "export.json")]
        output: PathBuf,
        #[arg(short = 'i', long)]
        input: Option<PathBuf>,
    },

    /// Reconcile two snapshot files and produce a diff report.
    Reconcile {
        snapshot_a: PathBuf,
        snapshot_b: PathBuf,
        #[arg(short, long, default_value = "reconciliation-report.json")]
        output: PathBuf,
    },

    /// Verify that a snapshot file has internally consistent roots.
    Verify {
        snapshot: PathBuf,
        #[arg(short = 'e', long)]
        export: Option<PathBuf>,
    },

    /// Generate a post-action verification report.
    Report {
        snapshot: PathBuf,
        #[arg(short, long)]
        action: String,
        #[arg(short, long)]
        description: String,
        #[arg(short, long, default_value = "post-action-report.json")]
        output: PathBuf,
    },

    /// Verify that a key-rotation operation produced the expected signer set
    /// configuration. Reads a configuration file describing the expected outcome
    /// and produces a verification report.
    VerifyRotation {
        /// Path to a JSON file containing the RotationVerificationRequest.
        config: PathBuf,
        /// Path to save the verification report.
        #[arg(short, long, default_value = "rotation-verification.json")]
        output: PathBuf,
        /// Actual service signer count observed after rotation.
        #[arg(short = 's', long)]
        actual_service_count: u32,
        /// Actual admin signer count observed after rotation.
        #[arg(short = 'a', long)]
        actual_admin_count: u32,
        /// Actual service threshold observed after rotation.
        #[arg(short = 't', long)]
        actual_service_threshold: u32,
        /// Actual admin threshold observed after rotation.
        #[arg(short = 'A', long)]
        actual_admin_threshold: u32,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Snapshot { output, score_root, config_root, auth_root, entry_count, ledger_seq, timestamp } => {
            cmd_snapshot(&output, &score_root, &config_root, &auth_root, entry_count, ledger_seq, timestamp)
        }
        Commands::Export { output, input } => cmd_export(&output, input.as_deref()),
        Commands::Reconcile { snapshot_a, snapshot_b, output } => cmd_reconcile(&snapshot_a, &snapshot_b, &output),
        Commands::Verify { snapshot, export } => cmd_verify(&snapshot, export.as_deref()),
        Commands::Report { snapshot, action, description, output } => cmd_report(&snapshot, &action, &description, &output),
        Commands::VerifyRotation { config, output, actual_service_count, actual_admin_count, actual_service_threshold, actual_admin_threshold } => {
            cmd_verify_rotation(&config, &output, actual_service_count, actual_admin_count, actual_service_threshold, actual_admin_threshold)
        }
    }
}

// ── Command handlers ────────────────────────────────────────────────────────

fn cmd_snapshot(
    output: &PathBuf,
    score_root: &str,
    config_root: &str,
    auth_root: &str,
    entry_count: u32,
    ledger_seq: u32,
    timestamp: u64,
) -> Result<()> {
    let snapshot = StateSnapshot {
        score_root: score_root.to_string(),
        config_root: config_root.to_string(),
        auth_root: auth_root.to_string(),
        entry_count,
        ledger_seq,
        timestamp,
    };
    let json = serde_json::to_string_pretty(&snapshot)
        .context("Failed to serialize snapshot")?;
    fs::write(output, &json)
        .with_context(|| format!("Failed to write snapshot to {}", output.display()))?;
    eprintln!("Snapshot saved to {}", output.display());
    Ok(())
}

fn cmd_export(output: &PathBuf, input: Option<&PathBuf>) -> Result<()> {
    if let Some(input_path) = input {
        let content = fs::read_to_string(input_path)
            .with_context(|| format!("Failed to read {}", input_path.display()))?;
        let entries: Vec<ExportableScoreEntry> = serde_json::from_str(&content)
            .context("Export file is not a valid JSON array of ExportableScoreEntry")?;
        eprintln!("Loaded {} entries from {}", entries.len(), input_path.display());
        fs::write(output, &content)
            .with_context(|| format!("Failed to write export to {}", output.display()))?;
        eprintln!("Export written to {} ({} entries)", output.display(), entries.len());
    } else {
        fs::write(output, "[]")
            .with_context(|| format!("Failed to write export to {}", output.display()))?;
        eprintln!(
            "Empty export created at {}. Populate it with data from \
             the contract's export_all_scores_paginated function.",
            output.display()
        );
    }
    Ok(())
}

fn cmd_reconcile(snapshot_a: &PathBuf, snapshot_b: &PathBuf, output: &PathBuf) -> Result<()> {
    let snap_a: StateSnapshot = load_snapshot(snapshot_a)?;
    let snap_b: StateSnapshot = load_snapshot(snapshot_b)?;

    let score_match = snap_a.score_root == snap_b.score_root;
    let config_match = snap_a.config_root == snap_b.config_root;
    let auth_match = snap_a.auth_root == snap_b.auth_root;
    let count_match = snap_a.entry_count == snap_b.entry_count;
    let all_match = score_match && config_match && auth_match && count_match;

    let mut details = Vec::new();
    details.push(format!(
        "Score root: {} {} → {}",
        &snap_a.score_root[..16],
        &snap_b.score_root[..16],
        if score_match { "MATCH" } else { "DIVERGE" }
    ));
    details.push(format!(
        "Config root: {} {} → {}",
        &snap_a.config_root[..16],
        &snap_b.config_root[..16],
        if config_match { "MATCH" } else { "DIVERGE" }
    ));
    details.push(format!(
        "Auth root: {} {} → {}",
        &snap_a.auth_root[..16],
        &snap_b.auth_root[..16],
        if auth_match { "MATCH" } else { "DIVERGE" }
    ));
    details.push(format!(
        "Entry count: {} vs {} → {}",
        snap_a.entry_count,
        snap_b.entry_count,
        if count_match { "MATCH" } else { "DIVERGE" }
    ));

    let report = ReconciliationReport {
        snapshot_a_path: snapshot_a.display().to_string(),
        snapshot_b_path: snapshot_b.display().to_string(),
        score_roots_match: score_match,
        config_roots_match: config_match,
        auth_roots_match: auth_match,
        entry_counts_match: count_match,
        all_match,
        details,
    };

    let json = serde_json::to_string_pretty(&report)?;
    fs::write(output, &json)
        .with_context(|| format!("Failed to write reconciliation report to {}", output.display()))?;

    if all_match {
        eprintln!("Snapshots MATCH — state is consistent.");
    } else {
        eprintln!("Snapshots DIVERGE — state has changed.");
    }
    eprintln!("Report saved to {}", output.display());
    Ok(())
}

fn cmd_verify(snapshot_path: &PathBuf, export_path: Option<&PathBuf>) -> Result<()> {
    let snapshot: StateSnapshot = load_snapshot(snapshot_path)?;
    eprintln!("Verifying snapshot from {}", snapshot_path.display());
    eprintln!("  Score root:  {}", snapshot.score_root);
    eprintln!("  Config root: {}", snapshot.config_root);
    eprintln!("  Auth root:   {}", snapshot.auth_root);
    eprintln!("  Entry count: {}", snapshot.entry_count);
    eprintln!("  Ledger seq:  {}", snapshot.ledger_seq);
    eprintln!("  Timestamp:   {}", snapshot.timestamp);

    if snapshot.score_root.len() != 64 {
        eprintln!("  WARNING: score_root is not 64 hex chars");
    }
    if snapshot.config_root.len() != 64 {
        eprintln!("  WARNING: config_root is not 64 hex chars");
    }
    if snapshot.auth_root.len() != 64 {
        eprintln!("  WARNING: auth_root is not 64 hex chars");
    }

    if let Some(export_path) = export_path {
        let content = fs::read_to_string(export_path)
            .with_context(|| format!("Failed to read export {}", export_path.display()))?;
        let entries: Vec<ExportableScoreEntry> = serde_json::from_str(&content)
            .context("Export is not a valid JSON array")?;
        if entries.len() as u32 != snapshot.entry_count {
            eprintln!(
                "  WARNING: Export entry count mismatch: {} vs expected {}",
                entries.len(),
                snapshot.entry_count
            );
        } else {
            eprintln!("  Export entry count ({}) matches snapshot.", entries.len());
        }
    }

    eprintln!("Snapshot verification complete.");
    eprintln!("Run `verify_state_checksum` on the contract for full on-chain verification.");
    Ok(())
}

fn cmd_report(
    snapshot_path: &PathBuf,
    action: &str,
    description: &str,
    output: &PathBuf,
) -> Result<()> {
    let snapshot: StateSnapshot = load_snapshot(snapshot_path)?;
    let now = iso_timestamp();

    let report = PostActionReport {
        snapshot,
        action_type: action.to_string(),
        action_timestamp: now,
        action_description: description.to_string(),
        pre_action_entry_count: 0,
        post_action_entry_count: None,
        checksum_verified: false,
        verification_notes: vec![
            "Pre-action snapshot recorded.".to_string(),
            "Run compute_state_checksum after the action and reconcile.".to_string(),
        ],
    };

    let json = serde_json::to_string_pretty(&report)?;
    fs::write(output, &json)
        .with_context(|| format!("Failed to write report to {}", output.display()))?;
    eprintln!("Post-action report saved to {}", output.display());
    Ok(())
}

fn cmd_verify_rotation(
    config_path: &PathBuf,
    output: &PathBuf,
    actual_service_count: u32,
    actual_admin_count: u32,
    actual_service_threshold: u32,
    actual_admin_threshold: u32,
) -> Result<()> {
    let content = fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read config from {}", config_path.display()))?;
    let request: RotationVerificationRequest = serde_json::from_str(&content)
        .context("Config is not a valid RotationVerificationRequest")?;

    let svc_count_match = request.expected_service_signer_count == actual_service_count;
    let adm_count_match = request.expected_admin_signer_count == actual_admin_count;
    let svc_thr_match = if request.expected_service_threshold > 0 {
        request.expected_service_threshold == actual_service_threshold
    } else {
        true // 0 means "don't verify"
    };
    let adm_thr_match = if request.expected_admin_threshold > 0 {
        request.expected_admin_threshold == actual_admin_threshold
    } else {
        true
    };

    let all_match = svc_count_match && adm_count_match && svc_thr_match && adm_thr_match;

    let mut notes = Vec::new();
    notes.push(format!(
        "Service signers: expected {}, got {} → {}",
        request.expected_service_signer_count,
        actual_service_count,
        if svc_count_match { "MATCH" } else { "MISMATCH" }
    ));
    notes.push(format!(
        "Admin signers: expected {}, got {} → {}",
        request.expected_admin_signer_count,
        actual_admin_count,
        if adm_count_match { "MATCH" } else { "MISMATCH" }
    ));

    if request.pubkey_rotated {
        notes.push(if request.overlap_used {
            "Pubkey rotation: overlap window used (gradual)".to_string()
        } else {
            "Pubkey rotation: instant (no overlap)".to_string()
        });
    }

    let report = RotationVerificationReport {
        request,
        service_signer_count_match: svc_count_match,
        admin_signer_count_match: adm_count_match,
        service_threshold_match: svc_thr_match,
        admin_threshold_match: adm_thr_match,
        all_match,
        notes,
    };

    let json = serde_json::to_string_pretty(&report)?;
    fs::write(output, &json)
        .with_context(|| format!("Failed to write verification report to {}", output.display()))?;

    if all_match {
        eprintln!("Key-rotation verification: ALL MATCH");
    } else {
        eprintln!("Key-rotation verification: MISMATCH DETECTED");
        for note in &report.notes {
            eprintln!("  {}", note);
        }
    }
    eprintln!("Report saved to {}", output.display());
    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn load_snapshot(path: &PathBuf) -> Result<StateSnapshot> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read snapshot from {}", path.display()))?;
    let snapshot: StateSnapshot = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse snapshot from {}", path.display()))?;
    Ok(snapshot)
}

fn iso_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;
    format!("2026-{:02}-{:02}T{:02}:{:02}:{:02}Z", days / 30 + 1, days % 30 + 1, hours, minutes, seconds)
}
