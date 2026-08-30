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

#[allow(non_upper_case_globals)]
impl Error {
    pub const InvalidMinConfidence: Error = Error::InvalidConfidence;
    pub const InvalidWithdrawalAmount: Error = Error::InvalidThreshold;
    pub const WithdrawalInProgress: Error = Error::Unauthorized;
    pub const PairPaused: Error = Error::ContractPaused;
    pub const PausedPairIndexFull: Error = Error::ServiceSetFull;
    pub const DelegateNotFound: Error = Error::ScoreNotFound;
    pub const InvalidDecayRate: Error = Error::InvalidThreshold;
    pub const CounterpartyLinkFull: Error = Error::ServiceSetFull;
    pub const CounterpartyNotFound: Error = Error::ScoreNotFound;
    pub const SelfLink: Error = Error::InvalidScore;
    pub const ScoreVelocityExceeded: Error = Error::RateLimitExceeded;
    pub const InvalidEscalation: Error = Error::InvalidThreshold;
    pub const InvalidJump: Error = Error::InvalidScore;
    pub const BelowScoreFloor: Error = Error::InvalidScore;
    pub const InvalidScoreFloorPolicy: Error = Error::InvalidThreshold;
    pub const DisputeIndexFull: Error = Error::ServiceSetFull;
    pub const ActorDisputeLimitExceeded: Error = Error::RateLimitExceeded;
    pub const EmbargoedWalletIndexFull: Error = Error::ServiceSetFull;

    pub const ModelVersionNotRegistered: Error = Error::InvalidScore;
    pub const ModelVersionDeprecated: Error = Error::Unauthorized;
    pub const ModelVersionAlreadyDeprecated: Error = Error::AlreadyInitialized;
    pub const ModelVersionAlreadyRegistered: Error = Error::SignerAlreadyInSet;
    pub const ModelVersionRegistryFull: Error = Error::ServiceSetFull;
    pub const ModelVersionNotReady: Error = Error::UpgradeNotReady;
    pub const ModelVersionAlreadyProposed: Error = Error::UpgradeAlreadyPending;
    pub const ModelVersionNotProposed: Error = Error::NoPendingUpgrade;
    pub const ModelVersionNotActive: Error = Error::Unauthorized;

    pub const NotFound: Error = Error::ScoreNotFound;
    pub const FeeRecipientNotSet: Error = Error::FeeTokenNotSet;
    pub const FeeRecipientMismatch: Error = Error::Unauthorized;

    // ── Adaptive Threshold ─────────────────────────────────────────────────
    /// Returned when an invalid target percentile is provided (must be 50-99).
    pub const InvalidPercentile: Error = Error::InvalidThreshold;

    pub const ParameterProposalNotFound: Error = Error::ScoreNotFound;
    pub const ParameterProposalNotReady: Error = Error::UpgradeNotReady;
    pub const ParameterProposalVetoPeriodEnded: Error = Error::QuorumFailureWindowNotElapsed;
    pub const ParameterProposalExpired: Error = Error::RevealWindowExpired;
    pub const TooManyPendingParameterProposals: Error = Error::ServiceSetFull;
    pub const ParameterProposalAlreadyExecuted: Error = Error::AlreadyInitialized;
    pub const ParameterProposalVetoed: Error = Error::DisputeAlreadyOpen;
    pub const InvalidParameterKey: Error = Error::InvalidThreshold;
    pub const InvalidParameterValue: Error = Error::InvalidScore;
    pub const InvalidParameterTimeLock: Error = Error::InvalidUpgradeDelay;

    pub const EpochClosed: Error = Error::ContractPaused;
    pub const InsufficientPairData: Error = Error::InsufficientConsensus;
    pub const GateCallerListFull: Error = Error::ServiceSetFull;
    pub const GateCallerNotInList: Error = Error::ScoreNotFound;
    pub const ParamChangeAlreadyPending: Error = Error::UpgradeAlreadyPending;

    // ── Memory-exhaustion guards (#612) ─────────────────────────────────────
    /// Returned by `submit_scores_batch_attested` when `signers.len()`
    /// exceeds the current service set size, i.e. more entries than could
    /// ever be legitimately required. Reused discriminant: the enum is
    /// already at the 50-variant XDR limit.
    pub const TooManySigners: Error = Error::ServiceSetFull;

    // ── Aggregator composability ────────────────────────────────────────────
    /// Returned by `ledgerlens-aggregator`'s `add_shard` when a candidate shard
    /// does not advertise the `ILedgerLensScore` capabilities the aggregator
    /// invokes across every shard. It signals that the shard's interface has
    /// drifted from the version the aggregator targets, so registering it would
    /// lead to failed or subtly incorrect cross-contract calls.
    pub const IncompatibleInterface: Error = Error::InvalidAttestation;

    // ── Architecture Governance & Reviewer Routing ─────────────────────────
    /// Returned when an architecture owner or reviewer address is invalid.
    pub const InvalidArchOwner: Error = Error::Unauthorized;
    /// Returned when trying to set more mandatory reviewers than MAX_MANDATORY_REVIEWERS (10).
    pub const MaxReviewersExceeded: Error = Error::ServiceSetFull;
    /// Returned when trying to add a duplicate mandatory reviewer address.
    pub const ReviewerAlreadyExists: Error = Error::SignerAlreadyInSet;
    /// Returned when a reviewer to be removed is not in the set.
    pub const ReviewerNotFound: Error = Error::SignerNotInSet;

    // ── Administrative capability partitioning (issue #695) ────────────────
    /// Returned by `set_policy_approval` when called with
    /// `Policy::DataDeletion`, which is configured via
    /// `set_deletion_approval_policy` instead.
    pub const InvalidPolicy: Error = Error::InvalidThreshold;
}
