# LedgerLens Contract 🔍

[![Built on Stellar](https://img.shields.io/badge/Built%20on-Stellar-blue?logo=stellar)](https://stellar.org)
[![Soroban Smart Contracts](https://img.shields.io/badge/Smart%20Contracts-Soroban-purple)](https://soroban.stellar.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

Soroban smart contract that serves as the on-chain risk-score registry for **LedgerLens**, a hybrid fraud detection system for the Stellar DEX combining Benford's Law digit analysis with ensemble machine learning.

## Overview

LedgerLens detects wash trading and artificial volume on the Stellar Decentralised Exchange (SDEX) by analysing trade data with statistical (Benford's Law) and machine learning techniques. The off-chain detection pipeline computes a **LedgerLens Risk Score (0-100)** for wallets and asset pairs, and this contract acts as the **on-chain truth layer** for those scores — making fraud signals composable with other Soroban protocols (AMMs, lending platforms, DEX aggregators) without relying on an external oracle.

## Features

- **On-Chain Risk Score Registry**: Stores the latest LedgerLens risk score, flags, confidence, and timestamp per wallet/asset-pair
- **Authorized Score Submission**: Only the authorised LedgerLens off-chain service account can write scores
- **Composable Read Access**: Any Soroban contract can query risk scores to gate suspicious activity
- **Benford & ML Flags**: Distinguishes between statistical anomaly flags and ML classifier flags
- **Confidence Scoring**: Each risk score carries a model confidence value (0-100)
- **Open and Auditable**: Methodology, scores, and contract logic are fully transparent

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     LAYER 1: DATA INGESTION                 │
│  Stellar Horizon API → trade history, order book events,    │
│  account activity, asset metadata                            │
└──────────────────────────┬──────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                  LAYER 2: DETECTION ENGINE                   │
│  Benford's Law Anomaly Engine + Ensemble ML Models           │
│  (Random Forest, XGBoost, LightGBM)                          │
│             → LedgerLens Risk Score (0-100)                  │
└──────────────────────────┬──────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│           LAYER 3: SOROBAN CONTRACT (this repo) + API        │
│  • submit_score() — write risk scores on-chain               │
│  • get_score()    — read risk scores from any contract       │
│  • Public REST API and dashboard consume this contract       │
└─────────────────────────────────────────────────────────────┘
```

### Core Components

- **lib.rs**: Main contract implementation — `submit_score` and `get_score`
- **types.rs**: `RiskScore` data structure (score, flags, confidence, timestamp)
- **storage.rs**: Persistent storage for per-wallet/asset-pair risk scores
- **errors.rs**: Custom error types for contract operations
- **test.rs**: Test suite covering submission, retrieval, and authorization

## Contract Functions

### `initialize(admin: Address, service: Address)`
One-time setup. Sets the admin (who can rotate the service address) and the LedgerLens off-chain service account authorised to submit scores.

### `submit_score(signers: Vec<Address>, wallet: Address, asset_pair: Symbol, score: u32, benford_flag: bool, ml_flag: bool, timestamp: u64, confidence: u32, model_version: u32, attestation: Option<ScoreAttestation>)`
Called by the authorised LedgerLens off-chain service to register a computed risk score on-chain. Requires authorization from the configured LedgerLens service account (or, under the M-of-N multisig model, from `threshold` of the listed `signers`). `score` and `confidence` must be in the range 0-100. `attestation` is required once `set_service_pubkey` has been configured — see [Score Attestation](#score-attestation).

### `get_score(wallet: Address, asset_pair: Symbol) -> RiskScore`
Read-only function callable by any Soroban contract. Returns the most recent LedgerLens risk score and metadata for a given wallet and asset pair.

### `get_score_count(wallet: Address, asset_pair: Symbol) -> u32`
Read-only function callable by any account or contract. Returns the total number of score submissions ever recorded for `wallet` / `asset_pair`. Unlike `get_score_history` (which caps at `HISTORY_MAX_DEPTH`), this counter is never truncated, giving off-chain services a cheap O(1) signal to distinguish newly monitored wallets from those with a long history.

### `set_history_max_depth(depth: u32)`
Admin-only. Sets the maximum number of entries retained in the per-wallet / per-asset-pair score history ring buffer. `depth` must be in the range `[1, 50]`; values outside this range are rejected with `InvalidHistoryDepth`. Defaults to `10` until configured.

**Lazy-truncation behaviour:** reducing the depth does not remove existing entries immediately. Entries beyond the new cap remain in the ring until the next `submit_score` (or `submit_scores_batch`) call for that pair triggers the eviction loop, at which point the ring is trimmed in a single pass. Off-chain consumers reading `get_score_history` between the depth change and the next submission may temporarily observe more entries than the new cap.

### `get_history_max_depth() -> u32`
Read-only. Returns the current ring-buffer depth. Defaults to `10` until the admin sets one explicitly.

### `set_service(new_service: Address)`
Rotates the authorised off-chain scoring service address. Admin only.

### `get_admin() -> Address` / `get_service() -> Address`
Read-only lookups of the current admin and authorised scoring service addresses.

### `get_pending_admin() -> Address` / `has_pending_admin_transfer() -> Address`
Read-only function to check the state of a pending admin.

### `get_aggregate_score(wallet: Address) -> AggregateRiskScore`
Read-only function. Returns `wallet`'s cross-asset aggregate risk score — a weighted average computed live from every asset pair the wallet has a `RiskScore` for. Always recomputed from current per-pair scores, never served from a stale cache. Returns `ScoreNotFound` if the wallet has no scores.

### `set_pair_weight(asset_pair: Symbol, weight: u32)`
Sets the weight used for `asset_pair` in the aggregate risk computation. Defaults to `1` (simple average) for any pair the admin hasn't configured. A weight of `0` excludes the pair from the aggregate's denominator. Admin only.

### `get_pair_weight(asset_pair: Symbol) -> u32`
Read-only lookup of the configured weight for `asset_pair`.

### `submit_scores_batch(submissions: Vec<ScoreSubmission>) -> BatchResult`

Called by the authorised LedgerLens off-chain service to register multiple risk scores in a single invocation. The service account authorises once for the whole batch.

Returns a `BatchResult` containing per-entry outcomes so the caller knows exactly which entries succeeded and why any failed. Entries with out-of-range `score` (>100) or `confidence` (>100), zero `timestamp`, or that arrive before the submission cooldown has elapsed, are recorded as rejected with an appropriate `rejection_code`.

**ABI change in contract version 2:** The return type changed from `u32` (count of accepted entries) to the structured `BatchResult`. Callers built against the old ABI must regenerate their client bindings.

### `query_risk_gate(wallet: Address, asset_pair: Symbol, gate_threshold: u32) -> bool`
The cross-contract integration primitive. Returns `true` when the wallet's score is **strictly below** `gate_threshold` (safe to proceed), and `false` when the score is `>= gate_threshold` **or no score exists**. It is **infallible** (returns `bool`, never an error), **never panics**, and is **side-effect free** — designed to be called directly from inside another protocol's guard clause. See [Composability](#composability) and [`docs/interface-spec.md`](docs/interface-spec.md).

### `supports_interface(capability: Symbol) -> bool`
Runtime capability detection for the composability interface. Returns `true` for the registered capabilities `score`, `history`, `batch`, `gate`, and `aggr`, letting integrators feature-detect instead of hardcoding contract version numbers.

### `propose_upgrade(new_wasm_hash: BytesN<32>)`
Admin only. Starts a time-locked contract upgrade by committing to `new_wasm_hash`. Stores an `UpgradeProposal` with `executable_after = now + get_upgrade_delay()` and emits `upgrade_proposed`. Does not change the code. Rejected with `UpgradeAlreadyPending` if a proposal is already in flight. See [Upgrade Governance](#upgrade-governance).

### `execute_upgrade()`
Admin only. After the time-lock elapses, re-verifies `now >= executable_after` and installs the new WASM via `env.deployer().update_current_contract_wasm(...)`, clears the proposal, and emits `upgrade_executed`. Returns `UpgradeNotReady` before the delay or `NoPendingUpgrade` if none exists.

### `veto_upgrade()`
Admin only. Cancels the pending proposal during the time-lock window (emergency escape hatch for a malicious proposal or compromised key) and emits `upgrade_vetoed`.

### `get_pending_upgrade() -> UpgradeProposal`
Permissionless. Returns the in-flight proposal so anyone can audit it during the window. Returns `NoPendingUpgrade` if none.

### `set_upgrade_delay(delay_secs: u64)` / `get_upgrade_delay() -> u64`
Admin sets the time-lock delay applied to future proposals, bounded to `[MIN_UPGRADE_DELAY_SECS, MAX_UPGRADE_DELAY_SECS]` (48 hours – 14 days); out-of-range values are rejected with `InvalidUpgradeDelay`. Defaults to 48 hours.

### `set_cooldown(secs: u64)` / `get_cooldown() -> u64`
Admin sets the cooldown enforced between accepted submissions for the same `(wallet, asset_pair)`, bounded to `[MIN_COOLDOWN_SECS, MAX_COOLDOWN_SECS]` (1 minute – 24 hours); out-of-range values are rejected with `InvalidCooldown`. Defaults to 1 hour. See [Rate Limiting](#rate-limiting).

### `override_rate_limit(wallet: Address, asset_pair: Symbol)`
Admin-only emergency escape hatch. Immediately clears the stored cooldown deadline for `(wallet, asset_pair)`, so the next `submit_score` / `submit_scores_batch` call for that pair is accepted regardless of how recently the last one was. Intended for correcting a known-bad score right away, not for routine use. Emits `rl_ovrd`.

### `get_last_submit_time(wallet: Address, asset_pair: Symbol) -> u64`
Read-only lookup of the ledger timestamp of the last accepted submission for `(wallet, asset_pair)`, or `0` if none has ever been accepted (or it was cleared by `override_rate_limit`).

### `clear_score_history(wallet: Address, asset_pair: Symbol)` ⚠️ irreversible
Admin only. Permanently erases the score history ring buffer for `wallet` / `asset_pair`. No-op if no history exists. Emits `clr_hist` for the on-chain audit trail. **Keep off-chain backups before calling — this cannot be undone on-chain.**

### `clear_score(wallet: Address, asset_pair: Symbol)` ⚠️ irreversible
Admin only. Permanently erases the latest score entry for `wallet` / `asset_pair`. After this call, `get_score` returns `ScoreNotFound`. No-op if no score exists. Emits `clr_scr` for the on-chain audit trail. **Keep off-chain backups before calling — this cannot be undone on-chain.**
### `set_service_pubkey(pubkey: Bytes)` / `get_service_pubkey() -> Bytes`
Admin sets (or rotates) the off-chain detection pipeline's secp256k1 public key — 33 bytes compressed or 65 bytes uncompressed, rejected otherwise with `InvalidPubkeyLength` — used to verify `ScoreAttestation`s. Once set it cannot be unset, only rotated. `get_service_pubkey` returns `ServicePubkeyNotSet` before one has been configured. See [Score Attestation](#score-attestation).

### `RiskScore` Structure

```rust
pub struct RiskScore {
    pub score: u32,          // 0-100; higher = more suspicious
    pub benford_flag: bool,  // True if Benford anomaly detected
    pub ml_flag: bool,       // True if ML classifier flagged
    pub timestamp: u64,      // Ledger timestamp of last update
    pub confidence: u32,     // Model confidence 0-100
    pub model_version: u32,  // Detection-pipeline model version
}
```

### `AggregateRiskScore` Structure

A wallet that is moderately suspicious across several asset pairs poses a higher *portfolio-level* risk than its individual per-pair scores suggest in isolation. `AggregateRiskScore` expresses that risk on-chain:

```rust
pub struct AggregateRiskScore {
    pub aggregate_score: u32,     // 0-100, weighted average across all pairs
    pub pair_count: u32,          // number of distinct pairs the wallet has a score for
    pub max_pair_score: u32,      // highest individual pair score
    pub max_pair: Symbol,         // the pair with the highest score
    pub benford_flag_count: u32,  // number of pairs with benford_flag = true
    pub ml_flag_count: u32,       // number of pairs with ml_flag = true
    pub last_updated: u64,        // timestamp of the most recently updated pair score
}
```

### `BatchResult` and `BatchEntryResult` Structures

`submit_scores_batch` returns a `BatchResult` that the off-chain API service can inspect to learn which entries succeeded and which were rejected:

```rust
pub struct BatchEntryResult {
    pub index: u32,           // zero-based position in the submitted batch
    pub accepted: bool,       // true if written to storage
    pub rejection_code: u32,  // 0 if accepted; Error discriminant if rejected
}

pub struct BatchResult {
    pub accepted_count: u32,                      // number of entries written to storage
    pub rejected_count: u32,                      // number of entries rejected
    pub results: Vec<BatchEntryResult>,            // per-entry outcomes, same order as input
}
```

Possible `rejection_code` values (from the `Error` enum):

| Code | Meaning |
|-----:|---------|
| 4 | `InvalidScore` — score > 100 |
| 5 | `InvalidConfidence` — confidence > 100 |
| 23 | `RateLimitExceeded` — submission cooldown not yet elapsed |
| 25 | `InvalidTimestamp` — timestamp == 0 |

The weighted average is:

```
aggregate_score = Σ (pair_weight[i] * pair_score[i]) / Σ pair_weight[i]
```

`pair_weight[i]` defaults to `1` for every pair (a plain average) unless the admin sets a different weight via `set_pair_weight`. A pair with weight `0` is excluded from the denominator — its score still counts toward `pair_count`, `max_pair_score`, the flag counts, and `last_updated`, but not toward `aggregate_score`.

#### Worked example

A wallet has three scored pairs:

| Pair | Score | Weight |
|---|---|---|
| XLM_USDC | 60 | 1 |
| XLM_BTC | 65 | 1 |
| XLM_ETH | 70 | 1 |

With default (equal) weights: `aggregate_score = (60 + 65 + 70) / 3 = 65`.

Now suppose the admin sets `XLM_BTC`'s weight to `2` (e.g. because BTC pairs carry more systemic risk):

```
aggregate_score = (60*1 + 65*2 + 70*1) / (1 + 2 + 1)
                = (60 + 130 + 70) / 4
                = 260 / 4
                = 65
```

A wallet scoring 60-70 on three pairs individually might not breach the per-pair `RiskThreshold` (default 75), but the aggregate view makes the *combined* exposure visible to any contract or dashboard that queries `get_aggregate_score` — without needing to fetch and average every pair manually.

`get_aggregate_score` iterates the wallet's full pair list, so its cost is O(N) in the number of distinct pairs the wallet has scores for. The contract is designed around a practical maximum of `MAX_WALLET_PAIRS` (20) pairs per wallet; this is documented as a constant but not enforced on-chain.

## Error Codes

| Code | Name | When returned |
|------|------|---------------|
| 1 | `AlreadyInitialized` | `initialize` called more than once |
| 2 | `NotInitialized` | Any state-mutating call before `initialize` |
| 3 | `Unauthorized` | Caller is not the authorised service or admin |
| 4 | `InvalidScore` | `score` outside 0-100 |
| 5 | `InvalidConfidence` | `confidence` outside 0-100 |
| 6 | `ScoreNotFound` | `get_score` / `get_aggregate_score` for an unknown pair |
| 7 | `ContractPaused` | Submission attempted while admin circuit-breaker is active |
| 8 | `NoPendingAdminTransfer` | `accept_admin` / `cancel_admin_transfer` with no transfer in flight |
| 9 | `EmptyBatch` | `submit_scores_batch` called with zero entries |
| 10 | `BatchTooLarge` | Batch exceeds `MAX_BATCH_SIZE` (20) |
| 11 | `ArithmeticOverflow` | Weighted aggregate computation overflows |
| 12 | `UpgradeAlreadyPending` | `propose_upgrade` while a proposal is already pending |
| 13 | `NoPendingUpgrade` | `execute_upgrade` / `veto_upgrade` / `get_pending_upgrade` with no proposal |
| 14 | `InsufficientSigners` | Fewer than threshold signers supplied to `submit_score` |
| 15 | `UnauthorizedSigner` | A supplied signer is not in the service set |
| 16 | `InvalidThreshold` | `set_service_threshold` given `0` or a value > set size |
| 17 | `ServiceSetFull` | `add_service_signer` when set already has `MAX_SERVICE_SIGNERS` members |
| 18 | `SignerAlreadyInSet` | `add_service_signer` with an address already present |
| 19 | `SignerNotInSet` | `remove_service_signer` with an address not in the set |
| 20 | `UpgradeNotReady` | `execute_upgrade` before the time-lock has elapsed |
| 21 | `InvalidUpgradeDelay` | `set_upgrade_delay` value outside `[MIN, MAX]` bounds |
| 22 | `InvalidStalenessWindow` | `set_staleness_window` called with `0` |
| 23 | `RateLimitExceeded` | Submission before the per-pair cooldown has elapsed |
| 24 | `InvalidCooldown` | `set_cooldown` value outside `[MIN_COOLDOWN_SECS, MAX_COOLDOWN_SECS]` |
| 25 | `InvalidTimestamp` | `submit_score` called with `timestamp = 0` |

## Upgrade Governance

Soroban contracts can be upgraded by the admin via `update_current_contract_wasm`, which replaces the **entire** contract logic in a single transaction. Without governance, one admin key — or a compromised one — could silently install a backdoor or disable a security check with no warning. LedgerLens gates every upgrade behind an on-chain **time-lock** so the community always gets a mandatory window to inspect and react.

**The flow:**

1. The admin **proposes** an upgrade, committing to a new WASM hash.
2. A mandatory delay passes (**minimum 48 hours**, configurable up to 14 days). During this window anyone can call `get_pending_upgrade` to inspect the committed hash and alert the community.
3. Only after the delay can the admin **execute** the upgrade. Alternatively, the admin can **veto** it at any time during the window (e.g. if the key was compromised).

```
   admin                         contract                        community
     │                              │                                │
     │ propose_upgrade(hash)        │                                │
     ├─────────────────────────────►│  store UpgradeProposal         │
     │                              │  emit upgrade_proposed ────────►│  inspect via
     │                              │  executable_after = now + delay │  get_pending_upgrade
     │                              │                                │  (≥ 48 h to react)
     │            ⏳  time-lock window (no execution possible)  ⏳    │
     │                              │                                │
     │   ┌── after executable_after ──┐                              │
     │   │ execute_upgrade()          │                              │
     ├───┘                            │  require now ≥ executable_after
     │                              │  update_current_contract_wasm  │
     │                              │  emit upgrade_executed ────────►│
     │                              │  clear PendingUpgrade          │
     │                              │                                │
     │   ── OR, any time in window ──                                │
     │ veto_upgrade()               │                                │
     ├─────────────────────────────►│  clear PendingUpgrade          │
     │                              │  emit upgrade_vetoed ──────────►│
```

The time-lock is computed from `env.ledger().timestamp()` (deterministic, not caller-settable) and re-verified at execution time — never cached. The configurable delay is bounded to `[MIN_UPGRADE_DELAY_SECS, MAX_UPGRADE_DELAY_SECS]`; **raising** it is always safe, while **lowering** it shortens the veto window and should require community consensus. See [`SECURITY.md`](SECURITY.md#upgrade-governance--threat-model) for the full threat model and monitoring guidance.

## Rate Limiting

A compromised or malfunctioning off-chain service could otherwise flood the contract with submissions for the same `(wallet, asset_pair)`, exhausting storage rent, overwhelming indexers, and poisoning the score signal with rapid fluctuations. LedgerLens enforces a configurable **cooldown** between accepted submissions for any given wallet/asset-pair to bound that blast radius.

**The flow:**

1. On every `submit_score` (and per-entry in `submit_scores_batch`), the contract compares `env.ledger().timestamp()` against the pair's last accepted submission time plus the configured cooldown.
2. If the cooldown hasn't elapsed, `submit_score` returns `RateLimitExceeded`; in `submit_scores_batch` the offending entry is silently skipped (the rest of the batch still processes) and counted as not accepted.
3. A successful submission updates the pair's last-submit timestamp, starting the next cooldown window.

The cooldown defaults to **1 hour** and is admin-configurable via `set_cooldown`, bounded to `[MIN_COOLDOWN_SECS, MAX_COOLDOWN_SECS]` (1 minute – 24 hours) so the admin can neither disable rate limiting entirely nor lock a pair out indefinitely. For situations that need an immediate re-score (e.g. correcting a known-bad score), the admin can call `override_rate_limit` to clear a specific pair's cooldown rather than lowering the global setting.

Like the upgrade time-lock, the cooldown deadline is computed from `env.ledger().timestamp()` — deterministic and not caller-settable — so it cannot be bypassed by manipulating submission metadata such as the `timestamp` field on `RiskScore` itself.

## Score Attestation

The service account's `require_auth` proves a transaction was sent by the authorised key, but says nothing about whether the score payload inside that transaction matches what the off-chain detection pipeline actually computed — relevant when the service key is held by infrastructure (a relayer, a batching service, a multisig signer) that's trusted to submit transactions but shouldn't be able to silently alter scores in transit.

`submit_score`'s optional `attestation: Option<ScoreAttestation>` closes that gap with a secp256k1 signature over the exact payload:

1. The admin registers the off-chain pipeline's public key via `set_service_pubkey`. Until this is called, `attestation` is ignored entirely and every existing integration keeps working unchanged.
2. Once a pubkey is configured, every `submit_score` call must carry a valid `ScoreAttestation` — a missing or invalid one is rejected with `InvalidAttestation`. There is no way to turn this back off short of a contract upgrade.
3. On each call, the contract independently recomputes the SHA-256 commitment over the wallet, asset pair, score fields, this contract's address, and the network id (binding the signature to one deployment on one network), and rejects the call if it disagrees with the attestation's `commitment` field — that field is never trusted as input, only checked.
4. The signature is then verified via `secp256k1_recover` against the registered pubkey, supporting both compressed and uncompressed key formats.

The full byte layout and verification algorithm are specified in [`docs/attestation-spec.md`](docs/attestation-spec.md).

## Batch Attestation

`submit_scores_batch` enforces the service account's `require_auth` for the whole batch but leaves a *payload integrity* gap: a compromised or unauthorised service key can fill a batch with arbitrary scores. `submit_scores_batch_attested` closes that gap for batch submissions specifically.

The off-chain pipeline builds a Merkle tree over every entry's per-score commitment (the same 175-byte SHA-256 preimage `submit_score` already binds) with **domain-separated prefix hashing** (RFC 9162 style: `0x00` for leaves, `0x01` for internal nodes), then signs `SHA256(merkle_root)` — *not* `merkle_root` directly — with the same secp256k1 key registered via `set_service_pubkey`. The pipeline submits the batch with one `BatchAttestation { merkle_root, signature }`. The contract performs one `secp256k1_recover` over the SHA-256-wrapped root (a soroban-sdk 21.x compatibility shim — see the spec for why), then walks each entry's inclusion proof `O(log N)` style. The result is one signature per batch (instead of one per entry) plus per-entry cryptographic payload integrity.

Key properties:

- **Single secp256k1 signature per batch.** One `secp256k1_recover` on-chain call regardless of batch size.
- **Same key as `submit_score`.** No key rotation needed — the same `set_service_pubkey`-registered key signs both per-score and per-batch attestations.
- **Backward-compatible surface.** `submit_scores_batch` is unchanged; `submit_scores_batch_attested` is a new opt-in entry point.
- **Cost-bounded.** Per-entry Merkle proofs are capped at `MAX_MERKLE_PROOF_DEPTH` (30 levels = up to ~2^30 leaves, far above `MAX_BATCH_SIZE` of 20) and the loop walks the proof in constant gas regardless of intermediate hash mismatches.
- **Domain-separated Merkle scheme.** Leaves (`0x00`-prefixed 33-byte preimage) and internal nodes (`0x01`-prefixed 65-byte preimage) cannot collide at any level.
- **Capability-detectable.** `supports_interface(Symbol::new(&env, "batch_attested"))` returns `true` on deployments that include this feature.

Detection: integrators can feature-detect before using it —

```rust
let cap = soroban_sdk::Symbol::new(&env, "batch_attested");
let client = LedgerLensScoreContractClient::new(&env, &contract_id);
if client.supports_interface(&cap) {
    // Use the attested batch path (one signature per batch, per-entry proofs).
} else {
    // Fall back to the plain `submit_scores_batch` (no payload integrity).
}
```

Full specification (off-chain tree-construction algorithm, on-chain verification, XDR layout, reference Python/TypeScript snippets, edge cases) is in [`docs/batch-attestation-spec.md`](docs/batch-attestation-spec.md).

## Composability

LedgerLens is only useful if other protocols can actually *act* on its scores. A risk score that lives in isolation is a dashboard widget; a risk score that an AMM, a lending market, or a DEX aggregator can read mid-transaction is a shared fraud-prevention layer for the entire Stellar DeFi ecosystem.

The problem with composing on a raw getter is fragility. If every integrator reverse-engineers `get_score` and decodes the `RiskScore` struct by hand, then the day we add a field or change an error code, every downstream protocol breaks silently. So LedgerLens exposes a **stable, versioned composability interface** — `ILedgerLensScore` — as the canonical integration point. It is fully specified in [`docs/interface-spec.md`](docs/interface-spec.md); the headline function is `query_risk_gate`.

### Why a dedicated gate function?

A guard clause inside someone else's contract has hard requirements that a normal getter doesn't meet:

- **It must never panic.** A panic in a cross-contract call traps the *caller's* transaction. If LedgerLens could panic, an attacker could craft inputs that disable the AMM's risk guard — or simply burn its gas. So `query_risk_gate` returns a plain `bool` and is engineered to be infallible.
- **It must fail closed.** Because the answer is a single `bool`, the "we have no score for this wallet" case has to collapse to one value — and that value is `false`. Unknown wallets are treated as *potentially risky*, not waved through.
- **It must be cheap and side-effect free.** It is a pure read that doesn't even extend storage TTL, so calling it from a hot path is safe.

### The AMM pattern

Here is the entire integration — drop `query_risk_gate` into your swap guard and refuse risky wallets:

```rust
fn swap(env: Env, user: Address, amount: i128) -> Result<(), AmmError> {
    // The LedgerLens contract ID you trust, stored at init time.
    let llens_contract: Address = env
        .storage()
        .instance()
        .get(&DataKey::LedgerLens)
        .ok_or(AmmError::NotConfigured)?;

    let client = LedgerLensScoreContractClient::new(&env, &llens_contract);

    // Note: no `try_`, no `?`, no error handling — the gate cannot fail.
    let is_safe = client.query_risk_gate(&user, &symbol_short!("XLM_USDC"), &75u32);
    if !is_safe {
        return Err(AmmError::HighRiskWallet);
    }

    // ... rest of swap logic ...
    Ok(())
}
```

A complete, compiling reference contract lives in [`examples/amm_gate.rs`](examples/amm_gate.rs) (build it with `cargo build --example amm_gate -p ledgerlens-score`). For versioning, error-code stability, threshold selection, and caching guidance, read the full [interface specification](docs/interface-spec.md).

## Security Features

1. **Authorization Checks**: Only the authorised LedgerLens service account can submit scores
2. **Read-Only Composability**: `get_score` is permissionless and side-effect free, safe for any contract to call
3. **Bounded Values**: Scores and confidence are constrained to the 0-100 range
4. **Overflow Protection**: Safe math operations with overflow checks
5. **Time-Locked Upgrades**: Contract WASM upgrades require a mandatory delay (≥48 h) with a public proposal anyone can inspect and an admin veto — see [Upgrade Governance](#upgrade-governance)
6. **Submission Rate Limiting**: A configurable per-`(wallet, asset_pair)` cooldown (default 1 h) bounds how often the service account can overwrite a score — see [Rate Limiting](#rate-limiting)
7. **Score Attestation**: An opt-in secp256k1 signature over the score payload lets the off-chain pipeline vouch for its contents independent of `require_auth` — see [Score Attestation](#score-attestation)

## Testing

Run the test suite with:

```bash
cargo test
```

## Quick Start

### 1. Build the Contract

```bash
cargo build --target wasm32-unknown-unknown --release
soroban contract optimize --wasm target/wasm32-unknown-unknown/release/ledgerlens_score.wasm
```

### 2. Deploy to Testnet

```bash
soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/ledgerlens_score.optimized.wasm \
  --source deployer \
  --network testnet
```

### 3. Submit a Risk Score

```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source ledgerlens_service \
  --network testnet \
  -- \
  submit_score \
  --wallet <WALLET_ADDRESS> \
  --asset_pair <ASSET_PAIR_SYMBOL> \
  --score 87 \
  --benford_flag true \
  --ml_flag true \
  --timestamp 1700000000 \
  --confidence 92
```

### 4. Query a Risk Score

```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source deployer \
  --network testnet \
  -- \
  get_score \
  --wallet <WALLET_ADDRESS> \
  --asset_pair <ASSET_PAIR_SYMBOL>
```

## Repository Structure

```
.
├── .github/
│   └── workflows/
│       └── ci.yml                      ← Format, lint, test, wasm build
├── Cargo.toml                          ← Workspace manifest
├── Cargo.lock                          ← Pinned dependency versions
├── rustfmt.toml
├── clippy.toml
├── deploy.sh                           ← Build, optimize, deploy, initialize
├── docs/
│   └── interface-spec.md               ← ILedgerLensScore composability spec
├── examples/
│   └── amm_gate.rs                     ← Reference AMM integration (query_risk_gate)
├── contracts/
│   └── ledgerlens-score/
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs                  ← Contract entrypoints
│           ├── types.rs                ← RiskScore, DataKey
│           ├── storage.rs              ← Persistent/instance storage helpers
│           ├── errors.rs               ← Contract error codes
│           ├── events.rs               ← Event emission helpers
│           ├── test.rs                 ← Implementation unit tests
│           ├── test_interface.rs       ← Interface stability tests
│           ├── test_upgrade.rs         ← Upgrade-governance tests
│           └── test_rate_limit.rs      ← Submission rate-limiting tests
├── LICENSE
├── CONTRIBUTING.md
└── README.md                            ← This file
```

## Organization Architecture

LedgerLens is split across **6 repositories**. This section orients anyone (or any AI agent) working in this contract repo on how it connects to the rest of the organization.

### The Six Repositories

| Repo | Language / Stack | Responsibility |
|---|---|---|
| **`.github`** | YAML / GitHub Actions | Org-wide CI/CD workflows, issue/PR templates, shared GitHub Actions used by all other repos |
| **`data`** | Python | Ingestion pipeline — pulls trade/order data from Stellar Horizon, stores raw + processed datasets, feature extraction for the ML layer |
| **`core`** | Python | Detection engine — Benford's Law analysis + ensemble ML models (Random Forest, XGBoost, LightGBM); consumes `data`, produces risk scores |
| **`api`** | Python (FastAPI) | Public REST API — serves risk scores and alerts; reads from `core` output and from this contract; the only repo with direct write access to this contract |
| **`dashboard`** | JS/TS (React) | Web dashboard — visualises risk scores and alerts via `api` |
| **`contract`** *(this repo)* | Rust (Soroban) | On-chain truth layer — `ledgerlens-score` Soroban contract storing the latest risk score per wallet/asset-pair |

### End-to-End Data Flow

```
 data (ingestion)
   │  raw + processed Horizon trade data
   ▼
 core (detection engine)
   │  Benford metrics + ML ensemble → RiskScore{score, benford_flag, ml_flag, confidence, timestamp}
   ▼
 api (FastAPI service)
   │  - persists scores for dashboard queries
   │  - holds the "service" keypair authorised on-chain
   │  - calls contract.submit_score(wallet, asset_pair, ...)
   ▼
 contract (this repo)        ◄── any external Soroban contract can call get_score()
   │  on-chain RiskScore registry
   ▼
 dashboard
   │  reads from api (which may itself read through to contract.get_score for verification)
   └─ renders risk scores, flags, and alerts to end users

 .github
   └─ provides CI workflows consumed by data / core / api / dashboard / contract for
      lint, test, build, and (for this repo) Soroban contract CI
```

### The Shared `RiskScore` Type — Source of Truth for Cross-Repo Types

The single most important cross-repo agreement is the **`RiskScore`** shape, defined canonically in this repo at `contracts/ledgerlens-score/src/types.rs`:

```rust
pub struct RiskScore {
    pub score: u32,          // 0-100, higher = more suspicious
    pub benford_flag: bool,  // Benford's Law anomaly detected
    pub ml_flag: bool,       // ML ensemble classifier flagged
    pub timestamp: u64,      // ledger timestamp of computation
    pub confidence: u32,     // model confidence, 0-100
    pub model_version: u32,  // detection-pipeline model version
}
```

- **`core`** must produce scores matching these fields and ranges (0-100) before handing off to `api`.
- **`api`** must mirror this shape in its Pydantic schemas (e.g. `api/schemas.py`) so JSON responses to `dashboard` stay consistent with on-chain data.
- **`dashboard`** should treat `score`/`confidence` as 0-100 integers and `benford_flag`/`ml_flag` as booleans when rendering badges.
- **Any change to this struct is a breaking change across all 6 repos** — coordinate via an issue in `.github` and update all consuming repos in the same release window.

### Contract Interface (what other repos call)

| Function | Caller | Auth required | Used by |
|---|---|---|---|
| `initialize(admin, service)` | deployer | admin (one-time) | deployment tooling only |
| `submit_score(wallet, asset_pair, score, benford_flag, ml_flag, timestamp, confidence)` | LedgerLens service account | `service.require_auth()` | **`api`** — writes scores produced by `core` |
| `get_score(wallet, asset_pair)` | anyone | none (read-only) | **`api`**, **`dashboard`** (via api), and any third-party Soroban contract that wants to gate on LedgerLens risk |
| `get_score_count(wallet, asset_pair)` | anyone | none (read-only) | **`api`** — detects newly monitored vs. long-history wallets |
| `set_service(new_service)` | admin | `admin.require_auth()` | ops/admin tooling for key rotation |
| `get_admin()` / `get_service()` | anyone | none (read-only) | ops tooling, `api` health checks |

`asset_pair` is a `Symbol` (≤ 9 chars in Soroban's short-symbol form, e.g. `XLM_USDC`). If `core`/`api` need pair identifiers longer than 9 characters, they must agree on a canonical short encoding here before the contract is deployed to mainnet.

### Events Emitted (for off-chain listeners)

- `score` — `(wallet, asset_pair) -> (score, benford_flag, ml_flag, confidence, timestamp)`, emitted on every `submit_score`
- `svc_upd` — emitted when the admin rotates the authorised service address
- `pw_upd` — `(asset_pair) -> weight`, emitted when the admin sets a pair's aggregate-risk weight via `set_pair_weight`
- `cd_upd` — `() -> cooldown_secs`, emitted when the admin changes the submission cooldown via `set_cooldown`
- `rl_ovrd` — `(wallet, asset_pair) -> admin`, emitted when the admin clears a pair's cooldown via `override_rate_limit`

`api` (or a dedicated indexer in `data`) should subscribe to these for audit trails and to keep an off-chain cache in sync with on-chain state.

### Conventions Shared Across Repos

- **Networks**: `testnet` for development, `mainnet` for production. Contract IDs per network are recorded in this repo's deployment docs and must be mirrored into `api`'s environment configuration (`CONTRACT_ID`, `RPC_URL`, `NETWORK`).
- **Secrets**: the "service" keypair that calls `submit_score` lives in `api`'s secret store — never in `core`, `data`, or `dashboard`. This repo only ever stores the **public address** of that account on-chain.
- **CI**: workflow templates live in `.github`; this repo's contract CI builds with `cargo build --target wasm32-unknown-unknown --release` and runs `cargo test`.
- **Versioning**: tag contract releases as `contract-vX.Y.Z`. `api` should pin against a specific deployed `CONTRACT_ID` + ABI version, not "latest".

### Notes for Other Repos

- **Working in `api`**: you depend on the contract interface and the `RiskScore` shape above. Check `contracts/ledgerlens-score/src/types.rs` and `lib.rs` in this repo for the current signatures before writing client code.
- **Working in `core`**: ensure your output scores conform to the 0-100 ranges above — the contract rejects out-of-range `score`/`confidence` values.
- **Working in `dashboard`**: you consume `api`, not this contract directly; but the field names/ranges above flow through unchanged.
- **Working in `data`**: no direct dependency on this contract, but feature definitions should stay consistent with what `core` ultimately reports here.
- **Working in `.github`**: any shared CI workflow for Rust/Soroban builds should target this repo's `Cargo.toml` workspace layout.

## Dependencies

- `soroban-sdk` - Soroban smart contract SDK

## License

MIT

## Roadmap

- [ ] Initial `submit_score` / `get_score` implementation
- [ ] Testnet deployment
- [ ] Integration with off-chain detection pipeline
- [ ] Mainnet deployment
- [ ] Support for batched score updates

## Contributing

Contributions are welcome. LedgerLens is an open-source public good built for the Stellar ecosystem. See [CONTRIBUTING.md](CONTRIBUTING.md) for setup, required checks, and PR guidelines.

## References

- Benford, F. (1938) 'The law of anomalous numbers', *Proceedings of the American Philosophical Society*, 78(4), pp. 551-572.
- Al Ali, A. et al. (2023) 'A powerful predicting model for financial statement fraud based on optimized XGBoost ensemble learning technique', *Applied Sciences*, 13(4).
- Antonio, G.R. (2023) 'Numbers don't lie: Decoding financial error and fraud through Benford's law', *Journal of Entrepreneurship*.
- Nti, I.K. and Somanathan, A.R. (2024) 'A scalable RF-XGBoost framework for financial fraud mitigation', *IEEE Transactions on Computational Social Systems*, 11(2), pp. 410-422.
- Yadavalli, R. and Polisetti, R. (2025) 'Optimized financial fraud detection using SMOTE-enhanced ensemble learning with CatBoost and LightGBM', *ICVADV 2025*.
- Harea, R. and Mihailă, S. (2025) 'Benford's law: Applicability in accounting and financial anomaly detection', *Challenges of Accounting for Young Researchers*, 3(1).
- Stellar Development Foundation (2024) *Horizon API Documentation*. Available at: https://developers.stellar.org/api/horizon
- Stellar Development Foundation (2024) *Soroban Smart Contract Documentation*. Available at: https://soroban.stellar.org/docs
