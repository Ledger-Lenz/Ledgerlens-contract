use soroban_sdk::contracterror;

// XDR spec hard-limits contracterror enums to 50 variants.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InvalidScore = 4,
    InvalidConfidence = 5,
    ScoreNotFound = 6,
    ContractPaused = 7,
    NoPendingAdminTransfer = 8,
    EmptyBatch = 9,
    BatchTooLarge = 10,
    ArithmeticOverflow = 11,
    UpgradeAlreadyPending = 12,
    NoPendingUpgrade = 13,
    InsufficientSigners = 14,
    UnauthorizedSigner = 15,
    InvalidThreshold = 16,
    ServiceSetFull = 17,
    SignerAlreadyInSet = 18,
    SignerNotInSet = 19,
    UpgradeNotReady = 20,
    InvalidUpgradeDelay = 21,
    InvalidStalenessWindow = 22,
    RateLimitExceeded = 23,
    InvalidCooldown = 24,
    InvalidTimestamp = 25,
    ServicePubkeyNotSet = 26,
    InvalidAttestation = 27,
    InvalidPubkeyLength = 28,
    InvalidHistoryDepth = 29,
    InsufficientConsensus = 30,
    ConsensusInputEmpty = 31,
    InvalidConsensusConfig = 32,
    AdminSetFull = 33,
    AdminSignerNotInSet = 34,
    InsufficientAdminSigners = 35,
    CyclicDelegation = 36,
    ScoreEmbargoed = 37,
    FeeTokenNotSet = 38,
    QuorumFailureWindowNotElapsed = 39,
    RevealWindowExpired = 40,
    CommitmentMismatch = 41,
    InvalidFinalityBuffer = 42,
    NoPendingScore = 43,
    FinalityWindowNotElapsed = 44,
    InvalidDisputeBond = 45,
    DisputeAlreadyOpen = 46,
    DisputeNotFound = 47,
    DisputeNotYetTimedOut = 48,
    InvalidHysteresisMargin = 49,
    InvalidModelPriorWeight = 50,
}

    /// Returned when `set_global_min_confidence` is called with a value
    /// above 100 (confidence is bounded to 0–100).
    InvalidMinConfidence = 49,

    // ── Fee withdrawal ─────────────────────────────────────────────────────
    /// Returned by `get_fee_token` and `withdraw_fees` when `set_fee_token`
    /// has not been called.
    FeeTokenNotSet = 52,
    /// Returned by `withdraw_fees` when `amount` is zero.
    InvalidWithdrawalAmount = 31,
    WithdrawalInProgress = 32,
    PairPaused = 33,
    PausedPairIndexFull = 34,
    AdminSetFull = 35,
    AdminSignerNotInSet = 36,
    InsufficientAdminSigners = 37,
    CyclicDelegation = 38,
    DelegateNotFound = 39,
    InvalidDecayRate = 41,
    ScoreEmbargoed = 42,
    CounterpartyLinkFull = 43,
    CounterpartyNotFound = 44,
    SelfLink = 45,

    // ── Velocity Cap ───────────────────────────────────────────────────────
    /// Returned when the requested score change exceeds the allowed points per hour.
    ScoreVelocityExceeded = 46,

    InvalidEscalation = 50,
    InvalidJump = 51,
    // ── Score submission floor ─────────────────────────────────────────────
    /// Returned by `submit_score` (and recorded as a `rejection_code` in
    /// `submit_scores_batch`) when the score-floor policy is enabled, the
    /// `(wallet, asset_pair)`'s historical peak score is at or above the
    /// configured high-water mark, and the submitted score is below the
    /// configured floor value — blocking an attempt to launder a known
    /// high-risk wallet's reputation by zeroing its score.
    BelowScoreFloor = 46,
    InvalidScoreFloorPolicy = 47,
    InvalidHysteresisMargin = 48,
    InsufficientConsensus = 49,
    /// `reveal_consensus` was called with zero model submissions.
    ConsensusInputEmpty = 50,
    /// `set_consensus_config` was called with `k == 0` or `epsilon > 100`.
    InvalidConsensusConfig = 51,
    /// `request_quorum_reduction` called before the failure window has elapsed.
    QuorumFailureWindowNotElapsed = 52,
    /// `reveal_consensus` was called after the commitment's TTL expired.
    RevealWindowExpired = 52,
    /// `reveal_consensus` was called but the score and nonce do not match the commitment.
    CommitmentMismatch = 53,

    // ── Score dispute mechanism ────────────────────────────────────────────
    /// Returned by `open_score_dispute` when a dispute already exists for the
    /// given `(wallet, asset_pair)`.
    DisputeAlreadyOpen = 55,
    /// Returned by `resolve_dispute_timeout` when the dispute deadline has not
    /// yet elapsed.
    DisputeNotYetTimedOut = 56,
    /// Returned when attempting to resolve a dispute that does not exist.
    DisputeNotFound = 57,
    /// Returned by `open_score_dispute` when the staked bond is zero or
    /// negative.
    InvalidDisputeBond = 58,
    /// Returned by `open_score_dispute` when the open-dispute index is already
    /// at `MAX_OPEN_DISPUTES`.
    DisputeIndexFull = 59,

    // ── Finality buffer (pending score commit window) ──────────────────────
    /// `set_finality_buffer` was called with a value above
    /// `MAX_FINALITY_BUFFER_SECS`.
    InvalidFinalityBuffer = 60,
    /// `commit_pending_score`, `cancel_pending_score`, or any function
    /// expecting a pending entry was called for a `(wallet, asset_pair)`
    /// with no pending score.
    NoPendingScore = 61,
    /// `commit_pending_score` was called before `commit_after` elapsed.
    FinalityWindowNotElapsed = 62,

    // ── Score embargo ────────────────────────────────────────────────────────
    /// Returned by `set_score_embargo` when the wallet is not already
    /// embargoed and the `EmbargoedWalletIndex` is already at
    /// `MAX_EMBARGOED_WALLETS`.
    EmbargoedWalletIndexFull = 63,
}
