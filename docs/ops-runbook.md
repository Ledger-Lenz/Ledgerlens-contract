# LedgerLens Operations Runbook

> **Issue:** #635 — Production Launch & Operations Readiness Review

## 1. Overview

This runbook covers the operational procedures for the LedgerLens score contract on each Stellar network (testnet, futurenet, mainnet). It includes routine operations, failure scenarios, diagnostic procedures, and recovery steps.

## 2. Routine Operations

### 2.1 Deploy a New Version

```bash
# 1. Build the release WASM
cargo build --target wasm32-unknown-unknown --release -p ledgerlens-score --locked

# 2. Run the canary deployment on testnet first
./scripts/canary-deploy.sh testnet <admin-identity> <service-address>

# 3. Verify the canary deployment
./scripts/verify-deployment.sh testnet <contract-id> <admin-identity>

# 4. If canary passes, deploy to the target network
./deploy.sh <target-network> <admin-identity> <service-address>

# 5. Verify the production deployment
./scripts/verify-deployment.sh <target-network> <contract-id> <admin-identity>
```

### 2.2 Rotate Service Key

```bash
# 1. Generate a new signing key for the service account
soroban keys generate new-service-key

# 2. Set the new service pubkey (attestation key)
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source <ADMIN_KEY> \
  --network <NETWORK> \
  -- \
  rotate_service_pubkey \
  --admin-signers '[<ADMIN_ADDRESS>]' \
  --new-pubkey <NEW_PUBKEY>

# 3. Update the off-chain detection pipeline to use the new key
# 4. Verify the pipeline is signing correctly with the new key
./scripts/verify-deployment.sh <NETWORK> <CONTRACT_ID> <ADMIN_KEY>
```

### 2.3 Extend Entry TTLs (Rental Sweep)

```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source <ADMIN_KEY> \
  --network <NETWORK> \
  -- \
  extend_entry_ttls \
  --admin-signers '[<ADMIN_ADDRESS>]' \
  --entries '[{"wallet":"...","asset_pair":"XLM_USDC","end_of_epoch":1234567890}]'
```

### 2.4 Monitor Contract Health

```bash
# Check if the contract is paused
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source <ANY_ACCOUNT> \
  --network <NETWORK> \
  -- \
  is_paused

# Check the current admin
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source <ANY_ACCOUNT> \
  --network <NETWORK> \
  -- \
  get_admin

# Check the current service
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source <ANY_ACCOUNT> \
  --network <NETWORK> \
  -- \
  get_service

# Get the contract version
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source <ANY_ACCOUNT> \
  --network <NETWORK> \
  -- \
  get_version

# Check the last global submission time
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source <ANY_ACCOUNT> \
  --network <NETWORK> \
  -- \
  get_last_global_submission_time

# Check for pending upgrade
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source <ANY_ACCOUNT> \
  --network <NETWORK> \
  -- \
  get_pending_upgrade
```

## 3. Failure Scenarios

### 3.1 Global Pause — Accidental Activation

**Symptom:** All score submissions rejected with `ContractPaused`.

**Diagnosis:**
```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source <ANY_ACCOUNT> \
  --network <NETWORK> \
  -- \
  is_paused
```

**Recovery:**
```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source <ADMIN_KEY> \
  --network <NETWORK> \
  -- \
  unpause \
  --admin-signers '[<ADMIN_ADDRESS>]'
```

**Verification:**
```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source <ANY_ACCOUNT> \
  --network <NETWORK> \
  -- \
  is_paused
# Should return false (0)
```

### 3.2 Pause — Per-Pair Freeze

**Symptom:** Submissions for a specific `asset_pair` rejected with `ContractPaused`; other pairs work normally.

**Diagnosis:**
```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source <ANY_ACCOUNT> \
  --network <NETWORK> \
  -- \
  is_pair_paused \
  --asset-pair XLM_USDC
```

**Recovery:**
```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source <ADMIN_KEY> \
  --network <NETWORK> \
  -- \
  set_pair_paused \
  --admin-signers '[<ADMIN_ADDRESS>]' \
  --asset-pair XLM_USDC \
  --paused false
```

### 3.3 Service Key Compromise

**Symptom:** Unauthenticated score submissions detected (anomalous scores, unexpected wallets, or signatures from unknown signers).

**Recovery:**
1. Call `pause()` to halt all submissions immediately.
2. Rotate the service signing key via `rotate_service_pubkey`.
3. If a service set is configured, remove compromised signers via `remove_service_signer` and add new ones via `add_service_signer`.
4. Adjust the service threshold if needed via `set_service_threshold`.
5. Update the off-chain pipeline with the new keys.
6. Unpause the contract.
7. Verify normal operations resume.

### 3.4 Stale Data / No Recent Submissions

**Symptom:** `get_last_global_submission_time` is older than expected.

**Diagnosis:**
- Check that the off-chain detection pipeline is running.
- Check that the service account has sufficient Soroban balance for transaction fees.
- Check that the network is not congested (look for high ledger close times).
- Check that the `is_paused()` is `false` and `is_epoch_open()` is `true`.

**Recovery:**
- Restart the detection pipeline.
- Re-submit outstanding scores manually if needed.

### 3.5 Upgrade Failure

**Symptom:** After `execute_upgrade`, contract behaviour is incorrect or inaccessible.

**Recovery (Option A — Re-propose previous WASM):**
1. Obtain the previous WASM binary from version control.
2. Compute its SHA-256 hash.
3. `propose_upgrade` with the previous hash.
4. Wait for the delay to elapse.
5. `execute_upgrade`.
6. Verify the rollback with smoke tests.

**Recovery (Option B — Propose a hotfix):**
1. Prepare a fixed WASM binary and compute its hash.
2. `propose_upgrade` with the hotfix hash.
3. Wait for the delay to elapse.
4. `execute_upgrade`.

### 3.6 Pending Score Stuck in Finality Buffer

**Symptom:** `get_pending_score` returns a score that was never committed; `get_score` returns the old score or `ScoreNotFound`.

**Diagnosis:**
- Check if the finality buffer is still active (`get_finality_buffer` > 0).
- Check if `commit_after` has elapsed.
- Check if the admin has the ability to cancel the pending score.

**Recovery:**
```bash
# Cancel the stuck pending score (admin-only)
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source <ADMIN_KEY> \
  --network <NETWORK> \
  -- \
  cancel_pending_score \
  --admin-signers '[<ADMIN_ADDRESS>]' \
  --wallet <WALLET_ADDRESS> \
  --asset-pair XLM_USDC

# Re-submit the score if needed
```

### 3.7 Signer/Configuration Rotation Failure

**Symptom:** After rotating service signers or threshold, some legitimate submissions are rejected with `InsufficientSigners` or `UnauthorizedSigner`.

**Recovery:**
1. Check the current service set: `soroban contract invoke ... -- get_service_signers`
2. Check the current threshold: `soroban contract invoke ... -- get_service_threshold`
3. If signers are missing, add them back with `add_service_signer` or adjust the threshold with `set_service_threshold`.
4. Verify that the correct number of signers can authorize a submission.

### 3.8 Unavailable Dependencies (Off-Chain Pipeline Down)

**Symptom:** No new scores being submitted for an extended period.

**Impact:** The contract continues to function; existing scores remain accessible. Risk coverage degrades for wallets that haven't been scored recently.

**Recovery:**
- Restore the off-chain detection pipeline.
- Re-submit scores for affected wallets and pairs.
- Consider lowering the model version's `Proposed` state to skip the time-lock for critical fixes (requires admin key).

### 3.9 Interrupted Retry / Duplicate Submission

**Symptom:** A score submission fails mid-transaction and the caller retries, but the retry is rejected with `RateLimitExceeded`.

**Diagnosis:** The failed transaction may have written state (e.g., last_submit_time) before failing. The retry is now within the cooldown window.

**Recovery:**
- Wait for the cooldown to elapse (default 1 hour, configurable via `set_cooldown`).
- Re-submit the score.
- To avoid this pattern, use idempotency keys in the off-chain pipeline and implement exponential backoff.

## 4. Backup and Restore

### 4.1 Backup Procedures

```bash
# 1. Export critical configuration
echo "Contract ID: <CONTRACT_ID>"
echo "Admin: $(soroban contract invoke --id <CONTRACT_ID> --source <ADMIN_KEY> --network <NETWORK> -- get_admin)"
echo "Service: $(soroban contract invoke --id <CONTRACT_ID> --source <ADMIN_KEY> --network <NETWORK> -- get_service)"
echo "Version: $(soroban contract invoke --id <CONTRACT_ID> --source <ADMIN_KEY> --network <NETWORK> -- get_version)"
echo "Upgrade Delay: $(soroban contract invoke --id <CONTRACT_ID> --source <ADMIN_KEY> --network <NETWORK> -- get_upgrade_delay)"
echo "Finality Buffer: $(soroban contract invoke --id <CONTRACT_ID> --source <ADMIN_KEY> --network <NETWORK> -- get_finality_buffer)"
echo "Paused Pairs: $(soroban contract invoke --id <CONTRACT_ID> --source <ADMIN_KEY> --network <NETWORK> -- get_paused_pairs)"
echo "Service Signers: $(soroban contract invoke --id <CONTRACT_ID> --source <ADMIN_KEY> --network <NETWORK> -- get_service_signers)"
echo "Service Threshold: $(soroban contract invoke --id <CONTRACT_ID> --source <ADMIN_KEY> --network <NETWORK> -- get_service_threshold)"
```

### 4.2 Restore Procedures

Restore from backup configuration if the contract state is corrupted:

1. Deploy a fresh contract instance.
2. Initialize with the backed-up admin and service addresses.
3. Reconfigure all parameters (cooldown, thresholds, etc.).
4. Re-add service signers and set the threshold.
5. If historical scores are needed, bulk-re-submit from the off-chain pipeline's data store.
6. Update all integrators with the new contract ID.

## 5. Diagnostic Commands

```bash
# Full contract state dump (run from a script or manually)
echo "=== Contract State ==="
echo "Admin: $(soroban contract invoke --id $CID --source $SRC --network $NET -- get_admin)"
echo "Service: $(soroban contract invoke --id $CID --source $SRC --network $NET -- get_service)"
echo "Version: $(soroban contract invoke --id $CID --source $SRC --network $NET -- get_version)"
echo "Paused: $(soroban contract invoke --id $CID --source $SRC --network $NET -- is_paused)"
echo "Upgrade Delay: $(soroban contract invoke --id $CID --source $SRC --network $NET -- get_upgrade_delay)"
echo "Finality Buffer: $(soroban contract invoke --id $CID --source $SRC --network $NET -- get_finality_buffer)"
echo "Cooldown: $(soroban contract invoke --id $CID --source $SRC --network $NET -- get_cooldown)"
echo "Service Threshold: $(soroban contract invoke --id $CID --source $SRC --network $NET -- get_service_threshold)"
echo "Service Signers: $(soroban contract invoke --id $CID --source $SRC --network $NET -- get_service_signers)"
echo "Paused Pairs: $(soroban contract invoke --id $CID --source $SRC --network $NET -- get_paused_pairs)"
echo "Score Floor Policy: $(soroban contract invoke --id $CID --source $SRC --network $NET -- get_score_floor_policy)"
echo "Pending Upgrade: $(soroban contract invoke --id $CID --source $SRC --network $NET -- get_pending_upgrade || echo 'None')"
```

## 6. Monitoring Dashboard Queries

| Metric | Query |
|--------|-------|
| Daily submission count | Count `score_submitted` events per day |
| Rejection rate | Count non-zero `rejection_code` entries in `BatchResult` per day |
| Average score | Aggregate `score` field from `score_submitted` events |
| Threshold breaches | Count `threshold_breached` events per day |
| Paused pairs | Call `get_paused_pairs()` and alert if non-empty outside maintenance windows |
| Upgrade activity | Watch for `upgrade_proposed`, `upgrade_executed`, `upgrade_vetoed` events |
| Service activity | Monitor `service_activity` event timestamps; alert if stale |
