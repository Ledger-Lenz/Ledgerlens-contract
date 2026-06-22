use soroban_sdk::{Env, Address};
use crate::types::{DataKey, TierBounds};
use crate::errors::Error;

use crate::constants::{
    DEFAULT_COOLDOWN_SECS, DEFAULT_ESCALATION_THRESHOLD, DEFAULT_RISK_THRESHOLD,
    DEFAULT_UPGRADE_DELAY_SECS, SCORE_TTL_EXTEND_TO, SCORE_TTL_THRESHOLD,
};
use crate::types::{AggregateRiskScore, DataKey, RiskScore, ScoreTrend, UpgradeProposal, SnapshotRecord};

use crate::Error;

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

// ── Per-asset-pair circuit breaker ───────────────────────────────────────────

/// Returns `true` only if `asset_pair` has been explicitly paused and not
/// since unpaused. This is the hot path consulted on every `submit_score` /
/// `submit_scores_batch` entry, so it is a direct key lookup — it never
/// touches `PausedPairIndex`.
pub fn is_pair_paused(env: &Env, asset_pair: &Symbol) -> bool {
    let key = DataKey::PairPaused(asset_pair.clone());
    let result: Option<bool> = env.storage().persistent().get(&key);
    if result.is_some() {
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }
    result.unwrap_or(false)
}

/// Raw flag setter, mirroring `set_watchlist`'s pattern: stores `true` (and
/// bumps TTL) when paused, removes the key entirely when unpaused so an
/// unpaused pair costs nothing in storage. Does **not** touch
/// `PausedPairIndex` — callers (`set_pair_paused`) are responsible for
/// keeping the index consistent via `add_to_paused_index` /
/// `remove_from_paused_index`.
pub fn set_pair_paused_flag(env: &Env, asset_pair: &Symbol, paused: bool) {
    let key = DataKey::PairPaused(asset_pair.clone());
    if paused {
        env.storage().persistent().set(&key, &true);
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    } else {
        env.storage().persistent().remove(&key);
    }
}

/// Returns every currently paused asset pair. O(1) storage read — the index
/// is maintained incrementally by `add_to_paused_index` /
/// `remove_from_paused_index` rather than rebuilt by scanning.
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

/// Adds `asset_pair` to `PausedPairIndex` if it isn't already present.
/// Returns `false` (without modifying the index) if the pair is new *and*
/// the index is already at `MAX_PAUSED_PAIRS` — the caller turns that into
/// `Error::PausedPairIndexFull`. Re-adding a pair already in the index is a
/// no-op that returns `true`, so this is safe to call unconditionally.
///
/// O(N) in the number of currently paused pairs, but only on this
/// infrequent admin-only path — the per-submission hot path
/// (`is_pair_paused`) never iterates the index.
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

/// Removes `asset_pair` from `PausedPairIndex`. No-op if it isn't present.
/// Same O(N) admin-only-path tradeoff as `add_to_paused_index`.
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

pub fn get_signer_tier(env: &Env, signer: &Address) -> TierBounds {
    env.storage()
        .instance()
        .get(&DataKey::SignerTier(signer.clone()))
        .unwrap_or(TierBounds { min_score: 0, max_score: 100 })
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

// ── Score embargo (regulatory hold) ──────────────────────────────────────────

pub fn set_score_embargo(env: &Env, wallet: &Address, expiry: &Option<u64>) {
    let key = DataKey::ScoreEmbargo(wallet.clone());
    env.storage().persistent().set(&key, expiry);
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

#[allow(dead_code)]
pub fn get_score_embargo(env: &Env, wallet: &Address) -> Option<Option<u64>> {
    let key = DataKey::ScoreEmbargo(wallet.clone());
    let result: Option<Option<u64>> = env.storage().persistent().get(&key);
    if result.is_some() {
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }
    result
}

pub fn remove_score_embargo(env: &Env, wallet: &Address) {
    let key = DataKey::ScoreEmbargo(wallet.clone());
    env.storage().persistent().remove(&key);
}

/// Returns `true` when the wallet is under an active, non-expired embargo.
pub fn is_embargoed(env: &Env, wallet: &Address) -> bool {
    match env.storage().persistent().get::<_, Option<u64>>(&DataKey::ScoreEmbargo(wallet.clone())) {
        None => false,
        Some(None) => true, // indefinite embargo
        Some(Some(expiry)) => env.ledger().timestamp() < expiry,
    }
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

// ── Score attestation ─────────────────────────────────────────────────────

/// Returns the off-chain detection pipeline's secp256k1 public key, or
/// `None` if `set_service_pubkey` has never been called.
pub fn get_service_pubkey(env: &Env) -> Option<Bytes> {
    env.storage().instance().get(&DataKey::ServicePubKey)
}

pub fn set_gate_callers(env: &Env, callers: &Vec<Address>) {
    env.storage().instance().set(&GateDataKey::GateCallers, callers);
}

// ── Time-weighted exponential decay ──────────────────────────────────────

/// Returns the numerator and denominator of the configured decay rate λ.
/// Defaults to (0, 1) when unset, representing no decay.
pub fn get_decay_rate(env: &Env) -> (u32, u32) {
    let num = env
        .storage()
        .instance()
        .get(&DataKey::DecayRateNumerator)
        .unwrap_or(crate::constants::DEFAULT_DECAY_LAMBDA_NUM);
    let den = env
        .storage()
        .instance()
        .get(&DataKey::DecayRateDenominator)
        .unwrap_or(crate::constants::DEFAULT_DECAY_LAMBDA_DEN);
    (num, den)
}

/// Sets the decay rate to numerator/denominator.
pub fn set_decay_rate(env: &Env, numerator: u32, denominator: u32) {
    env.storage().instance().set(&DataKey::DecayRateNumerator, &numerator);
    env.storage().instance().set(&DataKey::DecayRateDenominator, &denominator);
}

 feat/confidence-gated-risk-gate
// ── Global minimum confidence floor ──────────────────────────────────────────

/// Returns the admin-configured global minimum confidence floor (0–100).
/// Defaults to `0` (no floor) when unset.
///
/// This value is combined with the per-call `min_confidence` parameter in
/// `query_risk_gate_with_confidence` using `max(param, global)` so the admin
/// can enforce a system-wide floor without requiring every integrating protocol
/// to specify one. Both values are bounded to 0–100, so the `max` cannot
/// overflow.
pub fn get_global_min_confidence(env: &Env) -> u32 {
    let result: Option<u32> = env.storage().instance().get(&DataKey::GlobalMinConfidence);
    result.unwrap_or(0)
}

/// Persists `min_confidence` as the global confidence floor.
/// Caller is responsible for validating the range (0–100) before calling.
pub fn set_global_min_confidence(env: &Env, min_confidence: u32) {
    env.storage().instance().set(&DataKey::GlobalMinConfidence, &min_confidence);

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
 main
}

// ── Score delegation ──────────────────────────────────────────────────────────

/// Returns the custodian wallet delegated for `sub_wallet`, if any.
/// Extends TTL on read.
pub fn get_score_delegate(env: &Env, sub_wallet: &Address) -> Option<Address> {
    let key = DataKey::ScoreDelegate(sub_wallet.clone());
    let result: Option<Address> = env.storage().persistent().get(&key);
    if result.is_some() {
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }
    result
}

/// Read-only delegate lookup that does **not** extend TTL — used by the
/// infallible cross-contract gate path.
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

/// Returns the list of counterparties for a wallet on a specific asset pair.
/// Returns an empty Vec if no links exist.
pub fn get_counterparties(env: &Env, wallet: &Address, asset_pair: &Symbol) -> Vec<Address> {
    let key = DataKey::Counterparties(wallet.clone(), asset_pair.clone());
    env.storage().persistent().get(&key).unwrap_or_else(|| Vec::new(env))
}

/// Adds a bidirectional counterparty link between wallet_a and wallet_b.
///
/// # Errors
/// - Returns `Error::SelfLink` if wallet_a == wallet_b
/// - Returns `Error::CounterpartyLinkFull` if wallet_a or wallet_b would exceed MAX_COUNTERPARTY_LINKS_PER_WALLET
pub fn add_counterparty_link(
    env: &Env,
    wallet_a: &Address,
    wallet_b: &Address,
    asset_pair: &Symbol,
) -> Result<(), Error> {
    if wallet_a == wallet_b {
        return Err(Error::SelfLink);
    }

    // Check and update wallet_a's counterparties
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

    // Check and update wallet_b's counterparties
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

/// Removes a bidirectional counterparty link between wallet_a and wallet_b.
///
/// # Errors
/// - Returns `Error::CounterpartyNotFound` if the link does not exist in either direction
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
        return Err(Error::CounterpartyNotFound);
    }

    Ok(())
}

/// Returns the number of registered counterparties (graph degree) for a wallet/pair.
pub fn get_contagion_depth(env: &Env, wallet: &Address, asset_pair: &Symbol) -> u32 {
    let key = DataKey::Counterparties(wallet.clone(), asset_pair.clone());
    let links: Vec<Address> = env.storage().persistent().get(&key).unwrap_or_else(|| Vec::new(env));
    links.len()
}
