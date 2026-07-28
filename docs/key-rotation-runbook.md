# Key-Rotation Operator Runbook — LedgerLens Score Contract

> **Version:** 1.0 (issue #633)  
> **Audience:** On-call operators and contract admins  
> **Related:** `scripts/rotate-keys-rehearsal.sh`, `docs/incident-response-runbook.md`

---

## 1. Service Signer Rotation

### Trust Assumptions

| Role | Trust Model |
|------|-------------|
| **Admin multisig** (M-of-N) | Authorises all signer additions/removals. Threshold must be > N/2. |
| **Service signers** (M-of-N) | Each service signer may individually authorise score submissions. |
| **New signer onboarding** | The new signer's Stellar keypair must be generated securely off-chain and the public address known before the admin transaction is submitted. |

### Procedure: Add a Service Signer

```bash
soroban contract invoke \
  --id $CONTRACT_ID \
  --source $ADMIN_KEY \
  --network $NETWORK \
  -- \
  add_service_signer \
  --admin_signants '["<ADMIN_1>", "<ADMIN_2>"]' \
  --signer "<NEW_SIGNER_ADDRESS>"
```

### Procedure: Remove a Service Signer

```bash
soroban contract invoke \
  --id $CONTRACT_ID \
  --source $ADMIN_KEY \
  --network $NETWORK \
  -- \
  remove_service_signer \
  --admin_signants '["<ADMIN_1>", "<ADMIN_2>"]' \
  --signer "<SIGNER_TO_REMOVE>"
```

**Note:** The threshold auto-adjusts downward if it exceeds the new set size.

### Procedure: Full Rotation (replace entire set)

```bash
# 1. Add new signers
for signer in "<NEW_SIGNER_1>" "<NEW_SIGNER_2>" "<NEW_SIGNER_3>"; do
  soroban contract invoke --id $CONTRACT_ID --source $ADMIN_KEY --network $NETWORK -- \
    add_service_signer --admin_signants '["<ADMIN>"]' --signer "$signer"
done

# 2. Update threshold
soroban contract invoke --id $CONTRACT_ID --source $ADMIN_KEY --network $NETWORK -- \
  set_service_threshold --admin_signants '["<ADMIN>"]' --threshold 2

# 3. Remove old signers
for signer in "<OLD_SIGNER_1>" "<OLD_SIGNER_2>"; do
  soroban contract invoke --id $CONTRACT_ID --source $ADMIN_KEY --network $NETWORK -- \
    remove_service_signer --admin_signants '["<ADMIN>"]' --signer "$signer"
done
```

### Constraints

- Maximum service signers: `MAX_SERVICE_SIGNERS` (10)
- Threshold can never exceed the current set size
- Removing a signer when threshold > new size auto-adjusts threshold

---

## 2. Admin Signer Rotation

### Procedure: Add an Admin Signer

```bash
soroban contract invoke \
  --id $CONTRACT_ID \
  --source $ADMIN_KEY \
  --network $NETWORK \
  -- \
  add_admin_signer \
  --admin_signants '["<ADMIN_1>", "<ADMIN_2>"]' \
  --signer "<NEW_ADMIN_SIGNER>"
```

### Procedure: Remove an Admin Signer

```bash
soroban contract invoke \
  --id $CONTRACT_ID \
  --source $ADMIN_KEY \
  --network $NETWORK \
  -- \
  remove_admin_signer \
  --admin_signants '["<ADMIN_1>", "<ADMIN_2>"]' \
  --signer "<ADMIN_TO_REMOVE>"
```

### Constraints

- Maximum admin signers: `MAX_ADMIN_SIGNERS` (5)
- Setting threshold to 0 is not allowed (use remove to fully transition)
- Threshold auto-adjusts when signer removal would leave threshold > set size

---

## 3. Service Pubkey Rotation

### Procedure: Gradual Rotation (with Overlap Window)

```bash
# Rotate with a 24-hour overlap window so in-flight attestations complete
soroban contract invoke \
  --id $CONTRACT_ID \
  --source $ADMIN_KEY \
  --network $NETWORK \
  -- \
  rotate_service_pubkey \
  --admin_signants '["<ADMIN>"]' \
  --new_key "<NEW_65BYTE_PUBKEY>" \
  --overlap_secs 86400
```

During the overlap window both old and new pubkeys are accepted for attestation verification. After the overlap expires, only the new key is accepted.

### Procedure: Instant Rotation

```bash
soroban contract invoke \
  --id $CONTRACT_ID \
  --source $ADMIN_KEY \
  --network $NETWORK \
  -- \
  rotate_service_pubkey \
  --admin_signants '["<ADMIN>"]' \
  --new_key "<NEW_PUBKEY>" \
  --overlap_secs 0
```

### Verification

```bash
# Check if a pending rotation exists
soroban contract invoke \
  --id $CONTRACT_ID \
  --source $ADMIN_KEY \
  --network $NETWORK \
  -- \
  get_pending_service_pubkey

# Get active pubkey
soroban contract invoke \
  --id $CONTRACT_ID \
  --source $ADMIN_KEY \
  --network $NETWORK \
  -- \
  get_service_pubkey
```

---

## 4. Failure Scenarios & Recovery

### Scenario 1: Signer Loss (Compromised Key)

**Detection:** Monitoring alert from abnormal score submission pattern or security audit.  
**Containment:**

```bash
# 1. Freeze the contract (if available — requires contract version >= 5)
# 2. Remove compromised signer via admin multisig
soroban contract invoke \
  --id $CONTRACT_ID --source $ADMIN_KEY --network $NETWORK -- \
  remove_service_signer \
  --admin_signants '["<ADMIN_1>", "<ADMIN_2>"]' \
  --signer "<COMPROMISED_SIGNER>"

# 3. If threshold was N-of-M and we lost signers, add replacement
soroban contract invoke \
  --id $CONTRACT_ID --source $ADMIN_KEY --network $NETWORK -- \
  add_service_signer \
  --admin_signants '["<ADMIN_1>", "<ADMIN_2>"]' \
  --signer "<NEW_SIGNER>"

# 4. Rotate service pubkey if signer had attestation key access
soroban contract invoke \
  --id $CONTRACT_ID --source $ADMIN_KEY --network $NETWORK -- \
  rotate_service_pubkey \
  --admin_signants '["<ADMIN_1>", "<ADMIN_2>"]' \
  --new_key "<NEW_PUBKEY>" \
  --overlap_secs 3600

# 5. Unfreeze (if frozen)
```

### Scenario 2: Threshold Lost (Too Few Signers)

**Detection:** `submit_score` returns `InsufficientSigners` (14).  
**Recovery:**

```bash
# Add enough signers to meet the threshold
soroban contract invoke \
  --id $CONTRACT_ID --source $ADMIN_KEY --network $NETWORK -- \
  add_service_signer \
  --admin_signants '["<ADMIN>"]' \
  --signer "<NEW_SIGNER>"
```

### Scenario 3: Rotation Interrupted (Network Failure)

**Detection:** Transaction submission times out or returns unknown error mid-rotation.  
**Recovery:**

```bash
# 1. Verify current state
soroban contract invoke \
  --id $CONTRACT_ID --source $ADMIN_KEY --network $NETWORK -- \
  get_service_signer_count

# 2. Check if signer was partially added
soroban contract invoke \
  --id $CONTRACT_ID --source $ADMIN_KEY --network $NETWORK -- \
  get_service_signers

# 3. Depending on state, either add remaining signers or remove partial ones
```

### Scenario 4: Pubkey Rotation with Stale Signatures

**Detection:** Score submissions fail with `InvalidAttestation` after pubkey rotation.  
**Recovery:** Ensure overlap window is sufficiently long for in-flight submissions. If signatures are already failing:

```bash
# Extend overlap by rotating to the same new key with a fresh overlap window
soroban contract invoke \
  --id $CONTRACT_ID --source $ADMIN_KEY --network $NETWORK -- \
  rotate_service_pubkey \
  --admin_signants '["<ADMIN>"]' \
  --new_key "<CURRENT_NEW_KEY>" \
  --overlap_secs 3600
```

---

## 5. Rehearsal Automation

Run the key-rotation rehearsal on testnet before any production change:

```bash
./scripts/rotate-keys-rehearsal.sh
```

For a dry run:
```bash
./scripts/rotate-keys-rehearsal.sh --dry-run
```

To keep the deployment for manual inspection:
```bash
./scripts/rotate-keys-rehearsal.sh --keep-deployment
```

### What the Rehearsal Validates

1. **Service signer rotation** — add signers, set threshold, remove signers, verify auto-adjustment
2. **Admin signer rotation** — add signers, set threshold, remove signers
3. **Signer loss simulation** — remove signer and verify threshold auto-adjusts
4. **Partial failure handling** — invalid signer rejection, threshold > set size rejection
5. **Rollback** — re-add removed signers, restore thresholds
6. **Service pubkey rotation** — set initial key, rotate with overlap, instant rotation
7. **Stale data recovery** — submit score, refresh signer set, verify data persists
8. **Post-action report** — records every action with stable identifiers

---

## 6. Post-Action Report

After every key-rotation operation (production or rehearsal), a post-action report should be generated containing:

- Action log with stable action IDs (T0001, T0002, etc.)
- Pre-rotation signer configuration
- Post-rotation signer configuration
- Status (passed / failed / passed-with-warnings)
- Counts of operations attempted, succeeded, and failed
- Network and contract identifiers

The `rotate-keys-rehearsal.sh` script automatically generates this report. For manual operations, record the action log and configuration before/after.

---

## 7. Monitoring Signals

| Signal | Event | What to Watch For |
|--------|-------|-------------------|
| `sig_add` | signer added | Unexpected signer additions |
| `sig_rem` | signer removed | Unexpected signer removals |
| `sig_thr` | threshold changed | Threshold changes without change request |
| `pk_upd` | pubkey set | Unauthorised pubkey changes |
| `pk_rot` | pubkey rotation started | Pubkey rotation without incident ticket |
| InsufficientSigners | `Error::InsufficientSigners` | Signer set may be depleted |
| InvalidAttestation | `Error::InvalidAttestation` | May indicate stale pubkey after rotation |
