use soroban_sdk::{Address, Bytes, Env, Symbol, Vec};

use crate::constants::{
    BAND_STATE_TTL_EXTEND_TO, BAND_STATE_TTL_THRESHOLD, DEFAULT_COOLDOWN_SECS,
    DEFAULT_RISK_THRESHOLD, DEFAULT_UPGRADE_DELAY_SECS, SCORE_TTL_EXTEND_TO, SCORE_TTL_THRESHOLD,
};
use crate::types::{AggregateRiskScore, DataKey, RiskScore, UpgradeProposal};

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

/// Strictly read-only score lookup that, unlike [`get_score`], does **not**
/// extend the entry's TTL. Used by the infallible cross-contract gate
/// (`query_risk_gate`) so that calling it from another contract's guard
/// clause has no observable side effect on this contract's state.
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

// ── Score history ring buffer ────────────────────────────────────────────────

pub fn push_score_history(env: &Env, wallet: &Address, asset_pair: &Symbol, score: &RiskScore) {
    let key = DataKey::ScoreHistory(wallet.clone(), asset_pair.clone());
    let mut history: Vec<RiskScore> =
        env.storage().persistent().get(&key).unwrap_or_else(|| Vec::new(env));

    history.push_back(score.clone());

    // Evict oldest entry when the ring exceeds the configured depth cap.
    // Note: if the admin has *reduced* the depth since the last write, this
    // loop will evict multiple entries in one pass, trimming the ring down to
    // the new depth on the very next submission.
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

// ── Configurable history ring depth ──────────────────────────────────────────

/// Returns the admin-configured ring-buffer depth, or
/// [`DEFAULT_HISTORY_MAX_DEPTH`] when no value has been set yet.
pub fn get_history_max_depth(env: &Env) -> u32 {
    let result: Option<u32> = env.storage().instance().get(&DataKey::HistoryMaxDepth);
    result.unwrap_or(crate::constants::DEFAULT_HISTORY_MAX_DEPTH)
}

/// Persists `depth` as the ring-buffer cap for all future
/// `push_score_history` calls.
pub fn set_history_max_depth(env: &Env, depth: u32) {
    env.storage().instance().set(&DataKey::HistoryMaxDepth, &depth);
}

// ── Contract version ─────────────────────────────────────────────────────────

pub fn get_contract_version(env: &Env) -> u32 {
    let result: Option<u32> = env.storage().instance().get(&DataKey::ContractVersion);
    result.unwrap_or(crate::constants::CONTRACT_VERSION)
}

// ── Cross-asset aggregate risk ───────────────────────────────────────────────

/// Adds `asset_pair` to the wallet's tracked pair list if it isn't already
/// present. Idempotent — re-registering an existing pair is a no-op aside
/// from the TTL bump.
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

/// Returns the configured weight for `asset_pair`, defaulting to `1` (a
/// simple, unweighted average) when the admin has not set one explicitly.
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

/// Refreshes the cached aggregate snapshot at `AggregateScore(wallet)`.
/// This is a write-through cache only — `get_aggregate_score` always
/// recomputes from live per-pair scores rather than reading it back.
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

/// Returns the configured upgrade delay, defaulting to
/// `DEFAULT_UPGRADE_DELAY_SECS` until the admin sets one explicitly.
pub fn get_upgrade_delay(env: &Env) -> u64 {
    env.storage().instance().get(&DataKey::UpgradeDelay).unwrap_or(DEFAULT_UPGRADE_DELAY_SECS)
}

pub fn set_upgrade_delay(env: &Env, delay_secs: u64) {
    env.storage().instance().set(&DataKey::UpgradeDelay, &delay_secs);
}

// ── Multi-sig service set ─────────────────────────────────────────────────────

pub fn get_service_set(env: &Env) -> Vec<Address> {
    env.storage().instance().get(&DataKey::ServiceSet).unwrap_or_else(|| Vec::new(env))
}

pub fn set_service_set(env: &Env, set: &Vec<Address>) {
    env.storage().instance().set(&DataKey::ServiceSet, set);
}

pub fn get_service_threshold(env: &Env) -> u32 {
    env.storage().instance().get(&DataKey::ServiceThreshold).unwrap_or(0)
}

pub fn set_service_threshold(env: &Env, threshold: u32) {
    env.storage().instance().set(&DataKey::ServiceThreshold, &threshold);
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

/// Returns the ledger timestamp of the last accepted submission for
/// `(wallet, asset_pair)`, or `0` if none has ever been accepted (or it was
/// cleared by `override_rate_limit`).
pub fn get_last_submit_time(env: &Env, wallet: &Address, asset_pair: &Symbol) -> u64 {
    let key = DataKey::LastSubmitTime(wallet.clone(), asset_pair.clone());
    let result: Option<u64> = env.storage().persistent().get(&key);
    if result.is_some() {
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }
    result.unwrap_or(0)
}

/// Records `timestamp` as the most recent accepted submission time for
/// `(wallet, asset_pair)`. Uses the same TTL as `Score` so a cooldown entry
/// never outlives (or falls out of sync with) the score it gates.
pub fn set_last_submit_time(env: &Env, wallet: &Address, asset_pair: &Symbol, timestamp: u64) {
    let key = DataKey::LastSubmitTime(wallet.clone(), asset_pair.clone());
    env.storage().persistent().set(&key, &timestamp);
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

/// Clears the last-submit timestamp for `(wallet, asset_pair)`, immediately
/// lifting its cooldown. Used by the admin emergency path `override_rate_limit`.
pub fn clear_last_submit_time(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let key = DataKey::LastSubmitTime(wallet.clone(), asset_pair.clone());
    env.storage().persistent().remove(&key);
}

/// Returns the configured submission cooldown (seconds), defaulting to
/// `DEFAULT_COOLDOWN_SECS` (1 hour) until the admin sets one explicitly.
pub fn get_cooldown_secs(env: &Env) -> u64 {
    env.storage().instance().get(&DataKey::CooldownSecs).unwrap_or(DEFAULT_COOLDOWN_SECS)
}

pub fn set_cooldown_secs(env: &Env, secs: u64) {
    env.storage().instance().set(&DataKey::CooldownSecs, &secs);
}

// ── GDPR / data-erasure ───────────────────────────────────────────────────────

/// Removes the score history ring buffer for `wallet` / `asset_pair`.
/// No-op when no history exists.
pub fn clear_score_history(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let key = DataKey::ScoreHistory(wallet.clone(), asset_pair.clone());
    env.storage().persistent().remove(&key);
}

/// Removes the latest score entry for `wallet` / `asset_pair`.
/// No-op when no score exists.
pub fn clear_score(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let key = DataKey::Score(wallet.clone(), asset_pair.clone());
    env.storage().persistent().remove(&key);
}

// ── Score count ──────────────────────────────────────────────────────────────

/// Increments the monotonically increasing submission counter for a
/// (wallet, asset_pair) pair. Called by `submit_score` and
/// `submit_scores_batch` after each successful write.
pub fn increment_score_count(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let key = DataKey::ScoreCount(wallet.clone(), asset_pair.clone());
    let current: u32 = env.storage().persistent().get(&key).unwrap_or(0);
    env.storage().persistent().set(&key, &(current + 1));
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

/// Returns the total number of score submissions for a (wallet, asset_pair)
/// pair. Unlike `get_score_history` (which caps at `HISTORY_MAX_DEPTH`), this
/// counter is never truncated, so it can distinguish between a newly monitored
/// wallet (count = 1) and one with a long scoring history (count > 10 after
/// ring-buffer overflow).
///
/// Returns 0 when no scores have ever been submitted for this pair.
pub fn get_score_count(env: &Env, wallet: &Address, asset_pair: &Symbol) -> u32 {
    let key = DataKey::ScoreCount(wallet.clone(), asset_pair.clone());
    env.storage().persistent().get(&key).unwrap_or(0)
}

// ── Score attestation ─────────────────────────────────────────────────────

/// Returns the off-chain detection pipeline's secp256k1 public key, or
/// `None` if `set_service_pubkey` has never been called.
pub fn get_service_pubkey(env: &Env) -> Option<Bytes> {
    env.storage().instance().get(&DataKey::ServicePubKey)
}

pub fn set_service_pubkey(env: &Env, pubkey: &Bytes) {
    env.storage().instance().set(&DataKey::ServicePubKey, pubkey);
}

// ── Hysteresis margin ─────────────────────────────────────────────────────────

/// Returns the admin-configured hysteresis margin, defaulting to 0 (no
/// hysteresis — entry and exit thresholds are identical).
pub fn get_hysteresis_margin(env: &Env) -> u32 {
    let result: Option<u32> = env.storage().instance().get(&DataKey::HysteresisMargin);
    result.unwrap_or(0)
}

pub fn set_hysteresis_margin(env: &Env, margin: u32) {
    env.storage().instance().set(&DataKey::HysteresisMargin, &margin);
}

// ── Per-(wallet, asset_pair) risk band state ──────────────────────────────────

/// Returns `true` when `wallet` is currently inside the high-risk band for
/// `asset_pair`. Defaults to `false` when no state exists (first evaluation
/// or after TTL expiry). Extends the TTL on each read so active wallets keep
/// their state alive.
pub fn get_risk_band_state(env: &Env, wallet: &Address, asset_pair: &Symbol) -> bool {
    let key = DataKey::RiskBandState(wallet.clone(), asset_pair.clone());
    let result: Option<bool> = env.storage().temporary().get(&key);
    if result.is_some() {
        env.storage()
            .temporary()
            .extend_ttl(&key, BAND_STATE_TTL_THRESHOLD, BAND_STATE_TTL_EXTEND_TO);
    }
    result.unwrap_or(false)
}

/// Strictly read-only band state lookup that, unlike [`get_risk_band_state`],
/// does **not** extend the entry's TTL. Used by the infallible cross-contract
/// gate (`query_risk_gate`) so that calling it from another contract's guard
/// clause has no observable side effect on this contract's storage.
pub fn peek_risk_band_state(env: &Env, wallet: &Address, asset_pair: &Symbol) -> bool {
    let key = DataKey::RiskBandState(wallet.clone(), asset_pair.clone());
    let result: Option<bool> = env.storage().temporary().get(&key);
    result.unwrap_or(false)
}

/// Sets the risk band state for `(wallet, asset_pair)`. Passing `true`
/// records that the wallet has entered the high-risk band; passing `false`
/// removes the entry (equivalent to `false`, the default) so storage is not
/// wasted on cleared state.
pub fn set_risk_band_state(env: &Env, wallet: &Address, asset_pair: &Symbol, in_band: bool) {
    let key = DataKey::RiskBandState(wallet.clone(), asset_pair.clone());
    if in_band {
        env.storage().temporary().set(&key, &true);
        env.storage()
            .temporary()
            .extend_ttl(&key, BAND_STATE_TTL_THRESHOLD, BAND_STATE_TTL_EXTEND_TO);
    } else {
        env.storage().temporary().remove(&key);
    }
}
