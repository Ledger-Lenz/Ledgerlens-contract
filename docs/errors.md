# Error Codes Reference

All error codes are defined in [`contracts/ledgerlens-score/src/errors.rs`](../contracts/ledgerlens-score/src/errors.rs).

Errors are part of the deployed contract's ABI — their numeric values must remain stable. New variants should be appended with unused codes.

## Initialization Errors

| Code | Variant | Description |
|------|---------|-------------|
| 1 | `AlreadyInitialized` | Contract was already initialized; double-init rejected. |
| 2 | `NotInitialized` | Contract has not been initialized yet. |

## Authorization Errors

| Code | Variant | Description |
|------|---------|-------------|
| 3 | `Unauthorized` | Caller lacks required permissions. |
| 36 | `AdminSignerNotInSet` | A signer in `admin_signers` is not a member of the admin set. |
| 37 | `InsufficientAdminSigners` | Fewer than the configured threshold of admin signers were supplied. |

## Score Validation Errors

| Code | Variant | Description |
|------|---------|-------------|
| 4 | `InvalidScore` | Submitted score value is out of acceptable range. |
| 5 | `InvalidConfidence` | Confidence value is invalid (must be 0–100). |
| 6 | `ScoreNotFound` | No score record exists for the given (wallet, asset_pair). |
| 29 | `InvalidHistoryDepth` | `set_history_max_depth` called with 0 or above `MAX_HISTORY_DEPTH`. |
| 42 | `ScoreEmbargoed` | Wallet is under an active regulatory embargo. |
| 46 | `BelowScoreFloor` | Submitted score is below the configured floor, blocking potential reputation laundering. |
| 47 | `InvalidScoreFloorPolicy` | Score floor policy parameters are out of range. |

## Signer & Service Errors

| Code | Variant | Description |
|------|---------|-------------|
| 14 | `InsufficientSigners` | Fewer than the configured threshold of signers provided to `submit_score`. |
| 15 | `UnauthorizedSigner` | A signer passed to `submit_score` is not a member of the service set. |
| 16 | `InvalidThreshold` | `set_service_threshold` called with 0 or exceeding service-set size. |
| 17 | `ServiceSetFull` | `add_service_signer` called when the service set already contains `MAX_SERVICE_SIGNERS` members. |
| 18 | `SignerAlreadyInSet` | `add_service_signer` called with an address already in the set. |
| 19 | `SignerNotInSet` | `remove_service_signer` called with an address not in the set. |
| 26 | `ServicePubkeyNotSet` | No service pubkey has been configured for attestation verification. |
| 27 | `InvalidAttestation` | Attestation verification failed (commitment mismatch, bad signature, wrong pubkey). |
| 28 | `InvalidPubkeyLength` | Public key is neither 33 (compressed) nor 65 (uncompressed) bytes. |
| 52 | `SignerTierViolation` | A signer's tier does not meet the minimum requirement. |
| 53 | `InvalidSignerTier` | The signer tier value itself is invalid. |

## Batch Processing Errors

| Code | Variant | Description |
|------|---------|-------------|
| 9 | `EmptyBatch` | `submit_scores_batch` called with zero entries. |
| 10 | `BatchTooLarge` | Batch exceeds the `MAX_BATCH_SIZE` limit. |

## Upgrade & Administration Errors

| Code | Variant | Description |
|------|---------|-------------|
| 7 | `ContractPaused` | State-mutating call attempted while contract is paused by admin. |
| 8 | `NoPendingAdminTransfer` | `accept_admin` or `cancel_admin_transfer` called without a pending transfer. |
| 12 | `UpgradeAlreadyPending` | `propose_upgrade` called while a proposal is already pending. |
| 13 | `NoPendingUpgrade` | `execute_upgrade` called before time-lock elapsed, or `get_pending_upgrade` with no proposal. |
| 20 | `UpgradeNotReady` | `execute_upgrade` called before `executable_after` timestamp. |
| 21 | `InvalidUpgradeDelay` | `set_upgrade_delay` called with a value outside allowed bounds. |
| 35 | `AdminSetFull` | `add_admin_signer` called when admin set is already at `MAX_ADMIN_SIGNERS`. |

## Rate Limiting & Staleness Errors

| Code | Variant | Description |
|------|---------|-------------|
| 22 | `InvalidStalenessWindow` | A staleness window value of 0 was provided. |
| 23 | `RateLimitExceeded` | Submission for the same (wallet, asset_pair) arrived before cooldown elapsed. |
| 24 | `InvalidCooldown` | `set_cooldown` given a value below `MIN_COOLDOWN_SECS` or above `MAX_COOLDOWN_SECS`. |
| 25 | `InvalidTimestamp` | A timestamp of 0 was submitted (zero is reserved/invalid). |

## Fee Withdrawal Errors

| Code | Variant | Description |
|------|---------|-------------|
| 55 | `FeeTokenNotSet` | `set_fee_token` has not been called; fee token is unknown. |
| 31 | `InvalidWithdrawalAmount` | `withdraw_fees` called with amount of zero. |
| 32 | `WithdrawalInProgress` | Another withdrawal call is already in-flight (concurrency lock held). |

## Circuit Breaker Errors

| Code | Variant | Description |
|------|---------|-------------|
| 33 | `PairPaused` | Target `asset_pair` has been individually paused via `set_pair_paused`. |
| 34 | `PausedPairIndexFull` | Trying to pause a new pair but `PausedPairIndex` already holds `MAX_PAUSED_PAIRS` entries. |

## Delegation Errors

| Code | Variant | Description |
|------|---------|-------------|
| 38 | `CyclicDelegation` | `set_score_delegate` would create a cycle (wallet → custodian → wallet). |
| 39 | `DelegateNotFound` | `remove_score_delegate` called for a wallet that has no delegate. |

## Cross-Contract Gate Errors

| Code | Variant | Description |
|------|---------|-------------|
| 40 | `HighRiskWallet` | Integrating contract (e.g. AMM) called `query_risk_gate` and received `false`. |

## Decay & Consensus Errors

| Code | Variant | Description |
|------|---------|-------------|
| 41 | `InvalidDecayRate` | `set_decay_rate` called with denominator of 0, or ratio exceeding `MAX_DECAY_LAMBDA`. |
| 49 | `InsufficientConsensus` | Fewer than the configured consensus threshold of models agreed on a score. |
| 50 | `ConsensusInputEmpty` | `submit_consensus_score` called with zero model submissions. |
| 51 | `InvalidConsensusConfig` | `set_consensus_config` called with `k == 0` or `epsilon > 100`. |

## Hysteresis Errors

| Code | Variant | Description |
|------|---------|-------------|
| 48 | `InvalidHysteresisMargin` | `set_hysteresis_margin` called with a value above `MAX_HYSTERESIS_MARGIN` (50). |

## Wallet Relationship Graph Errors

| Code | Variant | Description |
|------|---------|-------------|
| 43 | `CounterpartyLinkFull` | `add_counterparty_link` would exceed the max links per wallet. |
| 44 | `CounterpartyNotFound` | `remove_counterparty_link` called for a non-existent link. |
| 45 | `SelfLink` | `add_counterparty_link` called with the same wallet twice. |

## Arithmetic Errors

| Code | Variant | Description |
|------|---------|-------------|
| 11 | `ArithmeticOverflow` | Weighted aggregate computation in `get_aggregate_score` would overflow. |
