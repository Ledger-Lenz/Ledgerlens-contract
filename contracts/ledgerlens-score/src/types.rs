use soroban_sdk::{contracttype, Address, Bytes, BytesN, Symbol, Vec};

/// Embargo expiry configuration stored per wallet in temporary storage.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EmbargoExpiry {
    Indefinite,
    Until(u64),
}

/// On-chain record of an open score dispute.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreDispute {
    pub challenger: Address,
    pub bond: i128,
    pub deadline: u64,
    pub challenged_score: u32,
}

/// On-chain record of the latest LedgerLens risk assessment for a
/// wallet / asset-pair combination.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiskScore {
    pub score: u32,
    pub benford_flag: bool,
    pub ml_flag: bool,
    pub timestamp: u64,
    pub confidence: u32,
    pub model_version: u32,
    pub benford_score: u32,
    pub ml_score: u32,
    pub network_score: u32,
    pub commitment: Option<Bytes>,
}

/// Query descriptor for a batch score read.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreQuery {
    pub wallet: Address,
    pub asset_pair: Symbol,
}

/// Optional `RiskScore` wrapper — used in `BatchScoreResult` to avoid
/// `Option<#[contracttype]>` which the Soroban SDK cannot represent in XDR spec.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MaybeRiskScore {
    None,
    Some(RiskScore),
}

impl MaybeRiskScore {
    pub fn unwrap(self) -> RiskScore {
        match self {
            MaybeRiskScore::Some(r) => r,
            MaybeRiskScore::None => panic!("called unwrap on None"),
        }
    }
    pub fn is_none(&self) -> bool {
        matches!(self, MaybeRiskScore::None)
    }
}

/// Per-entry result returned by `get_scores_batch`.
///
/// When `found` is `false`, the `score` field contains zero-valued sentinel
/// data and must not be used. Check `found` before accessing `score`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchScoreResult {
    pub index: u32,
    pub found: bool,
    pub score: MaybeRiskScore,
}

/// Decay-adjusted and delegation-resolved view of a stored risk score.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectiveRiskScore {
    pub original_score: u32,
    pub effective_score: u32,
    pub original_confidence: u32,
    pub confidence_floor: u32,
    pub delegated_to: Option<Address>,
}

/// A single entry in a batch score submission.
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

/// Cross-asset aggregate risk view for a single wallet.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AggregateRiskScore {
    pub aggregate_score: u32,
    pub pair_count: u32,
    pub max_pair_score: u32,
    pub max_pair: Symbol,
    pub benford_flag_count: u32,
    pub ml_flag_count: u32,
    pub last_updated: u64,
    pub decay_lambda_applied: bool,
}

/// A cryptographic attestation over a score payload.
/// Includes per-signer nonce for replay attack prevention.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreAttestation {
    pub commitment: BytesN<32>,
    pub signature: BytesN<65>,
    pub contract_id: BytesN<32>,
    pub contract_version: u32,
    pub nonce: u64,
}

/// Threshold-signature attestation: t-of-n signers produce one 65-byte proof.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThresholdAttestation {
    pub commitment: BytesN<32>,
    pub threshold_sig: BytesN<65>,
    pub participating_signers: soroban_sdk::Vec<Address>,
    pub contract_id: BytesN<32>,
    pub contract_version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MaybeScoreAttestation {
    None,
    Some(ScoreAttestation),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MaybeThresholdAttestation {
    None,
    Some(ThresholdAttestation),
}

/// Unified attestation input for `submit_score`.
/// Wraps both attestation variants so the function stays within
/// Soroban's 10-parameter limit.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreAttestationInput {
    pub attestation: MaybeScoreAttestation,
    pub threshold_attestation: MaybeThresholdAttestation,
    pub commitment: Option<Bytes>,
}

/// Per-model-version aggregate stats, returned by `get_model_version_stats`.
///
/// Canonical definition — includes both the compact form (`submission_count`,
/// `score_sum`) and the summary form (`total_submissions`, `average_score`).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelVersionStats {
    pub model_version: u32,
    pub submission_count: u32,
    pub score_sum: u64,
    pub total_submissions: u64,
    pub average_score: u32,
}

/// Governance status for an off-chain ML model version.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ModelVersionStatus {
    Proposed = 0,
    Active = 1,
    Deprecated = 2,
}

/// Pending, time-locked risk score submission.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingScoreEntry {
    pub score: u32,
    pub benford_flag: bool,
    pub ml_flag: bool,
    pub submitted_at: u64,
    pub confidence: u32,
    pub model_version: u32,
    pub timestamp: u64,
    pub commit_after: u64,
    pub submitted_by: Address,
    pub commitment: Option<Bytes>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HllSketch {
    pub precision: u32,
    pub registers: Vec<u32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelSubmission {
    pub model_version: u32,
    pub model: Address,
    pub score: u32,
    pub confidence: u32,
    pub benford_flag: bool,
    pub ml_flag: bool,
    pub attestation: ScoreAttestation,
}

/// Result for a single entry in a batch score submission.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchEntryResult {
    pub index: u32,
    pub accepted: bool,
    pub rejection_code: u32,
}

/// Structured result from `submit_scores_batch`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchResult {
    pub accepted_count: u32,
    pub rejected_count: u32,
    pub results: Vec<BatchEntryResult>,
}

/// Merkle-root attestation for an entire `submit_scores_batch_attested` call.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchAttestation {
    pub merkle_root: BytesN<32>,
    pub signature: BytesN<65>,
}

/// A single entry in an attested batch score submission.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreSubmissionWithProof {
    pub submission: ScoreSubmission,
    pub proof: Vec<BytesN<32>>,
    pub proof_flags: u32,
}

/// A pending, time-locked contract WASM upgrade.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeProposal {
    pub new_wasm_hash: BytesN<32>,
    pub proposed_at: u64,
    pub executable_after: u64,
    pub proposed_by: Address,
}

/// A pending, time-locked admin parameter change.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParameterProposal {
    pub param_key: Symbol,
    pub new_value: Bytes,
    pub proposer: Address,
    pub proposed_at: u64,
    pub time_lock_secs: u64,
}

/// Lifecycle status of a parameter change proposal.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ParameterProposalStatus {
    Pending = 0,
    Executed = 1,
    Vetoed = 2,
    Expired = 3,
}

/// Stored record combining a proposal with its current status.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParameterProposalRecord {
    pub proposal: ParameterProposal,
    pub status: ParameterProposalStatus,
}

/// Typed value for a simple, single-parameter time-locked change (see
/// `set_pending_param_change`). Distinct from the richer `ParameterProposal`
/// governance flow above.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParamValue {
    U32(u32),
    U64(u64),
}

/// A pending simple parameter change awaiting its time-lock delay.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParamChangeProposal {
    pub new_value: ParamValue,
    pub proposed_at: u64,
    pub apply_after: u64,
}

/// A named group of related risk-gate parameters that must be reviewed and
/// activated together, so the risk threshold and cooldown can never diverge
/// mid-rollout (one applied, the other still pending).
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct PolicyBundle {
    pub risk_threshold: u32,
    pub cooldown_secs: u64,
}

/// A pending policy bundle change awaiting its time-lock delay.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct PolicyBundleProposal {
    pub bundle: PolicyBundle,
    pub proposed_at: u64,
    pub apply_after: u64,
}

/// One entry in the `override_rate_limit` admin audit log.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RateLimitOverrideEntry {
    pub admin: Address,
    pub wallet: Address,
    pub asset_pair: Symbol,
    pub timestamp: u64,
    pub justification_hash: BytesN<32>,
}

/// Fixed warning returned by deletion preflight previews.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeletionAuditWarning {
    Irreversible,
}

/// Read-only preview of what a deletion operation would affect.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeletionPreflight {
    pub wallet: Address,
    pub asset_pair: Symbol,
    pub latest_score_present: bool,
    pub history_count: u32,
    pub audit_warning: DeletionAuditWarning,
}

/// Per-(wallet, asset_pair) trend state persisted between submissions.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreTrend {
    pub trend: i32,
    pub consecutive: u32,
}

/// Configuration and state for the adaptive threshold feature.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdaptiveThresholdConfig {
    /// Whether adaptive threshold mode is enabled.
    pub enabled: bool,
    /// Target percentile to set as threshold (e.g., 90 = top 10% are risky).
    pub target_percentile: u32,
    /// Minimum allowed threshold value.
    pub min_value: u32,
    /// Maximum allowed threshold value.
    pub max_value: u32,
    /// Last computed adaptive threshold value.
    pub last_computed: u32,
}

/// Largest score-jump anomaly observed so far for a (wallet, asset_pair)
/// pair. See `get_jump_stats`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JumpStats {
    pub max_jump: u32,
    pub at_timestamp: u64,
}

/// Global configuration for the per-wallet score submission floor.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreFloorPolicy {
    pub enabled: bool,
    pub high_water_mark: u32,
    pub floor_value: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotRecord {
    pub root: BytesN<32>,
    pub leaf_count: u64,
    pub committed_at: u64,
    pub committed_by: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreVelocityCap {
    pub enabled: bool,
    pub points_per_hour: u32,
}

/// Separate approval policy for irreversible score deletion operations.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeletionApprovalPolicy {
    pub enabled: bool,
    pub approver: Option<Address>,
}

/// One canonical key/value entry in the machine-readable configuration export.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigExportEntry {
    pub key: Symbol,
    pub value: Bytes,
}

/// One pending key/value entry in the machine-readable configuration export.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingConfigExportEntry {
    pub key: Symbol,
    pub value: Bytes,
    pub proposal_id: u64,
    pub proposed_at: u64,
    pub executable_after: u64,
}

/// Deterministic machine-readable export of governance-controlled configuration.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigExportBundle {
    pub schema_version: u32,
    pub active_hash: BytesN<32>,
    pub pending_hash: BytesN<32>,
    pub export_hash: BytesN<32>,
    pub active_values: Vec<ConfigExportEntry>,
    pub pending_values: Vec<PendingConfigExportEntry>,
    pub omitted_secret_rationale: Vec<Bytes>,
}

/// Score histogram returned by `get_score_histogram`.
#[contracttype]
#[derive(Clone)]
pub enum GateDataKey {
    GateCallers,
    GateOpen,
    GateEnforcementMode,
    GateQueryFee,
    AccumulatedFees,
    GateReadLedger(Address, Symbol),
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Service,
    /// Per-signer score range restriction. Maps a service signer address to
    /// its allowed `TierBounds`.
    SignerTier(Address),
    /// Per-signer nonce for multi-sig attestation replay attack prevention.
    /// Maps signer address to the next nonce that will be accepted.
    SignerNonce(Address),
    /// Latest risk score for a (wallet, asset_pair) pair.
    Score(Address, Symbol),
    Paused,
    PendingAdmin,
    Watchlist(Address),
    RiskThreshold,
    JumpThreshold,
    /// Largest score-jump anomaly observed for a (wallet, asset_pair) pair.
    /// See `get_jump_stats`.
    JumpStats(Address, Symbol),
    ScoreHistory(Address, Symbol),
    ContractVersion,
    AssetPairs(Address),
    PairWeight(Symbol),
    AggregateScore(Address),
    PendingUpgrade,
    UpgradeDelay,
    /// Ordered set of N addresses authorised to co-sign score submissions.
    ServiceSet,
    ServiceThreshold,
    StalenessWindow,
    LastSubmitTime(Address, Symbol),
    CooldownSecs,
    ScoreCount(Address, Symbol),
    ServicePubKey,
    HistoryMaxDepth,
    DecayRate,
    GlobalMinConfidence,
    FeeToken,
    WithdrawalLock,
    /// The only address allowed to receive fee withdrawals. Unset until
    /// `set_fee_recipient` is called; `withdraw_fees` requires both admin
    /// quorum and this address's own `require_auth()`.
    FeeRecipient,
    PairPaused(Symbol),
    PausedPairIndex,
    /// Ordered set of wallets currently under an active score embargo,
    /// maintained by `set_score_embargo` / `lift_score_embargo` so
    /// `revoke_all_embargoes` can enumerate and clear them without scanning
    /// the whole wallet space. Capped at `MAX_EMBARGOED_WALLETS`.
    EmbargoedWalletIndex,
    /// Global persistent counter of wallets currently under an active score
    /// embargo. Incremented by `set_score_embargo` (new embargoes only) and
    /// decremented by `lift_score_embargo`, `batch_lift_score_embargo`, and
    /// `revoke_all_embargoes`. Stored in persistent storage so it survives
    /// temporary-storage TTL eviction.
    ActiveEmbargoCount,
    AdminSet,
    AdminThreshold,
    /// Maximum value for adaptive threshold.
    AdaptiveThresholdMaxValue,
    /// Last computed adaptive threshold value.
    LastComputedThreshold,
    Counterparties(Address, Symbol),
    ScoreVelocityCapEnabled,
    ScoreVelocityCapPointsPerHour,
    VelocityCapOverride(Address, Symbol),
    /// Score-floor policy: historical peak (high-water mark) at or above which
    /// the floor applies. Global config, `u32`, defaults to
    /// `DEFAULT_SCORE_FLOOR_HWM` (80) when unset.
    ScoreFloorHighWaterMark,
    ScoreFloorMinValue,
    ScoreFloorEnabled,
    /// Packed (enabled, high_water_mark, floor_value) triple for the score-floor policy.
    ScoreFloorConfig,
    HistoricalMaxScore(Address, Symbol),
    HysteresisMargin,
    RiskBandState(Address, Symbol),
}

#[contracttype]
#[derive(Clone)]
pub enum DataKeyB {
    ScoreEmbargo(Address),
    ConsensusThresholdK,
    ConsensusEpsilon,

    /// Adaptive epsilon enabled flag (issue #204).
    AdaptiveEpsilonEnabled,
    /// Minimum epsilon bound for adaptive mode (issue #204).
    AdaptiveEpsilonMin,
    /// Maximum epsilon bound for adaptive mode (issue #204).
    AdaptiveEpsilonMax,
    /// Variance scale factor for adaptive epsilon mode (issue #287).
    AdaptiveEpsilonScaleFactor,
    /// Open dispute record for a (wallet, asset_pair) pair. Absent key means
    /// no active dispute. Stored in temporary TTL-bounded storage.
    ScoreDispute(Address, Symbol),
    /// Commit-reveal hash for dispute bond: H(bond || salt). Scoped to (challenger, wallet, asset_pair).
    /// Key: DisputeCommit(challenger, wallet, asset_pair) -> BytesN<32> (sha256 hash)
    DisputeCommit(Address, Address, Symbol),
    /// Timestamp when dispute bond commitment was made.
    /// Key: DisputeCommitTime(challenger, wallet, asset_pair) -> u64 (ledger timestamp)
    DisputeCommitTime(Address, Address, Symbol),
    /// Index of all currently open disputes: `Vec<(Address, Symbol)>`.
    /// Incrementally maintained so `get_open_disputes` is a single read.
    DisputeIndex,
    PendingScore(Address, Symbol),
    LastServiceActivityAt,
    FailoverContract,
    AdaptiveRateLimit,
    AggregateServicePubKey,
    AllModelVersions,
    DecayCheckpoint(Address, Symbol),
    DecayCurveConfig,
    DormancyDecayFractionBps,
    DormancyInactivitySecs,
    FinalityDepth,
    InterpolationMethod,
    ModelPosteriorWeight(u32),
    ModelVersionIndex,
    ModelVersionStatus(u32),
    MomentumAlertThreshold,
    MomentumWindow,
    PairScoreCount(Symbol),
    ParameterProposal(u64),
    ParameterProposalNextId,
    PendingParameterProposalIds,
    RevealWindowSecs,
    ScoreBreakdown(Address, Symbol),
    ScoreEntryIndex,
    ScoreEntryLastTouchedLedger(Address, Symbol),
    ScoreHistogram,
    ScoreSubmissionLedger(Address, Symbol),
    SignerAddedAt(Address),
    SignerGracePeriod,
    SignerTtl,
    TotalWalletsScored,
    UniqueWalletsHll(Symbol),
    HllPrecision,
    VerkleCommitment,
    VerkleLeaf(Address, Symbol),
    ModelStats(u32),
    ModelVersionSet,
    ModelVersionDeprecated(u32),
}

#[contracttype]
#[derive(Clone)]
pub enum DataKeyC {
    ModelPosteriorWeight(u32),
    SignerAddedAt(Address),
    SignerRotationTtl,
    SignerRotationGrace,
    ScoreHistogramBucket(u32),
    ScoreHistogramTotal,
    VerkleCommitmentRaw,
    AggregatePubKey,
    OriginalServiceThreshold,
    PairCooldown(Symbol),
    GateCallers,
    GateOpen,
    BandEntryTime(Address, Symbol),
    BreachCount(Address, Symbol),
    EscalationThreshold,
    RevealWindowSecs,
    FinalityBufferSecs,
    ServiceHeartbeatAlertThreshold,
    ServiceSilentAlertEmitted,
    /// Aggregate secp256k1 public key for threshold-signature attestation.
    AggregateServicePubKey,
    /// Window (seconds) for considering a quorum failure as recent.
    QuorumFailureWindow,
    /// Score histogram: 101 buckets (0–100), each storing a submission count.
    ScoreHistogram,
    /// Signer TTL in seconds (0 = never expires).
    SignerTtl,
    /// Grace period in seconds after signer TTL before auth is rejected.
    SignerGracePeriod,
    /// Packed (numerator, denominator) tuple for the exponential decay rate.
    DecayRate,
    /// Ledger timestamp of the most recent accepted score submission globally.
    LastGlobalSubmissionTime,
    ScoreEntryIndex,
    ScoreEntryLastTouchedLedger(Address, Symbol),
    ModelVersionIndex,
    /// Configured decay curve profile for score interpolation.
    DecayCurveConfig,
    /// Per-(wallet, asset_pair) dormancy decay checkpoint timestamp.
    DecayCheckpoint(Address, Symbol),
    /// Dormancy config: seconds of inactivity before decay applies.
    DormancyInactivitySecs,
    /// Dormancy config: fraction of (score - mean) to decay per checkpoint, in basis points.
    DormancyDecayFractionBps,
    /// Number of Stellar ledger closures required before a submitted score is final.
    FinalityDepth,
    /// Ledger sequence at which the current score for (wallet, asset_pair) was last written.
    ScoreSubmissionLedger(Address, Symbol),
    /// Optional sub-score breakdown for (wallet, asset_pair).
    ScoreBreakdown(Address, Symbol),
    /// Running total of score submissions for an asset pair (all wallets combined).
    /// Incremented on every successful submission for `asset_pair`.
    PairScoreCount(Symbol),
    /// Running total of unique (wallet, asset_pair) combinations ever scored.
    /// Incremented on the *first* successful submission for each new combination.
    TotalWalletsScored,
    /// Global configuration for adaptive rate limiting (issue #275).
    AdaptiveRateLimit,
    /// Configurable rolling window (seconds) for score momentum computation (issue #289).
    MomentumWindow,
    /// Alert threshold for momentum — emits `momentum_threshold_crossed` when exceeded (issue #289).
    MomentumAlertThreshold,
    /// Configured interpolation method for `get_interpolated_score` (issue #290).
    InterpolationMethod,
    /// Differential-privacy epsilon (scaled), issue #204 privacy model.
    PrivacyEpsilon,
    /// Commit-reveal hash for consensus model submissions, keyed by
    /// (model, wallet, asset_pair).
    ConsensusCommitment(Address, Address, Symbol),
    /// Rolling hash chain root over admin actions, for tamper-evident audit history.
    AdminAuditRoot,
    ScoreDelegate(Address),
    TrendState(Address, Symbol),
    /// Target percentile for adaptive threshold (e.g., 90 = top 10% are risky).
    AdaptiveThresholdTargetPct,
    /// Minimum value for adaptive threshold.
    AdaptiveThresholdMinValue,
    /// Whether adaptive threshold mode is enabled.
    AdaptiveThresholdEnabled,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKeyD {
    RegisteredOracle(Symbol),
    /// Timestamp (ledger seconds) of the last time the oracle for this asset
    /// pair was consulted and its price was written back by `get_effective_score`.
    /// Used by `is_oracle_stale` to detect feeds that have stopped updating.
    OracleLastUpdated(Symbol),
    /// Admin-configurable maximum age (seconds) for oracle price data before
    /// `get_effective_score` treats the oracle as stale and falls back to
    /// unadjusted confidence.  Defaults to `DEFAULT_ORACLE_STALENESS_THRESHOLD_SECS`.
    OracleStalenessThreshold,
    EpochOpen,
    CurrentEpoch,
    SignerAccuracy(Address),
    SignerRejectionCount(Address),
    WelfordCorrState(Symbol, Symbol),
    PairCorrelation(Symbol, Symbol),
    TokenBucket(Address, Symbol),
    ClusterBoundaries,
    WalletCluster(Address),
    PairVolatilityState(Symbol),
    PairVolatilityWindow,
    FlashProtectionMode,
    DpEpsilon,
    BurstCapacity,
    UpgradeApprovals,
    PendingServicePubKey,
    RateLimitOverrideLog,
    IqrRejectionMultiplier,
    PendingParamChange(Symbol),
    ModelVersionExecutableAfter(u32),
    ModelVersionDescription(u32),
    /// Latest operator acknowledgement record for a given alert class.
    /// Keyed by `AlertType` so each class has its own O(1) slot (issue #630).
    AlertAcknowledgement(AlertType),

    // ── SLO Burn-Rate Alerts (#677) ───────────────────────────────────────────
    /// Global SLO burn-rate alert configuration. Instance-scoped.
    SloBurnRateConfig,
    /// Per-(wallet, asset_pair) SLO window state (short + long accumulators).
    SloWindowState(Address, Symbol),
    /// Per-(wallet, asset_pair) active SLO alert state.
    SloAlertState(Address, Symbol),
    /// Index of all (wallet, asset_pair) pairs with an active (non-None) SLO alert.
    /// `Vec<(Address, Symbol)>` persisted in instance storage.
    SloActiveAlertIndex,
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TierBounds {
    pub min_score: u32,
    pub max_score: u32,
}


/// Histogram of all score submissions across 101 buckets (0–100).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreHistogram {
    pub buckets: Vec<u64>,
    pub total: u64,
}

/// A single model's signed score input for threshold-signature attestation.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelSubmissionWithSig {
    pub model_address: Address,
    pub score: u32,
    pub signature: BytesN<64>,
}

/// Snapshot / Verkle-tree leaf for a (wallet, asset_pair) entry.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerkleLeaf {
    pub score: u32,
    pub timestamp: u64,
    pub model_version: u32,
}

/// A single step entry for the `StepWise` decay curve.
/// When elapsed seconds since the score was recorded reaches `time_threshold_secs`,
/// the score is set to `score_value`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StepWiseEntry {
    pub time_threshold_secs: u64,
    pub score_value: u32,
}

/// Selectable decay curve applied in `get_interpolated_score` and `get_effective_score`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecayCurve {
    /// Linear interpolation between history points (existing default behaviour).
    Exponential,
    /// Quadratic easing: slow initial change, fast later (f² weighting).
    Quadratic,
    /// Logarithmic easing: fast initial drop, then levels off.
    Logarithmic,
    /// Discrete tier drops at configurable time thresholds.
    StepWise(Vec<StepWiseEntry>),
}

/// Optional sub-score breakdown submitted alongside a composite score.
/// Off-chain models populate whichever dimensions they compute.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubscorePayload {
    pub benford_score: Option<u32>,
    pub ml_score: Option<u32>,
    pub network_score: Option<u32>,
}

/// A risk score paired with its ledger-finality status.
/// Returned by `get_score_with_finality`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreWithFinality {
    pub score: RiskScore,
    /// `true` when the configured `finality_depth` ledgers have not yet
    /// elapsed since the score was submitted — consumers should treat the
    /// score as provisional.
    pub finality_pending: bool,
}

/// Configurable score decay profile.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FlashProtectionMode {
    Warn,
    Reject,
}

/// Signer accuracy record: tracks MAD (mean absolute deviation) scaled by 1000
/// and the total number of consensus submissions by this signer.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignerAccuracyRecord {
    pub mad_scaled: u32,
    pub count: u32,
}

/// Running state for Welford online variance on per-pair scores.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PairVolatilityState {
    pub count: i64,
    pub mean_scaled: i64,
    pub m2_scaled: i64,
    pub last_updated: u64,
}

/// Configurable score decay profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecayProfile {
    Linear(u32, u32),
    Exponential(u64),
    Step(Vec<(u64, u32)>),
}

/// Configuration for adaptive rate limiting based on score variance (issue #275).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdaptiveRateLimit {
    pub enabled: bool,
    pub variance_scale: u32,
}

/// Interpolation method for `get_interpolated_score` (issue #290).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InterpolationMethod {
    Linear,
    CubicSpline,
}

/// Incremental Welford state for online Pearson correlation tracking (issue #268).
/// Stores accumulated sums for computing r(pair_a, pair_b) on the fly.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WelfordCorrState {
    pub n: u32,
    pub sum_a: i64,
    pub sum_b: i64,
    pub sum_aa: i64,
    pub sum_bb: i64,
    pub sum_ab: i64,
}

/// Per-(wallet, asset_pair) token-bucket state for burst rate limiting (issue #269).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenBucket {
    pub tokens: u32,
    pub last_refill: u64,
}

// ── SLO Burn-Rate Alerts (#677) ───────────────────────────────────────────────

/// Severity level for an SLO burn-rate alert.
///
/// Severity is determined deterministically from the computed burn-rate ratio
/// against the short window (5 min) and long window (60 min) simultaneously:
///
/// | Level | Condition (both windows must fire) |
/// |-------|-------------------------------------|
/// | P3    | long_burn >= 1× AND short_burn >= 1× |
/// | P2    | long_burn >= 2× AND short_burn >= 2× |
/// | P1    | long_burn >= 5× AND short_burn >= 5× |
///
/// `None` means no active alert (all burn rates below P3 threshold).
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum SloSeverity {
    /// No active SLO violation.
    None = 0,
    /// Warn: burn rate ≥ 1×; budget will be exhausted within the window period.
    P3 = 1,
    /// Critical: burn rate ≥ 2×; budget exhausted in half the window period.
    P2 = 2,
    /// Page: burn rate ≥ 5×; budget exhausted in 20% of the window period.
    P1 = 3,
}

/// Measurement window descriptor for SLO burn-rate computation.
///
/// The burn rate for a window is:
/// ```text
/// burn_rate = seconds_above_threshold / window_secs
/// ```
/// Stored as a sliding counter: on each score write we add
/// `min(elapsed, window_secs)` seconds to the "above-threshold" accumulator
/// when the current score ≥ `slo_threshold`, then evict contributions older
/// than `window_secs`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SloWindow {
    /// Duration of this measurement window in seconds.
    pub window_secs: u64,
    /// Accumulated seconds where the score was ≥ `slo_threshold` within
    /// this window, scaled by `SLO_BURN_SCALE` (1 000 000) to avoid
    /// fixed-point division on-chain.
    pub above_threshold_secs_scaled: u64,
    /// Ledger timestamp of the most recent update.
    pub last_updated: u64,
}

/// Global configuration for SLO burn-rate alerts, set by the admin.
///
/// Two windows are always tracked:
/// - **short**: `short_window_secs` (default 300 s / 5 min)
/// - **long**: `long_window_secs` (default 3 600 s / 60 min)
///
/// An alert fires when *both* windows independently compute a burn rate
/// above the threshold for the severity tier. This dual-window approach
/// avoids false positives from momentary spikes (short window alone) or
/// slow-creeping degradation that never breaches the short window.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SloBurnRateConfig {
    /// Whether SLO burn-rate alerts are enabled globally.
    pub enabled: bool,
    /// Score value at or above which a score is considered an SLO violation.
    /// Must be in `[1, 100]`.
    pub slo_threshold: u32,
    /// Short measurement window in seconds. Default: 300 (5 min).
    pub short_window_secs: u64,
    /// Long measurement window in seconds. Default: 3 600 (60 min).
    pub long_window_secs: u64,
    /// Burn-rate multiplier (×1 000) at which P3 fires.
    /// Default: 1 000 (= 1×). Must be ≥ 1 000.
    pub p3_burn_rate_threshold_milli: u32,
    /// Burn-rate multiplier (×1 000) at which P2 fires.
    /// Default: 2 000 (= 2×). Must be > `p3_burn_rate_threshold_milli`.
    pub p2_burn_rate_threshold_milli: u32,
    /// Burn-rate multiplier (×1 000) at which P1 fires.
    /// Default: 5 000 (= 5×). Must be > `p2_burn_rate_threshold_milli`.
    pub p1_burn_rate_threshold_milli: u32,
}

/// Active SLO burn-rate alert state for a `(wallet, asset_pair)` pair.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SloAlert {
    /// Current severity level.
    pub severity: SloSeverity,
    /// Ledger timestamp when this severity was first reached.
    pub triggered_at: u64,
    /// Ledger timestamp of the most recent escalation or de-escalation.
    pub last_changed_at: u64,
    /// Whether this alert has been acknowledged by an operator.
    pub acknowledged: bool,
    /// Ledger timestamp of acknowledgment (0 if not acknowledged).
    pub acknowledged_at: u64,
    /// Short-window burn rate at time of last change, scaled ×1 000.
    pub short_burn_rate_milli: u32,
    /// Long-window burn rate at time of last change, scaled ×1 000.
    pub long_burn_rate_milli: u32,
}

/// Per-(wallet, asset_pair) SLO window counters persisted on-chain.
/// Contains both the short and long window states.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SloWindowState {
    /// Short measurement window state.
    pub short: SloWindow,
    /// Long measurement window state.
    pub long: SloWindow,
}
