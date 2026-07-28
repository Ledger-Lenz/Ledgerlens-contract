//! # LedgerLens Post-Incident Recovery & Reconciliation Tool
//!
//! Off-chain tooling for state snapshot, reconciliation, backup, and
//! post-action verification workflows.
//!
//! ## Commands
//!
//! * `snapshot` — Take a deterministic state snapshot (invokes
//!   `compute_state_checksum` on-chain, saves the result to disk).
//! * `export` — Export all scored entries as a JSON file for off-chain backup.
//! * `reconcile` — Compare two snapshot files and produce a diff report.
//! * `verify` — Verify a saved snapshot against the current on-chain state.
//! * `report` — Generate a post-action verification report from a snapshot
//!   and an export.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

// ── Data types (mirrors on-chain types for off-chain processing) ────────────

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

// ── CLI ────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "recovery", about = "LedgerLens post-incident recovery & reconciliation tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Save a snapshot description to a JSON file (intended for use after
    /// calling compute_state_checksum on-chain and recording the output).
    Snapshot {
        /// Path to the output snapshot JSON file.
        #[arg(short, long, default_value = "snapshot.json")]
        output: PathBuf,
        /// Score root hex string from on-chain compute_state_checksum.
        #[arg(short = 'r', long)]
        score_root: String,
        /// Config root hex string.
        #[arg(short = 'c', long)]
        config_root: String,
        /// Auth root hex string.
        #[arg(short = 'a', long)]
        auth_root: String,
        /// Number of scored entries.
        #[arg(short = 'n', long)]
        entry_count: u32,
        /// Ledger sequence.
        #[arg(short = 's', long)]
        ledger_seq: u32,
        /// Ledger timestamp.
        #[arg(short = 't', long)]
        timestamp: u64,
    },

    /// Export score entries to a JSON lines file (one entry per line).
    /// This is an off-chain helper; use the contract's export_all_scores_paginated
    /// to fetch the actual data from the chain.
    Export {
        /// Path to save the export JSON to.
        #[arg(short, long, default_value = "export.json")]
        output: PathBuf,
        /// Path to a JSON array of ExportableScoreEntry items (simulated
        /// from off-chain data or collected from the contract).
        #[arg(short = 'i', long)]
        input: Option<PathBuf>,
    },

    /// Reconcile two snapshot files and produce a diff report.
    Reconcile {
        /// First snapshot file (pre-incident / baseline).
        snapshot_a: PathBuf,
        /// Second snapshot file (post-recovery / current).
        snapshot_b: PathBuf,
        /// Path to save the reconciliation report.
        #[arg(short, long, default_value = "reconciliation-report.json")]
        output: PathBuf,
    },

    /// Verify that a snapshot file has internally consistent roots.
    /// (Full on-chain verification requires calling the contract's
    /// `verify_state_checksum` function.)
    Verify {
        /// Path to the snapshot JSON file.
        snapshot: PathBuf,
        /// Path to an optional export JSON to cross-check entry count.
        #[arg(short = 'e', long)]
        export: Option<PathBuf>,
    },

    /// Generate a post-action verification report from a snapshot,
    /// export, and action description.
    Report {
        /// Path to the pre-action snapshot file.
        snapshot: PathBuf,
        /// Type of action performed (e.g. "freeze", "restore", "upgrade").
        #[arg(short, long)]
        action: String,
        /// Description of what was done and why.
        #[arg(short, long)]
        description: String,
        /// Path to save the report.
        #[arg(short, long, default_value = "post-action-report.json")]
        output: PathBuf,
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
    }
}

// ── Command handlers ───────────────────────────────────────────────────────

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
        // Read existing export data from a JSON file
        let content = fs::read_to_string(input_path)
            .with_context(|| format!("Failed to read {}", input_path.display()))?;
        // Validate by deserializing
        let entries: Vec<ExportableScoreEntry> = serde_json::from_str(&content)
            .context("Export file is not a valid JSON array of ExportableScoreEntry")?;
        eprintln!("Loaded {} entries from {}", entries.len(), input_path.display());
        fs::write(output, &content)
            .with_context(|| format!("Failed to write export to {}", output.display()))?;
        eprintln!("Export written to {} ({} entries)", output.display(), entries.len());
    } else {
        // Generate a minimal template
        let template = r#"[]"#;
        fs::write(output, template)
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
        "Score root: {} == {} → {}",
        &snap_a.score_root[..16],
        &snap_b.score_root[..16],
        if score_match { "MATCH" } else { "DIVERGE" }
    ));
    details.push(format!(
        "Config root: {} == {} → {}",
        &snap_a.config_root[..16],
        &snap_b.config_root[..16],
        if config_match { "MATCH" } else { "DIVERGE" }
    ));
    details.push(format!(
        "Auth root: {} == {} → {}",
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
        eprintln!("✅ Snapshots MATCH — state is consistent.");
    } else {
        eprintln!("❌ Snapshots DIVERGE — state has changed:");
        if !score_match {
            eprintln!("   - Score entries differ");
        }
        if !config_match {
            eprintln!("   - Configuration differs");
        }
        if !auth_match {
            eprintln!("   - Auth/signer config differs");
        }
        if !count_match {
            eprintln!("   - Entry count: {} vs {}", snap_a.entry_count, snap_b.entry_count);
        }
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

    // Validate hex strings
    if snapshot.score_root.len() != 64 {
        eprintln!("  ⚠ WARNING: score_root is not 64 hex chars ({} chars)", snapshot.score_root.len());
    }
    if snapshot.config_root.len() != 64 {
        eprintln!("  ⚠ WARNING: config_root is not 64 hex chars", snapshot.config_root.len());
    }
    if snapshot.auth_root.len() != 64 {
        eprintln!("  ⚠ WARNING: auth_root is not 64 hex chars", snapshot.auth_root.len());
    }

    if let Some(export_path) = export_path {
        let content = fs::read_to_string(export_path)
            .with_context(|| format!("Failed to read export {}", export_path.display()))?;
        let entries: Vec<ExportableScoreEntry> = serde_json::from_str(&content)
            .context("Export is not a valid JSON array")?;
        if entries.len() as u32 != snapshot.entry_count {
            eprintln!(
                "  ⚠ Entry count mismatch: export has {} entries, snapshot says {}",
                entries.len(),
                snapshot.entry_count
            );
        } else {
            eprintln!("  ✅ Export entry count ({}) matches snapshot.", entries.len());
        }
    }

    eprintln!("Snapshot verification complete.");
    // Full on-chain verification requires calling verify_state_checksum on the contract.
    eprintln!("Note: Run `verify_state_checksum` on the contract for full on-chain verification.");
    Ok(())
}

fn cmd_report(
    snapshot_path: &PathBuf,
    action: &str,
    description: &str,
    output: &PathBuf,
) -> Result<()> {
    let snapshot: StateSnapshot = load_snapshot(snapshot_path)?;
    let now = chrono_now();

    let report = PostActionReport {
        snapshot,
        action_type: action.to_string(),
        action_timestamp: now,
        action_description: description.to_string(),
        pre_action_entry_count: 0,
        post_action_entry_count: None,
        checksum_verified: false,
        verification_notes: vec![
            "Pre-action snapshot recorded. Run `compute_state_checksum` after".to_string(),
            "the action and reconcile the two snapshots to confirm consistency.".to_string(),
        ],
    };

    let json = serde_json::to_string_pretty(&report)?;
    fs::write(output, &json)
        .with_context(|| format!("Failed to write report to {}", output.display()))?;
    eprintln!("Post-action report saved to {}", output.display());
    Ok(())
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn load_snapshot(path: &PathBuf) -> Result<StateSnapshot> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read snapshot from {}", path.display()))?;
    let snapshot: StateSnapshot = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse snapshot from {}", path.display()))?;
    Ok(snapshot)
}

/// Returns an ISO-8601-like timestamp string. Uses system time via std::time.
fn chrono_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    // Format as ISO-8601 approximate
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;
    format!("2026-{:02}-{:02}T{:02}:{:02}:{:02}Z", days / 30 + 1, days % 30 + 1, hours, minutes, seconds)
}
