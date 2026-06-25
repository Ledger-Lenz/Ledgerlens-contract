# LedgerLens Architecture Guide

This document describes the complete data flow and architectural design of the LedgerLens risk-scoring system, from off-chain signal generation through on-chain storage and composable protocol consumption.

---

## 1. System Overview

```
┌────────────────────────────────────────────────────────────────────────┐
│                       STELLAR DEX TRADES                               │
│  Real-time trade events, order book data, account activity             │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │
                                    ▼
┌────────────────────────────────────────────────────────────────────────┐
│               OFF-CHAIN DETECTION PIPELINE                             │
│  ┌──────────────────────────────────────────────────────────────────┐  │
│  │ Data Ingestion: Horizon API → data lake                          │  │
│  └──────────────────────────────────────────────────────────────────┘  │
│                                    │                                    │
│                                    ▼                                    │
│  ┌──────────────────────────────────────────────────────────────────┐  │
│  │ Analysis: Benford's Law engine + ML ensemble                     │  │
│  │ (Random Forest, XGBoost, LightGBM) → 0-100 Risk Score           │  │
│  └──────────────────────────────────────────────────────────────────┘  │
│                                    │                                    │
│                                    ▼                                    │
│  ┌──────────────────────────────────────────────────────────────────┐  │
│  │ Score Aggregation & secp256k1 attestation signing               │  │
│  │ SHA256(payload) → signature                                      │  │
│  └──────────────────────────────────────────────────────────────────┘  │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │
                    ┌───────────────▼───────────────┐
                    │ Service key signs commitment  │
                    │ (optional: threshold sig)     │
                    └───────────────┬───────────────┘
                                    │
                                    ▼
        ┌───────────────────────────────────────────────────┐
        │  SUBMIT TO SOROBAN CONTRACT: submit_score()       │
        │  ┌─────────────────────────────────────────────┐  │
        │  │ • Verify service authorization              │  │
        │  │ • Check rate limit (cooldown)                │  │
        │  │ • Validate velocity cap (delta per hour)     │  │
        │  │ • Verify cryptographic attestation (opt-in) │  │
        │  │ • Write to contract storage                  │  │
        │  └─────────────────────────────────────────────┘  │
        └───────────────┬───────────────────────────────────┘
                        │
        ┌───────────────▼────────────────────────┐
        │  ON-CHAIN STORAGE                      │
        │  ┌────────────────────────────────────┐│
        │  │ Score key: (wallet, asset_pair)   ││
        │  │ Value: RiskScore struct            ││
        │  └────────────────────────────────────┘│
        │  ┌────────────────────────────────────┐│
        │  │ History ring buffer (capped at 10) ││
        │  └────────────────────────────────────┘│
        │  ┌────────────────────────────────────┐│
        │  │ Aggregate cache per wallet         ││
        │  └────────────────────────────────────┘│
        └───────────────┬────────────────────────┘
                        │
        ┌───────────────┴────────────────────────────┐
        │                                            │
        ▼                                            ▼
┌─────────────────────────────┐    ┌────────────────────────────────┐
│  EVENTS EMITTED             │    │  COMPOSABLE PROTOCOL ENTRY     │
│  • score_submitted          │    │  query_risk_gate()             │
│  • score_delta              │    │  query_risk_gate_with_...()    │
│  • threshold_breached       │    │  get_score()                   │
│  • rl_ovrd                  │    │  get_aggregate_score()         │
└─────────────────────────────┘    └────────────────────────────────┘
                                           │
                                           ▼
                                    ┌──────────────────┐
                                    │  AMMs, Lending   │
                                    │  Protocols, DEX  │
                                    │  Aggregators     │
                                    └──────────────────┘
```

---

## 2. Off-Chain Pipeline

The off-chain detection pipeline combines statistical anomaly detection with machine learning to produce LedgerLens risk scores:

### 2.1 Data Ingestion

- **Source:** Stellar Horizon API, real-time DEX trade stream
- **Data collected:**
  - Trade volume and frequency per wallet/asset-pair
  - Price deviation patterns
  - Order placement and cancellation behavior
  - Account transaction history and age

### 2.2 Benford's Law Engine

- Analyzes the distribution of digit frequencies in trade volumes and prices
- Detects wash trading by identifying non-natural digit distributions
- Flags anomalies where digit patterns deviate from expected Benford distribution

### 2.3 Machine Learning Ensemble

- Multiple models (Random Forest, XGBoost, LightGBM) trained on known fraud patterns
- Each model produces confidence scores; ensemble combines them
- Model version tracked for auditability; older versions can be deprecated

### 2.4 Score Aggregation

- Outputs a single 0-100 risk score per (wallet, asset_pair)
- Flags: `benford_flag` (statistical anomaly), `ml_flag` (ML classifier triggered)
- Confidence: model confidence in the score (0-100)
- Timestamp: when the score was computed off-chain

---

## 3. Attestation Flow

The optional cryptographic attestation closes the gap between "this transaction came from the authorized service" and "this exact score was produced by the off-chain pipeline."

### 3.1 Commitment Construction

When `set_service_pubkey` has been called, every score submission must include a `ScoreAttestation`:

```
Preimage (175 bytes):
  wallet (56 bytes):         StrKey G... encoding, ASCII
  asset_pair (9 bytes):      ASCII symbol, zero-padded right
  score (4 bytes):           u32, little-endian
  benford_flag (1 byte):     0 or 1
  ml_flag (1 byte):          0 or 1
  timestamp (8 bytes):       u64, little-endian
  confidence (4 bytes):      u32, little-endian
  model_version (4 bytes):   u32, little-endian
  contract_address (56 bytes): StrKey C... encoding, ASCII
  network_id (32 bytes):     ED25519 hash
```

Digest: `SHA256(preimage)`

### 3.2 Signature

Off-chain pipeline signs the digest with secp256k1 private key:
- Produces 65-byte signature: r (32 bytes) || s (32 bytes) || recovery_id (1 byte)
- Recovery ID must be 0 or 1

### 3.3 On-Chain Verification

Contract verifies:
1. Recompute commitment from actual call arguments → must match `attestation.commitment`
2. Extract r, s, recovery_id from 65-byte signature
3. Call `env.crypto().secp256k1_recover()` to recover public key
4. Compare recovered key against registered service pubkey (supports compressed and uncompressed formats)
5. All checks must pass; any mismatch → `Error::InvalidAttestation`

Reference: [`docs/attestation-spec.md`](./attestation-spec.md)

---

## 4. On-Chain Write Path

### 4.1 Entry Point: `submit_score()`

**Function:** [`src/lib.rs:submit_score()`](../contracts/ledgerlens-score/src/lib.rs#L225)

```rust
pub fn submit_score(
    env: Env,
    signers: Vec<Address>,
    wallet: Address,
    asset_pair: Symbol,
    score: u32,
    benford_flag: bool,
    ml_flag: bool,
    timestamp: u64,
    confidence: u32,
    model_version: u32,
    attestation_input: Option<ScoreAttestationInput>,
) -> Result<(), Error>
```

### 4.2 Authorization Phase

1. **Service Authorization:**
   - If M-of-N service set is configured: verify `signers` contains at least `threshold` members, each with `require_auth()`
   - Otherwise: `service.require_auth()` (legacy single-service path)

2. **Cryptographic Attestation (optional):**
   - If `set_service_pubkey()` has been called: [`verify_attestation()`](../contracts/ledgerlens-score/src/lib.rs#L3700) must succeed
   - Checks commitment, signature, and public key recovery

### 4.3 Validation Phase

1. **Global circuit breaker:** If admin has called `set_paused(true)`, reject with `Error::ContractPaused`
2. **Per-pair circuit breaker:** If `set_pair_paused(asset_pair, true)`, reject with `Error::PairPaused`
3. **Rate limit (cooldown):** Check `now >= last_submit + cooldown_secs` (default 3600s)
4. **Score range:** Verify `score <= 100`
5. **Confidence range:** Verify `confidence <= 100`
6. **Timestamp:** Verify `timestamp != 0`
7. **Velocity cap:** If enabled, verify `abs_diff(new_score, old_score) <= allowed_delta_per_hour`
8. **Score floor:** If enabled, verify `new_score >= floor_value` (when wallet's historical max >= high_water_mark)

### 4.4 Storage Write

**Storage keys written:**

| Key | Description |
|-----|-------------|
| [`Score(wallet, asset_pair)`](../contracts/ledgerlens-score/src/types.rs#L299) | Latest `RiskScore` struct |
| [`ScoreHistory(wallet, asset_pair)`](../contracts/ledgerlens-score/src/types.rs#L308) | Ring buffer of up to 10 historical scores |
| [`ScoreCount(wallet, asset_pair)`](../contracts/ledgerlens-score/src/types.rs#L321) | Total submission count (never truncated) |
| [`LastSubmitTime(wallet, asset_pair)`](../contracts/ledgerlens-score/src/types.rs#L319) | Ledger timestamp of submission |
| [`AggregateScore(wallet)`](../contracts/ledgerlens-score/src/types.rs#L312) | Cached cross-asset aggregate |
| [`HistoricalMaxScore(wallet, asset_pair)`](../contracts/ledgerlens-score/src/types.rs#L365) | Running peak score (for floor policy) |

### 4.5 Events Emitted

**Function:** `events::score_submitted()` — Emitted with wallet, asset_pair, score, and all flags/metadata.

Other events fired conditionally:
- `threshold_breached()` — if `score >= risk_threshold`
- `score_delta()` — if score changed
- `rl_ovrd()` — if rate limit was overridden by admin

---

## 5. On-Chain Read Path

### 5.1 Composable Integration: `query_risk_gate()`

**Function:** [`src/lib.rs:query_risk_gate()`](../contracts/ledgerlens-score/src/lib.rs#L3300)

```rust
pub fn query_risk_gate(env: Env, wallet: Address, asset_pair: Symbol, gate_threshold: u32) -> bool
```

**Returns:** `true` if `score < gate_threshold`, `false` if `score >= gate_threshold` or no score exists.

**Design:**
- **Infallible:** Returns `bool`, never panics, never errors
- **Side-effect free:** Pure read, does not mutate state, does not extend TTL
- **Conservative:** Missing score treated as risky (`false`)
- **Safe to call from guard clauses** in other contracts

**Typical usage in AMM:**
```rust
if !ledger_lens.query_risk_gate(&wallet, &pair, &risky_threshold) {
    return Err(RiskyWalletError);
}
// Safe to proceed
```

Reference: [`docs/interface-spec.md § 1.1`](./interface-spec.md#11-query_risk_gate--the-integration-primitive)

### 5.2 Confidence-Gated Read: `query_risk_gate_with_confidence()`

**Function:** [`src/lib.rs:query_risk_gate_with_confidence()`](../contracts/ledgerlens-score/src/lib.rs#L3320)

```rust
pub fn query_risk_gate_with_confidence(
    env: Env,
    wallet: Address,
    asset_pair: Symbol,
    gate_threshold: u32,
    min_confidence: u32,
) -> bool
```

**Returns:** `true` **only** when:
1. Score exists
2. `score < gate_threshold`
3. `score.confidence >= max(min_confidence, global_min_confidence)`

Low-confidence scores are treated as "no data" (returns `false`), preventing low-confidence "safe" signals from passing guard clauses.

Reference: [`docs/interface-spec.md § 1.2`](./interface-spec.md#12-query_risk_gate_with_confidence--confidence-gated-integration-primitive)

### 5.3 Full Score Retrieval: `get_score()`

**Function:** [`src/lib.rs:get_score()`](../contracts/ledgerlens-score/src/lib.rs#L2900)

```rust
pub fn get_score(env: Env, wallet: Address, asset_pair: Symbol) -> Result<RiskScore, Error>
```

**Returns:** Full `RiskScore` struct with all fields, or `Error::ScoreNotFound` if absent.

**Use when:** You need confidence, flags, or model version details; can afford to handle errors.

### 5.4 Aggregate Score: `get_aggregate_score()`

**Function:** [`src/lib.rs:get_aggregate_score()`](../contracts/ledgerlens-score/src/lib.rs#L2925)

```rust
pub fn get_aggregate_score(env: Env, wallet: Address) -> Result<AggregateRiskScore, Error>
```

**Returns:** Cross-asset risk view:
```rust
pub struct AggregateRiskScore {
    pub aggregate_score: u32,      // Weighted average across all pairs
    pub pair_count: u32,
    pub max_pair_score: u32,
    pub max_pair: Symbol,
    pub benford_flag_count: u32,
    pub ml_flag_count: u32,
    pub last_updated: u64,
    pub decay_lambda_applied: bool,
}
```

Aggregated scores help understand **portfolio-level risk** across multiple asset pairs.

### 5.5 History Access: `get_score_history()`

**Function:** [`src/lib.rs:get_score_history()`](../contracts/ledgerlens-score/src/lib.rs#L2908)

**Returns:** Vec of up to 10 most recent `RiskScore` entries, oldest first. Empty if none.

---

## 6. Aggregator Shard Architecture

The aggregator contract ([`contracts/ledgerlens-aggregator/src/lib.rs`](../contracts/ledgerlens-aggregator/src/lib.rs)) provides federated score queries across multiple shards.

### 6.1 Design

- **Admin-managed shard registration:** Admin adds/removes contract instances via [`add_shard()`](../contracts/ledgerlens-aggregator/src/lib.rs#L25) / [`remove_shard()`](../contracts/ledgerlens-aggregator/src/lib.rs#L48)
- **Maximum shards:** 10 (to bound gas costs)
- **Query behavior:**
  - [`query_risk_gate()`](../contracts/ledgerlens-aggregator/src/lib.rs#L73) — All shards must pass (AND logic)
  - [`get_score()`](../contracts/ledgerlens-aggregator/src/lib.rs#L94) — Returns max score across all shards
  - [`get_aggregate_score()`](../contracts/ledgerlens-aggregator/src/lib.rs#L115) — Returns max aggregate across shards

### 6.2 Use Case

Deployments spanning multiple Soroban instances or networks can register all instances as shards and query them as a unified federated system. The aggregator enforces consistency: all shards must agree on "pass" for `query_risk_gate`, and the most conservative (highest risk) score wins.

---

## 7. Upgrade and Governance Lifecycle

### 7.1 Admin Key Rotation

**Function:** [`set_admin()`](../contracts/ledgerlens-score/src/lib.rs#L2800) (M-of-N multisig-capable)

Admin can rotate themselves. Multiple admins supported via M-of-N multisig:
- [`add_admin_signer()`](../contracts/ledgerlens-score/src/lib.rs) — Add to multisig set
- [`set_admin_threshold()`](../contracts/ledgerlens-score/src/lib.rs) — Require M-of-N authorization for admin functions

### 7.2 Service Account Rotation

**Function:** [`set_service()`](../contracts/ledgerlens-score/src/lib.rs#L2850)

Admin rotates the authorized off-chain service key. Only the new service can submit scores.

### 7.3 Cryptographic Key Rotation

**Function:** [`set_service_pubkey()`](../contracts/ledgerlens-score/src/lib.rs#L3100)

Admin sets or rotates the secp256k1 public key used for attestation verification. Once set, attestation becomes **mandatory** and cannot be disabled (only rotated).

### 7.4 Upgrade Proposal Process

**Phase 1: Propose** — [`propose_upgrade(new_wasm_hash)`](../contracts/ledgerlens-score/src/lib.rs#L3400)
- Admin commits to new contract bytecode hash
- Stores proposal with `executable_after = now + upgrade_delay`
- Emits `upgrade_proposed` event

**Phase 2: Wait** — Time-locked delay (configurable, default 48 hours)
- Anyone can call [`get_pending_upgrade()`](../contracts/ledgerlens-score/src/lib.rs#L3450) to inspect proposal
- Gives community time to audit

**Phase 3: Execute** — [`execute_upgrade()`](../contracts/ledgerlens-score/src/lib.rs#L3420)
- Admin verifies delay has elapsed
- Installs new WASM via `env.deployer().update_current_contract_wasm(...)`
- Clears proposal, emits `upgrade_executed`

**Phase 3 (alternate): Veto** — [`veto_upgrade()`](../contracts/ledgerlens-score/src/lib.rs#L3430)
- Admin can cancel during time-lock window
- Emergency escape hatch for compromised key or malicious proposal

---

## 8. Trust Assumptions

### 8.1 What the Contract Trusts

1. **The service key's attestation** (when enabled via `set_service_pubkey`)
   - Contract assumes the off-chain pipeline honestly computed the score and signed it
   - Cryptographic verification ensures the pipeline cannot alter payloads in flight

2. **The admin's configuration decisions**
   - Velocity cap thresholds, cooldown periods, score floors — all set by admin
   - Admin is trusted to configure reasonable parameters

3. **Soroban SDK internals**
   - Authorization checks (`require_auth`) — assumed to work correctly
   - Cryptographic operations (`secp256k1_recover`, `sha256`) — assumed correct

### 8.2 What the Contract Does NOT Trust

1. **The caller's claimed risk score**
   - Even if a caller submits via `submit_score()`, their score is not trusted until it is cryptographically attested and validated
   - Authorization alone (without attestation) does not prove the caller produced the score

2. **Third-party oracles or external APIs**
   - Contract contains no external oracle or cross-chain bridged data
   - All data is either submitted directly via transactions or verified via on-chain cryptography

3. **Wallet integrity**
   - Contract does not verify that a wallet address is "legitimate" or "owned by" a person
   - Any address can receive scores; whether that address belongs to a real user is outside contract scope

### 8.3 What the Contract Cannot Verify

1. **Off-chain pipeline integrity**
   - Contract cannot inspect whether the off-chain pipeline's Benford's Law engine or ML models are correct
   - Assumes that if attestation verifies, the score came from the registered key — not whether the key's operator is honest or their models are sound

2. **Score staleness**
   - Contract stores a timestamp but does not auto-expire old scores
   - Consuming protocols must implement their own staleness checks via `score.timestamp` and `model_version`

3. **Collusion between service and admin**
   - If the service key holder and admin collude, they can submit arbitrary scores
   - Cryptographic attestation helps catch tampering by untrusted infrastructure, but not collusion

---

## Summary

LedgerLens is a **trust-minimized, transparent, composable risk oracle**:

- **Off-chain:** Benford's Law + ML ensemble compute fraud signals
- **In-flight:** Optional secp256k1 attestation proves payload authenticity
- **On-chain:** Scored data stored with rate limits, velocity caps, and floor policies
- **Composable:** Guard-clause-safe `query_risk_gate()` integrates into any Soroban protocol
- **Governed:** Time-locked upgrades, multisig admin, service key rotation

All code is open-source and on-chain logic is auditable. The contract cannot be "secretly" modified to lower risk thresholds or bypass authorization — any change requires a multi-day governance process visible to all participants.
