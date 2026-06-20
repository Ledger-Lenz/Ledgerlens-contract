// Ledger TTL constants assume ~5 s per ledger on Stellar mainnet.
pub const SCORE_TTL_THRESHOLD: u32 = 518_400; // ~30 days
pub const SCORE_TTL_EXTEND_TO: u32 = 777_600; // ~45 days

/// Hard ceiling on the ring-buffer depth to bound storage costs.
/// The admin cannot configure a depth above this value.
pub const MAX_HISTORY_DEPTH: u32 = 50;

/// Default depth used when no admin configuration exists.
pub const DEFAULT_HISTORY_MAX_DEPTH: u32 = 10;

/// Maximum number of entries accepted in a single batch submission call.
pub const MAX_BATCH_SIZE: u32 = 20;

/// Default risk threshold used when no threshold has been configured by admin.
pub const DEFAULT_RISK_THRESHOLD: u32 = 75;

/// Semantic contract version; bump on breaking ABI changes.
///
/// Bumped to 3 when all admin-tier functions gained `admin_signers: Vec<Address>`
/// for M-of-N governance and the `AdminSet` / `AdminThreshold` storage keys
/// were introduced.
pub const CONTRACT_VERSION: u32 = 3;

/// Practical upper bound on the number of distinct asset pairs tracked per
/// wallet. `get_aggregate_score` iterates the wallet's full `AssetPairs`
/// list, so its cost is O(N) in this value; it is not enforced on-chain,
/// but documents the assumption the aggregate engine is designed around.
/// See the rustdoc on `get_aggregate_score` for detail.
pub const MAX_WALLET_PAIRS: u32 = 20;

// ── Per-wallet/pair submission rate limiting ──────────────────────────────────
//
// A compromised or malfunctioning off-chain service could otherwise flood the
// contract with submissions for the same wallet/asset-pair, exhausting
// storage rent, overwhelming indexers, and poisoning the score signal with
// rapid fluctuations. See `submit_score` / `set_cooldown` and the Rate
// Limiting section of the README.

/// Default cooldown applied between accepted submissions for the same
/// (wallet, asset_pair) until the admin configures one explicitly — 1 hour.
pub const DEFAULT_COOLDOWN_SECS: u64 = 3_600; // 1 hour

/// Minimum configurable cooldown — 1 minute floor, so the admin cannot
/// disable rate limiting entirely by setting it arbitrarily low.
pub const MIN_COOLDOWN_SECS: u64 = 60; // 1 minute

/// Maximum configurable cooldown — 24 hour ceiling, so a misconfigured admin
/// cannot lock a wallet/pair out of re-scoring for an unreasonable length of
/// time.
pub const MAX_COOLDOWN_SECS: u64 = 86_400; // 24 hours

// ── Time-locked upgrade governance ────────────────────────────────────────────
//
// A WASM upgrade can replace the entire contract logic in one transaction, so
// it is gated behind a mandatory delay during which the community can inspect
// the pending proposal and react. These bounds frame the admin-configurable
// delay; see `propose_upgrade` / `set_upgrade_delay` and the Upgrade Governance
// section of the README.

/// Minimum mandatory delay between proposing and executing an upgrade —
/// 48 hours. The delay can be raised (safer) but never lowered below this.
pub const MIN_UPGRADE_DELAY_SECS: u64 = 172_800; // 48 hours

/// Maximum configurable upgrade delay — 14 days. Caps the lock so a
/// legitimate, urgent fix is not stalled indefinitely.
pub const MAX_UPGRADE_DELAY_SECS: u64 = 1_209_600; // 14 days

/// Delay applied to a proposal when the admin has not configured one
/// explicitly. Equal to the minimum (most conservative) by default.
pub const DEFAULT_UPGRADE_DELAY_SECS: u64 = 172_800; // 48 hours

/// Maximum number of addresses in the M-of-N service signer set.
pub const MAX_SERVICE_SIGNERS: u32 = 10;

/// Maximum number of addresses in the M-of-N admin signer set.
pub const MAX_ADMIN_SIGNERS: u32 = 5;

/// Default staleness window: 7 days in seconds.
pub const DEFAULT_STALENESS_WINDOW_SECS: u64 = 604_800;

// ── Per-asset-pair circuit breaker ────────────────────────────────────────────

/// Hard ceiling on the number of distinct asset pairs that may be paused at
/// once. Bounds `PausedPairIndex`'s storage cost and the O(N) work done on
/// the rare admin pause/unpause path; the hot `is_pair_paused` read used by
/// every submission never touches the index. See `set_pair_paused`.
pub const MAX_PAUSED_PAIRS: u32 = 50;
