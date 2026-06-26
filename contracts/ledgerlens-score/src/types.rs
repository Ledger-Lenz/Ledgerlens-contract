use soroban_sdk::{contracttype, Address, BytesN, Env, Symbol, Vec};

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

/// Per-model-version aggregate stats, returned by `get_model_version_stats`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelVersionStats {
    pub model_version: u32,
    pub submission_count: u32,
    pub score_sum: u64,
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
#[derive(Clone)]
pub enum GateDataKey {
    GateCallers,
    GateOpen,
}

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
    /// Per-signer score range restriction. Maps a service signer address to
    /// its allowed `TierBounds`.
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
    /// Per-signer rolling accuracy record for reputation weighting in consensus.
    SignerAccuracy(Address),
    /// Registered oracle contract address for a given asset pair.
    RegisteredOracle(Symbol),
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
            DataKey::ScoreDispute(a, s) => k2!("ScoreDispute", a, s),
            DataKey::DisputeIndex => k0!("DisputeIndex"),
            DataKey::ConsensusCommitment(m, w, s) => k3!("ConsCommit", m, w, s),
            DataKey::RevealWindowSecs => k0!("RevealWinSecs"),
            DataKey::FinalityBufferSecs => k0!("FinalityBufSec"),
            DataKey::PendingScore(a, s) => k2!("PendingScore", a, s),
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
            DataKey::SignerAccuracy(a) => k1!("SignerAccuracy", a),
            DataKey::RegisteredOracle(s) => k1!("RegOracle", s),
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

/// Rolling accuracy record for a single consensus signer.
///
/// Tracks the mean absolute deviation (MAD) of the signer's submitted scores
/// from the accepted consensus median, used to weight each signer's contribution
/// in subsequent consensus rounds. `count` is the number of rounds the signer
/// has participated in; `mad_scaled` is the running MAD × 1000 to preserve
/// precision while staying within `u64`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignerAccuracyRecord {
    /// Number of consensus rounds this signer has participated in.
    pub count: u64,
    /// Running mean absolute deviation × 1000 (integer, rounded).
    pub mad_scaled: u64,
}

/// Snapshot of the price returned by a registered oracle for a single asset pair.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OraclePriceRecord {
    /// Latest price from the oracle (raw, oracle-defined units).
    pub price: i128,
    /// Ledger timestamp at which the price was fetched.
    pub fetched_at: u64,
}

/// Interface that every registered price-feed oracle must implement.
/// Register an oracle via `register_oracle(asset_pair, oracle_contract)`;
/// the contract calls `get_price(asset_pair)` on each `get_effective_score`
/// invocation to retrieve the current market price.
pub trait OracleAdapterTrait {
    fn get_price(env: soroban_sdk::Env, asset_pair: Symbol) -> i128;
}