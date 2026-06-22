pub const SCORE_TTL_THRESHOLD: u32 = 518_400;
pub const SCORE_TTL_EXTEND_TO: u32 = 777_600;

pub const MAX_HISTORY_DEPTH: u32 = 50;
pub const DEFAULT_HISTORY_MAX_DEPTH: u32 = 10;
pub const MAX_BATCH_SIZE: u32 = 20;
pub const DEFAULT_RISK_THRESHOLD: u32 = 75;
pub const CONTRACT_VERSION: u32 = 3;
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
pub const MAX_COUNTERPARTY_LINKS_PER_WALLET: u32 = 50;
pub const DEFAULT_SCORE_FLOOR_HWM: u32 = 80;
pub const DEFAULT_SCORE_FLOOR_MIN: u32 = 20;
pub const MIN_SCORE_FLOOR_HWM: u32 = 50;
pub const MAX_SCORE_FLOOR_HWM: u32 = 100;
pub const MAX_HYSTERESIS_MARGIN: u32 = 50;
pub const BAND_STATE_TTL_THRESHOLD: u32 = 518_400;
pub const BAND_STATE_TTL_EXTEND_TO: u32 = 777_600;
pub const EMBARGO_TTL_THRESHOLD: u32 = 1_555_200;
pub const EMBARGO_TTL_EXTEND_TO: u32 = 3_110_400;
pub const DEFAULT_CONSENSUS_THRESHOLD_K: u32 = 2;
pub const DEFAULT_CONSENSUS_EPSILON: u32 = 5;

// ── Score jump anomaly detection ─────────────────────────────────────────────

pub const DEFAULT_JUMP_THRESHOLD: u32 = 30;

// ── Escalation / consecutive breach ──────────────────────────────────────────

pub const ESCALATION_BREACH_TTL_THRESHOLD: u32 = 518_400;
pub const ESCALATION_BREACH_TTL_EXTEND_TO: u32 = 777_600;
pub const DEFAULT_ESCALATION_THRESHOLD: u32 = 5;
pub const MIN_ESCALATION_THRESHOLD: u32 = 1;
pub const MAX_ESCALATION_THRESHOLD: u32 = 100;

// ── Model stats ─────────────────────────────────────────────────────────────

pub const MODEL_STATS_TTL_THRESHOLD: u32 = 518_400;
pub const MODEL_STATS_TTL_EXTEND_TO: u32 = 777_600;
