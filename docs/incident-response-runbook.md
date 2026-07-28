# Incident Response Runbook — LedgerLens Score Contract

> **Version:** 1.0 (matching contract version 5, issue #631)  
> **Audience:** On-call operators and contract admins  
> **Severity levels:** SEV1 (critical), SEV2 (degraded), SEV3 (informational)

---

## 1. Trust Assumptions & Authorization Boundaries

| Boundary | Trust model |
|----------|-------------|
| **Admin multisig** (M-of-N) | Authorises all governance, freeze/thaw, snapshot, and restore operations. Threshold must be > `N/2`. |
| **Service signers** (M-of-N) | Authorise score submissions. Operationally independent of admin set. |
| **Failover contract** | Must be administratively trusted — the primary delegates `query_risk_gate` to it during pauses. |
| **Replay/recovery tool** | Read-only; never holds signing keys. All on-chain mutations require admin multisig signatures. |

---

## 2. State Transitions

```
Normal ──┬──> Paused ──> Normal
         └──> Frozen ──> Normal
```

- **Paused** (`pause`): Blocks `submit_score` and other non-admin mutations. Admin governance continues (e.g. `reconcile_state`, `compute_state_checksum`).
- **Frozen** (`freeze_contract`): Blocks **all** mutating operations including admin governance except `unfreeze_contract`. Designed for incident isolation.
- **Normal**: All operations allowed.

---

## 3. Failure Modes & Rollback / Recovery

### 3.1 Incorrect Score Submission

**Detection:** Alert from monitoring (score anomaly, batch attestation mismatch, dispute opened).  
**Containment:**

```bash
# Freeze the contract to prevent further submissions
soroban contract invoke \
  --id $CONTRACT_ID \
  --source $ADMIN_KEY \
  --network testnet \
  -- \
  freeze_contract \
  --admin_signers '["<ADMIN_ADDRESS>"]'
```

**Investigation:**

```bash
# 1. Take a state snapshot
soroban contract invoke \
  --id $CONTRACT_ID \
  --source $ADMIN_KEY \
  --network testnet \
  -- \
  compute_state_checksum \
  --admin_signers '["<ADMIN_ADDRESS>"]'

# 2. Export all scores for off-line analysis
# (Use export_all_scores_paginated with pagination)
```

**Recovery:**

1. Commit corrected scores through the normal multisig submission flow.
2. Take a post-recovery snapshot.
3. Reconcile using `reconcile_state` on-chain or the `recovery` CLI tool.
4. If scores must be erased: use `clear_score` and `clear_score_history` (note: irreversible — ensure off-chain backup exists first).
5. Unfreeze:

```bash
soroban contract invoke \
  --id $CONTRACT_ID \
  --source $ADMIN_KEY \
  --network testnet \
  -- \
  unfreeze_contract \
  --admin_signers '["<ADMIN_ADDRESS>"]'
```

### 3.2 Configuration Drift

**Detection:** Off-chain reconciliation alert (config root mismatch).  
**Recovery:** Use `propose_parameter_change` / `execute_parameter_change` to restore known-good values. Take before/after snapshots and reconcile.

### 3.3 Signer Compromise

**Detection:** Unauthorised score submission, signer reputation degradation.  
**Containment:** Freeze → rotate compromised signer via `remove_service_signer` → rotate service pubkey via `rotate_service_pubkey` → unfreeze.  
**Verification:** Reconcile state before and after.

### 3.4 Contract Upgrade Failure

**Detection:** Upgrade smoke test failure (see `upgrade_smoke.rs`), monitoring alert after `execute_upgrade`.  
**Recovery:** If upgrade was time-locked (proposed → delayed → executed), rollback requires deploying the previous WASM hash through a new upgrade proposal. Verify all scores and config survived using `verify_state_checksum` against a pre-upgrade snapshot.

---

## 4. Alert Thresholds & Decision Authority

| Signal | Threshold | Owner | Action |
|--------|-----------|-------|--------|
| Score root divergence | Any change between expected and actual | Operator | Freeze → investigate → reconcile |
| Config root divergence | Any unexpected change | Lead admin | Halt governance → reconcile |
| Auth root divergence | Any unexpected change | Security lead | Rotate keys → reconcile |
| Batch attestation failure | Any rejection | Operator | Review batch → resubmit |
| Service heartbeat silence | > alert threshold (default 1h) | Operator | Page service owner |
| Quorum failure | > `QuorumFailureWindow` (24h) | Lead admin | Review signer set |

---

## 5. On-Call Playbook

### 5.1 SEV1: Suspected Data Corruption

1. **Freeze** the contract immediately.
2. Take a **state snapshot** (`compute_state_checksum`).
3. **Export all scores** (`export_all_scores_paginated`).
4. **Reconcile** against the last known-good snapshot from off-chain backup.
5. If divergence is confirmed, determine the root cause:
   - Incorrect `submit_score` call → use dispute or embargo.
   - Configuration change → revert via parameter governance.
   - Signer compromise → rotate signers.
6. Apply corrective action.
7. Take a post-recovery snapshot and **verify** it matches expectations.
8. **Unfreeze** the contract.
9. Generate a post-action report.

### 5.2 SEV2: Configuration Drift

1. Do **not** freeze (admin operations still work).
2. Reconcile current state against a known-good snapshot.
3. Revert configuration via `propose_parameter_change`.
4. Verify with `verify_state_checksum`.

### 5.3 SEV3: Audit / Compliance Check

1. Take a snapshot for record-keeping.
2. Run off-chain reconciliation tool: `recovery reconcile baseline.json current.json`.
3. Save the reconciliation report for audit.
4. No on-chain action needed unless divergence is detected.

---

## 6. Backup / Restore Workflow

### 6.1 Off-Chain Backup

1. Freeze the contract.
2. Export all scores using `export_all_scores_paginated` (paginated, page size ≤ 50).
3. Take a state snapshot using `compute_state_checksum`.
4. Save both the export JSON and the snapshot JSON to secure off-chain storage.
5. Unfreeze.

### 6.2 Off-Chain Restore

> **Warning:** Restore requires admin multisig and must be coordinated with all stakeholders.

1. Deploy a fresh contract instance to isolate restored state.
2. Use admin multisig to submit each score from the backup export via `submit_score`.
3. After all scores are restored, take a snapshot and reconcile it against the original backup snapshot.
4. Verify `verify_state_checksum` returns `true` for the restored state.
5. Point consumers to the new contract address and deprecate the old one.

---

## 7. Reconciliation Workflow

```
Pre-incident snapshot ──┐
                        ├──> reconcile_state ──> Report
Post-recovery snapshot ─┘
```

**On-chain:** `reconcile_state(snapshot_a, snapshot_b)`  
**Off-chain:** `recovery reconcile pre.json post.json`

The reconciliation report fields:
- `entries_matched`: Number of score entries that agree between snapshots
- `entries_diverged`: Number of entries that differ
- `config_matches`: Whether admin config (thresholds, cooldown, etc.) agrees
- `auth_matches`: Whether auth/signer configuration agrees

A divergence in any field requires investigation before resuming normal operations.

---

## 8. Post-Action Verification Report

After any significant incident response action, generate a report containing:

1. Pre-action snapshot (score root, config root, auth root, entry count).
2. Action type and description (freeze, restore, upgrade, parameter change, signer rotation).
3. Post-action snapshot (after recovery action).
4. Reconciliation result between pre and post snapshots.
5. Verification of state checksum.

The off-chain `recovery report` command generates a structured report template.

---

## 9. Monitoring Signals

| Signal | Source | What to watch for |
|--------|--------|-------------------|
| `snap` event | `compute_state_checksum` | Unexpected snapshots without an incident ticket |
| `frozen` / `unfroz` events | `freeze_contract` / `unfreeze_contract` | Freeze without corresponding incident |
| `bk_rest` event | Backup restore | Unauthorised restore |
| `recncil` event | `reconcile_state` | Reconciliation that shows divergence |
| Admin audit root | `get_admin_audit_root` | Unexpected changes to the audit chain |

---

## 10. Testnet Canary Protocol

Before any incident response procedure is applied to mainnet:

1. Run the **rehearsal script** (`scripts/rehearsal.sh`) on a clean testnet deployment.
2. Verify that `freeze_contract` → `compute_state_checksum` → `unfreeze_contract` cycle completes.
3. Verify that `reconcile_state` detects injected failures.
4. Verify that `verify_state_checksum` returns expected results.
5. Have a reviewer who did **not** author the runbook validate each step.
6. Record the canary results and attach to the change request.
