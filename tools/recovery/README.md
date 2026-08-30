# Recovery & Reconciliation Tool

Off-chain post-incident snapshot, reconciliation, backup, and post-action
verification tooling for the LedgerLens score contract.

## Overview

`recovery` helps operators capture a point-in-time state snapshot, export score
data for off-chain backup, and prove that on-chain state is consistent before
and after an incident or recovery action. It is the off-chain counterpart to the
contract's `compute_state_checksum`, `verify_state_checksum`, and
`reconcile_state` functions, and is referenced by
[`docs/incident-response-runbook.md`](../../docs/incident-response-runbook.md).

Use it when you need to:

- **Snapshot** — record the current on-chain roots (`score`, `config`, `auth`),
  entry count, ledger sequence, and timestamp to a JSON file for baseline
  comparison
- **Export** — persist all scored entries to a JSON file for off-chain backup
- **Reconcile** — compare a pre-incident snapshot against a post-recovery
  snapshot and produce a diff report
- **Verify** — sanity-check a saved snapshot for internally consistent roots
  and a matching entry count
- **Report** — generate a structured post-action verification report for audit

The tool is **read-only**: it never holds signing keys and performs no on-chain
mutations. All on-chain state changes require admin multisig signatures through
the normal contract flows.

## Snapshot Format

A snapshot file is a JSON object captured from the contract's
`compute_state_checksum` output, plus ledger metadata:

```json
{
  "score_root": "0123456789abcdef...",
  "config_root": "0123456789abcdef...",
  "auth_root": "0123456789abcdef...",
  "entry_count": 42,
  "ledger_seq": 123456,
  "timestamp": 1750000000
}
```

The three roots are 64-char hex strings. The export file is a JSON array of
score entries (`wallet`, `asset_pair`, `score`, `benford_flag`, `ml_flag`, etc.),
one per scored entry.

## Building

```bash
cargo build -p recovery
```

## Running

```bash
cargo run -p recovery --manifest-path tools/recovery/Cargo.toml -- <command> ...
```

### 1. Take a snapshot

Capture the roots from `compute_state_checksum`, then record them to disk:

```bash
cargo run -p recovery --manifest-path tools/recovery/Cargo.toml -- snapshot \
  -r <score_root> -c <config_root> -a <auth_root> \
  -n <entry_count> -s <ledger_seq> -t <timestamp> \
  -o baseline.json
```

### 2. Export scores for backup

Convert a JSON array of score entries into a backup file:

```bash
cargo run -p recovery --manifest-path tools/recovery/Cargo.toml -- export \
  -i scores.json -o export.json
```

Without `--input`, an empty `[]` template is created for later population from
the contract's `export_all_scores_paginated`.

### 3. Reconcile two snapshots

Compare a baseline against a post-recovery snapshot and produce a diff report:

```bash
cargo run -p recovery --manifest-path tools/recovery/Cargo.toml -- reconcile \
  baseline.json current.json -o reconciliation-report.json
```

The report compares the `score`, `config`, and `auth` roots and the entry count,
flagging each as `MATCH` or `DIVERGE`. Any divergence requires investigation
before resuming normal operations.

### 4. Verify a snapshot

```bash
cargo run -p recovery --manifest-path tools/recovery/Cargo.toml -- verify \
  snapshot.json -e export.json
```

Checks that the roots are well-formed 64-char hex strings and that the export
entry count matches the snapshot. Full on-chain verification is done with the
contract's `verify_state_checksum`.

### 5. Generate a post-action report

```bash
cargo run -p recovery --manifest-path tools/recovery/Cargo.toml -- report \
  snapshot.json --action freeze --description "Contained incorrect submission" \
  -o post-action-report.json
```

Produces a structured report capturing the action type, description, and
verification notes for the audit trail.

## Testing

```bash
cargo test -p recovery
```

## CI Integration

There is currently no dedicated CI workflow for `recovery`; the crate builds as
part of the workspace in `.github/workflows/ci.yml`. It is expected to remain a
read-only operator utility, analogous to the scripts and tools under `tools/`.