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
    /// The SEP-41 token contract address from which fees are withdrawn.
    /// Unset until `set_fee_token` is called.
    FeeToken,
    /// Boolean flag set for the duration of a `withdraw_fees` call to
    /// prevent concurrent duplicate withdrawals.
    WithdrawalLock,
}
