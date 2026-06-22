use soroban_sdk::{Address, Bytes, Env, Symbol, Vec};

use crate::constants::{
    BAND_STATE_TTL_EXTEND_TO, BAND_STATE_TTL_THRESHOLD, DEFAULT_CONSENSUS_EPSILON,
    DEFAULT_CONSENSUS_THRESHOLD_K, DEFAULT_COOLDOWN_SECS, DEFAULT_ESCALATION_THRESHOLD,
    DEFAULT_JUMP_THRESHOLD, DEFAULT_RISK_THRESHOLD, DEFAULT_UPGRADE_DELAY_SECS, EMBARGO_TTL_EXTEND_TO,
    EMBARGO_TTL_THRESHOLD, ESCALATION_BREACH_TTL_EXTEND_TO, ESCALATION_BREACH_TTL_THRESHOLD,
    SCORE_TTL_EXTEND_TO, SCORE_TTL_THRESHOLD,
};
use crate::errors::Error;
use crate::types::{
    AggregateRiskScore, DataKey, EmbargoExpiry, ModelVersionStats, RiskScore, ScoreFloorPolicy,
    ScoreTrend, UpgradeProposal,
};

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
    env.storage().instance().get(&DataKey::ServiceThreshold).unwrap_or(0)
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

pub fn get_service_pubkey(env: &Env) -> Option<Bytes> {
    env.storage().instance().get(&DataKey::ServicePubKey)
}

pub fn set_service_pubkey(env: &Env, pubkey: &Bytes) {
    env.storage().instance().set(&DataKey::ServicePubKey, pubkey);
}

// ── Confidence-gated query gate allowlist ────────────────────────────────────

pub fn get_gate_open(env: &Env) -> bool {
    env.storage().instance().get(&DataKey::GateOpen).unwrap_or(true)
}

pub fn get_gate_callers(env: &Env) -> Vec<Address> {
    env.storage().instance().get(&DataKey::GateCallers).unwrap_or_else(|| Vec::new(env))
}

pub fn set_gate_callers(env: &Env, callers: &Vec<Address>) {
    env.storage().instance().set(&DataKey::GateCallers, callers);
}

// ── Time-weighted exponential decay ──────────────────────────────────────────

pub fn get_decay_rate(env: &Env) -> (u32, u32) {
    env.storage()
        .instance()
        .get::<_, (u32, u32)>(&DataKey::DecayRate)
        .unwrap_or((
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

// ── Fee withdrawal ────────────────────────────────────────────────────────────

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
            return Err(Error::CounterpartyLinkFull);
        }
        links_a.push_back(wallet_b.clone());
        let key_a = DataKey::Counterparties(wallet_a.clone(), asset_pair.clone());
        env.storage().persistent().set(&key_a, &links_a);
        env.storage().persistent().extend_ttl(&key_a, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }

    let mut links_b = get_counterparties(env, wallet_b, asset_pair);
    if !links_b.contains(wallet_a) {
        if links_b.len() >= crate::constants::MAX_COUNTERPARTY_LINKS_PER_WALLET {
            return Err(Error::CounterpartyLinkFull);
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
    let result: Option<(bool, u32, u32)> =
        env.storage().instance().get(&DataKey::ScoreFloorConfig);
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
    env.storage().instance().set(&DataKey::ScoreFloorConfig, &(enabled, high_water_mark, floor_value));
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
        env.storage()
            .temporary()
            .extend_ttl(&key, BAND_STATE_TTL_THRESHOLD, BAND_STATE_TTL_EXTEND_TO);
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
        env.storage()
            .temporary()
            .extend_ttl(&key, BAND_STATE_TTL_THRESHOLD, BAND_STATE_TTL_EXTEND_TO);
    } else {
        env.storage().temporary().remove(&key);
    }
}

// ── Score embargo ─────────────────────────────────────────────────────────────

pub fn set_embargo(env: &Env, wallet: &Address, expiry: &EmbargoExpiry) {
    let key = DataKey::ScoreEmbargo(wallet.clone());
    env.storage().temporary().set(&key, expiry);
    env.storage()
        .temporary()
        .extend_ttl(&key, EMBARGO_TTL_THRESHOLD, EMBARGO_TTL_EXTEND_TO);
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
            env.storage()
                .temporary()
                .extend_ttl(&key, EMBARGO_TTL_THRESHOLD, EMBARGO_TTL_EXTEND_TO);
            true
        }
        Some(EmbargoExpiry::Until(ts)) => {
            let now = env.ledger().timestamp();
            let active = now <= ts;
            if active {
                env.storage()
                    .temporary()
                    .extend_ttl(&key, EMBARGO_TTL_THRESHOLD, EMBARGO_TTL_EXTEND_TO);
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
    env.storage()
        .instance()
        .get(&DataKey::ConsensusEpsilon)
        .unwrap_or(DEFAULT_CONSENSUS_EPSILON)
}

pub fn set_consensus_epsilon(env: &Env, epsilon: u32) {
    env.storage().instance().set(&DataKey::ConsensusEpsilon, &epsilon);
}

// ── Breach count (consecutive threshold breaches for escalation) ────────────

pub fn get_breach_count(env: &Env, wallet: &Address, asset_pair: &Symbol) -> u32 {
    let key = DataKey::BreachCount(wallet.clone(), asset_pair.clone());
    let result: Option<u32> = env.storage().persistent().get(&key);
    if result.is_some() {
        env.storage()
            .persistent()
            .extend_ttl(&key, ESCALATION_BREACH_TTL_THRESHOLD, ESCALATION_BREACH_TTL_EXTEND_TO);
    }
    result.unwrap_or(0)
}

pub fn set_breach_count(env: &Env, wallet: &Address, asset_pair: &Symbol, count: u32) {
    let key = DataKey::BreachCount(wallet.clone(), asset_pair.clone());
    env.storage().persistent().set(&key, &count);
    env.storage()
        .persistent()
        .extend_ttl(&key, ESCALATION_BREACH_TTL_THRESHOLD, ESCALATION_BREACH_TTL_EXTEND_TO);
}

pub fn clear_breach_count(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let key = DataKey::BreachCount(wallet.clone(), asset_pair.clone());
    env.storage().persistent().remove(&key);
}

// ── Escalation threshold ──────────────────────────────────────────────────────

pub fn get_escalation_threshold(env: &Env) -> u32 {
    env.storage().instance().get(&DataKey::EscalationThreshold).unwrap_or(DEFAULT_ESCALATION_THRESHOLD)
}

pub fn set_escalation_threshold(env: &Env, threshold: u32) {
    env.storage().instance().set(&DataKey::EscalationThreshold, &threshold);
}

// ── Model statistics (per version) ────────────────────────────────────────────

pub fn update_model_stats(env: &Env, model_version: u32, score: u32) {
    let key = DataKey::ModelStats(model_version);
    let now = env.ledger().timestamp();
    let mut stats: ModelVersionStats =
        env.storage().instance().get(&key).unwrap_or(ModelVersionStats {
            model_version,
            submission_count: 0,
            score_sum: 0,
            score_max: 0,
            score_min: u32::MAX,
            first_seen: now,
            last_seen: 0,
        });
    stats.submission_count += 1;
    stats.score_sum = stats.score_sum.wrapping_add(score as u64);
    if score > stats.score_max {
        stats.score_max = score;
    }
    if score < stats.score_min {
        stats.score_min = score;
    }
    stats.last_seen = now;
    env.storage().instance().set(&key, &stats);

    let mut all: Vec<u32> =
        env.storage().instance().get(&DataKey::AllModelVersions).unwrap_or_else(|| Vec::new(env));
    if !all.contains(&model_version) {
        let mut inserted = false;
        for i in 0..all.len() {
            if all.get(i).unwrap() > model_version {
                all.insert(i, model_version);
                inserted = true;
                break;
            }
        }
        if !inserted {
            all.push_back(model_version);
        }
        env.storage().instance().set(&DataKey::AllModelVersions, &all);
    }
}

pub fn get_model_stats(env: &Env, model_version: u32) -> Option<ModelVersionStats> {
    env.storage().instance().get(&DataKey::ModelStats(model_version))
}

pub fn get_all_model_versions(env: &Env) -> Vec<u32> {
    env.storage().instance().get(&DataKey::AllModelVersions).unwrap_or_else(|| Vec::new(env))
}

// ── Signer TTL (Issue #79) ──────────────────────────────────────────────────

pub fn get_signer_ttl(env: &Env) -> u64 {
    let result: Option<(u64, u64)> = env.storage().instance().get(&DataKey::SignerTtl);
    result.map(|(ttl, _)| ttl).unwrap_or(0)
}

pub fn set_signer_ttl(env: &Env, ttl: u64) {
    let grace = get_signer_grace_period(env);
    env.storage().instance().set(&DataKey::SignerTtl, &(ttl, grace));
}

pub fn get_signer_grace_period(env: &Env) -> u64 {
    let result: Option<(u64, u64)> = env.storage().instance().get(&DataKey::SignerTtl);
    result.map(|(_, grace)| grace).unwrap_or(604_800)
}

pub fn set_signer_grace_period(env: &Env, grace: u64) {
    let ttl = get_signer_ttl(env);
    env.storage().instance().set(&DataKey::SignerTtl, &(ttl, grace));
}

pub fn get_signer_activation_time(env: &Env, signer: &Address) -> Option<u64> {
    env.storage().instance().get(&DataKey::SignerActivationTime(signer.clone()))
}

pub fn set_signer_activation_time(env: &Env, signer: &Address, timestamp: u64) {
    env.storage().instance().set(&DataKey::SignerActivationTime(signer.clone()), &timestamp);
}

pub fn clear_signer_activation_time(env: &Env, signer: &Address) {
    env.storage().instance().remove(&DataKey::SignerActivationTime(signer.clone()));
}

// ── Score histogram (Issue #81) — to be implemented ─────────────────────────
