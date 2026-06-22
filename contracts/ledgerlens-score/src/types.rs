use soroban_sdk::{contracttype, Address, BytesN, Symbol};

/// Embargo expiry configuration stored per wallet in temporary storage.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EmbargoExpiry {
    Indefinite,
    Until(u64),
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

/// A single model's contribution to an ensemble consensus submission.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelSubmission {
    pub model_version: u32,
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

/// Result of a decay-adjusted effective score lookup.
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

/// Running performance statistics for a model version.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelVersionStats {
    pub model_version: u32,
    pub submission_count: u32,
    pub score_sum: u64,
    pub score_max: u32,
    pub score_min: u32,
    pub first_seen: u64,
    pub last_seen: u64,
}

/// On-chain score percentile histogram: 10 buckets of width 10.
/// Bucket 0: [0,9], bucket 1: [10,19], ..., bucket 9: [90,100].
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreHistogram {
    pub buckets: soroban_sdk::Vec<u32>,
    pub total: u32,
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TierBounds {
    pub min_score: u32,
    pub max_score: u32,
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
    ServiceSet,
    ServiceThreshold,
    StalenessWindow,
    LastSubmitTime(Address, Symbol),
    CooldownSecs,
    ScoreCount(Address, Symbol),
    ServicePubKey,
    HistoryMaxDepth,
    DecayRate,
    FeeToken,
    WithdrawalLock,
    PairPaused(Symbol),
    PausedPairIndex,
    AdminSet,
    AdminThreshold,
    ScoreDelegate(Address),
    TrendState(Address, Symbol),
    Counterparties(Address, Symbol),
    ScoreFloorConfig,
    HistoricalMaxScore(Address, Symbol),
    HysteresisMargin,
    RiskBandState(Address, Symbol),
    ScoreEmbargo(Address),
    ConsensusThresholdK,
    ConsensusEpsilon,
    EscalationThreshold,
    BreachCount(Address, Symbol),
    ModelStats(u32),
    AllModelVersions,
    GlobalMinConfidence,
    SignerTier(Address),
    SignerTtl,
    SignerActivationTime(Address),
    GateCallers,
    GateOpen,
}
