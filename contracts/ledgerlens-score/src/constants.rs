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
/// The admin cannot configure a depth above this value.
pub const MAX_HISTORY_DEPTH: u32 = 50;
pub const DEFAULT_HISTORY_MAX_DEPTH: u32 = 10;
pub const MAX_BATCH_SIZE: u32 = 20;

/// Maximum number of entries accepted in a single batch score read call.
pub const BATCH_READ_MAX: u32 = 50;

/// Default risk threshold used when no threshold has been configured by admin.
pub const DEFAULT_RISK_THRESHOLD: u32 = 75;

/// Default threshold for score jump anomaly detection, used when no
/// threshold has been configured by admin. When the absolute delta
/// between consecutive scores exceeds this value, a jump event is emitted.
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

/// Hard upper bound on Merkle proof length accepted by
/// `submit_scores_batch_attested`. Thirty levels of a binary tree can
/// accommodate up to 2^30 ≈ 1.07 billion leaves — well above the
/// `MAX_BATCH_SIZE` of 20 today, but large enough that the field cannot be
/// exploited as an unbounded loop budget. Beyond this, the contract
/// rejects the call with `Error::InvalidAttestation` (see
/// `docs/batch-attestation-spec.md` for the rationale).
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
pub const MAX_PAUSED_PAIRS: u32 = 50;
pub const DECAY_FIXED_POINT_SCALE: u64 = 1_000_000;
pub const DEFAULT_DECAY_LAMBDA_NUM: u32 = 0;
pub const DEFAULT_DECAY_LAMBDA_DEN: u32 = 1;
pub const MAX_DECAY_LAMBDA_NUM: u32 = 1;
pub const MAX_DECAY_LAMBDA_DEN: u32 = 1;

/// Minimum configurable escalation threshold (consecutive breaches).
pub const MIN_ESCALATION_THRESHOLD: u32 = 1;
/// Maximum configurable escalation threshold.
pub const MAX_ESCALATION_THRESHOLD: u32 = 100;

// ── Wallet Relationship Graph ───────────────────────────────────────────────

/// Maximum number of counterparty links allowed per wallet per asset pair.
/// Prevents unbounded storage growth and gas exhaustion.
pub const MAX_COUNTERPARTY_LINKS_PER_WALLET: u32 = 50;

/// Maximum delegation chain depth to prevent unbounded traversal.
/// Prevents DoS attacks via deep circular delegation chains.
pub const MAX_DELEGATION_DEPTH: u32 = 5;

// ── Score submission floor ─────────────────────────────────────────────────────
//
// A compromised or colluding signer could otherwise submit an artificially low
// score for a wallet that has historically carried a high risk score, laundering
// its on-chain reputation. The configurable floor blocks sudden large downward
// revisions for wallets whose historical peak crossed a danger level. See
// `set_score_floor_policy` and the README's Score Submission Floor section.

/// Default high-water mark used until the admin configures the policy — a
/// `(wallet, asset_pair)` whose historical peak reached this score is treated
/// as high-risk and subject to the floor.
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

// ── Score dispute mechanism ─────────────────────────────────────────────────────
//
// A wallet operator can stake the fee token to challenge a risk score it
// believes is erroneous. Opening a dispute starts a challenge period during
// which the admin must resubmit a corrected score (resolving the dispute and
// returning the stake). If the admin does not act before the deadline, anyone
// may settle the dispute in the challenger's favour: the stake is returned with
// a bonus drawn from the contract's accumulated fee reserve.

/// Challenge period, in seconds, that the admin has to resubmit a corrected
/// score before a dispute can be settled by timeout. Default: 7 days.
pub const DISPUTE_CHALLENGE_PERIOD_SECS: u64 = 604_800;

/// Bonus percentage added to the returned bond when a dispute is settled by
/// timeout (e.g. `10` = 10%). The bonus is paid from the contract's fee
/// reserve, compensating the challenger for a score the admin failed to correct.
pub const DISPUTE_BONUS_PCT: i128 = 10;

/// Upper bound on the number of simultaneously open disputes tracked in the
/// dispute index, preventing unbounded growth of the index vector.
pub const MAX_OPEN_DISPUTES: u32 = 100;

/// Re-extend dispute TTL when remaining lifetime falls below this many ledgers
/// (~30 days at 5 s/ledger). Comfortably outlives the challenge period.
pub const DISPUTE_TTL_THRESHOLD: u32 = 518_400;

/// Target TTL for dispute entries on creation or refresh (~45 days at 5 s/ledger).
pub const DISPUTE_TTL_EXTEND_TO: u32 = 777_600;

/// Default reveal window for sealed-bid dispute bond: 10 minutes (600 seconds).
pub const DEFAULT_DISPUTE_REVEAL_WINDOW_SECS: u64 = 600;

// ── Finality buffer (pending score commit window) ────────────────────────────

/// Maximum configurable finality buffer — 24 hour ceiling, so a misconfigured
/// admin cannot delay score visibility indefinitely.
pub const MAX_FINALITY_BUFFER_SECS: u64 = 86_400; // 24 hours

// ── Service heartbeat monitor ─────────────────────────────────────────────
//
// If the off-chain scoring service goes down, on-chain scores silently age
// without update, and composable protocols have no global signal to
// distinguish a healthy wallet from a stale one. `LastServiceActivityAt`
// tracks the most recent accepted submission (or `ping_heartbeat`); see
// `is_service_alive`, `ping_heartbeat`, and `set_heartbeat_alert_threshold`.

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

