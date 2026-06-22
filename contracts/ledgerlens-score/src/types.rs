use soroban_sdk::{contracttype, Address, BytesN, Symbol};

/// On-chain record of the latest LedgerLens risk assessment for a
/// wallet / asset-pair combination. Written by `submit_score` and
/// read by `get_score`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiskScore {
    /// Overall risk score, 0-100. Higher = more suspicious.
    pub score: u32,
    /// True if the Benford's Law engine flagged this entity.
    pub benford_flag: bool,
    /// True if the ML ensemble classifier flagged this entity.
    pub ml_flag: bool,
    /// Ledger timestamp when this score was computed off-chain.
    pub timestamp: u64,
    /// Model confidence for this score, 0-100.
    pub confidence: u32,
    /// Integer version of the detection-pipeline model that produced
    /// this score.  Allows consumers to detect stale scores when the
    /// pipeline is retrained.
    pub model_version: u32,
}

/// A single entry in a batch score submission.  Mirrors the fields of
/// `submit_score` so the service can write many scores in one call.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreSubmission {
    pub wallet: Address,
    pub asset_pair: Symbol,
    pub score: u32,
    pub benford_flag: bool,
    pub ml_flag: bool,
    pub timestamp: u64,
    pub confidence: u32,
    pub model_version: u32,
}

/// Cross-asset aggregate risk view for a single wallet — a weighted
/// average of every per-pair `RiskScore` the wallet currently has.
/// Returned by `get_aggregate_score`; see that function's rustdoc for the
/// exact formula and complexity bound.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AggregateRiskScore {
    /// Weighted average of all contributing per-pair scores, 0-100.
    pub aggregate_score: u32,
    /// Number of distinct asset pairs the wallet has a score for.
    pub pair_count: u32,
    /// The highest individual per-pair score across all of the wallet's pairs.
    pub max_pair_score: u32,
    /// The asset pair that produced `max_pair_score`.
    pub max_pair: Symbol,
    /// Count of the wallet's pairs with `benford_flag == true`.
    pub benford_flag_count: u32,
    /// Count of the wallet's pairs with `ml_flag == true`.
    pub ml_flag_count: u32,
    /// Ledger timestamp of the most recently updated component score.
    pub last_updated: u64,
    /// True when the aggregate was computed with a non-zero decay rate applied.
    /// Allows callers to detect whether aging has affected the aggregate score.
    pub decay_lambda_applied: bool,
}

/// A cryptographic attestation over a score payload, produced by the
/// off-chain detection pipeline's secp256k1 signing key.
///
/// See `docs/attestation-spec.md` for the exact commitment serialization
/// this is checked against. Passed to `submit_score` only when the admin
/// has configured a service public key via `set_service_pubkey` — see that
/// function's rustdoc for the opt-in enforcement model.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreAttestation {
    /// SHA-256 commitment over the canonical score payload. The contract
    /// always recomputes this independently from the call's actual
    /// arguments and rejects the call if it disagrees with this field — the
    /// field exists so a mismatch surfaces as `InvalidAttestation` instead
    /// of a confusing signature-recovery failure, not as a trusted input.
    pub commitment: BytesN<32>,
    /// 65-byte secp256k1 ECDSA signature over `commitment`: 32-byte `r`,
    /// 32-byte `s`, then a 1-byte recovery id which must be `0` or `1`.
    pub signature: BytesN<65>,
}

/// Pending, time-locked risk score submission.
///
/// Written by `submit_score` when the admin has configured
/// `FinalityBufferSecs > 0`. The score is held in this pending state
/// (invisible to `get_score` / `query_risk_gate`) until
/// `commit_pending_score` observes that `env.ledger().timestamp() >=
/// commit_after`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingScoreEntry {
    /// Overall risk score, 0-100. Higher = more suspicious.
    pub score: u32,
    /// True if the Benford's Law engine flagged this entity.
    pub benford_flag: bool,
    /// True if the ML ensemble classifier flagged this entity.
    pub ml_flag: bool,
    /// Ledger timestamp when the score content was submitted.
    pub submitted_at: u64,
    /// The service's pipeline model confidence, 0-100.
    pub confidence: u32,
    /// Integer model version that produced this score.
    pub model_version: u32,
    /// Ledger timestamp when this payload was produced off-chain.
    /// Stored for consumers/audit trails.
    pub timestamp: u64,
    /// `submitted_at + FinalityBufferSecs`.
    pub commit_after: u64,
    /// Who submitted this score (service address / admin multisig signer in
    /// the off-chain pipeline context).
    pub submitted_by: Address,
}

/// Result for a single entry in a batch score submission.
/// Returned as part of `BatchResult` from `submit_scores_batch` so the
/// caller knows exactly which entries succeeded and why any failed,
/// without needing to re-query each (wallet, pair) individually.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchEntryResult {
    /// Zero-based index of this entry in the submitted batch.
    pub index: u32,
    /// True if the entry was written to storage.
    pub accepted: bool,
    /// Set to the Error code if rejected; 0 if accepted.
    pub rejection_code: u32,
}

/// Structured result from `submit_scores_batch` containing per-entry
/// outcomes so the caller knows exactly which entries succeeded and why
/// any failed, without needing to re-query each (wallet, pair) individually.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchResult {
    /// Number of entries that were successfully written to storage.
    pub accepted_count: u32,
    /// Number of entries that were rejected.
    pub rejected_count: u32,
    /// Per-entry results in the same order as the submitted batch.
    pub results: soroban_sdk::Vec<BatchEntryResult>,
}

/// A pending, time-locked contract WASM upgrade.
///
/// Created by `propose_upgrade` and cleared by `execute_upgrade` /
/// `veto_upgrade`. While one exists, any observer can read it via
/// `get_pending_upgrade` to inspect the committed WASM hash and the earliest
/// time the upgrade can take effect — the basis of the community monitoring
/// window described in the README's Upgrade Governance section.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeProposal {
    /// Hash of the new contract WASM the admin has committed to installing.
    pub new_wasm_hash: BytesN<32>,
    /// Ledger timestamp when the proposal was created.
    pub proposed_at: u64,
    /// Earliest ledger timestamp at which `execute_upgrade` may run
    /// (`proposed_at + upgrade_delay_secs`).
    pub executable_after: u64,
    /// The admin address that created the proposal — recorded for the audit
    /// trail so a veto can attribute the original proposer.
    pub proposed_by: Address,
}

/// Per-(wallet, asset_pair) trend state persisted between submissions.
///
/// `trend` encodes direction as a signed integer: `+1` = rising, `0` = flat,
/// `-1` = falling. `consecutive` counts how many consecutive submissions have
/// had the same non-zero direction; it is `0` on the first submission and
/// resets to `1` on every direction change. Flat submissions (`delta == 0`)
/// set both fields to `0`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreTrend {
    /// +1 = rising, 0 = flat / no history, -1 = falling.
    pub trend: i32,
    /// Number of consecutive submissions in the current trend direction.
    /// 0 on first submission or after a flat submission.
    pub consecutive: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotRecord {
    pub root: BytesN<32>,
    pub leaf_count: u64,
    pub committed_at: u64,      // ledger timestamp
    pub committed_by: Address,  // who called commit_snapshot
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Address allowed to call admin-only functions.
    Admin,
    /// Address of the authorised LedgerLens off-chain scoring service.
    Service,
    /// Latest risk score for a (wallet, asset_pair) pair.
    Score(Address, Symbol),
    /// Boolean flag — true when the contract is paused.
    Paused,
    /// Pending new admin address during a two-step admin transfer.
    PendingAdmin,
    /// Per-wallet watchlist flag (true = high-priority monitoring).
    Watchlist(Address),
    /// Global risk-score threshold; scores ≥ threshold emit a breach event.
    RiskThreshold,
    /// Ordered ring buffer of the last N risk scores for a wallet/pair.
    ScoreHistory(Address, Symbol),
    /// Baked-in contract version number.
    ContractVersion,
    /// Ordered, de-duplicated list of asset pairs a wallet has a score for.
    AssetPairs(Address),
    /// Per-asset-pair weight used by the aggregate risk computation.
    /// Defaults to 1 (simple average) when unset.
    PairWeight(Symbol),
    /// Cached snapshot of the most recently computed aggregate risk score
    /// for a wallet, refreshed as a side effect of `submit_score` /
    /// `submit_scores_batch`. `get_aggregate_score` never reads this cache —
    /// it always recomputes from the live per-pair scores — so this key
    /// exists purely as a cheap snapshot for off-chain indexers.
    AggregateScore(Address),
    /// The single in-flight time-locked upgrade proposal, if any.
    PendingUpgrade,
    /// Admin-configured delay (seconds) between proposing and executing an
    /// upgrade. Defaults to `DEFAULT_UPGRADE_DELAY_SECS` when unset.
    UpgradeDelay,
    /// Ordered set of N addresses authorised to co-sign score submissions.
    ServiceSet,
    /// The M-of-N threshold: minimum number of service-set members that must
    /// sign a `submit_score` call for it to be accepted.
    ServiceThreshold,
    /// Admin-configured staleness window (seconds). Scores older than this
    /// are considered stale by `is_score_stale`. Defaults to
    /// `DEFAULT_STALENESS_WINDOW_SECS` when unset.
    StalenessWindow,
    /// Ledger timestamp of the most recent accepted submission for a
    /// (wallet, asset_pair) pair, used to enforce the submission cooldown.
    LastSubmitTime(Address, Symbol),
    /// Admin-configured cooldown (seconds) enforced between accepted
    /// submissions for the same (wallet, asset_pair). Defaults to
    /// `DEFAULT_COOLDOWN_SECS` when unset.
    CooldownSecs,
    /// Monotonically increasing count of total score submissions for a
    /// (wallet, asset_pair) combination. Unlike `ScoreHistory` (which caps
    /// at `HISTORY_MAX_DEPTH`), this counter is never truncated — it tracks
    /// every submission since the first.
    ScoreCount(Address, Symbol),
    /// The off-chain detection pipeline's secp256k1 public key (33-byte
    /// compressed or 65-byte uncompressed SEC-1 encoding), used to verify
    /// `ScoreAttestation`s. Unset until `set_service_pubkey` is called.
    ServicePubKey,
    /// Admin-configured ring-buffer depth for `ScoreHistory`. Defaults to
    /// `DEFAULT_HISTORY_MAX_DEPTH` when unset; bounded above by
    /// `MAX_HISTORY_DEPTH`.
    HistoryMaxDepth,
    /// Numerator of the fixed-point decay rate λ = numerator / denominator.
    /// Stored separately to support fractional λ values in fixed-point arithmetic.
    /// Defaults to 0 (no decay) when unset.
    DecayRateNumerator,
    /// Denominator of the fixed-point decay rate λ = numerator / denominator.
    /// Defaults to 1 when unset.
    DecayRateDenominator,
    /// The SEP-41 token contract address from which fees are withdrawn.
    /// Unset until `set_fee_token` is called.
    FeeToken,
    /// Boolean flag set for the duration of a `withdraw_fees` call to
    /// prevent concurrent duplicate withdrawals.
    WithdrawalLock,
    /// Per-asset-pair pause flag. True when `set_pair_paused(pair, true)` has
    /// been called and not yet reversed. Hot-path key: looked up on every
    /// submission — never touches `PausedPairIndex`.
    PairPaused(Symbol),
    /// Ordered list of all currently paused asset pairs — an incrementally
    /// maintained index so `get_paused_pairs` is O(1).
    PausedPairIndex,
    /// u64, admin-configurable finality buffer (seconds). Defaults to 0
    /// (disabled).
    FinalityBufferSecs,
    /// Pending score for a (wallet, asset_pair) combination, when
    /// finality buffering is enabled.
    PendingScore(Address, Symbol),
    /// Ordered set of M-of-N admin co-signers.
    AdminSet,
    /// Minimum number of admin-set members that must sign an admin call.
    AdminThreshold,
    /// Score delegation: maps a sub-wallet to its custodian wallet.
    ScoreDelegate(Address),
    /// Per-wallet regulatory hold. Stores `Option<u64>` (expiry timestamp);
    /// `None` means indefinite. While active, read-path functions return
    /// `ScoreEmbargoed` / conservative denials; writes are unaffected.
    ScoreEmbargo(Address),
    /// Per-(wallet, asset_pair) trend state: current trend direction (+1/0/-1)
    /// and consecutive submission count in that direction. Updated by every
    /// successful `submit_score` / `submit_scores_batch` write.
    TrendState(Address, Symbol),
    /// BytesN<32>, current root of the score Merkle accumulator.
    MerkleRoot,
    /// u64, total leaves ever inserted (monotonic).
    MerkleLeafCount,
    /// bool, kill-switch.
    MerkleEnabled,
    /// Vec<SnapshotRecord>, ring buffer of past committed roots.
    SnapshotHistory,
    /// u32, max entries in snapshot ring (admin-configured, default 10).
    SnapshotHistoryDepth,
    /// u64, ledger timestamp of the most recent accepted submission
    /// (`submit_score` / `submit_scores_batch`) or `ping_heartbeat` call.
    /// `0` means the service has never been active. See `is_service_alive`.
    LastServiceActivityAt,
    /// u64, admin-configurable number of seconds of silence before the
    /// off-chain service is considered unresponsive. Defaults to
    /// `DEFAULT_HEARTBEAT_ALERT_THRESHOLD_SECS` (1 hour) when unset.
    ServiceHeartbeatAlertThreshold,
    /// bool, `true` once a `ServiceSilenceAlertEvent` has been emitted for
    /// the current silence window. Cleared (and a `ServiceResumedEvent`
    /// emitted) the next time a submission or `ping_heartbeat` is accepted —
    /// see `submit_score` / `ping_heartbeat`.
    ServiceSilentAlertEmitted,
}
