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
    ScoreNotFound = 6,
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

    // ── Fee withdrawal ─────────────────────────────────────────────────────
    /// Returned by `get_fee_token` and `withdraw_fees` when `set_fee_token`
    /// has not been called.
    FeeTokenNotSet = 30,
    /// Returned by `withdraw_fees` when `amount` is zero.
    InvalidWithdrawalAmount = 31,
    /// Returned by `withdraw_fees` when another withdrawal call is already
    /// in-flight (concurrency lock held).
    WithdrawalInProgress = 32,

    // ── Admin multi-sig ────────────────────────────────────────────────────
    /// `add_admin_signer` was called when the admin set already contains
    /// `MAX_ADMIN_SIGNERS` members.
    AdminSetFull = 33,
    /// A signer passed to an admin function is not a member of the admin set.
    AdminSignerNotInSet = 34,
    /// Fewer than the configured admin threshold of signers were provided to
    /// an admin-gated function.
    InsufficientAdminSigners = 35,

    // ── Parameter change time-lock ──────────────────────────────────────────
    /// `apply_param_change` or `cancel_param_change` called but no pending
    /// proposal exists for the given parameter key.
    NoPendingParamChange = 36,
    /// `apply_param_change` called before the proposal's `apply_after`
    /// timestamp has elapsed.
    ParamChangeNotReady = 37,
    /// A parameter change proposal already exists for this key; cancel or
    /// apply it before proposing a new one.
    ParamChangeAlreadyPending = 38,
    /// `set_param_change_delay` called with a value outside the allowed bounds
    /// `[MIN_PARAM_CHANGE_DELAY_SECS, MAX_PARAM_CHANGE_DELAY_SECS]`.
    InvalidParamChangeDelay = 39,
    /// `apply_param_change` called with a key that does not map to any known
    /// parameter — should not occur via the normal setter paths.
    UnknownParamKey = 40,
}
