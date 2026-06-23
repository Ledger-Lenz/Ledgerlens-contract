use soroban_sdk::{contracttype, Address, BytesN, Symbol};

/// Embargo expiry configuration stored per wallet in temporary storage.
///
/// - `Indefinite` — embargo has no built-in expiry; only `lift_score_embargo`
///   removes it.
/// - `Until(ts)` — embargo auto-expires when `ledger_timestamp > ts`; no
///   admin action needed once the timestamp is reached.
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

/// A single model's contribution to an ensemble consensus submission.
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

/// Merkle-root attestation for an entire `submit_scores_batch_attested`
/// call: a single secp256k1 signature over the Merkle root of every entry
/// in the batch. See `docs/batch-attestation-spec.md` for the off-chain
/// tree-construction algorithm, the on-chain verification path, and the
/// rationale for choosing domain-separated prefix hashing (RFC 9162 style,
/// `0x00` for leaves / `0x01` for internal nodes) over the alternative
/// sorted-pair scheme.
///
/// The signature format is intentionally byte-identical to that of
/// [`ScoreAttestation`] — 65 bytes: 32-byte `r`, 32-byte `s`, 1-byte
/// recovery id — so the same off-chain signing key can be reused for both
/// per-score and per-batch paths, and so `verify_signature` can be a
/// single shared helper.
///
/// # Verified-digest convention
///
/// `signature` is over `SHA256(merkle_root)`, **not** over `merkle_root`
/// directly. This is a one-extra-hash convention forced by the soroban-sdk
/// 21.x API: `env.crypto().secp256k1_recover` takes an opaque `Hash<32>`,
/// and `Hash<32>` has no public constructor — it can only be built via a
/// host crypto function call. Both sides wrap through SHA-256 once:
///
/// * **Off-chain** (`api`/`core` pipeline): build `root`, then sign
///   `SHA256(root)`.
/// * **On-chain** (`submit_scores_batch_attested`): wrap
///   `attestation.merkle_root` through `env.crypto().sha256` once before
///   calling `verify_signature`.
///
/// The pipeline produces exactly the same merkle_root as the verifier
/// recomputes from the entry commitments — the SHA-256 wrap is purely a
/// soroban-sdk compatibility shim, not a security downgrade.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchAttestation {
    /// `SHA-256(0x01 || SHA-256(0x00 || commit_i) || SHA-256(0x00 || commit_{i+1}))`
    /// (recursively, until the 32-byte root). Bound to one specific
    /// deployment on one specific network by including the contract
    /// address and `network_id` inside every leaf's underlying commitment
    /// (see [`ScoreAttestation`]'s preimage layout for context).
    ///
    /// The contract does **not** sign this value directly — see the struct
    /// rustdoc's "Verified-digest convention" above for the SHA-256 wrap.
    pub merkle_root: BytesN<32>,
    /// 65-byte secp256k1 ECDSA signature over `SHA256(merkle_root)`: 32-byte `r`,
    /// 32-byte `s`, then a 1-byte recovery id which must be `0` or `1`.
    pub signature: BytesN<65>,
}

/// A single entry in an attested batch score submission. Mirrors
/// [`ScoreSubmission`] so the service can submit many scores in one call,
/// and carries its own Merkle inclusion proof against the
/// [`BatchAttestation`]'s `merkle_root`.
///
/// # `proof_flags` bit layout
///
/// `proof_flags` is a `u32` bit field that records, for every level of the
/// Merkle tree from the leaf (`i == 0`) upward, whether the sibling at that
/// level sits to the **left** (1) or right (0) of the current node being
/// walked up. So an attestation for leaf index `5` in an 8-leaf tree will
/// typically produce three flag bits; a single-entry batch produces
/// `proof_flags == 0` and an empty `proof` ([`verify_merkle_proof`] handles
/// this case explicitly).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreSubmissionWithProof {
    pub submission: ScoreSubmission,
    /// Sibling hashes from leaf to root, left-to-right (ordered to match
    /// `proof_flags`'s LSB-first indexing). Length must be in
    /// `[0, MAX_MERKLE_PROOF_DEPTH]` — anything longer is rejected with
    /// `Error::InvalidAttestation`.
    pub proof: soroban_sdk::Vec<BytesN<32>>,
    /// Bit-field encoding the left/right direction at each level. Bit `i`
    /// (LSB = 0) is `0` if the sibling at level `i` is to the right, `1` if
    /// to the left.
    pub proof_flags: u32,
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

/// Global configuration for the per-wallet score submission floor.
///
/// Returned by `get_score_floor_policy` and configured by
/// `set_score_floor_policy`. When `enabled`, any `(wallet, asset_pair)`
/// whose historical peak score has reached `high_water_mark` can no longer
/// receive a submission below `floor_value` — a second line of defence
/// against a compromised or colluding signer laundering a known high-risk
/// wallet's reputation by zeroing its score. Disabled by default.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreFloorPolicy {
    /// Kill-switch: when `false`, no floor is enforced for any wallet.
    pub enabled: bool,
    /// Historical peak score at or above which the floor begins to apply for
    /// a given `(wallet, asset_pair)`. Bounded to `[50, 100]`.
    pub high_water_mark: u32,
    /// Minimum score a high-risk wallet may be assigned while the floor
    /// applies. Submissions below this are rejected with `BelowScoreFloor`.
    /// Bounded to `[0, high_water_mark - 1]`.
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
    /// Admin-configurable score jump anomaly detection threshold. When the
    /// absolute delta between consecutive scores exceeds this value, a
    /// `ScoreJumpAnomalyEvent` is emitted. Defaults to
    /// `DEFAULT_JUMP_THRESHOLD` (30) when unset.
    JumpThreshold,
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
    /// Ordered set of M-of-N admin co-signers.
    AdminSet,
    /// Minimum number of admin-set members that must sign an admin call.
    AdminThreshold,
    /// Score delegation: maps a sub-wallet to its custodian wallet.
    ScoreDelegate(Address),
    /// Per-(wallet, asset_pair) trend state: current trend direction (+1/0/-1)
    /// and consecutive submission count in that direction. Updated by every
    /// successful `submit_score` / `submit_scores_batch` write.
    TrendState(Address, Symbol),
    // ── Wallet Relationship Graph ──────────────────────────────────────────
    /// List of counterparty addresses for a wallet on a specific asset pair.
    /// Key: Counterparties(wallet, asset_pair) -> Vec<Address>
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
    /// Score-floor policy: minimum score permitted for high-risk wallets.
    /// Global config, `u32`, defaults to `DEFAULT_SCORE_FLOOR_MIN` (20).
    ScoreFloorMinValue,
    /// Score-floor policy kill-switch. Global config, `bool`, defaults to
    /// `false` (floor disabled) until the admin opts in.
    ScoreFloorEnabled,
    /// Per-(wallet, asset_pair) running maximum of every score ever accepted,
    /// used to decide whether the submission floor applies. Updated on every
    /// accepted `submit_score` / `submit_scores_batch` write.
    HistoricalMaxScore(Address, Symbol),
    /// Admin-configured hysteresis margin (u32). Used to widen the exit
    /// threshold below the entry threshold so scores must drop further to
    /// leave the high-risk band. Stored in instance storage; defaults to 0.
    HysteresisMargin,
    /// Per-(wallet, asset_pair) risk band state. `true` means the wallet is
    /// currently inside the high-risk band for this pair. Stored in
    /// temporary TTL-bounded storage so stale states expire automatically.
    RiskBandState(Address, Symbol),
    /// Per-wallet score embargo. Stores an `EmbargoExpiry` describing whether
    /// the embargo is indefinite or expires at a specific ledger timestamp.
    /// Absent key means no embargo. Stored in temporary TTL-bounded storage.
    ScoreEmbargo(Address),
    /// Minimum number of model submissions that must agree for a consensus
    /// score to be accepted.
    ConsensusThresholdK,
    /// Maximum allowed score deviation from the provisional median when
    /// building the consensus set.
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
