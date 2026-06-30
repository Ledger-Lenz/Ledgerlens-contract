# Changelog

All notable changes to the LedgerLens smart contract will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to Semantic Versioning.

---

## [Unreleased]

### Added
- **`get_admin_set`**: Read-only query returning the current M-of-N admin co-signer set, mirroring `get_admin_signers`. Closes #239.
- **Rustdoc examples**: Added runnable usage examples for `get_hysteresis_margin` (closes #229) and `get_staleness_window` (closes #227).
- **Parameter Change Governance**: Added `propose_parameter_change`, `execute_parameter_change`, and `veto_parameter_change` for time-locked admin parameter changes with service-signer veto during the first half of the delay window. Supports cooldown, history depth, decay rate, velocity cap, and upgrade delay parameters. See [`docs/governance.md`](docs/governance.md).
- **Mock AMM liquidity gate**: `contracts/mock-amm` adds `provide_liquidity_gated`, `set_risk_oracle`, and confidence-aware gate configuration. See [`examples/amm_gate_example.rs`](examples/amm_gate_example.rs).
- **Batch submit benchmarks**: Criterion suite in `contracts/ledgerlens-score/benches/batch_submit.rs` measuring throughput and Soroban budget cost at batch sizes 1, 10, 50, and 100. Results uploaded as CI artifacts on merge to `main`.

### Changed
- **Lazy score TTL extension**: `set_score` now skips `extend_ttl` when the entry's estimated remaining TTL is still at or above `SCORE_TTL_THRESHOLD`, reducing ledger instruction cost for batch resubmissions to pre-warmed entries.

---

## [3.0.0] - 2026-06-22

This version corresponds to on-chain `CONTRACT_VERSION = 3`. It merges all the advanced features introduced in recent pull requests (hysteresis, score embargo, score floor, batch attestation, and consensus scoring).

### Added
- **Hysteresis layer**: Added `set_hysteresis_margin`, `get_hysteresis_margin`, and `is_in_risk_band` functions to mitigate boundary event oscillations by enforcing exit threshold margins.
- **Score Embargo**: Added `set_score_embargo`, `lift_score_embargo`, and `is_embargoed` functions to block read queries on specific wallets.
- **Score Submission Floor**: Added `set_score_floor_policy`, `get_score_floor_policy`, `get_historical_max_score`, and `override_score_floor` to prevent reputation-laundering attacks on high-risk wallets.
- **Batch Attestation (Merkle-Root Verification)**: Added `submit_scores_batch_attested` to verify an entire batch of score submissions using a single secp256k1 signature over a domain-separated Merkle root. Added the `batch_attested` capability to `supports_interface`.
- **Consensus Scoring**: Added `submit_consensus_score` and `set_consensus_config` / `get_consensus_config` functions to derive an on-chain median score from multiple attested model outputs.
- **Time-weighted Exponential Decay**: Added `set_decay_rate` / `get_decay_rate` and `get_effective_score` to enable live decay adjustments based on score age.
- **Wallet Relationship Graph**: Added `add_counterparty_link`, `remove_counterparty_link`, `get_counterparties`, and `get_contagion_depth` to map high-risk connections on-chain.
- **Wallet Score Delegation**: Added `set_score_delegate`, `remove_score_delegate`, and `get_score_delegate` fallback queries.
- **Confidence-Gated Gate**: Added `query_risk_gate_with_confidence` and `set_global_min_confidence` / `get_global_min_confidence` to enforce minimum confidence floors. Registered under capability `cgate`.
- **Model Version Stats**: Added `get_model_version_stats` and `get_all_model_versions` to monitor performance metrics.
- **Fee Withdrawal**: Added `withdraw_fees` and `set_fee_token` / `get_fee_token` to support SEP-41 token fee collection.
- **Pair Pausing**: Added `set_pair_paused`, `is_pair_paused`, and `get_paused_pairs` for surgical asset-pair pausing.
- **Admin Signer Set (M-of-N)**: Added `add_admin_signer`, `remove_admin_signer`, and `set_admin_threshold` functions to support multi-sig admin actions.

### Changed
- `query_risk_gate` now respects active score embargoes, hysteresis bands, and score delegates.

### What Broke
- **Admin Multisig Signatures**: Functions that require admin privileges (e.g. `set_history_max_depth`, `set_pair_weight`, `transfer_admin`, `cancel_admin_transfer`, `pause`, `unpause`, `set_watchlist`, `set_escalation_threshold`, `reset_breach_count`, `set_risk_threshold`, `set_jump_threshold`) now require an `admin_signers: Vec<Address>` parameter.
- **Service Multisig Signatures**: `submit_score` now takes `signers: Vec<Address>` as its first parameter to support M-of-N threshold service authorizations.

### Migration Guide
- **Ingestion Pipeline**: Update calls to `submit_score` to include the `signers` array as the first argument. If using legacy single-service authorization, pass an empty vector (`Vec::new(&env)`).
- **Admin Scripts**: Recompile deployment and administrative scripts to supply the `admin_signers` vector argument to all configured admin functions.

---

## [2.0.0] - 2026-06-12

This version corresponds to on-chain `CONTRACT_VERSION = 2`. It introduces batch submissions and cryptographic score attestation.

### Added
- **Payload Attestation**: Added `attestation: Option<ScoreAttestation>` to `submit_score` and functions `set_service_pubkey` / `get_service_pubkey` to verify the off-chain pipeline signatures using secp256k1.
- **Batch Submissions**: Added `submit_scores_batch` to allow registering multiple scores in one transaction.
- Added `supports_interface` capability detection for `score`, `history`, `batch`, `gate`, `aggr`, and `count`.

### Changed
- The return type of `submit_scores_batch` changed from `u32` (count of accepted entries) to the structured `BatchResult` detailing accept/reject metrics.

### What Broke
- **Batch Integration**: Any integrations calling `submit_scores_batch` and expecting a raw `u32` return value will fail due to the change to `BatchResult`.

### Migration Guide
- **Batch Consumers**: Update the client binding code to parse the structured `BatchResult` struct rather than treating the response as a primitive `u32`.

---

## [1.0.0] - 2026-06-01

This version corresponds to on-chain `CONTRACT_VERSION = 1`.

### Added
- Initial release of the LedgerLens Soroban smart contract registry.
- Core functions:
  - `initialize`
  - `get_version`
  - `submit_score`
  - `get_score`
  - `get_score_history`
  - `get_history_max_depth` / `set_history_max_depth`
  - `get_aggregate_score`
  - `get_pair_weight` / `set_pair_weight`
  - `query_risk_gate`
  - `supports_interface`
  - `set_service` / `get_service`
  - `get_admin` / `transfer_admin` / `accept_admin` / `cancel_admin_transfer` / `get_pending_admin` / `has_pending_admin_transfer`
  - `pause` / `unpause` / `is_paused`
  - `set_watchlist` / `is_watchlisted`
  - `set_escalation_threshold` / `get_escalation_threshold`
  - `get_breach_count` / `reset_breach_count`
  - `set_risk_threshold` / `get_risk_threshold`
  - `set_jump_threshold` / `get_jump_threshold`
  - `clear_score_history` / `clear_score`
  - `get_last_submit_time`
  - `get_score_count`

### What Broke
- Not applicable (Initial Version).

### Migration Guide
- Not applicable (Initial Version).
