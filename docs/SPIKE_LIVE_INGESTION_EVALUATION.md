# Spike Evaluation: Live-RPC / Horizon Streaming Ingestion for Replay & Forensics (#933)

## Executive Summary

This spike evaluates whether `tools/replay` (and forensic tools in the LedgerLens ecosystem) should gain a mode that directly ingests live or historical streaming transaction/event data from Soroban RPC / Horizon endpoints, and how to maintain **deterministic, reproducible-bundle guarantees** while doing so.

### Key Finding & Recommendation
Directly streaming live, non-deterministic network data into the replay execution harness during forensic analysis compromises determinism guarantees and introduces operational fragility during active incidents.

We strongly recommend a **"Snapshot Exporter" companion tool architecture** ("Snapshot-then-Replay"). In this design:
1. An off-chain exporter CLI tool (e.g. `scripts/fetch_testnet_snapshot.py` or a dedicated `ledgerlens-snapshot` CLI) pulls transaction/event history for a specified contract and ledger/time window from Soroban RPC/Horizon into a immutable, frozen **NDJSON snapshot file**.
2. The snapshot file is hashed (SHA-256) and verified.
3. The existing deterministic `tools/replay` harness ingests this frozen snapshot as input, producing deterministic, bit-for-bit reproducible evidence bundles and cryptographic hashes for incident response and third-party audits.

---

## 1. Determinism & Reproducibility Analysis

### Replay Core Guarantees
`tools/replay` provides forensic auditability by taking a fixed set of NDJSON input events/transactions and executing them through contract logic, emitting a cryptographically hashed evidence bundle (`bundle_hash = SHA256(inputs + outputs + state_deltas)`).

### Why Direct Streaming Violates Determinism
If `tools/replay` fetches data dynamically over live RPC/Horizon streams while executing:
- **Provider / State Volatility:** Network responses can vary over time due to RPC node prune windows, ledger retention limits (e.g. 120,960 ledgers / ~7 days on Soroban testnet), or reorgs/unfinalized state near head.
- **Non-reproducibility:** Two forensic investigators running replay at different times or against different RPC providers (e.g., SDF RPC vs. QuickNode vs. FastForward) might receive different pagination counts or missing historical events, leading to mismatching evidence hashes.
- **Audit Failure:** Incident evidence bundles produced from live RPC calls cannot be verified off-line or archived permanently in legal/security disclosures.

### Preserving Determinism via "Snapshot-then-Replay"
By isolating the network ingestion phase into a distinct snapshot phase:
1. **Freezing the Inputs:** The snapshot phase generates a deterministic NDJSON document containing the exact ordered list of contract events, transaction envelopes, and ledger timestamps.
2. **Deterministic Fingerprinting:** A cryptographic hash (`sha256sum snapshot.ndjson`) is calculated immediately after export.
3. **Reproducible Execution:** Any forensic auditor with the same `snapshot.ndjson` file and WASM binary will produce identical replay results and incident evidence bundles, satisfying `determinism.rs` guarantees completely.

---

## 2. Architecture Comparison

| Dimension | Option A: Direct Streaming Ingestion in Replay | Option B: "Snapshot-then-Replay" (Recommended) |
| :--- | :--- | :--- |
| **Determinism Guarantee** | ❌ Vulnerable to RPC network/provider fluctuations | ✅ 100% Deterministic & reproducible off-line |
| **Incident Operational Complexity** | ❌ High risk of RPC rate-limiting/timeouts mid-incident | ✅ Simple 2-stage workflow: export snapshot once, analyze locally |
| **RPC Rate Limits & Cost** | ❌ High (re-fetching same network history on every replay run) | ✅ Low (single fetch per forensic snapshot; reusable locally) |
| **Auditability & Legal Evidence** | ❌ Hard to prove inputs were identical across runs | ✅ Easy: sign and share the `.ndjson` snapshot file |
| **Implementation Effort** | ❌ High: requires async networking & RPC retry logic inside Rust replay binary | ✅ Low: light CLI helper or script producing standard NDJSON |

---

## 3. Failure Modes & Mitigations for RPC Ingestion

When pulling history from live RPC/Horizon endpoints during an active incident, several edge cases must be handled:

1. **RPC Provider Incompatibility & Retention Window:**
   - *Issue:* Default public Soroban RPC nodes retain ledger history for a limited retention window (e.g., `ledgerRetentionWindow = 120960` ledgers, ~7 days). Older incident data returns empty or truncated results.
   - *Mitigation:* Support configurable RPC endpoints (e.g. archival nodes or Horizon historical API) and validate that `startLedger` falls within `oldestLedger` bounds before starting export.

2. **Partial / Truncated History & Pagination:**
   - *Issue:* RPC `getEvents` calls return paginated responses (typically capped at 100 events per page). Interrupted pagination yields incomplete event sets.
   - *Mitigation:* The exporter must iterate cursors until no further events are returned, validating event ordering (`ledger`, `txHash`, `eventIndex`) and checking for gapless sequence IDs before finalizing the NDJSON output.

3. **Rate Limits & Backoff:**
   - *Issue:* Active incidents trigger heavy RPC usage; providers return `429 Too Many Requests`.
   - *Mitigation:* Implement exponential backoff retry in the exporter client and limit request concurrency.

4. **Network Partition / RPC Disconnection:**
   - *Issue:* Disconnection halfway through an export produces a corrupted, partial snapshot.
   - *Mitigation:* Write output to a temporary file (`snapshot.ndjson.tmp`) and atomically rename upon complete, verified download.

---

## 4. Reuse of Existing Pipeline Patterns

- **`deploy.sh` / `scripts/`:** `deploy.sh` utilizes `soroban contract invoke` and CLI network aliases (`testnet`, `mainnet`). The snapshot exporter CLI should accept standard network parameters (`--network testnet` or `--rpc-url <URL>`).
- **`data` / `core` Repositories:** As detailed in `README.md`, `data` (Python) handles Stellar Horizon ingestion for trade and ledger data. A Python or Rust companion script following the existing Pydantic/JSON serialization models in the off-chain pipeline ensures clean alignment across repositories.

---

## 5. Scope Recommendation for Implementation

We recommend the following scope for follow-up implementation:

1. **Scope Boundary:** Do **not** embed network RPC client dependencies (e.g., `reqwest`, `tokio`) into the core `tools/replay` Rust binary. Keep `tools/replay` strictly offline, fast, and deterministic.
2. **Snapshot Exporter Tool:** Provide a standalone, lightweight exporter utility (e.g., `scripts/fetch_testnet_snapshot.py` or a dedicated subcommand) that takes `--contract-id`, `--start-ledger`, `--end-ledger`, and `--rpc-url`, and outputs a canonical NDJSON file.
3. **Workflow Integration:** Document the 2-step forensic workflow in `docs/AUDIT_REPLAY.md` and `tools/replay/README.md`:
   ```bash
   # Step 1: Export historical snapshot from RPC
   python3 scripts/fetch_testnet_snapshot.py --contract-id <CONTRACT_ID> --start-ledger 4324800 --output incident_4324800.ndjson

   # Step 2: Deterministic Replay & Evidence Generation
   cargo run -p tools-replay -- incident_4324800.ndjson
   ```
