# Operator WASM Upgrade Runbook

## Overview

A WASM upgrade replaces the contract's code on-chain, enabling bug fixes, feature additions, and security patches. Upgrades are high-stakes operations requiring a time-lock delay, admin authorization, and careful coordination with integrators. This guide walks you through the process step-by-step.

**Key principles:**
- **Admin-only:** Only the admin key can propose and execute upgrades.
- **Time-locked:** An upgrade must wait 48 hours (default, configurable) before it can be executed. This window allows integrators to prepare and gives time to cancel if issues are discovered.
- **One-at-a-time:** Only one upgrade proposal can be pending; veto or execute it before proposing a new one.
- **Irreversible:** On-chain WASM upgrades cannot be automatically rolled back. If the new code is broken, you must re-propose the previous WASM.

---

## Pre-Upgrade Checklist

Before proposing an upgrade, complete the following:

### 1. Verify Admin Access
- [ ] Admin key is available and accessible.
- [ ] Admin key has been added to your signing setup (e.g., Ledger, HSM, keystore).
- [ ] You can sign transactions with the admin key (test with a dummy transaction if unsure).

### 2. Verify New WASM Binary
- [ ] New WASM is built: `cargo build --target wasm32-unknown-unknown --release`
- [ ] Binary is located at `target/wasm32-unknown-unknown/release/ledgerlens_score.wasm`
- [ ] WASM hash is computed and noted (see Step 2 below).
- [ ] WASM is tested in a staging/testnet environment first.

### 3. Announce to Integrators
- [ ] Notify all integrators (AMMs, lending protocols, indexers) of the upcoming upgrade at least **48 hours** in advance.
- [ ] Provide a summary of changes: bug fixes, new features, breaking changes.
- [ ] Include the time window: "Upgrade will be executable after [timestamp]."
- [ ] Request acknowledgment of receipt.

### 4. Confirm No In-Flight Transactions
- [ ] Check that there are no critical transactions in-flight that depend on the current contract version.
- [ ] Wait for any ongoing batch score submissions to complete.

### 5. Back Up Contract State (Off-Chain)
- [ ] Document the current contract state:
  - Admin address: `client.get_admin()`
  - Service address: `client.set_service()` (if known)
  - Pending upgrade (if any): `client.get_pending_upgrade()`
  - Upgrade delay: `client.get_upgrade_delay()` = **172,800 seconds** (48 hours)
- [ ] Export a snapshot of critical configuration values.
- [ ] These backups help with diagnostics if the upgrade fails.

---

## Step 1 — Build the New WASM

```bash
cd /path/to/ledgerlens-contract
cargo build --target wasm32-unknown-unknown --release
```

**Output:** `target/wasm32-unknown-unknown/release/ledgerlens_score.wasm`

**Verify the binary exists and is non-empty:**
```bash
ls -lh target/wasm32-unknown-unknown/release/ledgerlens_score.wasm
```

---

## Step 2 — Compute the WASM Hash

The `propose_upgrade` function requires a 32-byte SHA-256 hash of the new WASM binary. Compute it:

```bash
# Option A: Using soroban contract install (this also uploads the WASM and gives you the hash)
soroban contract install \
  --network testnet \
  --source-account YOUR_ACCOUNT_ADDRESS \
  --wasm target/wasm32-unknown-unknown/release/ledgerlens_score.wasm

# Output will show: "WasmHash: abc123...def789" (64 hex characters)
```

**Option B: Using sha256sum (if you prefer to compute locally without uploading):**
```bash
sha256sum target/wasm32-unknown-unknown/release/ledgerlens_score.wasm
# Output: abc123...def789  ledgerlens_score.wasm
```

**Note:** If using `soroban contract install`, the WASM is uploaded to the network immediately. You can then propose the upgrade with confidence that the WASM is available.

**Save the hash** (you will use it in Step 3):
```
WASM_HASH=abc123...def789
```

---

## Step 3 — Propose the Upgrade

The `propose_upgrade` function registers a new WASM to be executed after the delay elapses. This function requires admin authorization.

```bash
soroban contract invoke \
  --network testnet \
  --source-account YOUR_ADMIN_ADDRESS \
  --contract-id LEDGERLENS_CONTRACT_ID \
  -- propose_upgrade \
  --admin-signers '[ADMIN_ADDRESS_1, ADMIN_ADDRESS_2, ...]' \
  --new-wasm-hash WASM_HASH
```

**Parameters:**
- `admin-signers`: List of admin addresses that authorize this proposal. Can be a single admin or a multisig set.
- `new-wasm-hash`: The 32-byte SHA-256 hash computed in Step 2 (as a byte array or hex string).

**Expected result:**
- Function returns `Ok(())` if the proposal is successfully registered.
- Event `upgrade_proposed` is emitted with the proposal details.
- The proposal becomes executable after **172,800 seconds** (48 hours) from the current ledger timestamp.

**If it fails:**
- `Error::NotInitialized`: Contract has no admin yet (initialize first).
- `Error::UpgradeAlreadyPending`: An upgrade proposal is already in flight. Veto it first or wait for it to be executed.
- `Error::Unauthorized`: Admin signature verification failed. Check that all required signers authorized the transaction.

---

## Step 4 — Monitor the Delay

The upgrade cannot be executed until the delay has elapsed. Use this window to:

### Check Remaining Time

```bash
soroban contract invoke \
  --network testnet \
  --source-account YOUR_ACCOUNT_ADDRESS \
  --contract-id LEDGERLENS_CONTRACT_ID \
  -- get_pending_upgrade
```

**Response** (if a proposal exists):
```json
{
  "new_wasm_hash": "0xabc123...def789",
  "proposed_at": 1234567890,
  "executable_after": 1234567890 + 172800 = 1234740690
}
```

**Calculate remaining time:**
```
remaining_secs = executable_after - current_ledger_timestamp
remaining_hours = remaining_secs / 3600
```

### If You Need to Cancel

If a critical issue is discovered during the delay window, veto the proposal:

```bash
soroban contract invoke \
  --network testnet \
  --source-account YOUR_ADMIN_ADDRESS \
  --contract-id LEDGERLENS_CONTRACT_ID \
  -- veto_upgrade \
  --admin-signers '[ADMIN_ADDRESS]'
```

This clears the pending proposal. You can then propose a new WASM if needed.

---

## Step 5 — Execute the Upgrade

Once the delay has elapsed, execute the upgrade:

```bash
soroban contract invoke \
  --network testnet \
  --source-account YOUR_ADMIN_ADDRESS \
  --contract-id LEDGERLENS_CONTRACT_ID \
  -- execute_upgrade \
  --admin-signers '[ADMIN_ADDRESS_1, ADMIN_ADDRESS_2, ...]'
```

**Expected result:**
- The contract's WASM code is replaced with the new binary.
- Event `upgrade_executed` is emitted.
- All state (scores, admins, settings) is preserved.
- New code is active immediately.

**If it fails:**
- `Error::UpgradeNotReady`: The delay has not elapsed yet. Wait longer.
- `Error::NoPendingUpgrade`: There is no proposal to execute. Propose first.
- `Error::Unauthorized`: Admin signature verification failed.

---

## Step 6 — Post-Upgrade Verification

### Verify Contract Still Works

```bash
soroban contract invoke \
  --network testnet \
  --source-account YOUR_ACCOUNT_ADDRESS \
  --contract-id LEDGERLENS_CONTRACT_ID \
  -- get_score \
  --wallet SOME_WALLET \
  --asset-pair XLM_USDC
```

Expected: The function returns a valid score (or `ScoreNotFound` if the wallet has no score, but the contract is responsive).

### Check Contract Version (if applicable)

```bash
soroban contract invoke \
  --network testnet \
  --source-account YOUR_ACCOUNT_ADDRESS \
  --contract-id LEDGERLENS_CONTRACT_ID \
  -- get_version
```

This returns the contract version number (e.g., `2` or `3`). If your new WASM incremented the version, you should see the new number.

### Verify Integrator Compatibility

- [ ] Contact a few key integrators and confirm their contracts can still call `query_risk_gate`, `get_score`, etc.
- [ ] Run an AMM swap on testnet and verify the risk gate is enforced.
- [ ] Check indexers and off-chain services; confirm they can still parse contract events and data.

---

## Rollback Options

**On-chain WASM upgrades are not automatically reversible.** If the new code is buggy, you must take action:

### Option A: Re-Propose the Previous WASM

1. Obtain the previous WASM binary (from version control or backups).
2. Compute its SHA-256 hash.
3. Propose that hash as a new upgrade.
4. Wait the delay again.
5. Execute.

**Time cost:** 48 hours + execution time.

### Option B: Propose a Hotfix

1. If the issue is minor, prepare a new WASM with a fix.
2. Follow the same proposal and execution flow.

**Time cost:** 48 hours + execution time.

### Option C: Pause the Contract (Temporary)

If the issue is critical and you cannot afford to wait 48 hours, call `pause`:

```bash
soroban contract invoke \
  --network testnet \
  --source-account YOUR_ADMIN_ADDRESS \
  --contract-id LEDGERLENS_CONTRACT_ID \
  -- pause \
  --admin-signers '[ADMIN_ADDRESS]'
```

This disables all score submissions and queries (returning `Error::ContractPaused`). Integrators will see the contract is offline. Resume it with `unpause` once the fix is ready.

---

## Troubleshooting

| Issue | Cause | Solution |
|-------|-------|----------|
| "UpgradeAlreadyPending" | An upgrade is already proposed | `veto_upgrade` to clear it, then propose again |
| "UpgradeNotReady" | Delay has not elapsed | Wait longer; check `get_pending_upgrade` for exact time |
| "NoPendingUpgrade" | Tried to execute without proposing first | Run `propose_upgrade` first |
| "Unauthorized" | Admin signature invalid | Ensure correct admin address(es) and proper signing |
| Contracts still call old function name | New WASM removed/renamed a function | Ensure breaking changes were announced to integrators |
| Queries fail after upgrade | Contract state is corrupted | Check backups, consider `pause` + rollback |

---

## Security Best Practices

1. **Test on testnet first:** Propose and execute the upgrade on testnet before mainnet.
2. **Sign with hardware wallet:** Use a Ledger or HSM for admin keys, never hot wallets.
3. **Multisig is recommended:** Require at least 2 of 3 admins to sign upgrades, preventing single-key compromise.
4. **Announce widely:** Give integrators 48+ hours notice so they can prepare.
5. **Monitor closely:** Watch on-chain events and integrator feedback immediately after execution.
6. **Have a rollback plan:** Know exactly how to re-propose the old WASM if needed.

---

## References

- **Interface specification:** [`docs/interface-spec.md`](interface-spec.md)
- **Upgrade-related constants:** `MIN_UPGRADE_DELAY_SECS = 172,800` (48 hours), `MAX_UPGRADE_DELAY_SECS = 1,209,600` (14 days)
- **Contract source:** `contracts/ledgerlens-score/src/lib.rs`

