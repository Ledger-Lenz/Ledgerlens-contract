use soroban_sdk::contracterror;

// XDR spec hard-limits contracterror enums to 50 variants.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotFound = 3,
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
    PairPaused = 30,
    PausedPairIndexFull = 31,
    BelowScoreFloor = 32,
    InvalidScoreFloorPolicy = 33,
    InvalidMinConfidence = 34,
    InvalidHysteresisMargin = 35,
    ScoreVelocityExceeded = 36,
    InsufficientConsensus = 37,
    CommitmentMismatch = 38,
    RevealWindowExpired = 39,
    DisputeAlreadyOpen = 40,
    DisputeNotFound = 41,
    InvalidDisputeBond = 42,
    InvalidFinalityBuffer = 43,
    NoPendingScore = 44,
    FinalityWindowNotElapsed = 45,
    ScoreEmbargoed = 46,
    InvalidDecayRate = 47,
    CyclicDelegation = 48,
    AdminSetFull = 49,
    FeeTokenNotSet = 50,
}

// Aliases for variants that exceed the 50-variant XDR limit.
// These resolve to the closest semantically-equivalent core variant.
#[allow(non_upper_case_globals)]
impl Error {
    pub const AdminSignerNotInSet: Error = Error::UnauthorizedSigner;
    pub const AggregatePubkeyNotSet: Error = Error::ServicePubkeyNotSet;
    pub const CounterpartyLinkFull: Error = Error::ServiceSetFull;
    pub const DelegateNotFound: Error = Error::NotFound;
    pub const DisputeIndexFull: Error = Error::BatchTooLarge;
    pub const DisputeNotYetTimedOut: Error = Error::FinalityWindowNotElapsed;
    pub const HighRiskWallet: Error = Error::ContractPaused;
    pub const InsufficientAdminSigners: Error = Error::InsufficientSigners;
    pub const InsufficientThresholdSigners: Error = Error::InsufficientSigners;
    pub const InvalidConsensusConfig: Error = Error::InvalidThreshold;
    pub const InvalidEscalation: Error = Error::InvalidThreshold;
    pub const InvalidJump: Error = Error::InvalidScore;
    pub const InvalidParameter: Error = Error::InvalidThreshold;
    pub const InvalidThresholdSignature: Error = Error::InvalidAttestation;
    pub const InvalidWithdrawalAmount: Error = Error::InvalidScore;
    pub const ModelVersionAlreadyDeprecated: Error = Error::AlreadyInitialized;
    pub const ModelVersionAlreadyRegistered: Error = Error::AlreadyInitialized;
    pub const ModelVersionDeprecated: Error = Error::NotFound;
    pub const ModelVersionNotRegistered: Error = Error::NotFound;
    pub const ModelVersionRegistryFull: Error = Error::ServiceSetFull;
    pub const ThresholdSignerNotInSet: Error = Error::SignerNotInSet;
    pub const WithdrawalInProgress: Error = Error::ContractPaused;
}
