use soroban_sdk::{contracttype, Address, BytesN, Symbol, Vec};

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
}

/// Query descriptor for a batch score read.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreQuery {
    pub wallet: Address,
    pub asset_pair: Symbol,
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
    pub score: RiskScore,
}

/// Decay-adjusted view of a stored risk score, returned by `get_effective_score`.
///
/// This is the canonical definition. Fields cover both the decay path
/// (`raw_score`, `effective_score`, `decay_applied`, `elapsed_secs`) and the
/// delegation path (`original_score`, `original_confidence`, `confidence_floor`,
/// `delegated_to`).
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
    pub original_score: u32,
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
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreAttestation {
    pub commitment: BytesN<32>,
    pub signature: BytesN<65>,
}

/// Threshold-signature attestation: t-of-n signers produce one 65-byte proof.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThresholdAttestation {
    pub commitment: BytesN<32>,
    pub threshold_sig: BytesN<65>,
    pub participating_signers: Vec<Address>,
}

/// Unified attestation input for `submit_score`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScoreAttestationInput {
    Single(ScoreAttestation),
    Threshold(ThresholdAttestation),
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
}

/// A model's score submission for consensus reveal.
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

/// Per-(wallet, asset_pair) trend state persisted between submissions.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreTrend {
    pub trend: i32,
    pub consecutive: u32,
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreHistogram {
    pub buckets: Vec<u32>,
    pub total: u64,
}

#[contracttype]
#[derive(Clone)]
pub enum GateDataKey {
    GateCallers,
    GateOpen,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Service,
    Score(Address, Symbol),
    Paused,
    PendingAdmin,
    Watchlist(Address),
    RiskThreshold,
    JumpThreshold,
    ScoreHistory(Address, Symbol),
    ContractVersion,
    AssetPairs(Address),
    PairWeight(Symbol),
    AggregateScore(Address),
    PendingUpgrade,
    UpgradeDelay,
    SignerTier(Address),
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
    PairPaused(Symbol),
    PausedPairIndex,
    AdminSet,
    AdminThreshold,
    ScoreDelegate(Address),
    TrendState(Address, Symbol),
    Counterparties(Address, Symbol),
    ScoreVelocityCapEnabled,
    ScoreVelocityCapPointsPerHour,
    VelocityCapOverride(Address, Symbol),
    ScoreFloorConfig,
    HistoricalMaxScore(Address, Symbol),
    HysteresisMargin,
    RiskBandState(Address, Symbol),
    ScoreEmbargo(Address),
    ConsensusThresholdK,
    ConsensusEpsilon,
    ScoreDispute(Address, Symbol),
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
    LastGlobalSubmissionTime,
    QuorumFailureWindow,
    ConsensusCommitment(Address, Address, Symbol),
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TierBounds {
    pub min_score: u32,
    pub max_score: u32,
}
