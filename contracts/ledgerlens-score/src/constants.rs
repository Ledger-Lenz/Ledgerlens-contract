#![cfg_attr(target_family = "wasm", allow(dead_code))]

// ── Numeric cast lint gate (issue #1184) ─────────────────────────────────────
//
// `as` casts silently truncate, wrap, or lose sign/precision. In score, fee,
// time and weight arithmetic that is a correctness (and sometimes security)
// defect. The cast lints below are denied for this contract crate so any new
// narrowing cast must be replaced with a checked conversion or an explicit,
// commented, proven-safe cast. Tooling crates are handled separately and are
// not covered by this module-level gate.
#![deny(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

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
pub const MAX_SCORE_CHANGE_HOOKS_PER_PAIR: u32 = 4;
pub const MAX_SCORE_CHANGE_HOOKS_GLOBAL: u32 = 32;
pub const MAX_SCORE_CHANGE_HOOKS_PER_DISPATCH: u32 = 1;
pub const DEFAULT_SCORE_HOOK_FAILURE_THRESHOLD: u32 = 3;
pub const MAX_SCORE_HOOK_FAILURE_THRESHOLD: u32 = 10;
pub const MAX_ASSET_PAIR_BYTES: u32 = 9;
pub const MAX_SCORE_COMMITMENT_BYTES: u32 = 32;
pub const MAX_DISPUTE_BOND_PREIMAGE_BYTES: u32 = 80;
pub const MAX_DISPUTE_BOND_SALT_BYTES: u32 = 64;

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
/// * `5` — Added post-incident replay and reconciliation workflow (#631): emergency
///   freeze/unfreeze, deterministic state checksums, paginated score export, on-chain
///   snapshot history, reconciliation verification, and off-chain recovery tooling.
pub const CONTRACT_VERSION: u32 = 5;

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
/// Grace period (seconds) before a newly added signer transitions from Pending to Active (issue #691).
pub const DEFAULT_SIGNER_GRACE_PERIOD_SECS: u64 = 3_600; // 1 hour
pub const DEFAULT_STALENESS_WINDOW_SECS: u64 = 604_800;

/// Default maximum age (seconds) of an oracle's last price update before the
/// oracle is considered stale and `get_effective_score` falls back to
/// unadjusted confidence.  Default: 1 hour, matching `FAILOVER_STALENESS_WINDOW`.
/// Configurable via `set_oracle_staleness_threshold` / `get_oracle_staleness_threshold`.
pub const DEFAULT_ORACLE_STALENESS_THRESHOLD_SECS: u64 = 3_600;

/// Maximum age (seconds) of a secondary score for it to be accepted during
/// failover. Secondary scores older than this window cause the gate to
/// return `false` (fail-closed). Default: 1 hour.
pub const FAILOVER_STALENESS_WINDOW: u64 = 3_600;

pub const MAX_PAUSED_PAIRS: u32 = 50;
pub const DECAY_FIXED_POINT_SCALE: u64 = 1_000_000;
pub const DEFAULT_DECAY_LAMBDA_NUM: u64 = 0;
pub const DEFAULT_DECAY_LAMBDA_DEN: u64 = 1;
pub const MAX_DECAY_LAMBDA_NUM: u64 = 1;
pub const MAX_DECAY_LAMBDA_DEN: u64 = 1;

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
pub const MIN_ESCALATION_THRESHOLD: u32 = 2;
pub const MAX_ESCALATION_THRESHOLD: u32 = 20;

// ── Model version registry ────────────────────────────────────────────────────

/// Hard upper bound on the number of model versions that can be registered.
pub const MAX_MODEL_VERSIONS: u32 = 20;

/// Challenge period in seconds (7 days).
pub const DISPUTE_CHALLENGE_PERIOD_SECS: u64 = 604_800;

/// Bonus percentage added to the returned bond on timeout settlement.
pub const DISPUTE_BONUS_PCT: i128 = 10;

/// Maximum simultaneously open disputes.
pub const MAX_OPEN_DISPUTES: u32 = 100;

/// Maximum concurrently open disputes allowed per single challenger actor.
pub const MAX_DISPUTES_PER_ACTOR: u32 = 5;

pub const DISPUTE_TTL_THRESHOLD: u32 = 518_400;
pub const DISPUTE_TTL_EXTEND_TO: u32 = 777_600;

/// Default reveal window for sealed-bid dispute bond: 10 minutes (600 seconds).
pub const DEFAULT_DISPUTE_REVEAL_WINDOW_SECS: u64 = 600;

// ── Finality buffer (pending score commit window) ────────────────────────────

/// Maximum finality buffer duration — 24 hours.
pub const MAX_FINALITY_BUFFER_SECS: u64 = 86_400;

/// Default heartbeat alert threshold — 1 hour.
pub const DEFAULT_HEARTBEAT_ALERT_THRESHOLD_SECS: u64 = 3_600;

/// Maximum overlap window accepted by a service-key rotation — 24 hours.
///
/// The overlap exists to bound how long a *retired* key still signs
/// attestations, so an unbounded value is a security setting, not a
/// convenience: `u64::MAX` would make the retired key acceptable forever.
/// Issue #1246.
pub const MAX_KEY_OVERLAP_SECS: u64 = 86_400;

/// Maximum reveal window accepted for a sealed-bid consensus commitment —
/// 7 days.
///
/// The window is stored in seconds but the commitment's temporary entry is
/// kept alive in *ledgers*, and the conversion narrows to u32; a window above
/// ~2^32 s truncated to 0 and collapsed the entry to its 12-ledger floor,
/// making the reveal permanently impossible. Issue #1246.
pub const MAX_REVEAL_WINDOW_SECS: u64 = 604_800;

// ── HyperLogLog unique-wallet estimation ─────────────────────────────────────

/// Minimum allowed HLL precision (2^4 = 16 registers).
pub const HLL_MIN_PRECISION: u32 = 4;
/// Maximum allowed HLL precision (2^16 = 65536 registers).
pub const HLL_MAX_PRECISION: u32 = 16;
/// Default HLL precision until the admin configures one explicitly.
pub const HLL_DEFAULT_PRECISION: u32 = 8;

// ── Quorum reduction ──────────────────────────────────────────────────────────

// ── Quorum / consensus ────────────────────────────────────────────────────────

/// Default window (seconds) for which a quorum-failure is considered recent.
/// After this window the failure state is cleared automatically.
pub const DEFAULT_QUORUM_FAILURE_WINDOW_SECS: u64 = 86_400; // 24 hours

pub const MAX_TRACKED_SCORE_ENTRIES: u32 = 500;
pub const MAX_EXPIRING_ENTRIES_PER_CALL: u32 = 100;

// ── Permissionless keeper TTL-extension reward (rent-griefing follow-up) ────
//
// See `docs/rent-griefing-analysis.md`. Keepers batch-call
// `keeper_extend_entry_ttls` to renew dormant score entries and are paid a
// small, governance-configured reward per entry actually renewed, funded
// from a dedicated pool (never user credits). Eligibility reuses the same
// conservative TTL estimate as `get_expiring_entries`/`extend_entry_ttls`, so
// a keeper can only be paid for entries genuinely close to expiry, and
// renewing an entry resets its estimated remaining TTL back to
// `SCORE_TTL_THRESHOLD` — far outside the reward window — which is what
// makes repeat/split-work farming of the same entry unprofitable.

/// Batch size cap for `keeper_extend_entry_ttls`. Reuses the existing
/// `get_expiring_entries`/`extend_entry_ttls` cap so keeper tooling can feed
/// `get_expiring_entries`'s output straight into the keeper call.
pub const KEEPER_BATCH_MAX: u32 = MAX_EXPIRING_ENTRIES_PER_CALL;

/// Default reward-eligibility window (ledgers of estimated remaining TTL at
/// or below which an entry qualifies for a keeper reward). `0` means "only
/// entries already at/past `SCORE_TTL_THRESHOLD`", matching
/// `get_expiring_entries`'s existing "due" definition. Admin/governance may
/// widen this via `set_keeper_reward_params` up to `SCORE_TTL_THRESHOLD` to
/// pay keepers for renewing somewhat earlier.
pub const DEFAULT_KEEPER_REWARD_WINDOW: u32 = 0;

/// Hard ceiling on the per-entry keeper reward, regardless of what
/// governance configures — bounds worst-case pool drain per call even if a
/// governance parameter is mis-set.
pub const MAX_KEEPER_REWARD_PER_ENTRY: i128 = 1_000_000_000;

// ── Tiered gate fee schedule ──────────────────────────────────────────────
//
// See `docs/operations/runbook.md`. Bounds the fee-tier schedule's storage
// footprint (a single `Vec<FeeTier>` read/written whole, so it must stay
// small) and each tier's fee amount, independent of what governance sets.

/// Maximum number of tiers `set_fee_tier_schedule` accepts.
pub const MAX_FEE_TIERS: u32 = 10;

/// Hard ceiling on any single tier's flat per-call fee.
pub const MAX_GATE_FEE_TIER: i128 = 1_000_000_000;

/// Default rolling-window length (ledgers) for per-consumer call-volume
/// accounting — roughly 1 day at Stellar's ~5s average ledger close time.
pub const DEFAULT_FEE_TIER_WINDOW_LEDGERS: u32 = 17_280;

/// Maximum number of concurrently pending parameter-change proposals.
pub const MAX_PENDING_PARAMETER_PROPOSALS: u32 = 10;

/// Time-lock delay before a pending simple parameter change may be applied.
pub const DEFAULT_PARAM_CHANGE_DELAY_SECS: u64 = 86_400;

/// Maximum number of entries retained in the rate-limit override audit log.
pub const MAX_RATE_LIMIT_OVERRIDE_LOG: u32 = 100;

/// Operator-facing manifest fields that the drift checker treats as the
/// stable configuration surface for deployed instances.
pub const CONFIG_DRIFT_MANIFEST_FIELDS: &[&str] = &[
    "contract_version",
    "min_score",
    "max_score",
    "max_history_depth",
    "default_history_max_depth",
    "max_batch_size",
    "max_asset_pair_bytes",
    "max_score_commitment_bytes",
    "max_dispute_bond_preimage_bytes",
    "max_dispute_bond_salt_bytes",
    "batch_read_max",
    "default_risk_threshold",
    "default_jump_threshold",
    "max_gate_callers",
    "max_wallet_pairs",
    "max_service_signers",
    "max_admin_signers",
    "max_paused_pairs",
    "max_counterparty_links_per_wallet",
    "max_delegation_depth",
    "max_model_versions",
    "max_open_disputes",
    "max_disputes_per_actor",
    "max_embargoed_wallets",
    "max_tracked_score_entries",
    "max_expiring_entries_per_call",
    "max_pending_parameter_proposals",
    "max_rate_limit_override_log",
];

// ── Boundary tests for the numeric cast lint gate (issue #1184) ──────────────
//
// These tests pin the type limits of the constants above so that any future
// narrowing cast introduced around them is caught at the boundary values.
// They also document the approved conversion patterns: use `u32::try_from` /
// `i128::try_from` (or `checked_*`) instead of `as`.
#[cfg(test)]
mod cast_boundary_tests {
    use super::*;

    /// `MAX_SCORE` must fit in `u8` so score arithmetic can use `u8` safely.
    #[test]
    fn max_score_fits_in_u8() {
        assert_eq!(u8::try_from(MAX_SCORE), Ok(100));
        assert_eq!(u8::try_from(MIN_SCORE), Ok(0));
    }

    /// Score-floor bounds must fit in `u8`.
    #[test]
    fn score_floor_bounds_fit_in_u8() {
        assert_eq!(u8::try_from(MAX_SCORE_FLOOR_HWM), Ok(100));
        assert_eq!(u8::try_from(MIN_SCORE_FLOOR_HWM), Ok(50));
        assert_eq!(u8::try_from(DEFAULT_SCORE_FLOOR_HWM), Ok(80));
        assert_eq!(u8::try_from(DEFAULT_SCORE_FLOOR_MIN), Ok(20));
    }

    /// HLL precision bounds must fit in `u8` (register count is `1 << p`).
    #[test]
    fn hll_precision_bounds_fit_in_u8() {
        assert_eq!(u8::try_from(HLL_MIN_PRECISION), Ok(4));
        assert_eq!(u8::try_from(HLL_MAX_PRECISION), Ok(16));
        assert_eq!(u8::try_from(HLL_DEFAULT_PRECISION), Ok(8));
    }

    /// `u32` TTL constants must widen losslessly into `u64`.
    #[test]
    fn ttl_constants_widen_to_u64() {
        assert_eq!(u64::from(SCORE_TTL_THRESHOLD), 518_400);
        assert_eq!(u64::from(SCORE_TTL_EXTEND_TO), 777_600);
        assert_eq!(u64::from(BAND_STATE_TTL_THRESHOLD), 518_400);
        assert_eq!(u64::from(BAND_STATE_TTL_EXTEND_TO), 777_600);
        assert_eq!(u64::from(EMBARGO_TTL_THRESHOLD), 1_555_200);
        assert_eq!(u64::from(EMBARGO_TTL_EXTEND_TO), 3_110_400);
    }

    /// `u64` second-based constants must fit in `i64` (ledger timestamps).
    #[test]
    fn second_constants_fit_in_i64() {
        assert_eq!(i64::try_from(DEFAULT_COOLDOWN_SECS), Ok(3_600));
        assert_eq!(i64::try_from(MAX_COOLDOWN_SECS), Ok(86_400));
        assert_eq!(i64::try_from(MAX_UPGRADE_DELAY_SECS), Ok(1_209_600));
        assert_eq!(i64::try_from(DISPUTE_CHALLENGE_PERIOD_SECS), Ok(604_800));
        assert_eq!(i64::try_from(MAX_FINALITY_BUFFER_SECS), Ok(86_400));
        assert_eq!(i64::try_from(DEFAULT_QUORUM_FAILURE_WINDOW_SECS), Ok(86_400));
    }

    /// `DISPUTE_BONUS_PCT` is a signed percentage and must fit in `i64`.
    #[test]
    fn dispute_bonus_pct_fits_in_i64() {
        assert_eq!(i64::try_from(DISPUTE_BONUS_PCT), Ok(10));
    }

    /// Count-style `u32` limits must fit in `usize` on 32-bit targets.
    #[test]
    fn count_limits_fit_in_usize() {
        assert_eq!(usize::try_from(MAX_GATE_CALLERS), Ok(20));
        assert_eq!(usize::try_from(MAX_HISTORY_DEPTH), Ok(50));
        assert_eq!(usize::try_from(MAX_BATCH_SIZE), Ok(20));
        assert_eq!(usize::try_from(BATCH_READ_MAX), Ok(50));
        assert_eq!(usize::try_from(MAX_EMBARGOED_WALLETS), Ok(100));
        assert_eq!(usize::try_from(MAX_TRACKED_SCORE_ENTRIES), Ok(500));
    }
}
