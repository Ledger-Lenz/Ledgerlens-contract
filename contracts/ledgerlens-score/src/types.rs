use soroban_sdk::{contracttype, Address, BytesN, Symbol};

/// Embargo expiry configuration stored per wallet in temporary storage.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EmbargoExpiry {
    Indefinite,
    Until(u64),
}

/// On-chain record of an open score dispute, tracking the challenger's staked
/// bond, the deadline by which the admin must resubmit a corrected score, and
/// the score that was being challenged when the dispute was opened.
///
/// Stored in temporary TTL-bounded storage keyed by `(wallet, asset_pair)`;
/// removed once the dispute is resolved by admin correction or by timeout.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreDispute {
    /// Wallet operator that opened the dispute and staked the bond.
    pub challenger: Address,
    /// Fee-token amount staked to open the dispute (escrowed in the contract).
    pub bond: i128,
    /// Ledger timestamp after which the dispute may be settled by timeout.
    pub deadline: u64,
    /// The disputed score at the time the dispute was opened, recorded for
    /// audit purposes.
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
}

/// Query descriptor for a batch score read.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreQuery {
    pub wallet: Address,
    pub asset_pair: Symbol,
}

/// Per-entry result returned by `get_scores_batch`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchScoreResult {
    pub index: u32,
    pub found: bool,
    pub score: Option<RiskScore>,
}

/// Read-only decay-adjusted view of a stored risk score.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectiveRiskScore {
    pub raw_score: u32,
    pub effective_score: u32,
    pub decay_applied: bool,
    pub elapsed_secs: u64,
    pub timestamp: u64,
    pub confidence: u32,
    pub model_version: u32,
    pub benford_flag: bool,
    pub ml_flag: bool,
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
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreAttestation {
    pub commitment: BytesN<32>,
    pub signature: BytesN<65>,
}

/// Threshold-signature attestation: t-of-n signers produce one 65-byte proof.
/// See `docs/threshold-attestation-spec.md`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThresholdAttestation {
    pub commitment: BytesN<32>,
    pub threshold_sig: BytesN<65>,
    pub participating_signers: soroban_sdk::Vec<Address>,
}

/// Unified attestation input for `submit_score`.
/// Wraps both attestation variants so the function stays within
/// Soroban's 10-parameter limit.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScoreAttestationInput {
    Single(ScoreAttestation),
    Threshold(ThresholdAttestation),
}

/// Decay-adjusted view of a score, returned by `get_effective_score`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectiveRiskScore {
    pub raw_score: u32,
    pub effective_score: u32,
    pub decay_applied: bool,
    pub elapsed_secs: u64,
    pub timestamp: u64,
    pub confidence: u32,
    pub model_version: u32,
    pub benford_flag: bool,
    pub ml_flag: bool,
}

/// Per-model-version aggregate stats, returned by `get_model_version_stats`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelVersionStats {
    pub model_version: u32,
    pub submission_count: u32,
    pub score_sum: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]

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
    pub score: u32,
    pub benford_flag: bool,
    pub ml_flag: bool,
    pub submitted_at: u64,
    pub confidence: u32,
    pub model_version: u32,
    pub timestamp: u64,
    pub commit_after: u64,
    pub submitted_by: Address,
}

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
    pub results: soroban_sdk::Vec<BatchEntryResult>,
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
    pub proof: soroban_sdk::Vec<BytesN<32>>,
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
    pub committed_at: u64,     // ledger timestamp
    pub committed_by: Address, // who called commit_snapshot
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreVelocityCap {
    pub enabled: bool,
    pub points_per_hour: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectiveRiskScore {
    pub effective_score: u32,
    pub original_score: u32,
    pub original_confidence: u32,
    pub confidence_floor: u32,
    pub delegated_to: Option<Address>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelVersionStats {
    pub model_version: u32,
    pub total_submissions: u64,
    pub average_score: u32,
}

#[contracttype]
#[derive(Clone)]
pub enum GateDataKey {
    GateCallers,
}

pub const MAX_GATE_CALLERS: u32 = 100;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Model version registry status.
    /// Encoded as a `u32` discriminant (see `ModelVersionStatus`).
    ModelVersionStatus(u32),
    /// When a model version is `Proposed`, this holds `proposed_at + upgrade_delay`.
    ModelVersionExecutableAfter(u32),
    /// Optional description committed at proposal time (for auditability).
    ModelVersionDescription(u32),
    Admin,
    Service,
    SignerTier(Address),

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
    /// Per-signer score range restriction. Maps a service signer address to
    /// its allowed `TierBounds`.
    SignerTier(Address),
    /// Ordered set of N addresses authorised to co-sign score submissions.
    ServiceSet,
    ServiceThreshold,
    StalenessWindow,
    LastSubmitTime(Address, Symbol),
    CooldownSecs,
    ScoreCount(Address, Symbol),
    ServicePubKey,
    HistoryMaxDepth,
    DecayRateNumerator,
    DecayRateDenominator,
    /// Global minimum confidence floor (0–100) enforced by
    /// `query_risk_gate_with_confidence`. The effective floor is
    /// `max(caller_param, global_floor)`. Defaults to 0 (no floor) when unset.
    GlobalMinConfidence,
    /// The SEP-41 token contract address from which fees are withdrawn.
    /// Unset until `set_fee_token` is called.
    FeeToken,
    WithdrawalLock,
    PairPaused(Symbol),
    PausedPairIndex,
    AdminSet,
    AdminThreshold,
    ScoreDelegate(Address),
    TrendState(Address, Symbol),
    Counterparties(Address, Symbol),
    /// Global boolean kill-switch for score velocity checks.
    ScoreVelocityCapEnabled,
    /// Maximum points a score can change per hour when the cap is enabled.
    ScoreVelocityCapPointsPerHour,
    /// One-time bypass flag set by the admin to allow a single submission
    /// for a specific (wallet, asset_pair) to bypass the velocity cap.
    VelocityCapOverride(Address, Symbol),
    SignerTier(Address),
    GlobalMinConfidence,
    /// Score-floor policy: historical peak (high-water mark) at or above which
    /// the floor applies. Global config, `u32`, defaults to
    /// `DEFAULT_SCORE_FLOOR_HWM` (80) when unset.
    ScoreFloorHighWaterMark,
    ScoreFloorMinValue,
    ScoreFloorEnabled,
    HistoricalMaxScore(Address, Symbol),
    HysteresisMargin,
    RiskBandState(Address, Symbol),
    ScoreEmbargo(Address),
    ConsensusThresholdK,
    ConsensusEpsilon,
    /// Open dispute record for a (wallet, asset_pair) pair. Absent key means
    /// no active dispute. Stored in temporary TTL-bounded storage.
    ScoreDispute(Address, Symbol),
    /// Index of all currently open disputes: `Vec<(Address, Symbol)>`.
    /// Incrementally maintained so `get_open_disputes` is a single read.
    DisputeIndex,
    /// A single model's commitment (sha256(score || nonce)) for consensus.
    /// Key: ConsensusCommitment(model_address, wallet, asset_pair) -> BytesN<32>
    ConsensusCommitment(Address, Address, Symbol),
    /// Configurable window for reveal in seconds.
    RevealWindowSecs,
    /// Admin-configured finality buffer in seconds.
    FinalityBufferSecs,
    /// Pending score entry held before commit. Invisible to get_score/query_risk_gate.
    PendingScore(Address, Symbol),
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

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TierBounds {
    pub min_score: u32,
    pub max_score: u32,
}
