pub const SCORE_TTL_THRESHOLD: u32 = 518_400;
pub const SCORE_TTL_EXTEND_TO: u32 = 777_600;

/// Maximum number of allowed gate callers in the allowlist.
pub const MAX_GATE_CALLERS: u32 = 20;

/// Hard lower bound for all score values submitted to the contract.
/// `submit_score` accepts scores in `[MIN_SCORE, MAX_SCORE]`; any value
/// below this is rejected with [`Error::InvalidScore`].
pub const MIN_SCORE: u32 = 0;

/// Hard upper bound for all score values submitted to the contract.
pub const MAX_SCORE: u32 = 100;

/// Hard ceiling on the ring-buffer depth to bound storage costs.
pub const MAX_HISTORY_DEPTH: u32 = 50;
pub const DEFAULT_HISTORY_MAX_DEPTH: u32 = 10;
pub const MAX_BATCH_SIZE: u32 = 20;

/// Maximum number of entries accepted in a single batch score read call.
pub const BATCH_READ_MAX: u32 = 50;

/// Default risk threshold used when no threshold has been configured by admin.
pub const DEFAULT_RISK_THRESHOLD: u32 = 75;

/// Default threshold for score jump anomaly detection.
pub const DEFAULT_JUMP_THRESHOLD: u32 = 30;

/// Semantic contract version; bump on breaking ABI changes.
///
/// History:
///
/// * `1` — initial release (`submit_score` / `get_score`).
/// * `2` — `submit_score` gained the `attestation: Option<ScoreAttestation>`
///   parameter and `set_service_pubkey` / `get_service_pubkey` were added
///   (see `docs/attestation-spec.md`).
/// * `3` — `submit_scores_batch_attested` and the `batch_attested`
///   `supports_interface` capability were added (see
///   `docs/batch-attestation-spec.md`).
/// * `4` — Added contract_id and contract_version binding to attestations (#200),
///   Merkle audit chain for admin actions (#201), configurable decay profiles (#202),
///   and multi-dimensional risk scores with sub-components (#203).
pub const CONTRACT_VERSION: u32 = 4;

/// Hard upper bound on Merkle proof length.
pub const MAX_MERKLE_PROOF_DEPTH: u32 = 30;
pub const MAX_WALLET_PAIRS: u32 = 20;
pub const DEFAULT_COOLDOWN_SECS: u64 = 3_600;
pub const MIN_COOLDOWN_SECS: u64 = 60;
pub const MAX_COOLDOWN_SECS: u64 = 86_400;
pub const MIN_UPGRADE_DELAY_SECS: u64 = 172_800;
pub const MAX_UPGRADE_DELAY_SECS: u64 = 1_209_600;
pub const DEFAULT_UPGRADE_DELAY_SECS: u64 = 172_800;
pub const MAX_SERVICE_SIGNERS: u32 = 10;
pub const MAX_ADMIN_SIGNERS: u32 = 5;
pub const DEFAULT_STALENESS_WINDOW_SECS: u64 = 604_800;

/// Maximum age (seconds) of a secondary score for it to be accepted during
/// failover. Secondary scores older than this window cause the gate to
/// return `false` (fail-closed). Default: 1 hour.
pub const FAILOVER_STALENESS_WINDOW: u64 = 3_600;

pub const MAX_PAUSED_PAIRS: u32 = 50;
pub const DECAY_FIXED_POINT_SCALE: u64 = 1_000_000;
pub const DEFAULT_DECAY_LAMBDA_NUM: u32 = 0;
pub const DEFAULT_DECAY_LAMBDA_DEN: u32 = 1;
pub const MAX_DECAY_LAMBDA_NUM: u32 = 1;
pub const MAX_DECAY_LAMBDA_DEN: u32 = 1;

/// Maximum number of counterparty links allowed per wallet per asset pair.
pub const MAX_COUNTERPARTY_LINKS_PER_WALLET: u32 = 50;

/// Maximum delegation chain depth to prevent unbounded traversal.
/// Prevents DoS attacks via deep circular delegation chains.
pub const MAX_DELEGATION_DEPTH: u32 = 5;

// ── Score submission floor ─────────────────────────────────────────────────────

/// Default high-water mark for the score floor policy.
pub const DEFAULT_SCORE_FLOOR_HWM: u32 = 80;
pub const DEFAULT_SCORE_FLOOR_MIN: u32 = 20;
pub const MIN_SCORE_FLOOR_HWM: u32 = 50;
pub const MAX_SCORE_FLOOR_HWM: u32 = 100;
pub const MAX_HYSTERESIS_MARGIN: u32 = 50;
pub const BAND_STATE_TTL_THRESHOLD: u32 = 518_400;
pub const BAND_STATE_TTL_EXTEND_TO: u32 = 777_600;
pub const EMBARGO_TTL_THRESHOLD: u32 = 1_555_200;
pub const EMBARGO_TTL_EXTEND_TO: u32 = 3_110_400;

/// Hard ceiling on the `EmbargoedWalletIndex` so `revoke_all_embargoes` stays
/// within a single transaction's resource budget.
pub const MAX_EMBARGOED_WALLETS: u32 = 100;
pub const DEFAULT_CONSENSUS_THRESHOLD_K: u32 = 2;
pub const DEFAULT_CONSENSUS_EPSILON: u32 = 5;

// ── Escalation / consecutive breach ──────────────────────────────────────────

pub const ESCALATION_BREACH_TTL_THRESHOLD: u32 = 518_400;
pub const ESCALATION_BREACH_TTL_EXTEND_TO: u32 = 777_600;
pub const DEFAULT_ESCALATION_THRESHOLD: u32 = 5;

// ── Model version registry ────────────────────────────────────────────────────

/// Hard upper bound on the number of model versions that can be registered.
pub const MAX_MODEL_VERSIONS: u32 = 20;

/// Challenge period in seconds (7 days).
pub const DISPUTE_CHALLENGE_PERIOD_SECS: u64 = 604_800;

/// Bonus percentage added to the returned bond on timeout settlement.
pub const DISPUTE_BONUS_PCT: i128 = 10;

/// Maximum simultaneously open disputes.
pub const MAX_OPEN_DISPUTES: u32 = 100;

pub const DISPUTE_TTL_THRESHOLD: u32 = 518_400;
pub const DISPUTE_TTL_EXTEND_TO: u32 = 777_600;

/// Default reveal window for sealed-bid dispute bond: 10 minutes (600 seconds).
pub const DEFAULT_DISPUTE_REVEAL_WINDOW_SECS: u64 = 600;

// ── Finality buffer (pending score commit window) ────────────────────────────

/// Default heartbeat alert threshold — 1 hour.
pub const DEFAULT_HEARTBEAT_ALERT_THRESHOLD_SECS: u64 = 3_600;

// ── Quorum reduction ──────────────────────────────────────────────────────────

/// Default heartbeat alert threshold (seconds) until the admin configures
/// one explicitly via `set_heartbeat_alert_threshold` — 1 hour.
pub const DEFAULT_HEARTBEAT_ALERT_THRESHOLD_SECS: u64 = 3_600; // 1 hour

// ── Quorum / consensus ────────────────────────────────────────────────────────

/// Default window (seconds) for which a quorum-failure is considered recent.
/// After this window the failure state is cleared automatically.
pub const DEFAULT_QUORUM_FAILURE_WINDOW_SECS: u64 = 86_400; // 24 hours

pub const MAX_TRACKED_SCORE_ENTRIES: u32 = 500;
pub const MAX_EXPIRING_ENTRIES_PER_CALL: u32 = 100;

/// Maximum number of concurrently pending parameter-change proposals.
pub const MAX_PENDING_PARAMETER_PROPOSALS: u32 = 10;

