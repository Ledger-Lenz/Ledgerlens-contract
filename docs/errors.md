# LedgerLens Contract Error Reference

Human-readable reference for all error codes returned by the `ledgerlens-score` Soroban contract. Intended for API integrators, auditors, and dashboard builders who need to parse and handle contract errors without reading the Rust source.

> **Source of truth:** [`contracts/ledgerlens-score/src/errors.rs`](../contracts/ledgerlens-score/src/errors.rs)

## How errors surface

Soroban contract errors are returned as `u32` discriminant values. When a transaction fails, the error code appears in the transaction result's diagnostic events. SDK clients (JavaScript, Python, etc.) surface this as the numeric `code` field on the error object.

---

## Error table

### Lifecycle & initialisation

| Code | Name | Description | When returned | Client action |
|-----:|------|-------------|---------------|---------------|
| 1 | `AlreadyInitialized` | Contract has already been initialised | `initialize` called a second time | No action needed — the contract is ready to use. Do not retry. |
| 2 | `NotInitialized` | Contract has not been initialised yet | Any admin or service function called before `initialize` | Call `initialize` with the admin and service addresses first. |
| 3 | `Unauthorized` | Caller lacks authorisation | A function requiring admin or service auth is called without valid credentials | Ensure the correct account signs the transaction. |

### Score submission validation

| Code | Name | Description | When returned | Client action |
|-----:|------|-------------|---------------|---------------|
| 4 | `InvalidScore` | Score value is out of range (must be 0–100) | `submit_score`, `submit_scores_batch` (per-entry rejection), or `set_risk_threshold` with a value > 100 | Clamp the score to 0–100 before submitting. |
| 5 | `InvalidConfidence` | Confidence value is out of range (must be 0–100) | `submit_score`, `submit_scores_batch` (per-entry rejection) when `confidence > 100` | Clamp confidence to 0–100 before submitting. |
| 6 | `ScoreNotFound` | No score exists for the requested wallet / asset-pair | `get_score`, `get_effective_score`, `get_aggregate_score` when no data is stored (and no delegate fallback exists) | The wallet has not been scored yet. Submit a score first or handle the absence gracefully. |
| 25 | `InvalidTimestamp` | Timestamp is zero (reserved / uninitialised) | `submit_score`, `submit_scores_batch` (per-entry rejection), `submit_consensus_score` when `timestamp == 0` | Supply a valid non-zero Unix timestamp. |

### Circuit breaker (global & per-pair)

| Code | Name | Description | When returned | Client action |
|-----:|------|-------------|---------------|---------------|
| 7 | `ContractPaused` | The global circuit breaker is active | Any state-mutating call (`submit_score`, `submit_scores_batch`, `submit_scores_batch_attested`, `withdraw_fees`, `set_decay_rate`) while the admin has paused the contract | Wait for the admin to call `unpause`. Monitor the `contract_unpaused` event. |
| 33 | `PairPaused` | A specific asset pair has been individually frozen | `submit_score` or `submit_scores_batch` (per-entry rejection) targeting a pair paused via `set_pair_paused` | Check `is_pair_paused` before submitting for this pair. Wait for the admin to unfreeze it. |
| 34 | `PausedPairIndexFull` | The paused-pair index is at capacity (50 pairs) | `set_pair_paused(pair, true)` when `MAX_PAUSED_PAIRS` (50) pairs are already paused | Unpause an existing pair before pausing a new one. |

### Admin transfer

| Code | Name | Description | When returned | Client action |
|-----:|------|-------------|---------------|---------------|
| 8 | `NoPendingAdminTransfer` | No admin transfer is in progress | `accept_admin`, `cancel_admin_transfer`, or `get_pending_admin` when no transfer has been initiated | Call `transfer_admin` first to nominate a new admin. |

### Batch submission

| Code | Name | Description | When returned | Client action |
|-----:|------|-------------|---------------|---------------|
| 9 | `EmptyBatch` | Batch submission contains zero entries | `submit_scores_batch` or `submit_scores_batch_attested` with an empty `submissions` vec | Supply at least one entry in the batch. |
| 10 | `BatchTooLarge` | Batch exceeds the maximum size (20 entries) | `submit_scores_batch` or `submit_scores_batch_attested` when `submissions.len() > MAX_BATCH_SIZE` | Split the batch into chunks of 20 or fewer. |

### Arithmetic

| Code | Name | Description | When returned | Client action |
|-----:|------|-------------|---------------|---------------|
| 11 | `ArithmeticOverflow` | An internal weighted computation overflowed | `get_aggregate_score` or `get_effective_score` when extreme admin-configured weights cause overflow | Report to the contract admin — the pair weights may need adjustment. |

### Upgrade governance

| Code | Name | Description | When returned | Client action |
|-----:|------|-------------|---------------|---------------|
| 12 | `UpgradeAlreadyPending` | A WASM upgrade proposal already exists | `propose_upgrade` called while a proposal is pending | Veto (`veto_upgrade`) or execute (`execute_upgrade`) the current proposal first. |
| 13 | `NoPendingUpgrade` | No upgrade proposal exists | `execute_upgrade`, `veto_upgrade`, or `get_pending_upgrade` when no proposal is pending | Call `propose_upgrade` to create a proposal first. |
| 20 | `UpgradeNotReady` | The time-lock has not elapsed yet | `execute_upgrade` called before `executable_after` timestamp | Wait until the time-lock elapses. Check `get_pending_upgrade().executable_after`. |
| 21 | `InvalidUpgradeDelay` | Delay value is outside allowed bounds | `set_upgrade_delay` with a value outside `[172_800, 1_209_600]` (48 hours – 14 days) | Use a delay between 48 hours and 14 days (in seconds). |

### Multi-sig service set (M-of-N)

| Code | Name | Description | When returned | Client action |
|-----:|------|-------------|---------------|---------------|
| 14 | `InsufficientSigners` | Fewer than M signers were provided | `submit_score`, `submit_consensus_score`, or `submit_scores_batch_attested` when `signers.len() < threshold` | Include at least `threshold` valid signers. Check `get_service_threshold()`. |
| 15 | `UnauthorizedSigner` | A signer is not in the service set | `submit_score`, `submit_consensus_score`, or `submit_scores_batch_attested` when a signer is not a member of the service set | Only include addresses added via `add_service_signer`. Check `get_service_signers()`. |
| 16 | `InvalidThreshold` | Threshold is 0 or exceeds the set size | `set_service_threshold` or `set_admin_threshold` with `threshold == 0` or `threshold > set.len()` | Use a value in `[1, current_set_size]`. |
| 17 | `ServiceSetFull` | Service signer set is at capacity (10 members) | `add_service_signer` when the set already has `MAX_SERVICE_SIGNERS` (10) members | Remove an existing signer before adding a new one. |
| 18 | `SignerAlreadyInSet` | Address is already a member | `add_service_signer` or `add_admin_signer` with a duplicate address | No action needed — the signer is already registered. |
| 19 | `SignerNotInSet` | Address is not a member | `remove_service_signer` with an address not in the set | Verify the address. Check `get_service_signers()`. |

### Staleness

| Code | Name | Description | When returned | Client action |
|-----:|------|-------------|---------------|---------------|
| 22 | `InvalidStalenessWindow` | Staleness window value is zero | `set_staleness_window` with `window_secs == 0` | Use a positive value. Default is 604,800 (7 days). |

### Rate limiting

| Code | Name | Description | When returned | Client action |
|-----:|------|-------------|---------------|---------------|
| 23 | `RateLimitExceeded` | Submission arrived before cooldown elapsed | `submit_score` (hard fail) or `submit_scores_batch` / `submit_scores_batch_attested` (per-entry rejection) when the same `(wallet, asset_pair)` was scored too recently | Wait for the cooldown to elapse. Check `get_last_submit_time` and `get_cooldown`. Default cooldown is 3,600 s (1 hour). Admin can call `override_rate_limit` in emergencies. |
| 24 | `InvalidCooldown` | Cooldown value is outside allowed bounds | `set_cooldown` with a value outside `[60, 86_400]` (1 minute – 24 hours) | Use a value between 60 and 86,400 seconds. |

### Score attestation (secp256k1)

| Code | Name | Description | When returned | Client action |
|-----:|------|-------------|---------------|---------------|
| 26 | `ServicePubkeyNotSet` | No service public key has been configured | `submit_score` with an attestation when no pubkey exists; `submit_scores_batch_attested` always (hard requirement); `get_service_pubkey` before one is set | Admin must call `set_service_pubkey` before attestation-guarded submission paths work. |
| 27 | `InvalidAttestation` | Attestation verification failed | `submit_score` when the commitment mismatch, invalid recovery id, or recovered pubkey doesn't match; `submit_scores_batch_attested` (per-entry rejection) on Merkle proof mismatch | Regenerate the attestation. Verify the commitment is computed over the exact same payload fields. See `docs/attestation-spec.md`. |
| 28 | `InvalidPubkeyLength` | Public key is not 33 or 65 bytes | `set_service_pubkey` with a key that is not SEC-1 compressed (33 bytes) or uncompressed (65 bytes) | Supply a valid secp256k1 public key in SEC-1 encoding. |

### Signer tiers

| Code | Name | Description | When returned | Client action |
|-----:|------|-------------|---------------|---------------|
| 26 | `SignerTierViolation` | Signer does not meet the required tier level | A signer's tier is insufficient for the operation | Ensure the signer has the required tier assignment. |
| 27 | `InvalidSignerTier` | The specified signer tier is not valid | An invalid tier value was provided | Use a valid tier value. |

> **Note on codes 26–27:** The `SignerTierViolation` / `InvalidSignerTier` and `ServicePubkeyNotSet` / `InvalidAttestation` pairs share discriminants 26 and 27 respectively due to an in-progress feature branch merge. Integrators should use the variant name (not just the numeric code) to disambiguate and should consult the latest release for the resolved mapping.

### History & confidence

| Code | Name | Description | When returned | Client action |
|-----:|------|-------------|---------------|---------------|
| 29 | `InvalidHistoryDepth` | History depth is 0 or exceeds the maximum (50) | `set_history_max_depth` with `depth == 0` or `depth > 50` | Use a value in `[1, 50]`. Default is 10. |
| 30 | `InvalidMinConfidence` | Minimum confidence value exceeds 100 | `set_global_min_confidence` with `min_confidence > 100` | Use a value in `[0, 100]`. |

### Fee withdrawal

| Code | Name | Description | When returned | Client action |
|-----:|------|-------------|---------------|---------------|
| 30 | `FeeTokenNotSet` | No SEP-41 fee token has been configured | `get_fee_token` or `withdraw_fees` before `set_fee_token` is called | Admin must call `set_fee_token` with the token contract address. |
| 31 | `InvalidWithdrawalAmount` | Withdrawal amount is zero | `withdraw_fees` with `amount == 0` | Supply a positive withdrawal amount. |
| 32 | `WithdrawalInProgress` | Another withdrawal is already in-flight | `withdraw_fees` while the concurrency lock is held | Wait for the current withdrawal to complete, then retry. |

> **Note on code 30:** `InvalidMinConfidence` and `FeeTokenNotSet` share discriminant 30 due to an in-progress feature branch merge. Integrators should use the variant name to disambiguate and consult the latest release for the resolved mapping.

### Admin multi-sig

| Code | Name | Description | When returned | Client action |
|-----:|------|-------------|---------------|---------------|
| 35 | `AdminSetFull` | Admin signer set is at capacity (5 members) | `add_admin_signer` when the set already has `MAX_ADMIN_SIGNERS` (5) members | Remove an existing admin signer before adding a new one. |
| 36 | `AdminSignerNotInSet` | Address is not in the admin signer set | `remove_admin_signer` with an address not in the set, or `require_admin_auth` with a non-member signer | Verify the address. Check `get_admin_signers()`. |
| 37 | `InsufficientAdminSigners` | Fewer than M admin signers were supplied | Any admin-gated function when `admin_signers.len() < admin_threshold` | Include at least `threshold` valid admin signers. Check `get_admin_threshold()`. |

### Wallet-score delegation

| Code | Name | Description | When returned | Client action |
|-----:|------|-------------|---------------|---------------|
| 38 | `CyclicDelegation` | Delegation would create a cycle | `set_score_delegate` when `sub_wallet == custodian`, or when the custodian already delegates back to `sub_wallet` | Choose a different custodian that does not create a delegation cycle. |
| 39 | `DelegateNotFound` | No delegate is registered for this wallet | `remove_score_delegate` when the wallet has no active delegation | No action needed — there is nothing to remove. |

### Cross-contract risk gate

| Code | Name | Description | When returned | Client action |
|-----:|------|-------------|---------------|---------------|
| 40 | `HighRiskWallet` | Wallet failed the risk gate check | Returned by **integrating contracts** (e.g. AMMs, lending protocols) when `query_risk_gate` returns `false`. Not returned by the LedgerLens contract itself. | The wallet's risk score is too high for the operation. Inform the user and/or apply alternative handling. |

### Time-weighted decay

| Code | Name | Description | When returned | Client action |
|-----:|------|-------------|---------------|---------------|
| 41 | `InvalidDecayRate` | Decay rate parameters are invalid | `set_decay_rate` with `denominator == 0`, or when the `numerator/denominator` ratio exceeds `MAX_DECAY_LAMBDA` (1/1) | Use a denominator > 0 and ensure the ratio does not exceed 1. |

### Score embargo

| Code | Name | Description | When returned | Client action |
|-----:|------|-------------|---------------|---------------|
| 42 | `ScoreEmbargoed` | Wallet is under an active regulatory embargo | `get_score`, `get_effective_score`, `get_aggregate_score` for an embargoed wallet. `query_risk_gate` returns `false` (no error) for embargoed wallets. | The wallet's scores are temporarily sealed. Contact the contract admin or wait for a timed embargo to expire. Check `is_embargoed`. |

### Wallet relationship graph

| Code | Name | Description | When returned | Client action |
|-----:|------|-------------|---------------|---------------|
| 43 | `CounterpartyLinkFull` | Wallet has reached the maximum number of counterparty links (50) | `add_counterparty_link` when the wallet already has `MAX_COUNTERPARTY_LINKS_PER_WALLET` links | Remove an existing link before adding a new one. |
| 44 | `CounterpartyNotFound` | The specified counterparty link does not exist | `remove_counterparty_link` for a non-existent link | Verify the counterparty address. |
| 45 | `SelfLink` | Cannot link a wallet to itself | `add_counterparty_link` with the same wallet address for both sides | Use two distinct wallet addresses. |

### Score submission floor

| Code | Name | Description | When returned | Client action |
|-----:|------|-------------|---------------|---------------|
| 46 | `BelowScoreFloor` | Score is below the configured floor for a high-risk wallet | `submit_score` (hard fail) or `submit_scores_batch` (per-entry rejection) when the wallet's historical peak is at/above the high-water mark and the new score is below the floor value | The submission was blocked to prevent score-zeroing of a known high-risk wallet. If this is a legitimate re-scoring, the admin can call `override_score_floor` for a one-shot exception. |
| 47 | `InvalidScoreFloorPolicy` | Floor policy parameters are invalid | `set_score_floor_policy` when `high_water_mark` is outside `[50, 100]`, or `floor_value >= high_water_mark` | Use `high_water_mark` in `[50, 100]` and `floor_value` strictly below it. |

### Hysteresis layer

| Code | Name | Description | When returned | Client action |
|-----:|------|-------------|---------------|---------------|
| 48 | `InvalidHysteresisMargin` | Margin exceeds the maximum (50) | `set_hysteresis_margin` with `margin > MAX_HYSTERESIS_MARGIN` (50) | Use a value in `[0, 50]`. `0` disables hysteresis. |

### Multi-model consensus scoring

| Code | Name | Description | When returned | Client action |
|-----:|------|-------------|---------------|---------------|
| 49 | `InsufficientConsensus` | Not enough models agreed within the epsilon window | `submit_consensus_score` when fewer than `k` models produced scores within `±epsilon` of the provisional median | Submit more model outputs, widen `epsilon`, or lower `k` via `set_consensus_config`. Investigate model divergence. |
| 50 | `ConsensusInputEmpty` | Consensus submission has zero model entries | `submit_consensus_score` with an empty `submissions` vec | Supply at least one `ModelSubmission`. |
| 51 | `InvalidConsensusConfig` | Consensus configuration parameters are invalid | `set_consensus_config` with `k == 0` or `epsilon > 100` | Use `k >= 1` and `epsilon` in `[0, 100]`. Defaults: `k = 2`, `epsilon = 5`. |

---

## Gate caller tracking constants

These are standalone `u32` constants (not `Error` enum variants) used by the gate-caller allowlist subsystem for structural protection of `query_risk_gate`. They share the numeric namespace with the error enum but are returned through a separate code path.

| Code | Constant | Description |
|-----:|----------|-------------|
| 26 | `GATE_CALLER_ALREADY_ALLOWED` | The caller contract is already in the allowlist |
| 27 | `GATE_CALLER_NOT_FOUND` | The caller contract is not in the allowlist |
| 28 | `GATE_CALLER_LIST_FULL` | The gate-caller allowlist is at capacity |

---

## Batch rejection codes

In `submit_scores_batch` and `submit_scores_batch_attested`, individual entries are not hard-failed — they are recorded in the `BatchEntryResult` with `accepted = false` and a `rejection_code`. The `rejection_code` is the `u32` discriminant of the error that would have been returned in a single-entry call:

| `rejection_code` | Meaning |
|------------------:|---------|
| 4 | `InvalidScore` — score > 100 |
| 5 | `InvalidConfidence` — confidence > 100 |
| 23 | `RateLimitExceeded` — cooldown not elapsed |
| 25 | `InvalidTimestamp` — timestamp is 0 |
| 27 | `InvalidAttestation` — Merkle proof mismatch (attested batch only) |
| 33 | `PairPaused` — target pair is frozen |
| 46 | `BelowScoreFloor` — score below floor for high-risk wallet |
