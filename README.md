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

### `submit_score(wallet: Address, asset_pair: Symbol, score: u32, benford_flag: bool, ml_flag: bool, timestamp: u64, confidence: u32)`
Called by the authorised LedgerLens off-chain service to register a computed risk score on-chain. Requires authorization from the configured LedgerLens service account. `score` and `confidence` must be in the range 0-100.

### `get_score(wallet: Address, asset_pair: Symbol) -> RiskScore`
Read-only function callable by any Soroban contract. Returns the most recent LedgerLens risk score and metadata for a given wallet and asset pair.

### `is_score_stale(wallet: Address, asset_pair: Symbol) -> bool`
Read-only function callable by any Soroban contract. Returns `true` when no score exists, or when the stored score is older than the configured staleness window.

### `set_staleness_window(window_secs: u64)`
Updates the staleness window in seconds. Admin only; zero is rejected.

### `get_staleness_window() -> u64`
Read-only lookup of the current staleness window in seconds. Defaults to 604800 seconds (7 days).

### `set_service(new_service: Address)`
Rotates the authorised off-chain scoring service address. Admin only.

### `get_admin() -> Address` / `get_service() -> Address`
Read-only lookups of the current admin and authorised scoring service addresses.

### `RiskScore` Structure

```rust
pub struct RiskScore {
    pub score: u32,          // 0-100; higher = more suspicious
    pub benford_flag: bool,  // True if Benford anomaly detected
    pub ml_flag: bool,       // True if ML classifier flagged
    pub timestamp: u64,      // Ledger timestamp of last update
    pub confidence: u32,     // Model confidence 0-100
}
```

## Security Features

1. **Authorization Checks**: Only the authorised LedgerLens service account can submit scores
2. **Read-Only Composability**: `get_score` is permissionless and side-effect free, safe for any contract to call
3. **Bounded Values**: Scores and confidence are constrained to the 0-100 range
4. **Overflow Protection**: Safe math operations with overflow checks

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
├── contracts/
│   └── ledgerlens-score/
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs                  ← Contract entrypoints
│           ├── types.rs                ← RiskScore, DataKey
│           ├── storage.rs              ← Persistent/instance storage helpers
│           ├── errors.rs               ← Contract error codes
│           ├── events.rs               ← Event emission helpers
│           └── test.rs                 ← Unit tests
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
| `is_score_stale(wallet, asset_pair)` | anyone | none (read-only) | **`api`**, **`dashboard`**, and third-party contracts that need to reject outdated scores |
| `set_staleness_window(window_secs)` | admin | `admin.require_auth()` | ops/admin tooling for freshness policy changes |
| `get_staleness_window()` | anyone | none (read-only) | **`api`**, **`dashboard`**, and monitoring tools that display freshness policy |
| `set_service(new_service)` | admin | `admin.require_auth()` | ops/admin tooling for key rotation |
| `get_admin()` / `get_service()` | anyone | none (read-only) | ops tooling, `api` health checks |

`asset_pair` is a `Symbol` (≤ 9 chars in Soroban's short-symbol form, e.g. `XLM_USDC`). If `core`/`api` need pair identifiers longer than 9 characters, they must agree on a canonical short encoding here before the contract is deployed to mainnet.

### Events Emitted (for off-chain listeners)

- `score` — `(wallet, asset_pair) -> (score, benford_flag, ml_flag, confidence, timestamp)`, emitted on every `submit_score`
- `svc_upd` — emitted when the admin rotates the authorised service address

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
