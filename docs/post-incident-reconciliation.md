# Post-Incident Reconciliation Workflow

This document defines how an operator compares on-chain contract state against
the off-chain pipeline records after a scoring incident (e.g. a compromised
service key, a bad model run, or unexpected score submissions) and produces a
deterministic reconciliation report with mismatches and recommended remediation.

---

## Overview

```
off-chain pipeline records (NDJSON)
        │
        ▼
 reconcile tool  ──────────────────────────────────────► reconciliation report
        │                                                  (mismatches + actions)
        ▼
on-chain state  (via replay harness / get_score queries)
```

A **reconciliation report** lists every `(wallet, asset_pair)` tuple that
appears in either source and classifies it as one of:

| Status | Meaning |
|---|---|
| `ok` | On-chain score matches the pipeline record within tolerance. |
| `mismatch` | Score or metadata diverges beyond the configured tolerance. |
| `onchain_only` | Entry exists on-chain but has no matching pipeline record. |
| `pipeline_only` | Pipeline record exists but was never written on-chain. |
| `stale` | On-chain entry exists but its timestamp is older than `--max-age-secs`. |

---

## Prerequisites

* `stellar-cli` (or `soroban`) installed and on `PATH`.
* `CONTRACT_ID`, `RPC_URL`, and `NETWORK` set in environment or passed as flags.
* The pipeline exports its scored records as **NDJSON** with one JSON object per
  line, each containing at least:

  ```json
  {"wallet":"G...", "asset_pair":"XLM_USDC", "score":72, "confidence":88,
   "timestamp":1700000000, "benford_flag":false, "ml_flag":true}
  ```

---

## Step-by-Step Procedure

### 1. Export pipeline records

```bash
# From the `core` repository or its output store:
python -m ledgerlens.export_scores \
  --since <INCIDENT_START_UNIX> \
  --until <INCIDENT_END_UNIX> \
  --output pipeline_records.ndjson
```

### 2. Run the reconciliation script

```bash
./scripts/reconcile.sh \
  --pipeline pipeline_records.ndjson \
  --contract-id "$CONTRACT_ID" \
  --rpc-url    "$RPC_URL" \
  --network    "$NETWORK" \
  --max-age-secs 86400 \
  --score-tolerance 0 \
  --output reconciliation_report.json
```

`reconcile.sh` calls the replay harness internally and emits
`reconciliation_report.json` (see [Report Schema](#report-schema) below).

### 3. Review the report

```bash
jq '.summary' reconciliation_report.json
jq '.entries[] | select(.status != "ok")' reconciliation_report.json
```

### 4. Apply remediation

For each `mismatch` or `onchain_only` entry the recommended action is included
in the report under `remediation`.  Typical remediations:

| Status | Recommended action |
|---|---|
| `mismatch` | Re-submit the correct score via `submit_score` after `override_rate_limit`. |
| `onchain_only` | Investigate whether the pipeline missed the wallet; re-score if needed. |
| `pipeline_only` | Re-submit the score; check service-key auth and cooldown state. |
| `stale` | Extend TTL via `extend_entry_ttls`; re-score if the score itself is outdated. |

Apply re-submissions:

```bash
while IFS= read -r line; do
  wallet=$(echo "$line" | jq -r '.wallet')
  pair=$(echo "$line" | jq -r '.asset_pair')
  score=$(echo "$line" | jq -r '.pipeline_score')
  confidence=$(echo "$line" | jq -r '.pipeline_confidence')
  timestamp=$(echo "$line" | jq -r '.pipeline_timestamp')
  stellar contract invoke \
    --id "$CONTRACT_ID" --source ledgerlens_service --network "$NETWORK" -- \
    submit_score \
    --signers '[]' \
    --wallet "$wallet" --asset_pair "$pair" \
    --score "$score" --benford_flag false --ml_flag false \
    --timestamp "$timestamp" --confidence "$confidence" \
    --model_version 1 --attestation_input null
done < <(jq -c '.entries[] | select(.status == "mismatch" or .status == "pipeline_only")' \
           reconciliation_report.json)
```

### 5. Re-run reconciliation to confirm

After applying remediations, repeat steps 2–3.  The report `summary.mismatch_count`
and `summary.pipeline_only_count` should reach `0`.

---

## Report Schema

```json
{
  "generated_at": 1700000000,
  "incident_window": { "start": 0, "end": 0 },
  "summary": {
    "total_entries": 0,
    "ok_count": 0,
    "mismatch_count": 0,
    "onchain_only_count": 0,
    "pipeline_only_count": 0,
    "stale_count": 0
  },
  "entries": [
    {
      "wallet": "G...",
      "asset_pair": "XLM_USDC",
      "status": "mismatch",
      "onchain_score": 10,
      "pipeline_score": 72,
      "onchain_confidence": 40,
      "pipeline_confidence": 88,
      "onchain_timestamp": 1700000000,
      "pipeline_timestamp": 1700000050,
      "delta_score": 62,
      "remediation": "re-submit pipeline score after override_rate_limit"
    }
  ]
}
```

---

## Automated Cadence

Run the reconciliation workflow after every incident and on a weekly schedule:

```
# crontab entry (weekly, Sundays 02:00 UTC)
0 2 * * 0  /path/to/scripts/reconcile.sh \
              --pipeline /data/latest_pipeline_dump.ndjson \
              --contract-id "$CONTRACT_ID" \
              --rpc-url "$RPC_URL" \
              --network "$NETWORK" \
              > /var/log/ledgerlens/reconcile_$(date +\%F).json
```

---

## Storage and Audit Compatibility

* `reconcile.sh` is read-only against the contract — it calls only `get_score`,
  `get_score_history`, and `get_score_count`.  No on-chain state is mutated by
  the reconciliation read phase.
* Remediation re-submissions go through the normal `submit_score` path, meaning
  they are subject to rate-limiting, score-floor policy, and attestation
  requirements exactly like any other submission.
* The report is append-only offline; store previous reports to build an audit
  trail of what was corrected and when.

---

## Resource Bounds

* `get_score` is O(1) per entry; `get_score_history` is O(depth) capped at
  `HISTORY_MAX_DEPTH` (50 by default).
* The reconciliation report size is bounded by the number of entries in the
  pipeline dump (`MAX_TRACKED_SCORE_ENTRIES` = 500 on-chain).
* `extend_entry_ttls` rejects batches exceeding `MAX_EXPIRING_ENTRIES_PER_CALL`
  (100); split larger lists across multiple calls.
