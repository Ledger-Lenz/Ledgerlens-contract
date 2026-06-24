use crate::constants::{
    BAND_STATE_TTL_EXTEND_TO, BAND_STATE_TTL_THRESHOLD, DEFAULT_CONSENSUS_EPSILON,
    DEFAULT_CONSENSUS_THRESHOLD_K, DEFAULT_COOLDOWN_SECS, DEFAULT_JUMP_THRESHOLD,
    DEFAULT_QUORUM_FAILURE_WINDOW_SECS, DEFAULT_RISK_THRESHOLD, DEFAULT_UPGRADE_DELAY_SECS,
    EMBARGO_TTL_EXTEND_TO, EMBARGO_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO, SCORE_TTL_THRESHOLD,
};
use crate::errors::Error;
use crate::types::{
    AggregateRiskScore, DataKey, EmbargoExpiry, GateDataKey, ModelVersionStats, PendingScoreEntry,
    RiskScore, ScoreDispute, ScoreFloorPolicy, ScoreHistogram, ScoreTrend, ScoreVelocityCap,
    UpgradeProposal,
};
use soroban_sdk::{Address, Bytes, Env, Symbol, Vec};

// ── Admin / Service ─────────────────────────────────────────────────────────

pub fn has_admin(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::Admin)
}

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

pub fn get_admin(env: &Env) -> Address {
    env.storage().instance().get(&DataKey::Admin).unwrap()
}

pub fn set_service(env: &Env, service: &Address) {
    env.storage().instance().set(&DataKey::Service, service);
}

pub fn get_service(env: &Env) -> Address {
    env.storage().instance().get(&DataKey::Service).unwrap()
}

// ── Latest score ─────────────────────────────────────────────────────────────

pub fn set_score(env: &Env, wallet: &Address, asset_pair: &Symbol, score: &RiskScore) {
    let key = DataKey::Score(wallet.clone(), asset_pair.clone());
    env.storage().persistent().set(&key, score);
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

pub fn get_score(env: &Env, wallet: &Address, asset_pair: &Symbol) -> Option<RiskScore> {
    let key = DataKey::Score(wallet.clone(), asset_pair.clone());
    let score: Option<RiskScore> = env.storage().persistent().get(&key);
    if score.is_some() {
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }
    score
}

pub fn peek_score(env: &Env, wallet: &Address, asset_pair: &Symbol) -> Option<RiskScore> {
    let key = DataKey::Score(wallet.clone(), asset_pair.clone());
    env.storage().persistent().get(&key)
}

// ── Pause circuit breaker ────────────────────────────────────────────────────

pub fn is_paused(env: &Env) -> bool {
    let result: Option<bool> = env.storage().instance().get(&DataKey::Paused);
    result.unwrap_or(false)
}

pub fn set_paused(env: &Env, paused: bool) {
    env.storage().instance().set(&DataKey::Paused, &paused);
}

// ── Per-asset-pair circuit breaker ───────────────────────────────────────────

pub fn is_pair_paused(env: &Env, asset_pair: &Symbol) -> bool {
    let key = DataKey::PairPaused(asset_pair.clone());
    let result: Option<bool> = env.storage().persistent().get(&key);
    if result.is_some() {
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }
    result.unwrap_or(false)
}

pub fn set_pair_paused_flag(env: &Env, asset_pair: &Symbol, paused: bool) {
    let key = DataKey::PairPaused(asset_pair.clone());
    if paused {
        env.storage().persistent().set(&key, &true);
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    } else {
        env.storage().persistent().remove(&key);
    }
}

pub fn get_paused_pairs(env: &Env) -> Vec<Symbol> {
    let pairs: Vec<Symbol> =
        env.storage().persistent().get(&DataKey::PausedPairIndex).unwrap_or_else(|| Vec::new(env));
    if !pairs.is_empty() {
        env.storage().persistent().extend_ttl(
            &DataKey::PausedPairIndex,
            SCORE_TTL_THRESHOLD,
            SCORE_TTL_EXTEND_TO,
        );
    }
    pairs
}

pub fn add_to_paused_index(env: &Env, asset_pair: &Symbol) -> bool {
    let mut pairs = get_paused_pairs(env);
    if pairs.contains(asset_pair) {
        return true;
    }
    if pairs.len() >= crate::constants::MAX_PAUSED_PAIRS {
        return false;
    }
    pairs.push_back(asset_pair.clone());
    env.storage().persistent().set(&DataKey::PausedPairIndex, &pairs);
    env.storage().persistent().extend_ttl(
        &DataKey::PausedPairIndex,
        SCORE_TTL_THRESHOLD,
        SCORE_TTL_EXTEND_TO,
    );
    true
}

pub fn remove_from_paused_index(env: &Env, asset_pair: &Symbol) {
    let mut pairs = get_paused_pairs(env);
    if let Some(idx) = pairs.first_index_of(asset_pair) {
        pairs.remove(idx);
        env.storage().persistent().set(&DataKey::PausedPairIndex, &pairs);
    }
}

// ── Two-step admin transfer ──────────────────────────────────────────────────

pub fn has_pending_admin(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::PendingAdmin)
}

pub fn set_pending_admin(env: &Env, new_admin: &Address) {
    env.storage().instance().set(&DataKey::PendingAdmin, new_admin);
}

pub fn get_pending_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::PendingAdmin)
}

pub fn clear_pending_admin(env: &Env) {
    env.storage().instance().remove(&DataKey::PendingAdmin);
}

// ── Watchlist ────────────────────────────────────────────────────────────────

pub fn is_watchlisted(env: &Env, wallet: &Address) -> bool {
    let result: Option<bool> = env.storage().persistent().get(&DataKey::Watchlist(wallet.clone()));
    result.unwrap_or(false)
}

pub fn set_watchlist(env: &Env, wallet: &Address, flagged: bool) {
    let key = DataKey::Watchlist(wallet.clone());
    if flagged {
        env.storage().persistent().set(&key, &true);
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    } else {
        env.storage().persistent().remove(&key);
    }
}

// ── Risk threshold ───────────────────────────────────────────────────────────

pub fn get_risk_threshold(env: &Env) -> u32 {
    let result: Option<u32> = env.storage().instance().get(&DataKey::RiskThreshold);
    result.unwrap_or(DEFAULT_RISK_THRESHOLD)
}

pub fn set_risk_threshold(env: &Env, threshold: u32) {
    env.storage().instance().set(&DataKey::RiskThreshold, &threshold);
}

// ── Score jump anomaly detection ──────────────────────────────────────────────

pub fn get_jump_threshold(env: &Env) -> u32 {
    let result: Option<u32> = env.storage().instance().get(&DataKey::JumpThreshold);
    result.unwrap_or(DEFAULT_JUMP_THRESHOLD)
}

pub fn set_jump_threshold(env: &Env, threshold: u32) {
    env.storage().instance().set(&DataKey::JumpThreshold, &threshold);
}

// ── Score history ring buffer ────────────────────────────────────────────────

pub fn push_score_history(env: &Env, wallet: &Address, asset_pair: &Symbol, score: &RiskScore) {
    let key = DataKey::ScoreHistory(wallet.clone(), asset_pair.clone());
    let mut history: Vec<RiskScore> =
        env.storage().persistent().get(&key).unwrap_or_else(|| Vec::new(env));

    history.push_back(score.clone());

    let depth = get_history_max_depth(env);
    while history.len() > depth {
        history.remove(0);
    }

    env.storage().persistent().set(&key, &history);
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

pub fn get_score_history(env: &Env, wallet: &Address, asset_pair: &Symbol) -> Vec<RiskScore> {
    let key = DataKey::ScoreHistory(wallet.clone(), asset_pair.clone());
    let history: Vec<RiskScore> =
        env.storage().persistent().get(&key).unwrap_or_else(|| Vec::new(env));
    if !history.is_empty() {
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }
    history
}

/// Read-only windowed view into the score-history ring buffer.
///
/// `offset` is 0-indexed from the **most recent** entry (offset `0` == newest);
/// at most `limit` entries are returned, ordered most-recent first. `limit` is
/// clamped to [`MAX_HISTORY_DEPTH`](crate::constants::MAX_HISTORY_DEPTH). An
/// `offset` at or beyond the current history length yields an empty `Vec`.
///
/// The whole ring entry is a single persistent value, so the read cost is the
/// same as [`get_score_history`]; the saving is purely in the size of the
/// returned slice. This function never mutates the ring (it only refreshes the
/// entry TTL, exactly as `get_score_history` does).
pub fn get_score_history_paginated(
    env: &Env,
    wallet: &Address,
    asset_pair: &Symbol,
    offset: u32,
    limit: u32,
) -> Vec<RiskScore> {
    let key = DataKey::ScoreHistory(wallet.clone(), asset_pair.clone());
    let history: Vec<RiskScore> =
        env.storage().persistent().get(&key).unwrap_or_else(|| Vec::new(env));

    let mut page = Vec::new(env);
    let len = history.len();
    // Out-of-bounds offset (including any read against an empty ring) is not an
    // error — callers paging off the end simply get nothing back.
    if offset >= len {
        return page;
    }

    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);

    let capped_limit = limit.min(crate::constants::MAX_HISTORY_DEPTH);
    // History is stored oldest-first, so the newest entry sits at `len - 1`.
    // Walk backwards from the `offset`-th most recent entry, emitting up to
    // `capped_limit` entries in most-recent-first order.
    let mut idx = len - 1 - offset;
    let mut produced = 0u32;
    while produced < capped_limit {
        page.push_back(history.get(idx).unwrap());
        produced += 1;
        if idx == 0 {
            break;
        }
        idx -= 1;
    }
    page
}

// ── Configurable history ring depth ──────────────────────────────────────────

pub fn get_history_max_depth(env: &Env) -> u32 {
    let result: Option<u32> = env.storage().instance().get(&DataKey::HistoryMaxDepth);
    result.unwrap_or(crate::constants::DEFAULT_HISTORY_MAX_DEPTH)
}

pub fn set_history_max_depth(env: &Env, depth: u32) {
    env.storage().instance().set(&DataKey::HistoryMaxDepth, &depth);
}

// ── Contract version ─────────────────────────────────────────────────────────

pub fn get_contract_version(env: &Env) -> u32 {
    let result: Option<u32> = env.storage().instance().get(&DataKey::ContractVersion);
    result.unwrap_or(crate::constants::CONTRACT_VERSION)
}

// ── Cross-asset aggregate risk ───────────────────────────────────────────────

pub fn register_pair_for_wallet(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let key = DataKey::AssetPairs(wallet.clone());
    let mut pairs: Vec<Symbol> =
        env.storage().persistent().get(&key).unwrap_or_else(|| Vec::new(env));

    if !pairs.contains(asset_pair) {
        pairs.push_back(asset_pair.clone());
        env.storage().persistent().set(&key, &pairs);
    }
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

pub fn get_wallet_pairs(env: &Env, wallet: &Address) -> Vec<Symbol> {
    let key = DataKey::AssetPairs(wallet.clone());
    let pairs: Vec<Symbol> = env.storage().persistent().get(&key).unwrap_or_else(|| Vec::new(env));
    if !pairs.is_empty() {
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }
    pairs
}

pub fn get_pair_weight(env: &Env, asset_pair: &Symbol) -> u32 {
    let key = DataKey::PairWeight(asset_pair.clone());
    let weight: Option<u32> = env.storage().persistent().get(&key);
    if weight.is_some() {
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }
    weight.unwrap_or(1)
}

pub fn set_pair_weight(env: &Env, asset_pair: &Symbol, weight: u32) {
    let key = DataKey::PairWeight(asset_pair.clone());
    env.storage().persistent().set(&key, &weight);
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

pub fn set_aggregate_score(env: &Env, wallet: &Address, aggregate: &AggregateRiskScore) {
    let key = DataKey::AggregateScore(wallet.clone());
    env.storage().persistent().set(&key, aggregate);
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

// ── Time-locked upgrade governance ────────────────────────────────────────────

pub fn has_pending_upgrade(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::PendingUpgrade)
}

pub fn set_pending_upgrade(env: &Env, proposal: &UpgradeProposal) {
    env.storage().instance().set(&DataKey::PendingUpgrade, proposal);
}

pub fn get_pending_upgrade(env: &Env) -> Option<UpgradeProposal> {
    env.storage().instance().get(&DataKey::PendingUpgrade)
}

pub fn clear_pending_upgrade(env: &Env) {
    env.storage().instance().remove(&DataKey::PendingUpgrade);
}

pub fn get_upgrade_delay(env: &Env) -> u64 {
    env.storage().instance().get(&DataKey::UpgradeDelay).unwrap_or(DEFAULT_UPGRADE_DELAY_SECS)
}

pub fn set_upgrade_delay(env: &Env, delay_secs: u64) {
    env.storage().instance().set(&DataKey::UpgradeDelay, &delay_secs);
}

// ── Multi-sig admin set ──────────────────────────────────────────────────────

pub fn get_admin_set(env: &Env) -> Vec<Address> {
    env.storage().instance().get(&DataKey::AdminSet).unwrap_or_else(|| Vec::new(env))
}

pub fn set_admin_set(env: &Env, set: &Vec<Address>) {
    env.storage().instance().set(&DataKey::AdminSet, set);
}

pub fn get_admin_threshold(env: &Env) -> u32 {
    env.storage().instance().get(&DataKey::AdminThreshold).unwrap_or(0)
}

pub fn set_admin_threshold(env: &Env, threshold: u32) {
    env.storage().instance().set(&DataKey::AdminThreshold, &threshold);
}

// ── Multi-sig service set ─────────────────────────────────────────────────────

pub fn get_service_set(env: &Env) -> Vec<Address> {
    env.storage().instance().get(&DataKey::ServiceSet).unwrap_or_else(|| Vec::new(env))
}

pub fn set_service_set(env: &Env, set: &Vec<Address>) {
    env.storage().instance().set(&DataKey::ServiceSet, set);
}

pub fn get_signer_tier(env: &Env, signer: &Address) -> crate::types::TierBounds {
    env.storage()
        .instance()
        .get(&DataKey::SignerTier(signer.clone()))
        .unwrap_or(crate::types::TierBounds { min_score: 0, max_score: 100 })
}

pub fn set_service_threshold(env: &Env, threshold: u32) {
    env.storage().instance().set(&DataKey::ServiceThreshold, &threshold);
}

pub fn get_service_threshold(env: &Env) -> u32 {
    env.storage().instance().get(&DataKey::ServiceThreshold).unwrap_or(1)
}

// ── Escalation / breach count ─────────────────────────────────────────────────

pub fn get_escalation_threshold(env: &Env) -> u32 {
    env.storage().instance().get(&DataKey::EscalationThreshold).unwrap_or(3)
}

pub fn set_escalation_threshold(env: &Env, n: u32) {
    env.storage().instance().set(&DataKey::EscalationThreshold, &n);
}

pub fn get_breach_count(env: &Env, wallet: &Address, asset_pair: &Symbol) -> u32 {
    let key = DataKey::BreachCount(wallet.clone(), asset_pair.clone());
    env.storage().temporary().get(&key).unwrap_or(0)
}

pub fn set_breach_count(env: &Env, wallet: &Address, asset_pair: &Symbol, count: u32) {
    let key = DataKey::BreachCount(wallet.clone(), asset_pair.clone());
    env.storage().temporary().set(&key, &count);
}

pub fn clear_breach_count(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let key = DataKey::BreachCount(wallet.clone(), asset_pair.clone());
    env.storage().temporary().remove(&key);
}

// ── Model stats ───────────────────────────────────────────────────────────────

pub fn update_model_stats(env: &Env, model_version: u32, score: u32) {
    let key = DataKey::ModelStats(model_version);
    let mut stats: ModelVersionStats = env
        .storage()
        .instance()
        .get(&key)
        .unwrap_or(ModelVersionStats { model_version, submission_count: 0, score_sum: 0 });
    stats.submission_count += 1;
    stats.score_sum += score as u64;
    env.storage().instance().set(&key, &stats);
}

pub fn get_model_stats(env: &Env, model_version: u32) -> Option<ModelVersionStats> {
    env.storage().instance().get(&DataKey::ModelStats(model_version))
}

pub fn get_all_model_versions(env: &Env) -> Vec<u32> {
    env.storage().instance().get(&DataKey::AllModelVersions).unwrap_or_else(|| Vec::new(env))
}

// ── Staleness window ──────────────────────────────────────────────────────────

pub fn get_staleness_window(env: &Env) -> u64 {
    let result: Option<u64> = env.storage().instance().get(&DataKey::StalenessWindow);
    result.unwrap_or(crate::constants::DEFAULT_STALENESS_WINDOW_SECS)
}

pub fn set_staleness_window(env: &Env, window_secs: u64) {
    env.storage().instance().set(&DataKey::StalenessWindow, &window_secs);
}

// ── Per-wallet/pair submission rate limiting ─────────────────────────────────

pub fn get_last_submit_time(env: &Env, wallet: &Address, asset_pair: &Symbol) -> u64 {
    let key = DataKey::LastSubmitTime(wallet.clone(), asset_pair.clone());
    let result: Option<u64> = env.storage().persistent().get(&key);
    if result.is_some() {
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }
    result.unwrap_or(0)
}

pub fn set_last_submit_time(env: &Env, wallet: &Address, asset_pair: &Symbol, timestamp: u64) {
    let key = DataKey::LastSubmitTime(wallet.clone(), asset_pair.clone());
    env.storage().persistent().set(&key, &timestamp);
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

pub fn clear_last_submit_time(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let key = DataKey::LastSubmitTime(wallet.clone(), asset_pair.clone());
    env.storage().persistent().remove(&key);
}

pub fn get_cooldown_secs(env: &Env) -> u64 {
    env.storage().instance().get(&DataKey::CooldownSecs).unwrap_or(DEFAULT_COOLDOWN_SECS)
}

pub fn set_cooldown_secs(env: &Env, secs: u64) {
    env.storage().instance().set(&DataKey::CooldownSecs, &secs);
}

/// Returns the cooldown for `asset_pair`, falling back to the global default
/// when no pair-specific override has been configured.
pub fn get_pair_cooldown_secs(env: &Env, asset_pair: &Symbol) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::PairCooldown(asset_pair.clone()))
        .unwrap_or_else(|| get_cooldown_secs(env))
}

pub fn set_pair_cooldown_secs(env: &Env, asset_pair: &Symbol, secs: u64) {
    env.storage().instance().set(&DataKey::PairCooldown(asset_pair.clone()), &secs);
}

pub fn clear_pair_cooldown_secs(env: &Env, asset_pair: &Symbol) {
    env.storage().instance().remove(&DataKey::PairCooldown(asset_pair.clone()));
}

// ── Score Velocity Cap ────────────────────────────────────────────────────────

pub fn get_score_velocity_cap(env: &Env) -> ScoreVelocityCap {
    let enabled = env.storage().instance().get(&DataKey::ScoreVelocityCapEnabled).unwrap_or(false);
    let points_per_hour =
        env.storage().instance().get(&DataKey::ScoreVelocityCapPointsPerHour).unwrap_or(0);
    ScoreVelocityCap { enabled, points_per_hour }
}

pub fn set_score_velocity_cap(env: &Env, cap: &ScoreVelocityCap) {
    env.storage().instance().set(&DataKey::ScoreVelocityCapEnabled, &cap.enabled);
    env.storage().instance().set(&DataKey::ScoreVelocityCapPointsPerHour, &cap.points_per_hour);
}

pub fn is_velocity_cap_overridden(env: &Env, wallet: &Address, asset_pair: &Symbol) -> bool {
    let key = DataKey::VelocityCapOverride(wallet.clone(), asset_pair.clone());
    env.storage().persistent().get(&key).unwrap_or(false)
}

pub fn set_velocity_cap_override(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let key = DataKey::VelocityCapOverride(wallet.clone(), asset_pair.clone());
    env.storage().persistent().set(&key, &true);
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

pub fn clear_velocity_cap_override(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let key = DataKey::VelocityCapOverride(wallet.clone(), asset_pair.clone());
    env.storage().persistent().remove(&key);
}

// ── GDPR / data-erasure ───────────────────────────────────────────────────────

pub fn clear_score_history(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let key = DataKey::ScoreHistory(wallet.clone(), asset_pair.clone());
    env.storage().persistent().remove(&key);
}

pub fn clear_score(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let key = DataKey::Score(wallet.clone(), asset_pair.clone());
    env.storage().persistent().remove(&key);
}

// ── Score count ──────────────────────────────────────────────────────────────

pub fn increment_score_count(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let key = DataKey::ScoreCount(wallet.clone(), asset_pair.clone());
    let current: u32 = env.storage().persistent().get(&key).unwrap_or(0);
    env.storage().persistent().set(&key, &(current + 1));
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

pub fn get_score_count(env: &Env, wallet: &Address, asset_pair: &Symbol) -> u32 {
    let key = DataKey::ScoreCount(wallet.clone(), asset_pair.clone());
    env.storage().persistent().get(&key).unwrap_or(0)
}

// ── Score trend state ─────────────────────────────────────────────────────────

pub fn get_trend_state(env: &Env, wallet: &Address, asset_pair: &Symbol) -> ScoreTrend {
    let key = DataKey::TrendState(wallet.clone(), asset_pair.clone());
    let result: Option<ScoreTrend> = env.storage().persistent().get(&key);
    if result.is_some() {
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }
    result.unwrap_or(ScoreTrend { trend: 0, consecutive: 0 })
}

pub fn set_trend_state(env: &Env, wallet: &Address, asset_pair: &Symbol, state: &ScoreTrend) {
    let key = DataKey::TrendState(wallet.clone(), asset_pair.clone());
    env.storage().persistent().set(&key, state);
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

// ── Score attestation ─────────────────────────────────────────────────────────

/// Returns the off-chain detection pipeline's secp256k1 public key, or
/// `None` if `set_service_pubkey` has never been called.
pub fn get_service_pubkey(env: &Env) -> Option<soroban_sdk::Bytes> {
    env.storage().instance().get(&DataKey::ServicePubKey)
}

pub fn set_service_pubkey(env: &Env, pubkey: &Bytes) {
    env.storage().instance().set(&DataKey::ServicePubKey, pubkey);
}

pub fn set_gate_callers(env: &Env, callers: &Vec<Address>) {
    env.storage().instance().set(&GateDataKey::GateCallers, callers);
}

pub fn get_gate_callers(env: &Env) -> Vec<Address> {
    env.storage().instance().get(&GateDataKey::GateCallers).unwrap_or_else(|| Vec::new(env))
}

pub fn set_gate_open(env: &Env, open: bool) {
    env.storage().instance().set(&GateDataKey::GateOpen, &open);
}

pub fn get_gate_open(env: &Env) -> bool {
    env.storage().instance().get(&GateDataKey::GateOpen).unwrap_or(true)
}

// ── Time-weighted exponential decay ──────────────────────────────────────────

pub fn get_decay_rate(env: &Env) -> (u32, u32) {
    env.storage().instance().get::<_, (u32, u32)>(&DataKey::DecayRate).unwrap_or((
        crate::constants::DEFAULT_DECAY_LAMBDA_NUM,
        crate::constants::DEFAULT_DECAY_LAMBDA_DEN,
    ))
}

pub fn set_decay_rate(env: &Env, numerator: u32, denominator: u32) {
    env.storage().instance().set(&DataKey::DecayRate, &(numerator, denominator));
}

// ── Global minimum confidence floor ──────────────────────────────────────────

pub fn get_global_min_confidence(env: &Env) -> u32 {
    let result: Option<u32> = env.storage().instance().get(&DataKey::GlobalMinConfidence);
    result.unwrap_or(0)
}

pub fn set_global_min_confidence(env: &Env, min_confidence: u32) {
    env.storage().instance().set(&DataKey::GlobalMinConfidence, &min_confidence);
}

// Fee withdrawal

pub fn get_fee_token(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::FeeToken)
}

pub fn set_fee_token(env: &Env, token: &Address) {
    env.storage().instance().set(&DataKey::FeeToken, token);
}

pub fn is_withdrawal_locked(env: &Env) -> bool {
    env.storage().instance().get::<_, bool>(&DataKey::WithdrawalLock).unwrap_or(false)
}

pub fn set_withdrawal_lock(env: &Env) {
    env.storage().instance().set(&DataKey::WithdrawalLock, &true);
}

pub fn clear_withdrawal_lock(env: &Env) {
    env.storage().instance().remove(&DataKey::WithdrawalLock);
}

// ── Score delegation ──────────────────────────────────────────────────────────

pub fn get_score_delegate(env: &Env, sub_wallet: &Address) -> Option<Address> {
    let key = DataKey::ScoreDelegate(sub_wallet.clone());
    let result: Option<Address> = env.storage().persistent().get(&key);
    if result.is_some() {
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }
    result
}

pub fn peek_score_delegate(env: &Env, sub_wallet: &Address) -> Option<Address> {
    let key = DataKey::ScoreDelegate(sub_wallet.clone());
    env.storage().persistent().get(&key)
}

pub fn set_score_delegate(env: &Env, sub_wallet: &Address, custodian: &Address) {
    let key = DataKey::ScoreDelegate(sub_wallet.clone());
    env.storage().persistent().set(&key, custodian);
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

pub fn remove_score_delegate(env: &Env, sub_wallet: &Address) {
    let key = DataKey::ScoreDelegate(sub_wallet.clone());
    env.storage().persistent().remove(&key);
}

// ── Wallet Relationship Graph ───────────────────────────────────────────────

pub fn get_counterparties(env: &Env, wallet: &Address, asset_pair: &Symbol) -> Vec<Address> {
    let key = DataKey::Counterparties(wallet.clone(), asset_pair.clone());
    env.storage().persistent().get(&key).unwrap_or_else(|| Vec::new(env))
}

pub fn add_counterparty_link(
    env: &Env,
    wallet_a: &Address,
    wallet_b: &Address,
    asset_pair: &Symbol,
) -> Result<(), Error> {
    if wallet_a == wallet_b {
        return Err(Error::CounterpartyLinkFull);
    }

    let mut links_a = get_counterparties(env, wallet_a, asset_pair);
    if !links_a.contains(wallet_b) {
        if links_a.len() >= crate::constants::MAX_COUNTERPARTY_LINKS_PER_WALLET {
            return Err(Error::ServiceSetFull);
        }
        links_a.push_back(wallet_b.clone());
        let key_a = DataKey::Counterparties(wallet_a.clone(), asset_pair.clone());
        env.storage().persistent().set(&key_a, &links_a);
        env.storage().persistent().extend_ttl(&key_a, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }

    let mut links_b = get_counterparties(env, wallet_b, asset_pair);
    if !links_b.contains(wallet_a) {
        if links_b.len() >= crate::constants::MAX_COUNTERPARTY_LINKS_PER_WALLET {
            return Err(Error::ServiceSetFull);
        }
        links_b.push_back(wallet_a.clone());
        let key_b = DataKey::Counterparties(wallet_b.clone(), asset_pair.clone());
        env.storage().persistent().set(&key_b, &links_b);
        env.storage().persistent().extend_ttl(&key_b, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }

    Ok(())
}

pub fn remove_counterparty_link(
    env: &Env,
    wallet_a: &Address,
    wallet_b: &Address,
    asset_pair: &Symbol,
) -> Result<(), Error> {
    let mut links_a = get_counterparties(env, wallet_a, asset_pair);
    let pos_a = links_a.first_index_of(wallet_b);
    if let Some(idx) = pos_a {
        links_a.remove(idx);
        let key_a = DataKey::Counterparties(wallet_a.clone(), asset_pair.clone());
        if links_a.is_empty() {
            env.storage().persistent().remove(&key_a);
        } else {
            env.storage().persistent().set(&key_a, &links_a);
            env.storage().persistent().extend_ttl(&key_a, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
        }
    }

    let mut links_b = get_counterparties(env, wallet_b, asset_pair);
    let pos_b = links_b.first_index_of(wallet_a);
    if let Some(idx) = pos_b {
        links_b.remove(idx);
        let key_b = DataKey::Counterparties(wallet_b.clone(), asset_pair.clone());
        if links_b.is_empty() {
            env.storage().persistent().remove(&key_b);
        } else {
            env.storage().persistent().set(&key_b, &links_b);
            env.storage().persistent().extend_ttl(&key_b, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
        }
    }

    if pos_a.is_none() && pos_b.is_none() {
        return Err(Error::CounterpartyLinkFull);
    }

    Ok(())
}

pub fn get_contagion_depth(env: &Env, wallet: &Address, asset_pair: &Symbol) -> u32 {
    let key = DataKey::Counterparties(wallet.clone(), asset_pair.clone());
    let links: Vec<Address> = env.storage().persistent().get(&key).unwrap_or_else(|| Vec::new(env));
    links.len()
}

// ── Score submission floor ────────────────────────────────────────────────────

pub fn get_score_floor_policy(env: &Env) -> ScoreFloorPolicy {
    let result: Option<(bool, u32, u32)> = env.storage().instance().get(&DataKey::ScoreFloorConfig);
    if let Some((enabled, high_water_mark, floor_value)) = result {
        ScoreFloorPolicy { enabled, high_water_mark, floor_value }
    } else {
        ScoreFloorPolicy {
            enabled: false,
            high_water_mark: crate::constants::DEFAULT_SCORE_FLOOR_HWM,
            floor_value: crate::constants::DEFAULT_SCORE_FLOOR_MIN,
        }
    }
}

pub fn set_score_floor_policy(env: &Env, enabled: bool, high_water_mark: u32, floor_value: u32) {
    env.storage()
        .instance()
        .set(&DataKey::ScoreFloorConfig, &(enabled, high_water_mark, floor_value));
}

pub fn get_historical_max_score(env: &Env, wallet: &Address, asset_pair: &Symbol) -> u32 {
    let key = DataKey::HistoricalMaxScore(wallet.clone(), asset_pair.clone());
    let result: Option<u32> = env.storage().persistent().get(&key);
    if result.is_some() {
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }
    result.unwrap_or(0)
}

pub fn update_historical_max_score(env: &Env, wallet: &Address, asset_pair: &Symbol, score: u32) {
    let key = DataKey::HistoricalMaxScore(wallet.clone(), asset_pair.clone());
    let current: Option<u32> = env.storage().persistent().get(&key);
    if score > current.unwrap_or(0) {
        env.storage().persistent().set(&key, &score);
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    } else if current.is_some() {
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }
}

pub fn clear_historical_max_score(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let key = DataKey::HistoricalMaxScore(wallet.clone(), asset_pair.clone());
    env.storage().persistent().remove(&key);
}

// ── Hysteresis margin ─────────────────────────────────────────────────────────

pub fn get_hysteresis_margin(env: &Env) -> u32 {
    let result: Option<u32> = env.storage().instance().get(&DataKey::HysteresisMargin);
    result.unwrap_or(0)
}

pub fn set_hysteresis_margin(env: &Env, margin: u32) {
    env.storage().instance().set(&DataKey::HysteresisMargin, &margin);
}

// ── Per-(wallet, asset_pair) risk band state ──────────────────────────────────

pub fn get_risk_band_state(env: &Env, wallet: &Address, asset_pair: &Symbol) -> bool {
    let key = DataKey::RiskBandState(wallet.clone(), asset_pair.clone());
    let result: Option<bool> = env.storage().temporary().get(&key);
    if result.is_some() {
        env.storage().temporary().extend_ttl(
            &key,
            BAND_STATE_TTL_THRESHOLD,
            BAND_STATE_TTL_EXTEND_TO,
        );
    }
    result.unwrap_or(false)
}

pub fn peek_risk_band_state(env: &Env, wallet: &Address, asset_pair: &Symbol) -> bool {
    let key = DataKey::RiskBandState(wallet.clone(), asset_pair.clone());
    let result: Option<bool> = env.storage().temporary().get(&key);
    result.unwrap_or(false)
}

pub fn set_risk_band_state(env: &Env, wallet: &Address, asset_pair: &Symbol, in_band: bool) {
    let key = DataKey::RiskBandState(wallet.clone(), asset_pair.clone());
    if in_band {
        env.storage().temporary().set(&key, &true);
        env.storage().temporary().extend_ttl(
            &key,
            BAND_STATE_TTL_THRESHOLD,
            BAND_STATE_TTL_EXTEND_TO,
        );
    } else {
        env.storage().temporary().remove(&key);
    }
}

// ── Score embargo ─────────────────────────────────────────────────────────────

pub fn set_embargo(env: &Env, wallet: &Address, expiry: &EmbargoExpiry) {
    let key = DataKey::ScoreEmbargo(wallet.clone());
    env.storage().temporary().set(&key, expiry);
    env.storage().temporary().extend_ttl(&key, EMBARGO_TTL_THRESHOLD, EMBARGO_TTL_EXTEND_TO);
}

pub fn remove_embargo(env: &Env, wallet: &Address) {
    let key = DataKey::ScoreEmbargo(wallet.clone());
    env.storage().temporary().remove(&key);
}

pub fn is_embargoed(env: &Env, wallet: &Address) -> bool {
    let key = DataKey::ScoreEmbargo(wallet.clone());
    let expiry: Option<EmbargoExpiry> = env.storage().temporary().get(&key);
    match expiry {
        None => false,
        Some(EmbargoExpiry::Indefinite) => {
            env.storage().temporary().extend_ttl(
                &key,
                EMBARGO_TTL_THRESHOLD,
                EMBARGO_TTL_EXTEND_TO,
            );
            true
        }
        Some(EmbargoExpiry::Until(ts)) => {
            let now = env.ledger().timestamp();
            let active = now <= ts;
            if active {
                env.storage().temporary().extend_ttl(
                    &key,
                    EMBARGO_TTL_THRESHOLD,
                    EMBARGO_TTL_EXTEND_TO,
                );
            }
            active
        }
    }
}

pub fn peek_is_embargoed(env: &Env, wallet: &Address) -> bool {
    let key = DataKey::ScoreEmbargo(wallet.clone());
    let expiry: Option<EmbargoExpiry> = env.storage().temporary().get(&key);
    match expiry {
        None => false,
        Some(EmbargoExpiry::Indefinite) => true,
        Some(EmbargoExpiry::Until(ts)) => env.ledger().timestamp() <= ts,
    }
}

pub fn get_embargoed_wallets(env: &Env) -> Vec<Address> {
    let wallets: Vec<Address> =
        env.storage().temporary().get(&DataKey::EmbargoedWalletIndex).unwrap_or_else(|| Vec::new(env));
    if !wallets.is_empty() {
        env.storage().temporary().extend_ttl(
            &DataKey::EmbargoedWalletIndex,
            EMBARGO_TTL_THRESHOLD,
            EMBARGO_TTL_EXTEND_TO,
        );
    }
    wallets
}

pub fn add_to_embargoed_index(env: &Env, wallet: &Address) -> bool {
    let mut wallets = get_embargoed_wallets(env);
    if wallets.contains(wallet) {
        return true;
    }
    if wallets.len() >= crate::constants::MAX_EMBARGOED_WALLETS {
        return false;
    }
    wallets.push_back(wallet.clone());
    env.storage().temporary().set(&DataKey::EmbargoedWalletIndex, &wallets);
    env.storage().temporary().extend_ttl(
        &DataKey::EmbargoedWalletIndex,
        EMBARGO_TTL_THRESHOLD,
        EMBARGO_TTL_EXTEND_TO,
    );
    true
}

pub fn remove_from_embargoed_index(env: &Env, wallet: &Address) {
    let mut wallets = get_embargoed_wallets(env);
    if let Some(idx) = wallets.first_index_of(wallet) {
        wallets.remove(idx);
        env.storage().temporary().set(&DataKey::EmbargoedWalletIndex, &wallets);
    }
}

pub fn clear_embargoed_index(env: &Env) {
    env.storage().temporary().remove(&DataKey::EmbargoedWalletIndex);
}

/// Sets the risk band state for `(wallet, asset_pair)`. Passing `true`
/// records that the wallet has entered the high-risk band; passing `false`
/// removes the entry (equivalent to `false`, the default) so storage is not
/// wasted on cleared state.
pub fn set_risk_band_state(env: &Env, wallet: &Address, asset_pair: &Symbol, in_band: bool) {
    let key = DataKey::RiskBandState(wallet.clone(), asset_pair.clone());
    if in_band {
        env.storage().temporary().set(&key, &true);
        env.storage().temporary().extend_ttl(
            &key,
            BAND_STATE_TTL_THRESHOLD,
            BAND_STATE_TTL_EXTEND_TO,
        );
    } else {
        env.storage().temporary().remove(&key);
    }
}

// ── Band entry timestamp ──────────────────────────────────────────────────────

/// Returns the ledger timestamp at which `wallet` first entered the high-risk
/// band for `asset_pair`, or `None` when the wallet is not currently in the
/// band (never entered, or the entry time has been cleared on exit). Extends
/// TTL on read so active band memberships keep their entry time alive.
pub fn get_band_entry_time(env: &Env, wallet: &Address, asset_pair: &Symbol) -> Option<u64> {
    let key = DataKey::BandEntryTime(wallet.clone(), asset_pair.clone());
    let result: Option<u64> = env.storage().temporary().get(&key);
    if result.is_some() {
        env.storage().temporary().extend_ttl(
            &key,
            BAND_STATE_TTL_THRESHOLD,
            BAND_STATE_TTL_EXTEND_TO,
        );
    }
    result
}

/// Records `timestamp` as the ledger time when `wallet` entered the high-risk
/// band for `asset_pair`. Uses the same TTL constants as `RiskBandState` so
/// both keys expire together if they go cold.
pub fn set_band_entry_time(env: &Env, wallet: &Address, asset_pair: &Symbol, timestamp: u64) {
    let key = DataKey::BandEntryTime(wallet.clone(), asset_pair.clone());
    env.storage().temporary().set(&key, &timestamp);
    env.storage().temporary().extend_ttl(&key, BAND_STATE_TTL_THRESHOLD, BAND_STATE_TTL_EXTEND_TO);
}

/// Removes the band entry timestamp for `wallet` / `asset_pair`. Called when
/// the wallet exits the high-risk band so the key is absent whenever the
/// wallet is not in the band.
pub fn clear_band_entry_time(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let key = DataKey::BandEntryTime(wallet.clone(), asset_pair.clone());
    env.storage().temporary().remove(&key);
}

// ── Consensus configuration ─────────────────────────────────────────────────

pub fn get_consensus_threshold_k(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::ConsensusThresholdK)
        .unwrap_or(DEFAULT_CONSENSUS_THRESHOLD_K)
}

pub fn set_consensus_threshold_k(env: &Env, k: u32) {
    env.storage().instance().set(&DataKey::ConsensusThresholdK, &k);
}

pub fn get_consensus_epsilon(env: &Env) -> u32 {
    env.storage().instance().get(&DataKey::ConsensusEpsilon).unwrap_or(DEFAULT_CONSENSUS_EPSILON)
}

pub fn set_consensus_epsilon(env: &Env, epsilon: u32) {
    env.storage().instance().set(&DataKey::ConsensusEpsilon, &epsilon);
}

// ── Score dispute mechanism ─────────────────────────────────────────────────────

/// Writes (or replaces) the open dispute record for `(wallet, asset_pair)` and
/// refreshes its TTL. Stored in temporary storage so abandoned disputes
/// eventually expire on their own.
pub fn set_dispute(env: &Env, wallet: &Address, asset_pair: &Symbol, dispute: &ScoreDispute) {
    let key = DataKey::ScoreDispute(wallet.clone(), asset_pair.clone());
    env.storage().temporary().set(&key, dispute);
    env.storage().temporary().extend_ttl(
        &key,
        crate::constants::DISPUTE_TTL_THRESHOLD,
        crate::constants::DISPUTE_TTL_EXTEND_TO,
    );
}

/// Returns the open dispute for `(wallet, asset_pair)`, if any, extending its
/// TTL on read.
pub fn get_dispute(env: &Env, wallet: &Address, asset_pair: &Symbol) -> Option<ScoreDispute> {
    let key = DataKey::ScoreDispute(wallet.clone(), asset_pair.clone());
    let dispute: Option<ScoreDispute> = env.storage().temporary().get(&key);
    if dispute.is_some() {
        env.storage().temporary().extend_ttl(
            &key,
            crate::constants::DISPUTE_TTL_THRESHOLD,
            crate::constants::DISPUTE_TTL_EXTEND_TO,
        );
    }
    dispute
}

/// Removes the dispute record for `(wallet, asset_pair)`. No-op if absent.
pub fn remove_dispute(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let key = DataKey::ScoreDispute(wallet.clone(), asset_pair.clone());
    env.storage().temporary().remove(&key);
}

/// Returns every currently open dispute as `(challenger, asset_pair)` pairs.
/// O(1) storage read — the index is maintained incrementally by
/// `add_to_dispute_index` / `remove_from_dispute_index`.
pub fn get_dispute_index(env: &Env) -> Vec<(Address, Symbol)> {
    let disputes: Vec<(Address, Symbol)> =
        env.storage().persistent().get(&DataKey::DisputeIndex).unwrap_or_else(|| Vec::new(env));
    if !disputes.is_empty() {
        env.storage().persistent().extend_ttl(
            &DataKey::DisputeIndex,
            SCORE_TTL_THRESHOLD,
            SCORE_TTL_EXTEND_TO,
        );
    }
    disputes
}

/// Adds `(wallet, asset_pair)` to the dispute index if it isn't already there.
/// Returns `false` (without modifying the index) if the entry is new *and* the
/// index is already at `MAX_OPEN_DISPUTES` — the caller turns that into an
/// error. Re-adding an existing entry is a no-op that returns `true`.
pub fn add_to_dispute_index(env: &Env, wallet: &Address, asset_pair: &Symbol) -> bool {
    let mut disputes = get_dispute_index(env);
    let entry = (wallet.clone(), asset_pair.clone());
    if disputes.contains(&entry) {
        return true;
    }
    if disputes.len() >= crate::constants::MAX_OPEN_DISPUTES {
        return false;
    }
    disputes.push_back(entry);
    env.storage().persistent().set(&DataKey::DisputeIndex, &disputes);
    env.storage().persistent().extend_ttl(
        &DataKey::DisputeIndex,
        SCORE_TTL_THRESHOLD,
        SCORE_TTL_EXTEND_TO,
    );
    true
}

/// Removes `(wallet, asset_pair)` from the dispute index. No-op if absent.
pub fn remove_from_dispute_index(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let mut disputes = get_dispute_index(env);
    let entry = (wallet.clone(), asset_pair.clone());
    if let Some(idx) = disputes.first_index_of(&entry) {
        disputes.remove(idx);
        env.storage().persistent().set(&DataKey::DisputeIndex, &disputes);
    }
}

// ── MEV-Resistant Commit-Reveal ──────────────────────────────────────────────

pub fn get_last_global_submission_time(env: &Env) -> u64 {
    env.storage().instance().get(&DataKey::LastGlobalSubmissionTime).unwrap_or(0)
}

pub fn set_last_global_submission_time(env: &Env, timestamp: u64) {
    env.storage().instance().set(&DataKey::LastGlobalSubmissionTime, &timestamp);
}

pub fn get_quorum_failure_window(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::QuorumFailureWindow)
        .unwrap_or(DEFAULT_QUORUM_FAILURE_WINDOW_SECS)
}

pub fn set_quorum_failure_window(env: &Env, window_secs: u64) {
    env.storage().instance().set(&DataKey::QuorumFailureWindow, &window_secs);
}

pub fn set_consensus_commitment(
    env: &Env,
    model: &Address,
    wallet: &Address,
    asset_pair: &Symbol,
    commitment: &soroban_sdk::BytesN<32>,
) {
    let key = DataKey::ConsensusCommitment(model.clone(), wallet.clone(), asset_pair.clone());
    let ttl = get_reveal_window_secs(env) as u32;
    let ledgers_to_live = (ttl / 5).max(12);
    env.storage().temporary().set(&key, commitment);
    env.storage().temporary().extend_ttl(&key, ledgers_to_live, ledgers_to_live);
}

pub fn get_consensus_commitment(
    env: &Env,
    model: &Address,
    wallet: &Address,
    asset_pair: &Symbol,
) -> Option<soroban_sdk::BytesN<32>> {
    let key = DataKey::ConsensusCommitment(model.clone(), wallet.clone(), asset_pair.clone());
    env.storage().temporary().get(&key)
}

pub fn set_original_service_threshold(env: &Env, threshold: u32) {
    env.storage().instance().set(&DataKey::OriginalServiceThreshold, &threshold);
}

pub fn clear_original_service_threshold(env: &Env) {
    env.storage().instance().remove(&DataKey::OriginalServiceThreshold);
}

// ── Finality buffer (pending score commit window) ────────────────────────────

/// Returns the admin-configured finality buffer in seconds, defaulting to `0`
/// (disabled) until `set_finality_buffer` is called.
pub fn get_finality_buffer_secs(env: &Env) -> u64 {
    env.storage().instance().get(&DataKey::FinalityBufferSecs).unwrap_or(0)
}

pub fn set_finality_buffer_secs(env: &Env, secs: u64) {
    env.storage().instance().set(&DataKey::FinalityBufferSecs, &secs);
}

/// Returns the pending score held for `(wallet, asset_pair)`, if any.
/// Invisible to `get_score` / `query_risk_gate`.
pub fn get_pending_score(
    env: &Env,
    wallet: &Address,
    asset_pair: &Symbol,
) -> Option<PendingScoreEntry> {
    let key = DataKey::PendingScore(wallet.clone(), asset_pair.clone());
    let entry: Option<PendingScoreEntry> = env.storage().persistent().get(&key);
    if entry.is_some() {
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }
    entry
}

/// Writes `entry` as the pending score for `(wallet, asset_pair)`, replacing
/// any existing pending entry rather than queuing alongside it.
pub fn set_pending_score(
    env: &Env,
    wallet: &Address,
    asset_pair: &Symbol,
    entry: &PendingScoreEntry,
) {
    let key = DataKey::PendingScore(wallet.clone(), asset_pair.clone());
    env.storage().persistent().set(&key, entry);
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

/// Removes the pending score for `(wallet, asset_pair)`. No-op if none exists.
pub fn clear_pending_score(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let key = DataKey::PendingScore(wallet.clone(), asset_pair.clone());
    env.storage().persistent().remove(&key);
}

// ── Service heartbeat monitor ────────────────────────────────────────────

/// Returns the ledger timestamp of the most recent accepted submission or
/// `ping_heartbeat` call, or `0` if the service has never been active.
pub fn get_last_service_activity(env: &Env) -> u64 {
    env.storage().instance().get(&DataKey::LastServiceActivityAt).unwrap_or(0)
}

/// Records `timestamp` as the most recent service activity. Called by
/// `submit_score`, `submit_scores_batch`, and `ping_heartbeat`.
pub fn set_last_service_activity(env: &Env, timestamp: u64) {
    env.storage().instance().set(&DataKey::LastServiceActivityAt, &timestamp);
}

/// Returns the admin-configured heartbeat alert threshold (seconds),
/// defaulting to `DEFAULT_HEARTBEAT_ALERT_THRESHOLD_SECS` until
/// `set_heartbeat_alert_threshold` is called.
pub fn get_heartbeat_alert_threshold(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::ServiceHeartbeatAlertThreshold)
        .unwrap_or(crate::constants::DEFAULT_HEARTBEAT_ALERT_THRESHOLD_SECS)
}

pub fn set_heartbeat_alert_threshold(env: &Env, secs: u64) {
    env.storage().instance().set(&DataKey::ServiceHeartbeatAlertThreshold, &secs);
}

/// Returns `true` once a `ServiceSilenceAlertEvent` has been emitted for the
/// current silence window and not yet cleared by a resumed submission.
pub fn is_silent_alert_emitted(env: &Env) -> bool {
    env.storage().instance().get(&DataKey::ServiceSilentAlertEmitted).unwrap_or(false)
}

pub fn set_silent_alert_emitted(env: &Env) {
    env.storage().instance().set(&DataKey::ServiceSilentAlertEmitted, &true);
}

pub fn clear_silent_alert_emitted(env: &Env) {
    env.storage().instance().remove(&DataKey::ServiceSilentAlertEmitted);
}

// ── Aggregate service pubkey (threshold attestation) ─────────────────────────

pub fn get_aggregate_service_pubkey(env: &Env) -> Option<Bytes> {
    env.storage().instance().get(&DataKey::AggregateServicePubKey)
}

pub fn set_aggregate_service_pubkey(env: &Env, pubkey: &Bytes) {
    env.storage().instance().set(&DataKey::AggregateServicePubKey, pubkey);
}

// ── Consensus commitment (commit-reveal) ─────────────────────────────────────

pub fn remove_consensus_commitment(
    env: &Env,
    model: &Address,
    wallet: &Address,
    asset_pair: &Symbol,
) {
    let key = DataKey::ConsensusCommitment(model.clone(), wallet.clone(), asset_pair.clone());
    env.storage().temporary().remove(&key);
}

pub fn get_reveal_window_secs(env: &Env) -> u64 {
    env.storage().instance().get(&DataKey::RevealWindowSecs).unwrap_or(3_600)
}

// ── Signer expiry ─────────────────────────────────────────────────────────────

pub fn check_signer_expired(_env: &Env, _signer: &Address) -> Result<(), crate::errors::Error> {
    Ok(())
}

pub fn get_signer_ttl(env: &Env) -> u64 {
    env.storage().instance().get(&DataKey::SignerTtl).unwrap_or(0)
}

pub fn set_signer_ttl(env: &Env, ttl_secs: u64) {
    env.storage().instance().set(&DataKey::SignerTtl, &ttl_secs);
}

pub fn get_signer_grace_period(env: &Env) -> u64 {
    env.storage().instance().get(&DataKey::SignerGracePeriod).unwrap_or(0)
}

pub fn set_signer_grace_period(env: &Env, grace_secs: u64) {
    env.storage().instance().set(&DataKey::SignerGracePeriod, &grace_secs);
}

// ── Model version registry ────────────────────────────────────────────────────

pub fn get_model_version_set(env: &Env) -> Vec<u32> {
    env.storage().instance().get(&DataKey::AllModelVersions).unwrap_or_else(|| Vec::new(env))
}

pub fn set_model_version_set(env: &Env, versions: &Vec<u32>) {
    env.storage().instance().set(&DataKey::AllModelVersions, versions);
}

pub fn is_model_version_registered(env: &Env, version: u32) -> bool {
    get_model_version_set(env).contains(&version)
}

pub fn is_model_version_deprecated(env: &Env, version: u32) -> bool {
    let key = DataKey::ModelVersionStatus(version);
    let status: Option<u32> = env.storage().instance().get(&key);
    status.unwrap_or(0) == 1
}

pub fn set_model_version_deprecated(env: &Env, version: u32) {
    env.storage().instance().set(&DataKey::ModelVersionStatus(version), &1u32);
}

// ── Bayesian model posterior weights ─────────────────────────────────────────

pub fn get_model_posterior_weight(env: &Env, version: u32) -> u64 {
    env.storage().instance().get(&DataKey::ModelPosteriorWeight(version)).unwrap_or(1_000_000u64)
}

pub fn set_model_posterior_weight(env: &Env, version: u32, weight: u64) {
    env.storage().instance().set(&DataKey::ModelPosteriorWeight(version), &weight);
}

// ── Score histogram ───────────────────────────────────────────────────────────

fn get_histogram_vec(env: &Env) -> Vec<u64> {
    env.storage().instance().get(&DataKey::ScoreHistogram).unwrap_or_else(|| {
        let mut v = Vec::new(env);
        for _ in 0..=100u32 {
            v.push_back(0u64);
        }
        v
    })
}

pub fn get_score_histogram(env: &Env) -> ScoreHistogram {
    let buckets = get_histogram_vec(env);
    let mut total: u64 = 0;
    for i in 0..buckets.len() {
        total = total.saturating_add(buckets.get(i).unwrap_or(0));
    }
    ScoreHistogram { buckets, total }
}

pub fn get_histogram_total(env: &Env) -> u32 {
    let buckets = get_histogram_vec(env);
    let mut total: u64 = 0;
    for i in 0..buckets.len() {
        total = total.saturating_add(buckets.get(i).unwrap_or(0));
    }
    total as u32
}

pub fn get_histogram_bucket(env: &Env, bucket: u32) -> u32 {
    let buckets = get_histogram_vec(env);
    buckets.get(bucket).unwrap_or(0) as u32
}

pub fn update_histogram_on_clear(env: &Env, removed_score: u32) {
    let key = DataKey::ScoreHistogram;
    let mut histogram = get_histogram_vec(env);
    if removed_score <= 100 && histogram.len() >= 101 {
        let count = histogram.get(removed_score).unwrap_or(0).saturating_sub(1);
        histogram.set(removed_score, count);
        env.storage().instance().set(&key, &histogram);
    }
}

pub fn update_histogram_on_write(env: &Env, previous_score: Option<u32>, new_score: u32) {
    let key = DataKey::ScoreHistogram;
    let mut histogram = get_histogram_vec(env);
    if histogram.len() < 101 {
        return;
    }
    if let Some(prev) = previous_score {
        if prev <= 100 {
            let prev_count = histogram.get(prev).unwrap_or(0).saturating_sub(1);
            histogram.set(prev, prev_count);
        }
    }
    if new_score <= 100 {
        let new_count = histogram.get(new_score).unwrap_or(0).saturating_add(1);
        histogram.set(new_score, new_count);
    }
    env.storage().instance().set(&key, &histogram);
}

// ── Verkle commitment ─────────────────────────────────────────────────────────

pub fn get_verkle_commitment_raw(env: &Env) -> [u8; 32] {
    let stored: Option<soroban_sdk::Bytes> =
        env.storage().instance().get(&DataKey::VerkleCommitment);
    match stored {
        Some(b) if b.len() == 32 => {
            let mut arr = [0u8; 32];
            b.copy_into_slice(&mut arr);
            arr
        }
        _ => [0u8; 32],
    }
}

pub fn set_verkle_commitment_raw(env: &Env, commitment: &[u8; 32]) {
    let bytes = soroban_sdk::Bytes::from_array(env, commitment);
    env.storage().instance().set(&DataKey::VerkleCommitment, &bytes);
}

pub fn get_verkle_leaf(env: &Env, wallet: &Address, asset_pair: &Symbol) -> Option<[u8; 32]> {
    let key = DataKey::VerkleLeaf(wallet.clone(), asset_pair.clone());
    let stored: Option<soroban_sdk::Bytes> = env.storage().persistent().get(&key);
    match stored {
        Some(b) if b.len() == 32 => {
            let mut arr = [0u8; 32];
            b.copy_into_slice(&mut arr);
            Some(arr)
        }
        _ => None,
    }
}

pub fn set_verkle_leaf(env: &Env, wallet: &Address, asset_pair: &Symbol, leaf: &[u8; 32]) {
    let key = DataKey::VerkleLeaf(wallet.clone(), asset_pair.clone());
    let bytes = soroban_sdk::Bytes::from_array(env, leaf);
    env.storage().persistent().set(&key, &bytes);
}

// ── Signer lifecycle ─────────────────────────────────────────────────────────

pub fn set_signer_added_at(env: &Env, signer: &Address, timestamp: u64) {
    env.storage().instance().set(&DataKey::SignerAddedAt(signer.clone()), &timestamp);
}

pub fn remove_signer_added_at(env: &Env, signer: &Address) {
    env.storage().instance().remove(&DataKey::SignerAddedAt(signer.clone()));
}

pub fn get_signer_age(env: &Env, signer: &Address) -> Option<u64> {
    let added_at: Option<u64> =
        env.storage().instance().get(&DataKey::SignerAddedAt(signer.clone()));
    added_at.map(|t| env.ledger().timestamp().saturating_sub(t))
}

pub fn set_signer_rotation_ttl(env: &Env, ttl_secs: u64) {
    env.storage().instance().set(&DataKey::SignerTtl, &ttl_secs);
}

pub fn get_signer_rotation_ttl(env: &Env) -> u64 {
    env.storage().instance().get(&DataKey::SignerTtl).unwrap_or(0)
}

pub fn set_signer_rotation_grace(env: &Env, grace_secs: u64) {
    env.storage().instance().set(&DataKey::SignerGracePeriod, &grace_secs);
}

pub fn set_reveal_window_secs(env: &Env, secs: u64) {
    env.storage().instance().set(&DataKey::RevealWindowSecs, &secs);
}
