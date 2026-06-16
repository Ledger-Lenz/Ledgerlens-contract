use soroban_sdk::{Address, Env, Symbol, Vec};

use crate::constants::{
    DEFAULT_RISK_THRESHOLD, HISTORY_MAX_DEPTH, SCORE_TTL_EXTEND_TO, SCORE_TTL_THRESHOLD,
};
use crate::types::{DataKey, RiskScore};

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

pub fn increment_score_count(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let key = DataKey::ScoreCount(wallet.clone(), asset_pair.clone());
    let current: u32 = env.storage().persistent().get(&key).unwrap_or(0);
    env.storage().persistent().set(&key, &(current + 1));
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

pub fn get_score_count(env: &Env, wallet: &Address, asset_pair: &Symbol) -> u32 {
    let key = DataKey::ScoreCount(wallet.clone(), asset_pair.clone());
    let count: u32 = env.storage().persistent().get(&key).unwrap_or(0);
    if count > 0 {
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }
    count
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

    // Evict oldest entry when the ring exceeds the depth cap.
    while history.len() > HISTORY_MAX_DEPTH {
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

// ── Contract version ─────────────────────────────────────────────────────────

pub fn get_contract_version(env: &Env) -> u32 {
    let result: Option<u32> = env.storage().instance().get(&DataKey::ContractVersion);
    result.unwrap_or(crate::constants::CONTRACT_VERSION)
}
