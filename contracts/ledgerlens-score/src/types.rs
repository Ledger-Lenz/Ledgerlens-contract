use soroban_sdk::{contracttype, Address, Bytes, BytesN, Env, Symbol, Vec};

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
    pub fn is_none(&self) -> bool { matches!(self, MaybeRiskScore::None) }
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
    pub precision: u8,
    pub registers: Vec<u8>,
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

/// Per-(wallet, asset_pair) trend state persisted between submissions.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreTrend {
    pub trend: i32,
    pub consecutive: u32,
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
    ScoreDelegate(Address),
    TrendState(Address, Symbol),
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
    ScoreEmbargo(Address),
    ConsensusThresholdK,
    ConsensusEpsilon,
    /// Adaptive epsilon enabled flag (issue #204).
    AdaptiveEpsilonEnabled,
    /// Minimum epsilon bound for adaptive mode (issue #204).
    AdaptiveEpsilonMin,
    /// Maximum epsilon bound for adaptive mode (issue #204).
    AdaptiveEpsilonMax,
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
}

/// Extended storage keys for less-frequently-accessed features.
#[contracttype]
#[derive(Clone)]
pub enum ExtDataKey {
    AllModelVersions,
    ModelStats(u32),
    ModelVersionSet,
    ModelVersionDeprecated(u32),
    ModelPosteriorWeight(u32),
    SignerAddedAt(Address),
    SignerRotationTtl,
    SignerRotationGrace,
    ScoreHistogramBucket(u32),
    ScoreHistogramTotal,
    VerkleLeaf(Address, Symbol),
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
    /// Per-asset-pair cooldown override in seconds.
    PairCooldown(Symbol),
    /// Aggregate secp256k1 public key for threshold-signature attestation.
    AggregateServicePubKey,
    /// Per-model-version score statistics (submission count and score sum).
    ModelStats(u32),
    /// Ordered set of all model versions that have been submitted at least once.
    AllModelVersions,
    /// Escalation threshold for consecutive breach detection.
    EscalationThreshold,
    /// Per-(wallet, asset_pair) consecutive breach counter.
    BreachCount(Address, Symbol),
    /// Window (seconds) for considering a quorum failure as recent.
    QuorumFailureWindow,
    /// Original service threshold saved before a quorum-reduction event.
    OriginalServiceThreshold,
    /// Per-model-version Bayesian posterior weight (u64, scaled).
    ModelPosteriorWeight(u32),
    /// Score histogram: 101 buckets (0–100), each storing a submission count.
    ScoreHistogram,
    /// Signer TTL in seconds (0 = never expires).
    SignerTtl,
    /// Grace period in seconds after signer TTL before auth is rejected.
    SignerGracePeriod,
    /// Ledger timestamp when a wallet first entered the high-risk band for an asset pair.
    BandEntryTime(Address, Symbol),
    /// Raw Verkle commitment bytes for the snapshot Merkle/Verkle tree.
    VerkleCommitment,
    /// Per-(wallet, asset_pair) Verkle tree leaf hash ([u8; 32] stored as Bytes).
    VerkleLeaf(Address, Symbol),
    /// Ledger timestamp when a signer was added to the service set.
    SignerAddedAt(Address),
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
}

impl DataKey {
    fn as_val(&self, e: &Env) -> soroban_sdk::Val {
        use soroban_sdk::IntoVal as _;
        macro_rules! k0 {
            ($s:expr) => {{
                (soroban_sdk::Symbol::new(e, $s),).into_val(e)
            }};
        }
        macro_rules! k1 {
            ($s:expr, $a:expr) => {{
                (soroban_sdk::Symbol::new(e, $s), $a.clone()).into_val(e)
            }};
        }
        macro_rules! k2 {
            ($s:expr, $a:expr, $b:expr) => {{
                (soroban_sdk::Symbol::new(e, $s), $a.clone(), $b.clone()).into_val(e)
            }};
        }
        macro_rules! k3 {
            ($s:expr, $a:expr, $b:expr, $c:expr) => {{
                (soroban_sdk::Symbol::new(e, $s), $a.clone(), $b.clone(), $c.clone()).into_val(e)
            }};
        }
        match self {
            DataKey::ModelVersionExecutableAfter(v) => k1!("MvExecAfter", v),
            DataKey::ModelVersionDescription(v) => k1!("MvDesc", v),
            DataKey::Admin => k0!("Admin"),
            DataKey::Service => k0!("Service"),
            DataKey::SignerTier(a) => k1!("SignerTier", a),
            DataKey::SignerNonce(a) => k1!("SignerNonce", a),
            DataKey::Score(a, s) => k2!("Score", a, s),
            DataKey::Paused => k0!("Paused"),
            DataKey::PendingAdmin => k0!("PendingAdmin"),
            DataKey::Watchlist(a) => k1!("Watchlist", a),
            DataKey::RiskThreshold => k0!("RiskThreshold"),
            DataKey::JumpThreshold => k0!("JumpThreshold"),
            DataKey::ScoreHistory(a, s) => k2!("ScoreHistory", a, s),
            DataKey::ContractVersion => k0!("ContractVersion"),
            DataKey::AssetPairs(a) => k1!("AssetPairs", a),
            DataKey::PairWeight(s) => k1!("PairWeight", s),
            DataKey::AggregateScore(a) => k1!("AggregateScore", a),
            DataKey::PendingUpgrade => k0!("PendingUpgrade"),
            DataKey::UpgradeDelay => k0!("UpgradeDelay"),
            DataKey::ServiceSet => k0!("ServiceSet"),
            DataKey::ServiceThreshold => k0!("ServiceThreshold"),
            DataKey::StalenessWindow => k0!("StalenessWindow"),
            DataKey::LastSubmitTime(a, s) => k2!("LastSubmitTime", a, s),
            DataKey::CooldownSecs => k0!("CooldownSecs"),
            DataKey::ScoreCount(a, s) => k2!("ScoreCount", a, s),
            DataKey::ServicePubKey => k0!("ServicePubKey"),
            DataKey::HistoryMaxDepth => k0!("HistoryMaxDepth"),
            DataKey::DecayRateNumerator => k0!("DecayRateNumer"),
            DataKey::DecayRateDenominator => k0!("DecayRateDenom"),
            DataKey::GlobalMinConfidence => k0!("GlobalMinConf"),
            DataKey::FeeToken => k0!("FeeToken"),
            DataKey::WithdrawalLock => k0!("WithdrawalLock"),
            DataKey::PairPaused(s) => k1!("PairPaused", s),
            DataKey::PausedPairIndex => k0!("PausedPairIdx"),
            DataKey::AdminSet => k0!("AdminSet"),
            DataKey::AdminThreshold => k0!("AdminThreshold"),
            DataKey::ScoreDelegate(a) => k1!("ScoreDelegate", a),
            DataKey::TrendState(a, s) => k2!("TrendState", a, s),
            DataKey::Counterparties(a, s) => k2!("Counterparties", a, s),
            DataKey::ScoreVelocityCapEnabled => k0!("VelCapEnabled"),
            DataKey::ScoreVelocityCapPointsPerHour => k0!("VelCapPPH"),
            DataKey::VelocityCapOverride(a, s) => k2!("VelCapOverride", a, s),
            DataKey::ScoreFloorHighWaterMark => k0!("FloorHWM"),
            DataKey::ScoreFloorMinValue => k0!("FloorMinVal"),
            DataKey::ScoreFloorEnabled => k0!("FloorEnabled"),
            DataKey::ScoreFloorConfig => k0!("FloorConfig"),
            DataKey::HistoricalMaxScore(a, s) => k2!("HistMaxScore", a, s),
            DataKey::HysteresisMargin => k0!("HysteresisM"),
            DataKey::RiskBandState(a, s) => k2!("RiskBandState", a, s),
            DataKey::ScoreEmbargo(a) => k1!("ScoreEmbargo", a),
            DataKey::ConsensusThresholdK => k0!("ConsThresholdK"),
            DataKey::ConsensusEpsilon => k0!("ConsEpsilon"),
            DataKey::AdaptiveEpsilonEnabled => k0!("AdaptEpsEn"),
            DataKey::AdaptiveEpsilonMin => k0!("AdaptEpsMin"),
            DataKey::AdaptiveEpsilonMax => k0!("AdaptEpsMax"),
            DataKey::ScoreDispute(a, s) => k2!("ScoreDispute", a, s),
            DataKey::DisputeCommit(c, w, s) => k3!("DisputeCommit", c, w, s),
            DataKey::DisputeCommitTime(c, w, s) => k3!("DisputeCommitTime", c, w, s),
            DataKey::DisputeIndex => k0!("DisputeIndex"),
            DataKey::ConsensusCommitment(m, w, s) => k3!("ConsCommit", m, w, s),
            DataKey::RevealWindowSecs => k0!("RevealWinSecs"),
            DataKey::FinalityBufferSecs => k0!("FinalityBufSec"),
            DataKey::PendingScore(a, s) => k2!("PendingScore", a, s),
            DataKey::UniqueWalletsHll(s) => k1!("UniqueWalletsHll", s),
            DataKey::LastServiceActivityAt => k0!("LastSvcActivity"),
            DataKey::ServiceHeartbeatAlertThreshold => k0!("SvcHbAlert"),
            DataKey::ServiceSilentAlertEmitted => k0!("SvcSilentAlert"),
            DataKey::PairCooldown(s) => k1!("PairCooldown", s),
            DataKey::AggregateServicePubKey => k0!("AggSvcPubKey"),
            DataKey::ModelStats(v) => k1!("ModelStats", v),
            DataKey::AllModelVersions => k0!("AllModelVers"),
            DataKey::EscalationThreshold => k0!("EscalThresh"),
            DataKey::BreachCount(a, s) => k2!("BreachCount", a, s),
            DataKey::QuorumFailureWindow => k0!("QuorumFailWin"),
            DataKey::OriginalServiceThreshold => k0!("OrigSvcThresh"),
            DataKey::ModelPosteriorWeight(v) => k1!("ModelPostWt", v),
            DataKey::ScoreHistogram => k0!("ScoreHistogram"),
            DataKey::SignerTtl => k0!("SignerTtl"),
            DataKey::SignerGracePeriod => k0!("SignerGrace"),
            DataKey::BandEntryTime(a, s) => k2!("BandEntryTime", a, s),
            DataKey::VerkleCommitment => k0!("VerkleCommit"),
            DataKey::VerkleLeaf(a, s) => k2!("VerkleLeaf", a, s),
            DataKey::SignerAddedAt(a) => k1!("SignerAddedAt", a),
            DataKey::ModelVersionStatus(v) => k1!("MvStatus", v),
            DataKey::DecayRate => k0!("DecayRate"),
            DataKey::LastGlobalSubmissionTime => k0!("LastGlobalSub"),
            DataKey::ModelVersionIndex => k0!("MvIndex"),
            DataKey::ScoreEntryIndex => k0!("ScoreEntryIndex"),
            DataKey::ScoreEntryLastTouchedLedger(w, s) => k2!("ScoreEntryLTL", w, s),
            DataKey::JumpStats(w, s) => k2!("JumpStats", w, s),
            DataKey::FeeRecipient => k0!("FeeRecipient"),
            DataKey::EmbargoedWalletIndex => k0!("EmbargoedWIndex"),
            DataKey::DecayCurveConfig => k0!("DecayCurveConf"),
            DataKey::DecayCheckpoint(a, s) => k2!("DecayChkpt", a, s),
            DataKey::DormancyInactivitySecs => k0!("DrmInactSecs"),
            DataKey::DormancyDecayFractionBps => k0!("DrmFracBps"),
            DataKey::FinalityDepth => k0!("FinalityDepth"),
            DataKey::ScoreSubmissionLedger(a, s) => k2!("SubLedger", a, s),
            DataKey::ScoreBreakdown(a, s) => k2!("ScoreBreak", a, s),
            DataKey::PairScoreCount(s) => k1!("PairScoreCnt", s),
            DataKey::TotalWalletsScored => k0!("TotalWalletsScored"),
            DataKey::AdaptiveRateLimit => k0!("AdaptiveRateLimit"),
            DataKey::MomentumWindow => k0!("MomentumWindow"),
            DataKey::MomentumAlertThreshold => k0!("MomentumAlertThr"),
            DataKey::InterpolationMethod => k0!("InterpMethod"),
        }
    }
}

impl soroban_sdk::IntoVal<Env, soroban_sdk::Val> for DataKey {
    fn into_val(&self, e: &Env) -> soroban_sdk::Val {
        self.as_val(e)
    }
}

impl<'a> soroban_sdk::IntoVal<Env, soroban_sdk::Val> for &'a DataKey {
    fn into_val(&self, e: &Env) -> soroban_sdk::Val {
        self.as_val(e)
    }
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
