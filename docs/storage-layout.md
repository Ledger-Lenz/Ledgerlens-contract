# Storage Layout and Rent Mechanics

This document provides a comprehensive specification of the on-chain storage structure, keys, storage tiers, Time-to-Live (TTL) settings, and rent mechanics utilized in the LedgerLens smart contract.

---

## Soroban Storage Tiers & Rent Mechanics

The Stellar Soroban platform implements a state-archiving rent model. Smart contracts and their associated data consume ledger space, which incurs rent fees. Rent is determined by the storage footprint size (in bytes) and the duration (in ledgers) the entry resides on the network.

To manage rent efficiently, Soroban offers three distinct storage tiers, each with unique pricing, lifecycle properties, and restoration paths:

1. **Instance Storage**
   - **Characteristics**: Shared storage bound directly to the contract instance. Contains contract configurations, administration state, and global constants. 
   - **Behavior**: Loaded automatically whenever any function in the contract is called. Accessing instance storage has a higher initial gas charge but requires no separate key lookup gas or footprint declaration.
   - **TTL Lifecycle**: Shared directly with the contract instance itself. It does not have an independent `extend_ttl` invocation in standard storage helper paths; its lifetime is extended automatically whenever the contract is executed or upgraded.

2. **Persistent Storage**
   - **Characteristics**: Stored under individual keys separate from the contract instance. Best suited for user data, transaction records, and variables that must remain intact across upgrades.
   - **Behavior**: Loaded on demand. Accessing a persistent key requires declaring it in the transaction footprint.
   - **TTL Lifecycle**: If the TTL (Time-To-Live) of a persistent entry expires, the entry is **archived** (moved to cold storage). Archived data can be restored by submitting a restoration transaction and paying a recovery fee.

3. **Temporary Storage**
   - **Characteristics**: The least expensive tier, designed for ephemeral state such as rate limits, cooldowns, or time-locked flags.
   - **Behavior**: Loaded on demand. Requires footprint declaration.
   - **TTL Lifecycle**: Once a temporary entry's TTL expires, it is **permanently deleted** and cannot be restored. To re-establish the state, the key must be written anew.

---

## TTL Extension Triggers (Read vs. Write Paths)

LedgerLens dynamically extends the TTL of active keys to prevent unexpected archiving or expiration. However, how and when these extensions are triggered is critical to gas consumption:

### Write Paths
When writing or updating a key (e.g., `set_score`), the contract always calls `extend_ttl(&key, threshold, extend_to)` immediately following the write. This resets the entry's lifetime on the ledger to the target maximum (`extend_to`) if its remaining lifetime falls below `threshold`.

### Read Paths
When reading a key via standard getters (e.g., `get_score`), the contract checks if the key exists. If it is present, it calls `extend_ttl` to refresh the entry's lifetime. This keeps active entries alive indefinitely as long as they are regularly accessed.

### "Peek" / Read-only Paths (Side-effect-free Queries)
Soroban treats TTL extension as a **state-mutating write operation**. Consequently, any transaction invoking a function that extends a TTL cannot be run in a read-only context, and triggers state fees.

To allow gas-free and infallible integrations from external smart contracts (e.g., AMM guards calling `query_risk_gate`), LedgerLens provides separate **peek** read paths (e.g., `peek_score`, `peek_risk_band_state`, `peek_is_embargoed`). These functions perform direct reads without calling `extend_ttl`. 
> [!NOTE]
> Integrating contracts should always call the `peek` versions or view methods that do not trigger TTL extensions to avoid adding unnecessary gas overhead and write footprint requirements to their query paths.

---

## Storage Layout Specifications

The following tables specify every key stored by LedgerLens, mapped to its storage tier, TTL parameters, and purpose.

### Instance Storage Keys
*Instance storage keys do not have independent TTL properties; they inherit the contract instance's TTL.*

| Key Name | Storage Tier | TTL Threshold | TTL Extend-To | Description | Cross-Reference |
| :--- | :--- | :---: | :---: | :--- | :--- |
| `Admin` | Instance | N/A | N/A | The contract administrator address. | - |
| `AdminSet` | Instance | N/A | N/A | The list of multi-sig admin co-signers. | [`MAX_ADMIN_SIGNERS`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L92) |
| `AdminThreshold` | Instance | N/A | N/A | The required number of co-signatures for administrative commands. | - |
| `Service` | Instance | N/A | N/A | The address of the primary off-chain scoring service (single-signer path). | - |
| `ServiceSet` | Instance | N/A | N/A | The set of addresses authorized to co-sign score submissions. | [`MAX_SERVICE_SIGNERS`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L89) |
| `ServiceThreshold` | Instance | N/A | N/A | The required signature count for M-of-N consensus. | - |
| `ServicePubKey` | Instance | N/A | N/A | The off-chain pipeline's secp256k1 public key used to verify ECDSA signatures. | - |
| `SignerTier(Address)` | Instance | N/A | N/A | The authorized score range limits (`TierBounds`) for service signers. Defaults to `[0, 100]` if unset. | - |
| `Paused` | Instance | N/A | N/A | Global boolean pause switch. | - |
| `PendingAdmin` | Instance | N/A | N/A | Pending new admin address during administrative handovers. | - |
| `RiskThreshold` | Instance | N/A | N/A | Global threshold above which scores trigger breach events. | [`DEFAULT_RISK_THRESHOLD`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L16) |
| `JumpThreshold` | Instance | N/A | N/A | Absolute delta limit between consecutive scores that triggers anomaly events. | `DEFAULT_JUMP_THRESHOLD` |
| `HistoryMaxDepth` | Instance | N/A | N/A | Depth of the `ScoreHistory` ring buffer. | [`DEFAULT_HISTORY_MAX_DEPTH`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L10), [`MAX_HISTORY_DEPTH`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L7) |
| `ContractVersion` | Instance | N/A | N/A | Semantic contract version. | [`CONTRACT_VERSION`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L29) |
| `PendingUpgrade` | Instance | N/A | N/A | Current locked-in `UpgradeProposal` for contract WASM upgrades. | - |
| `UpgradeDelay` | Instance | N/A | N/A | Delay in seconds between proposal and execution of WASM upgrades. | [`DEFAULT_UPGRADE_DELAY_SECS`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L86) |
| `StalenessWindow` | Instance | N/A | N/A | Maximum age in seconds before a score is considered stale. | [`DEFAULT_STALENESS_WINDOW_SECS`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L95) |
| `CooldownSecs` | Instance | N/A | N/A | Rate limit cooldown delay between submissions for the same key. | [`DEFAULT_COOLDOWN_SECS`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L57) |
| `DecayRateNumerator` | Instance | N/A | N/A | Fixed-point exponential decay numerator λ. Defaults to 0. | [`DEFAULT_DECAY_LAMBDA_NUM`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L113) |
| `DecayRateDenominator` | Instance | N/A | N/A | Fixed-point exponential decay denominator λ. Defaults to 1. | [`DEFAULT_DECAY_LAMBDA_DEN`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L116) |
| `GlobalMinConfidence` | Instance | N/A | N/A | System-wide minimum confidence floor. Defaults to 0. | - |
| `FeeToken` | Instance | N/A | N/A | SEP-41 token contract address from which fees are drawn. | - |
| `WithdrawalLock` | Instance | N/A | N/A | Reentrancy guard for withdrawal routines. | - |
| `ScoreFloorHighWaterMark` | Instance | N/A | N/A | Peak score threshold for reputation laundering protection. | [`DEFAULT_SCORE_FLOOR_HWM`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L143) |
| `ScoreFloorMinValue` | Instance | N/A | N/A | Minimum score allowed once the floor applies. | [`DEFAULT_SCORE_FLOOR_MIN`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L147) |
| `ScoreFloorEnabled` | Instance | N/A | N/A | Global activation state of the score floor policy. Defaults to false. | - |
| `HysteresisMargin` | Instance | N/A | N/A | Margin used to widen the exit threshold below the risk threshold. Defaults to 0. | [`MAX_HYSTERESIS_MARGIN`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L167) |
| `ConsensusThresholdK` | Instance | N/A | N/A | Minimum model submissions that must agree for consensus. | [`DEFAULT_CONSENSUS_THRESHOLD_K`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L194) |
| `ConsensusEpsilon` | Instance | N/A | N/A | Maximum absolute deviation from the provisional median allowed. | [`DEFAULT_CONSENSUS_EPSILON`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L197) |
| `GateCallers` | Instance | N/A | N/A | List of contract addresses authorized to call gate functions. | - |

---

### Persistent Storage Keys
*All persistent keys in LedgerLens share the same TTL bounds.*
* **TTL Threshold**: `SCORE_TTL_THRESHOLD` (~30 days / 518,400 ledgers)
* **TTL Extend-To**: `SCORE_TTL_EXTEND_TO` (~45 days / 777,600 ledgers)

| Key Name | Storage Tier | TTL Threshold | TTL Extend-To | Description | Cross-Reference |
| :--- | :--- | :---: | :---: | :--- | :--- |
| `Score(Address, Symbol)` | Persistent | 518,400 | 777,600 | Holds the latest `RiskScore` struct for a (wallet, asset_pair). | [`SCORE_TTL_THRESHOLD`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L2), [`SCORE_TTL_EXTEND_TO`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L3) |
| `Watchlist(Address)` | Persistent | 518,400 | 777,600 | Watchlist monitoring flag (`bool`). Removed when unset. | - |
| `ScoreHistory(Address, Symbol)` | Persistent | 518,400 | 777,600 | Ring buffer (`Vec<RiskScore>`) of historical scores. | - |
| `AssetPairs(Address)` | Persistent | 518,400 | 777,600 | List of asset pairs (`Vec<Symbol>`) a wallet is tracked on. | [`MAX_WALLET_PAIRS`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L45) |
| `PairWeight(Symbol)` | Persistent | 518,400 | 777,600 | Weighted multiplier used in weighted average aggregates. | - |
| `AggregateScore(Address)` | Persistent | 518,400 | 777,600 | Cached snapshot of `AggregateRiskScore` (write-through only). | - |
| `LastSubmitTime(Address, Symbol)` | Persistent | 518,400 | 777,600 | Timestamp of the last accepted submission (cooldown logic). | - |
| `ScoreCount(Address, Symbol)` | Persistent | 518,400 | 777,600 | Cumulative counter of all historical submissions. | - |
| `PairPaused(Symbol)` | Persistent | 518,400 | 777,600 | Boolean paused flag per asset pair. Removed when unpaused. | - |
| `PausedPairIndex` | Persistent | 518,400 | 777,600 | Incrementally maintained list (`Vec<Symbol>`) of paused pairs. | [`MAX_PAUSED_PAIRS`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L103) |
| `ScoreDelegate(Address)` | Persistent | 518,400 | 777,600 | Maps a sub-wallet to its custodian wallet. | - |
| `TrendState(Address, Symbol)` | Persistent | 518,400 | 777,600 | Risk trend metadata (`ScoreTrend`). | - |
| `Counterparties(Address, Symbol)` | Persistent | 518,400 | 777,600 | Bidirectional links list (`Vec<Address>`) for wallet graphing. | [`MAX_COUNTERPARTY_LINKS_PER_WALLET`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L130) |
| `HistoricalMaxScore(Address, Symbol)` | Persistent | 518,400 | 777,600 | Running historical peak score. Used for floor checks. | - |

---

### Temporary Storage Keys
*Temporary keys expire and are deleted automatically once their TTL has elapsed.*

| Key Name | Storage Tier | TTL Threshold | TTL Extend-To | Description | Cross-Reference |
| :--- | :--- | :---: | :---: | :--- | :--- |
| `RiskBandState(Address, Symbol)` | Temporary | 518,400 | 777,600 | Records if a wallet is in the high-risk band for an asset pair. | [`BAND_STATE_TTL_THRESHOLD`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L171), [`BAND_STATE_TTL_EXTEND_TO`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L175) |
| `ScoreEmbargo(Address)` | Temporary | 1,555,200 | 3,110,400 | Regulatory embargo details (`EmbargoExpiry`). Expire threshold is ~90 days; target extend-to is ~180 days. | [`EMBARGO_TTL_THRESHOLD`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L186), [`EMBARGO_TTL_EXTEND_TO`](file:///c:/Users/HP/Desktop/opensource/Ledgerlens-contract/contracts/ledgerlens-score/src/constants.rs#L189) |
