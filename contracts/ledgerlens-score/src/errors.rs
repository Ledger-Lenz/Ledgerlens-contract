use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InvalidScore = 4,
    InvalidConfidence = 5,
   SignerTierViolation = 26,
    InvalidSignerTier = 27, ScoreNotFound = 6,
    /// Returned when any state-mutating call is attempted while the
    /// contract is paused by the admin.
    ContractPaused = 7,
    /// Returned when `accept_admin` or `cancel_admin_transfer` is called
    /// but no transfer has been initiated.
    NoPendingAdminTransfer = 8,
    /// Returned when `submit_scores_batch` is called with zero entries.
    EmptyBatch = 9,
    /// Returned when a batch exceeds the MAX_BATCH_SIZE limit.
    BatchTooLarge = 10,
    /// Returned when the weighted aggregate computation in
    /// `get_aggregate_score` would overflow.
    ArithmeticOverflow = 11,
    /// Fewer than the configured threshold of signers were provided to
    /// `submit_score`.
    InsufficientSigners = 14,
    /// A signer passed to `submit_score` is not a member of the service set.
    UnauthorizedSigner = 15,
    /// `set_service_threshold` was called with `0` or a value exceeding
    /// the current service-set size.
    InvalidThreshold = 16,
    /// `add_service_signer` was called when the service set already contains
    /// `MAX_SERVICE_SIGNERS` members.
    ServiceSetFull = 17,
    /// `add_service_signer` was called with an address already in the set.
    SignerAlreadyInSet = 18,
    /// `remove_service_signer` was called with an address not in the set.
    SignerNotInSet = 19,
    /// `propose_upgrade` was called while a proposal is already pending.
    UpgradeAlreadyPending = 12,
    /// `execute_upgrade` was called before the time-lock elapsed, or
    /// `get_pending_upgrade` was called when no proposal exists.
    NoPendingUpgrade = 13,
    /// `execute_upgrade` called before `executable_after` timestamp.
    UpgradeNotReady = 20,
    /// `set_upgrade_delay` called with a value outside the allowed bounds.
    InvalidUpgradeDelay = 21,
    /// Returned when a staleness window value of 0 is provided.
    InvalidStalenessWindow = 22,

    // ── Per-wallet/pair submission rate limiting ────────────────────────────
    /// Returned by `submit_score` when a submission for the same
    /// (wallet, asset_pair) arrives before the configured cooldown has
    /// elapsed since the last accepted submission. In `submit_scores_batch`
    /// the offending entry is skipped instead of failing the whole batch.
    RateLimitExceeded = 23,
    /// Returned when `set_cooldown` is given a value below
    /// `MIN_COOLDOWN_SECS` or above `MAX_COOLDOWN_SECS`.
    InvalidCooldown = 24,
    /// Returned when a timestamp of 0 is submitted (zero is reserved and
    /// indicates an uninitialised / invalid timestamp).
    InvalidTimestamp = 25,

    // ── Score attestation ───────────────────────────────────────────────────
    /// Returned by `submit_score` when a `ScoreAttestation` is supplied but
    /// `set_service_pubkey` has never been called — there is no key to
    /// verify the signature against. Also returned by `get_service_pubkey`
    /// before one has been configured.
    ServicePubkeyNotSet = 26,
    /// Returned by `submit_score` when an attestation is required (a
    /// service pubkey is configured) but missing, or when a supplied
    /// `ScoreAttestation` fails verification: the recomputed commitment
    /// disagrees with the supplied one, the signature's recovery id is not
    /// `0`/`1`, or the recovered public key does not match the registered
    /// service pubkey.
    InvalidAttestation = 27,
    /// `set_service_pubkey` was called with a pubkey whose length is
    /// neither 33 (compressed) nor 65 (uncompressed) bytes.
    InvalidPubkeyLength = 28,
    /// Returned when `set_history_max_depth` is called with `0` or a value
    /// above `MAX_HISTORY_DEPTH`.
    InvalidHistoryDepth = 29,
feat/confidence-gated-risk-gate
    /// Returned when `set_global_min_confidence` is called with a value
    /// above 100 (confidence is bounded to 0–100).
    InvalidMinConfidence = 30,


    // ── Fee withdrawal ─────────────────────────────────────────────────────
    /// Returned by `get_fee_token` and `withdraw_fees` when `set_fee_token`
    /// has not been called.
    FeeTokenNotSet = 30,
    /// Returned by `withdraw_fees` when `amount` is zero.
    InvalidWithdrawalAmount = 31,
    /// Returned by `withdraw_fees` when another withdrawal call is already
    /// in-flight (concurrency lock held).
    WithdrawalInProgress = 32,

    // ── Per-pair circuit breaker ───────────────────────────────────────────
    /// Returned by `submit_score` / `submit_scores_batch` when the target
    /// `asset_pair` has been individually paused via `set_pair_paused`.
    PairPaused = 33,
    /// Returned by `set_pair_paused` when trying to pause a new pair but the
    /// `PausedPairIndex` already holds `MAX_PAUSED_PAIRS` entries.
    PausedPairIndexFull = 34,

    // ── Admin M-of-N multi-sig ─────────────────────────────────────────────
    /// `add_admin_signer` called when the admin set is already at
    /// `MAX_ADMIN_SIGNERS`.
    AdminSetFull = 35,
    /// A signer in `admin_signers` is not a member of the admin set.
    AdminSignerNotInSet = 36,
    /// Fewer than the configured threshold of admin signers were supplied.
    InsufficientAdminSigners = 37,

    // ── Wallet-score delegation ────────────────────────────────────────────
    /// `set_score_delegate` would create a cycle (wallet → custodian →
    /// wallet).
    CyclicDelegation = 38,
    /// `remove_score_delegate` called for a wallet that has no delegate.
    DelegateNotFound = 39,

    // ── Cross-contract gate ────────────────────────────────────────────────
    /// Returned by an integrating contract (e.g. AMM) when `query_risk_gate`
    /// returns `false`. Not returned by the LedgerLens contract itself.
    HighRiskWallet = 40,

    // ── Time-weighted exponential decay ───────────────────────────────────
    /// `set_decay_rate` called with a denominator of 0, or with a
    /// numerator/denominator ratio exceeding `MAX_DECAY_LAMBDA`.
    InvalidDecayRate = 41,

    // ── Score embargo ──────────────────────────────────────────────────────
    /// Returned by read-path functions (`get_score`, `get_aggregate_score`)
    /// when the requested wallet is under an active regulatory embargo.
    ScoreEmbargoed = 42,

    // ── Wallet Relationship Graph ──────────────────────────────────────────
    /// Returned when `add_counterparty_link` would exceed the max links per wallet.
    CounterpartyLinkFull = 43,
    /// Returned when `remove_counterparty_link` is called for a non-existent link.
    CounterpartyNotFound = 44,
    /// Returned when `add_counterparty_link` is called with the same wallet twice.
    SelfLink = 45,
}

// Gate caller tracking error variants for structural protection
pub const GATE_CALLER_ALREADY_ALLOWED: u32 = 26;
pub const GATE_CALLER_NOT_FOUND: u32 = 27;
pub const GATE_CALLER_LIST_FULL: u32 = 28;
