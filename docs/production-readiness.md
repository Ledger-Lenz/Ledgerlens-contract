# Production Readiness Review — LedgerLens Score Contract

> **Issue:** #635
> **Status:** Draft
> **Last updated:** 2026-07-25

## 1. Readiness Criteria

Before production activation, the following criteria must be met. Each criterion has an owner, an alert threshold, and evidence required.

| # | Criterion | Owner | Alert Threshold | Evidence Required |
|---|-----------|-------|-----------------|-------------------|
| R1 | All native workspace tests pass | Author | Any failure | CI run log, `cargo test --workspace` |
| R2 | `cargo clippy --all-targets -- -D warnings` passes | Author | Any warning | CI run log |
| R3 | `cargo fmt --all -- --check` passes | Author | Any diff | CI run log |
| R4 | Locked release WASM build succeeds | Author | Build failure | `cargo build --target wasm32-unknown-unknown --release -p ledgerlens-score --locked` log |
| R5 | Reproducible build (two independent builds produce identical WASM) | Author | SHA-256 mismatch | CI reproducible-build job output |
| R6 | Error discriminant stability check passes | Author | Any regression | `tools/check_error_discriminants.sh` output |
| R7 | Wasm size within budget | Author | Exceeds `wasm-size-budget.md` limit | `scripts/wasm-size-report.sh` output |
| R8 | Repository compatibility checks pass (lockfile, toolchain) | Author | Any mismatch | `cargo check --locked` output |
| R9 | Design document reviewed and approved | Maintainer | Missing or incomplete design | PR design section |
| R10 | Runbook reviewed by a reviewer who did not author it | Reviewer | No review record | Signed review comment |
| R11 | Testnet canary deployment completed successfully | SRE | Deployment failure | Canary deployment log |
| R12 | Failure-injection scenarios all pass | SRE | Any scenario failure | Failure-injection test log |
| R13 | Backup/restore drill completed with integrity reconciliation | SRE | Any mismatch | Drill report |
| R14 | Rollback drill completed within target RTO | SRE | Exceeds RTO | Drill report |
| R15 | Monitoring signals configured and verified | SRE | No data appearing | Dashboard screenshot |
| R16 | On-call handoff completed | SRE | No handoff document | Signed handoff record |

## 2. Trust Assumptions

1. **Stellar network honesty:** The Stellar consensus network correctly orders and applies ledger operations. Ledger timestamps are trusted for time-based governance (finality buffer, upgrade delay, model version timelocks).
2. **Admin key security:** The admin key (or multi-sig) is stored in a hardware wallet or HSM accessible only to authorised operators. Compromise of the admin key is the primary threat model.
3. **Service key security:** The off-chain service account's secret key is stored securely and used only by the authorised detection pipeline.
4. **Signing key compromise:** If the service signing key (`set_service_pubkey`) is compromised, the attacker can submit forged attestations. The mitigation is that the admin can rotate the key via `set_service_pubkey` or `rotate_service_pubkey`.
5. **Deterministic execution:** Soroban contract execution is deterministic across all nodes. No Byzantine behaviour from the Stellar network itself.
6. **Wasm binary integrity:** The WASM built from the audited source code is the binary deployed to mainnet. Reproducible build verification provides this guarantee.

## 3. Authorization Boundaries

### 3.1 Admin Boundaries
- `admin` controls all governance operations: parameter changes, upgrade proposals, pause/unpause, pair pausing, service rotation, model version governance.
- Admin can be a single address or a multi-sig set (M-of-N via `add_service_signer` + `set_service_threshold`).
- Admin transfer is time-locked via `transfer_admin` / `accept_admin_transfer` / `cancel_admin_transfer`.

### 3.2 Service Boundaries
- `service` is the off-chain account authorised to call `submit_score` directly (legacy mode).
- When a service set is configured, `submit_score` requires M-of-N service signer authorisation instead of the single service key.
- The service signing key is used for single-key cryptographic attestation (opt-in via `set_service_pubkey`).

### 3.3 Key Rotation
- Admin rotation: `transfer_admin` → `accept_admin_transfer` (time-locked by `upgrade_delay`).
- Service key rotation: `set_service` (admin-only, immediate).
- Attestation key rotation: `set_service_pubkey` / `rotate_service_pubkey` (admin-only, immediate with optional pending key for staged rotation).

### 3.4 Pause Boundaries
- Global pause (`pause` / `unpause`): Admin-only. Halts all `submit_score` and `submit_scores_batch` calls. Read-only paths (`get_score`, `get_admin`, etc.) are unaffected.
- Per-pair pause (`set_pair_paused`): Admin-only. Freezes submissions for a specific `asset_pair` while the contract remains globally active.

## 4. State Transitions

### 4.1 Contract Lifecycle
```
Uninitialized → Initialized → Active
                                ↓
                        Paused (global circuit breaker)
                                ↓
                        Active (unpause restores)
```

### 4.2 Score Lifecycle
```
Pending (with finality buffer > 0) → Committed (after buffer elapsed)
Direct submission (buffer = 0) → Committed immediately
```

### 4.3 Upgrade Lifecycle
```
No pending upgrade → Proposed → (48h delay) → Executed
                                   ↓
                              Vetoed (within first half of delay)
                                   ↓
                              Expired (2× delay elapses)
```

### 4.4 Parameter Change Lifecycle
```
No pending proposal → Proposed → (time-lock) → Executed
                                    ↓
                               Vetoed (within first half)
                                    ↓
                               Expired (2× time-lock)
```

### 4.5 Model Version Lifecycle
```
Proposed → (upgrade_delay) → Active → Deprecated (permanent)
```

## 5. Failure Modes and Mitigations

| Failure Mode | Impact | Mitigation | Recovery |
|-------------|--------|------------|----------|
| Admin key compromise | Attacker can change any parameter, upgrade contract, pause/unpause | Time-locked changes, multi-sig requirement, audit trail | Rotate admin key via `transfer_admin`, revoke compromised key |
| Service key compromise | Attacker can submit forged scores with valid attestations | Key rotation via `set_service_pubkey`, per-signature nonce | Rotate service key, invalidate old nonce |
| Finality buffer stuck | Scores remain pending and never committed | Admin can cancel pending scores via `cancel_pending_score` | Cancel stuck pending scores, adjust buffer |
| Stale data | Scores don't reflect current risk | TTL-based archival, `extend_entry_ttls` rental sweep | Run TTL sweep, re-submit fresh scores |
| Partial execution (batch) | Some entries in batch fail, others succeed | Per-entry rejection codes in `BatchResult`, batch continues | Review rejection codes, fix and re-submit failed entries |
| Signer loss (service set) | Not enough signers to meet threshold | `set_service_threshold` can lower threshold, `remove_service_signer` can remove compromised signers | Replace lost signers, adjust threshold |
| Unavailable dependencies | Off-chain detection pipeline down | Scores can still be submitted manually; contract doesn't depend on off-chain pipeline availability | Restore pipeline, re-submit scores |
| Interrupted retry | Duplicate score submission rejected by cooldown | Cooldown prevents rapid re-submission; same (wallet, pair) within cooldown window is rejected with `RateLimitExceeded` | Wait for cooldown, re-submit |
| Replay attack | Old score submission replayed | Timestamp validation, nonce-based attestation verification, commitment binding | N/A — prevented by design |
| Network congestion | Score submissions delayed | Finality buffer holds scores for configurable window; submissions retry automatically | Adjust buffer if needed |
| WASM upgrade failure | Contract logic broken after upgrade | Time-locked upgrade with veto window, rollback via re-proposing previous WASM | Re-propose previous WASM as new upgrade |
| Stale epoch | Contract rejects submissions when epoch is closed | Admin can open/close epochs via `open_epoch` / `close_epoch` | Admin opens new epoch |

## 6. Rollback and Recovery

### 6.1 Rollback Procedures

#### 6.1.1 Contract Code Rollback (WASM)
1. Obtain the previous WASM binary from version control (tag `contract-vX.Y.Z`).
2. Compute its SHA-256 hash.
3. `propose_upgrade` with the previous WASM hash.
4. Wait for the upgrade delay (default 48 hours) to elapse.
5. `execute_upgrade` to install the previous WASM.
6. Verify the rollback with `get_version` and post-upgrade smoke tests.

#### 6.1.2 Parameter Rollback
1. Identify the parameter change via `get_parameter_proposal(proposal_id)`.
2. If still in the veto window, `veto_parameter_change`.
3. If past the veto window but not yet executed, wait for expiry or propose the inverse change.
4. If already executed, propose the original value as a new parameter change.

#### 6.1.3 Data Rollback
- On-chain state cannot be rolled back automatically.
- Scores can be deleted via `clear_score` (admin-only, irreversible).
- History can be truncated via `clear_score_history` (admin-only, irreversible).
- The recommended approach for data corruption is to deploy a new contract and migrate authorised integrators.

### 6.2 Recovery Procedures

#### 6.2.1 Service Key Recovery
1. Admin calls `set_service(new_service_address)` to point to a new service account.
2. The detection pipeline is reconfigured to use the new service account.
3. Verify with `get_service()` that the change took effect.

#### 6.2.2 Pause Recovery
1. If the contract was accidentally paused, admin calls `unpause(admin_signers)`.
2. Verify with `is_paused()` that the contract is active again.
3. Check that score submissions resume correctly.

#### 6.2.3 Epoch Recovery
1. Admin calls `open_epoch(admin_signers, epoch_id)` to start a new epoch.
2. Verify with `is_epoch_open()` that the epoch is active.
3. Scores submitted during the open epoch are accepted.

## 7. Monitoring Signals

| Signal | Source | Alert Threshold | Description |
|--------|--------|-----------------|-------------|
| `contract_paused` | Event | Any occurrence | Global circuit breaker was activated |
| `score_submitted` | Event | Rate < expected | Score submissions dropped below baseline |
| `threshold_breached` | Event | Count > 0 | A score crossed the alert threshold |
| `rate_limit_exceeded` | Rejection code | Spike in rate | Too many submissions within cooldown window |
| `upgrade_proposed` | Event | Any occurrence | Upgrade proposal created — review required |
| `upgrade_executed` | Event | Any occurrence | WASM was upgraded — verify correctness |
| `upgrade_vetoed` | Event | Any occurrence | Upgrade was vetoed — investigate reason |
| `service_rotated` | Event | Any occurrence | Service address changed — verify new service |
| `signer_activated` / `signer_demoted` | Event | Sudden changes | Service signer tier changes |
| `pending_score` / `score_committed` | Event | Discrepancy | Pending scores not being committed within buffer window |
| Model version `proposed` / `activated` / `deprecated` | Event | Unexpected transitions | Model version governance activity |

## 8. Operational Checklists

### 8.1 Pre-Production Activation
- [ ] All readiness criteria (R1–R16) are satisfied
- [ ] Design document reviewed and approved by maintainer
- [ ] Runbook reviewed by a non-author reviewer
- [ ] Testnet canary deployment completed and verified
- [ ] Failure-injection scenarios all pass
- [ ] Backup/restore drill completed successfully
- [ ] Rollback drill completed within target RTO
- [ ] Monitoring signals configured and verified
- [ ] On-call handoff completed
- [ ] Integrators notified of activation and provided with contract IDs

### 8.2 Post-Deployment Verification
- [ ] `get_admin()` returns expected admin address
- [ ] `get_service()` returns expected service address
- [ ] `get_version()` returns expected version
- [ ] `is_paused()` returns `false`
- [ ] Submit a test score and verify it appears in `get_score()`
- [ ] Verify `get_aggregate_score()` returns expected values for test wallets
- [ ] Confirm event emission matches expectations
- [ ] Run smoke test suite against the deployed contract

### 8.3 Emergency Response
1. **Identify:** Determine the nature and scope of the incident.
2. **Contain:** Call `pause()` to halt all score submissions if safety is at risk.
3. **Diagnose:** Check event logs, rejection codes, and monitoring dashboards.
4. **Resolve:** Apply the appropriate fix (parameter rollback, code rollback, key rotation).
5. **Recover:** Call `unpause()` and verify normal operations resume.
6. **Post-mortem:** Document the incident, root cause, and preventive measures.

## 9. Resource Bounding

### 9.1 CPU
- `submit_score`: O(1) for the main path; O(S) for service signer validation (S = service set size, bounded by `MAX_SERVICE_SIGNERS` = 32).
- `submit_scores_batch`: O(N) where N = batch size, bounded by `MAX_BATCH_SIZE` = 100.
- All loops are over bounded collections; no unbounded iteration is possible.

### 9.2 Memory
- Soroban contract memory is bounded by the host execution limit. All collections (`Vec`, `Map`) release memory when they go out of scope at the end of the contract invocation.
- The `BatchResult.results` vector is bounded by `MAX_BATCH_SIZE`.
- No persistent allocations that grow without bound.

### 9.3 Ledger Reads/Writes
- `submit_score`: 1 write to live score + 1 write to score history + 1 read for rate limit check + O(1) additional reads for configuration.
- `submit_scores_batch`: N writes + N history pushes + N reads per batch entry (N ≤ `MAX_BATCH_SIZE`).
- All reads are direct key lookups (O(1)); no full-table scans.

### 9.4 Event Bytes
- Each event is a fixed-size or small-variable-size Struct. The `score_submitted` event is approximately 150 bytes.
- Batch events are emitted once per batch, not per entry, minimising event overhead.
- Event byte totals are bounded by the number of successful submissions per transaction.

### 9.5 Encoded Input Size
- `submit_score` input is bounded by the fixed-size parameters plus the optional attestation (max 133 bytes for a secp256k1 signature + nonce).
- `submit_scores_batch` input is bounded by `MAX_BATCH_SIZE` × (fixed-size `ScoreSubmission` ≈ 60 bytes) ≈ 6 KB maximum.
- All `BytesN<32>` and `BytesN<64>` inputs are fixed size.

## 10. Alternatives Considered

| Alternative | Rejected Because | Invariant Protected |
|-------------|------------------|---------------------|
| Immediate admin parameter changes (no time-lock) | Allows instant parameter manipulation by compromised admin | No unauthorized parameter mutation without community reaction time |
| Unlimited batch size | Unbounded CPU/memory/ledger ops per transaction | Bounded resource consumption per transaction |
| No finality buffer | Scores committed immediately with no review window | Admin ability to catch and cancel erroneous submissions |
| No pause mechanism | Cannot halt the contract in an emergency | Safety — the ability to stop all score submission activity |
| No model version governance | Any model version can submit scores immediately | Gradual rollout of new ML models with community reaction time |
| Per-entry attestation in batch (one signature per entry) | Prohibitively expensive in Soroban transaction fees | Batch attestation provides cryptographic integrity at lower cost |
| No replay tooling | Cannot verify deterministic behavior with real historical data | Verification — reproducible testing with real data |
| No failure injection | Cannot verify resilience under adverse conditions | Resilience — the contract handles partial failures gracefully |

## 11. Invariant Summary

1. **Authorization invariant:** No state-changing operation succeeds without the correct administrative or service authorization.
2. **Score range invariant:** All stored scores are in [0, 100]; out-of-range values are rejected before storage.
3. **Deterministic ordering invariant:** All batch operations process entries in insertion order; no reordering is possible.
4. **Checked arithmetic invariant:** All arithmetic uses checked operations (`saturating_add`, `check_add`, etc.); no overflow is possible.
5. **Bounded collection invariant:** All collections are sized by constants (`MAX_BATCH_SIZE`, `MAX_SERVICE_SIGNERS`, `MAX_PENDING_PROPOSALS`, `MAX_PAUSED_PAIRS`).
6. **Read-only non-persistence invariant:** Read-only paths (`get_score`, `get_admin`, etc.) do not introduce persistent writes.
7. **Time-lock invariant:** All governance changes (upgrades, parameter changes) are time-locked and cannot be executed before the delay elapses.
8. **Version compatibility invariant:** Error discriminants and stored data are append-only; existing deployments continue to work after upgrades.
