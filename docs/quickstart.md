# LedgerLens Developer Quickstart

Get up and running with LedgerLens on the Stellar testnet in under 10 minutes. This guide walks you through building, deploying, and submitting your first risk score.

> ⚠️ **Testnet-Only Warning**  
> Keys and accounts used in this guide are for testnet only. **Never reuse these keys on mainnet.** Testnet and mainnet are separate networks with different consensus; keys are not interchangeable.

---

## Prerequisites

1. **Rust (stable)** — [Install Rust](https://rustup.rs/)
   ```bash
   rustup target add wasm32-unknown-unknown
   ```

2. **Soroban CLI** — [Installation guide](https://soroban.stellar.org/docs/build/setup)
   ```bash
   cargo install soroban-cli
   soroban --version
   ```

3. **Stellar Testnet Account** — Fund with XLM
   - Visit [Friendbot](https://friendbot.stellar.org)
   - Paste your public key (generated below)
   - Receive 10,000 XLM for testing

4. **soroban CLI Testnet Network Alias** — Configure once:
   ```bash
   soroban network add --global testnet \
     --rpc-url https://soroban-testnet.stellar.org \
     --network-passphrase "Test SDF Network ; September 2015"
   ```

---

## Step 1: Generate Testnet Identities

Create two Soroban CLI identities: one for admin (contract deployer), one for service (score submitter).

```bash
# Admin identity (controls contract configuration)
soroban keys generate --global deployer

# Service identity (submits risk scores)
soroban keys generate --global service

# View the public keys
soroban keys address deployer
soroban keys address service
```

**Save these addresses** — you'll use them in the next step.

---

## Step 2: Fund Your Testnet Account

Fund the deployer identity with XLM for gas:

```bash
DEPLOYER_ADDRESS=$(soroban keys address deployer)
echo "Fund this address on testnet: $DEPLOYER_ADDRESS"
```

Visit [Friendbot](https://friendbot.stellar.org), paste the address, and receive 10,000 XLM.

Verify funding:
```bash
soroban account balance --account deployer --network testnet
```

---

## Step 3: Build the WASM Binary

```bash
cd contracts/ledgerlens-score

# Build the WASM binary (release mode, optimized)
cargo build --target wasm32-unknown-unknown --release

# Expected output:
# ├── target/wasm32-unknown-unknown/release/ledgerlens_score.wasm
# └── (2.5 MB unoptimized)
```

---

## Step 4: Deploy to Testnet

### Option A: Using `deploy.sh` (Recommended)

```bash
# From the repo root
export SERVICE_ADDRESS=$(soroban keys address service)
./deploy.sh testnet deployer "$SERVICE_ADDRESS"

# Script will:
# 1. Build the WASM binary
# 2. Optimize it
# 3. Deploy to testnet
# 4. Initialize the contract
# 5. Verify deployment
# 6. Print the CONTRACT_ID
```

**Save the CONTRACT_ID** printed at the end.

### Option B: Manual Deployment

```bash
export WASM_PATH="target/wasm32-unknown-unknown/release/ledgerlens_score.wasm"
export ADMIN=$(soroban keys address deployer)
export SERVICE=$(soroban keys address service)
export NETWORK=testnet

# Optimize
soroban contract optimize --wasm "$WASM_PATH"

# Deploy
CONTRACT_ID=$(soroban contract deploy \
  --wasm "$WASM_PATH.optimized" \
  --source deployer \
  --network $NETWORK)

echo "Deployed: $CONTRACT_ID"

# Initialize
soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source deployer \
  --network $NETWORK \
  -- initialize \
  --admin "$ADMIN" \
  --service "$SERVICE"
```

---

## Step 5: Initialize the Contract

**⚠️ Security Note**: Do not use the same address for both `admin` and `service` in production. Admin controls configuration; service submits scores. Separate them for security.

```bash
# Already done by deploy.sh, but for reference:
soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source deployer \
  --network testnet \
  -- initialize \
  --admin "$(soroban keys address deployer)" \
  --service "$(soroban keys address service)"
```

Verify initialization:
```bash
soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source deployer \
  --network testnet \
  -- get_admin
```

Expected: The admin address you provided.

---

## Step 6: Submit a Test Risk Score

### Generate Test Wallet and Asset Pair

```bash
# Create a test wallet address
WALLET=$(soroban keys generate | grep "Public Key:" | awk '{print $3}')

# Define an asset pair (any short symbol)
ASSET_PAIR="XLM_USDC"
```

### Submit Score

```bash
CONTRACT_ID="C..." # from Step 4
WALLET="G..."      # generated above
SERVICE_ADDRESS=$(soroban keys address service)

soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source service \
  --network testnet \
  -- submit_score \
  --signers "[]" \
  --wallet "$WALLET" \
  --asset_pair "$ASSET_PAIR" \
  --score 42 \
  --benford_flag false \
  --ml_flag false \
  --timestamp "$(date +%s)" \
  --confidence 85 \
  --model_version 1 \
  --attestation_input none
```

**Expected response:** `Ok(())` — transaction accepted and stored on-chain.

---

## Step 7: Query the Score

```bash
soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source service \
  --network testnet \
  -- get_score \
  --wallet "$WALLET" \
  --asset_pair "$ASSET_PAIR"
```

**Expected output:**
```rust
RiskScore {
    score: 42,
    benford_flag: false,
    ml_flag: false,
    timestamp: <your_timestamp>,
    confidence: 85,
    model_version: 1,
}
```

---

## Step 8: Test the Composable Gate

Query the risk gate to gate access based on the risk score:

```bash
soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source service \
  --network testnet \
  -- query_risk_gate \
  --wallet "$WALLET" \
  --asset_pair "$ASSET_PAIR" \
  --gate_threshold 50
```

**Expected output:** `false` (score 42 is below threshold 50, so wallet is safe)

Try with a lower threshold:
```bash
soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source service \
  --network testnet \
  -- query_risk_gate \
  --wallet "$WALLET" \
  --asset_pair "$ASSET_PAIR" \
  --gate_threshold 30
```

**Expected output:** `true` (score 42 is above threshold 30, so wallet is risky)

---

## Next Steps

### Learn More

- **Attestation & Crypto Verification** — See [`docs/attestation-spec.md`](./attestation-spec.md) for cryptographic score signing and verification
- **Composable Integration** — See [`docs/interface-spec.md`](./interface-spec.md) for integrating LedgerLens into your own protocol
- **Complete Architecture** — See [`docs/architecture.md`](./architecture.md) for the full system design

### Explore the Codebase

- **Contract Logic** — [`contracts/ledgerlens-score/src/lib.rs`](../contracts/ledgerlens-score/src/lib.rs)
- **Data Types** — [`contracts/ledgerlens-score/src/types.rs`](../contracts/ledgerlens-score/src/types.rs)
- **Tests** — [`contracts/ledgerlens-score/src/test.rs`](../contracts/ledgerlens-score/src/test.rs)
- **Contributing** — [`CONTRIBUTING.md`](../CONTRIBUTING.md)

### Run Your Own Tests

```bash
cd contracts/ledgerlens-score

# Unit tests
cargo test

# Property-based tests (extended)
PROPTEST_CASES=10000 cargo test test_velocity_cap_prop

# Mutation testing
cargo install cargo-mutants
cargo mutants --jobs 4
```

---

## Troubleshooting

### "ERROR: Account not found"
**Cause:** The deployer account hasn't been funded with XLM.  
**Fix:** Use [Friendbot](https://friendbot.stellar.org) to fund the deployer address.

### "ERROR: Contract already initialized"
**Cause:** The contract was already initialized.  
**Fix:** Either use a new CONTRACT_ID (redeploy), or skip the initialize step.

### "ERROR: Unauthorized"
**Cause:** The transaction wasn't signed by the service account.  
**Fix:** Ensure you're using `--source service` for `submit_score` calls.

### "Transaction failed: RateLimitExceeded"
**Cause:** You submitted two scores for the same wallet/pair too quickly.  
**Fix:** Wait 3600 seconds (default cooldown) between submissions, or ask the admin to override with `override_rate_limit`.

### "Signature verification failed"
**Cause:** The optional cryptographic attestation signature was invalid.  
**Fix:** Either:
1. Use `--attestation_input none` (no attestation required until `set_service_pubkey` is called)
2. Or, have the off-chain pipeline sign the payload and include the valid signature

---

## Summary

You've successfully:
1. ✓ Built the LedgerLens contract binary
2. ✓ Deployed it to Stellar testnet
3. ✓ Initialized with admin and service accounts
4. ✓ Submitted a risk score on-chain
5. ✓ Queried the score
6. ✓ Tested the composable gate API

The contract is now ready for integration into your own protocols or for experimenting with advanced features like velocity caps, score floors, and cryptographic attestation.

For production use, familiarize yourself with the [governance lifecycle](./architecture.md#7-upgrade-and-governance-lifecycle) and [trust assumptions](./architecture.md#8-trust-assumptions).
