use crate::constants::{
    BAND_STATE_TTL_EXTEND_TO, BAND_STATE_TTL_THRESHOLD, DEFAULT_CONSENSUS_EPSILON,
    DEFAULT_CONSENSUS_THRESHOLD_K, DEFAULT_COOLDOWN_SECS, DEFAULT_JUMP_THRESHOLD,
    DEFAULT_QUORUM_FAILURE_WINDOW_SECS, DEFAULT_RISK_THRESHOLD, DEFAULT_UPGRADE_DELAY_SECS,
    EMBARGO_TTL_EXTEND_TO, EMBARGO_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO, SCORE_TTL_THRESHOLD,
};
use crate::errors::Error;
use crate::types::{
    AdaptiveRateLimit, AggregateRiskScore, DataKey, DataKeyB, DataKeyC, DataKeyD, DecayCurve,
    EmbargoExpiry, FlashProtectionMode, GateDataKey, HllSketch, InterpolationMethod, JumpStats,
    ModelVersionStats, ModelVersionStatus, PairVolatilityState, ParamChangeProposal,
    ParameterProposalRecord, ParameterProposalStatus, PendingScoreEntry, RateLimitOverrideEntry,
    RiskScore, ScoreDispute, ScoreFloorPolicy, ScoreHistogram, ScoreTrend, ScoreVelocityCap,
    SignerAccuracyRecord, SubscorePayload, TokenBucket, UpgradeProposal, WelfordCorrState,
};
use soroban_sdk::{Address, Bytes, BytesN, Env, Symbol, Vec};

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

// ── Differential privacy ─────────────────────────────────────────────────────

pub fn set_privacy_epsilon(env: &Env, epsilon_scaled: u32) {
    env.storage().instance().set(&DataKeyC::PrivacyEpsilon, &epsilon_scaled);
}

pub fn get_privacy_epsilon(env: &Env) -> u32 {
    env.storage().instance().get(&DataKeyC::PrivacyEpsilon).unwrap_or(0)
}

// ── Latest score ─────────────────────────────────────────────────────────────

pub fn set_score(env: &Env, wallet: &Address, asset_pair: &Symbol, score: &RiskScore) {
    let key = DataKey::Score(wallet.clone(), asset_pair.clone());
    env.storage().persistent().set(&key, score);
    // Lazy TTL extension: only renew the score entry when the touch marker shows
    // SCORE_TTL_THRESHOLD ledgers have elapsed since the last write. Strict `>=`
    // on elapsed so entries at exactly the threshold still renew. Untracked
    // entries (first write) always extend.
    let needs_extend = match ledgers_since_touch(env, wallet, asset_pair) {
        None => true,
        Some(elapsed) => elapsed >= SCORE_TTL_THRESHOLD,
    };
    if needs_extend {
        extend_persistent_ttl(env, &key);
    }
    track_score_entry(env, wallet, asset_pair);
}

/// Eager TTL path retained for instruction-count regression tests only.
#[cfg(test)]
pub fn set_score_eager_ttl(env: &Env, wallet: &Address, asset_pair: &Symbol, score: &RiskScore) {
    let key = DataKey::Score(wallet.clone(), asset_pair.clone());
    env.storage().persistent().set(&key, score);
    extend_persistent_ttl(env, &key);
    track_score_entry(env, wallet, asset_pair);
    let touch_key = DataKeyB::ScoreEntryLastTouchedLedger(wallet.clone(), asset_pair.clone());
    extend_persistent_ttl(env, &touch_key);
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

// ── Proactive TTL rent management ────────────────────────────────────────────
//
// Soroban contracts have no host function to read another entry's remaining
// TTL, so this module can't ask "how close to expiry is this entry?"
// directly. Instead it tracks the ledger sequence at which each entry was
// last written or proactively renewed (`ScoreEntryLastTouchedLedger`) and
// estimates remaining TTL from elapsed ledgers since that touch, against the
// same `SCORE_TTL_THRESHOLD` the live entry's own TTL is bounded by.
//
// This is a conservative estimate, not the literal on-chain TTL: immediately
// after a touch, `extend_ttl(SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO)`
// guarantees the entry's actual remaining TTL is at least
// `SCORE_TTL_THRESHOLD` ledgers. So flagging an entry "due" once that many
// ledgers have elapsed since its last touch can only run early, never late.

/// Returns every (wallet, asset_pair) entry tracked for proactive rent
/// management. O(1) storage read — the index is maintained incrementally by
/// `track_score_entry`.
pub fn get_score_entry_index(env: &Env) -> Vec<(Address, Symbol)> {
    let index: Vec<(Address, Symbol)> =
        env.storage().persistent().get(&DataKeyB::ScoreEntryIndex).unwrap_or_else(|| Vec::new(env));
    if !index.is_empty() {
        env.storage().persistent().extend_ttl(
            &DataKeyB::ScoreEntryIndex,
            SCORE_TTL_THRESHOLD,
            SCORE_TTL_EXTEND_TO,
        );
    }
    index
}

/// Registers `(wallet, asset_pair)` in the rent-management index — if it
/// isn't already present and the index has room — and stamps its
/// last-touched ledger to now. Called from `set_score` so every write is
/// automatically covered, and from `extend_score_entry_ttl` when the admin
/// proactively renews an entry.
///
/// Silently leaves the index untouched once it holds
/// `MAX_TRACKED_SCORE_ENTRIES` entries — newly written entries beyond that
/// cap still get their TTL extended by `set_score` itself, they just aren't
/// visible to `get_expiring_entries`'s sweep. An already-tracked entry's
/// last-touched ledger is always refreshed regardless of index capacity.
pub fn track_score_entry(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let entry = (wallet.clone(), asset_pair.clone());
    reindex_entry_to_back(env, &entry);
    touch_score_entry(env, wallet, asset_pair);
}

/// Moves `entry` to the back of the rent-management index, or appends it if
/// it isn't tracked yet and the index has room. Called every time an
/// entry's last-touched ledger is refreshed (`track_score_entry` and
/// `extend_score_entry_ttl`), so the index is always maintained as a queue
/// ordered from least-recently-touched (front) to most-recently-touched
/// (back) — i.e. in descending order of estimated remaining TTL.
///
/// `get_expiring_entries` relies on this invariant to stop scanning as soon
/// as it reaches an entry that isn't due yet: everything after it was
/// touched more recently and can't be due either.
fn reindex_entry_to_back(env: &Env, entry: &(Address, Symbol)) {
    let mut index = get_score_entry_index(env);
    match index.first_index_of(entry) {
        Some(pos) => {
            index.remove(pos);
            index.push_back(entry.clone());
        }
        None => {
            if index.len() >= crate::constants::MAX_TRACKED_SCORE_ENTRIES {
                return;
            }
            index.push_back(entry.clone());
        }
    }
    env.storage().persistent().set(&DataKeyB::ScoreEntryIndex, &index);
    env.storage().persistent().extend_ttl(
        &DataKeyB::ScoreEntryIndex,
        SCORE_TTL_THRESHOLD,
        SCORE_TTL_EXTEND_TO,
    );
}

fn touch_score_entry(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let key = DataKeyB::ScoreEntryLastTouchedLedger(wallet.clone(), asset_pair.clone());
    let had_touch = env.storage().persistent().has(&key);
    env.storage().persistent().set(&key, &env.ledger().sequence());
    // Lazy TTL on the touch marker: skip extend while the entry is still tracked.
    if !had_touch {
        extend_persistent_ttl(env, &key);
    }
}

/// Extends a persistent-storage entry's TTL using the standard score-entry
/// threshold/extend-to window.
fn extend_persistent_ttl<K: soroban_sdk::IntoVal<Env, soroban_sdk::Val>>(env: &Env, key: &K) {
    #[cfg(test)]
    {
        let count = test_extend_count(env);
        env.storage().instance().set(&test_instrumentation::TestKey::ExtendCount, &(count + 1));
    }
    env.storage().persistent().extend_ttl(key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

/// Test-only instrumentation: number of times `extend_persistent_ttl` has
/// been called since the last `reset_test_extend_count`. Used by TTL-cost
/// regression tests to compare eager vs. lazy extension strategies.
#[cfg(test)]
pub fn test_extend_count(env: &Env) -> u32 {
    env.storage().instance().get(&test_instrumentation::TestKey::ExtendCount).unwrap_or(0)
}

#[cfg(test)]
pub fn reset_test_extend_count(env: &Env) {
    env.storage().instance().set(&test_instrumentation::TestKey::ExtendCount, &0u32);
}

/// Ledgers elapsed since `(wallet, asset_pair)` was last touched, or `None`
/// if it has never been tracked.
fn ledgers_since_touch(env: &Env, wallet: &Address, asset_pair: &Symbol) -> Option<u32> {
    let key = DataKeyB::ScoreEntryLastTouchedLedger(wallet.clone(), asset_pair.clone());
    let last_touched: Option<u32> = env.storage().persistent().get(&key);
    last_touched.map(|last| env.ledger().sequence().saturating_sub(last))
}

/// Estimated number of ledgers remaining before `(wallet, asset_pair)`'s
/// score entry should be proactively renewed, floored at `0`. Returns `None`
/// if the entry has never been tracked. See the module doc comment above for
/// why this is a conservative estimate rather than the literal on-chain TTL.
pub fn estimate_entry_ttl(env: &Env, wallet: &Address, asset_pair: &Symbol) -> Option<u32> {
    ledgers_since_touch(env, wallet, asset_pair)
        .map(|elapsed| SCORE_TTL_THRESHOLD.saturating_sub(elapsed))
}

/// Returns up to `max_entries` tracked entries whose estimated remaining TTL
/// has dropped to or below `SCORE_TTL_THRESHOLD` — i.e. entries `set_score`'s
/// own extend-on-write would now renew if it were called again — ordered
/// most-urgent (longest elapsed since last touch) first.
///
/// `track_score_entry`/`extend_score_entry_ttl` maintain the index as a
/// queue ordered from least- to most-recently-touched (see
/// `reindex_entry_to_back`), which is the same thing as descending order of
/// elapsed-since-touch. That lets this scan stop the moment it reaches an
/// entry that isn't due yet, instead of always walking all
/// `MAX_TRACKED_SCORE_ENTRIES` entries: everything after it was touched
/// more recently and can't be due either. See `test_ttl_rent_manager` for
/// the instruction-count regression test against the old unconditional
/// full-scan-plus-selection-sort behavior (kept for comparison as
/// `get_expiring_entries_full_scan_baseline`, test-only).
pub fn get_expiring_entries(env: &Env, max_entries: u32) -> Vec<(Address, Symbol)> {
    let index = get_score_entry_index(env);
    let capped = max_entries.min(crate::constants::MAX_EXPIRING_ENTRIES_PER_CALL);

    let mut result = Vec::new(env);
    for i in 0..index.len() {
        if result.len() >= capped {
            break;
        }
        let (wallet, asset_pair) = index.get(i).unwrap();
        match ledgers_since_touch(env, &wallet, &asset_pair) {
            Some(elapsed) if elapsed >= SCORE_TTL_THRESHOLD => {
                result.push_back((wallet, asset_pair));
            }
            // Front-to-back the queue is sorted by descending elapsed time,
            // so the first not-yet-due entry means nothing after it is due
            // either — safe to stop here.
            _ => break,
        }
    }
    result
}

/// Pre-fix baseline kept for the instruction-count regression test in
/// `test_ttl_rent_manager`: unconditionally scans the entire index and
/// selection-sorts the due entries, regardless of how many are actually
/// due. This is what `get_expiring_entries` did before the index was
/// restructured into an expiry-ordered queue.
#[cfg(test)]
pub fn get_expiring_entries_full_scan_baseline(
    env: &Env,
    max_entries: u32,
) -> Vec<(Address, Symbol)> {
    let index = get_score_entry_index(env);
    let capped = max_entries.min(crate::constants::MAX_EXPIRING_ENTRIES_PER_CALL);

    let mut due: Vec<(u32, Address, Symbol)> = Vec::new(env);
    for i in 0..index.len() {
        let (wallet, asset_pair) = index.get(i).unwrap();
        if let Some(elapsed) = ledgers_since_touch(env, &wallet, &asset_pair) {
            if elapsed >= SCORE_TTL_THRESHOLD {
                due.push_back((elapsed, wallet, asset_pair));
            }
        }
    }

    let mut result = Vec::new(env);
    let take = capped.min(due.len());
    for _ in 0..take {
        let mut best_idx = 0;
        let mut best_elapsed = due.get(0).unwrap().0;
        for i in 1..due.len() {
            let elapsed = due.get(i).unwrap().0;
            if elapsed > best_elapsed {
                best_elapsed = elapsed;
                best_idx = i;
            }
        }
        let (_, wallet, asset_pair) = due.get(best_idx).unwrap();
        due.remove(best_idx);
        result.push_back((wallet, asset_pair));
    }
    result
}

/// Proactively renews `(wallet, asset_pair)`'s score entry TTL and refreshes
/// its tracked last-touched ledger, as if `set_score` had just written to
/// it. No-op if the entry doesn't actually have a live score (`peek_score`
/// returns `None`) — there's nothing on-chain to extend, and tracking a
/// never-written entry would let `get_expiring_entries` report ghosts.
/// Returns `true` if the entry existed and was renewed.
pub fn extend_score_entry_ttl(env: &Env, wallet: &Address, asset_pair: &Symbol) -> bool {
    if peek_score(env, wallet, asset_pair).is_none() {
        return false;
    }
    let key = DataKey::Score(wallet.clone(), asset_pair.clone());
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    reindex_entry_to_back(env, &(wallet.clone(), asset_pair.clone()));
    touch_score_entry(env, wallet, asset_pair);
    true
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

/// Returns `(max_jump, at_timestamp)` for the largest score-jump anomaly
/// observed so far for `(wallet, asset_pair)`, or `(0, 0)` if none has been
/// recorded.
pub fn get_jump_stats(env: &Env, wallet: &Address, asset_pair: &Symbol) -> (u32, u64) {
    let key = DataKey::JumpStats(wallet.clone(), asset_pair.clone());
    let stats: Option<JumpStats> = env.storage().persistent().get(&key);
    match stats {
        Some(stats) => (stats.max_jump, stats.at_timestamp),
        None => (0, 0),
    }
}

/// Records `jump` as the new largest observed jump for `(wallet, asset_pair)`
/// if it exceeds the currently stored maximum (or none is stored yet).
pub fn record_jump_stats(
    env: &Env,
    wallet: &Address,
    asset_pair: &Symbol,
    jump: u32,
    timestamp: u64,
) {
    let key = DataKey::JumpStats(wallet.clone(), asset_pair.clone());
    let current: Option<JumpStats> = env.storage().persistent().get(&key);
    let is_new_max = match &current {
        Some(stats) => jump > stats.max_jump,
        None => true,
    };
    if is_new_max {
        env.storage()
            .persistent()
            .set(&key, &JumpStats { max_jump: jump, at_timestamp: timestamp });
    }
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

// ── Score history ring buffer ────────────────────────────────────────────────

/// Pushes `score` onto the ring and evicts from the front until the ring is
/// back at `HistoryMaxDepth`.
///
/// When depth was just lowered a lot via `set_history_max_depth` (up to
/// 50 -> 1 in one time-locked change), this single call pays for evicting
/// all of the excess in one pass rather than spreading it across several
/// submissions. That single-pass cost is intentionally kept as-is rather
/// than spread out: it is already bounded by `MAX_HISTORY_DEPTH` (50) — at
/// most 49 `Vec::remove(0)` shifts on a 50-entry `Vec`, independent of how
/// many scores have ever been submitted — and spreading it out would break
/// the documented, tested guarantee that the ring is fully trimmed on the
/// very next write (see `test_set_history_max_depth_decreases_ring_on_next_write`
/// in `test.rs`). Measured worst-case vs. steady-state cost:
/// `benches/history_eviction.rs` (issue #424).
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

pub fn set_contract_version(env: &Env, contract_version: &u32) {
    env.storage().instance().set(&DataKey::ContractVersion, contract_version);
}

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

/// Returns `true` when a custom weight has been set for `asset_pair`.
pub fn has_pair_weight(env: &Env, asset_pair: &Symbol) -> bool {
    let key = DataKey::PairWeight(asset_pair.clone());
    env.storage().persistent().has(&key)
}

/// Removes the custom weight for `asset_pair`, causing `get_pair_weight` to
/// fall back to the default of `1`.
pub fn remove_pair_weight(env: &Env, asset_pair: &Symbol) {
    let key = DataKey::PairWeight(asset_pair.clone());
    env.storage().persistent().remove(&key);
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

// ── Parameter change governance ───────────────────────────────────────────────

pub fn next_parameter_proposal_id(env: &Env) -> u64 {
    let id: u64 = env.storage().instance().get(&DataKeyB::ParameterProposalNextId).unwrap_or(1);
    env.storage().instance().set(&DataKeyB::ParameterProposalNextId, &(id.saturating_add(1)));
    id
}

pub fn get_parameter_proposal_record(
    env: &Env,
    proposal_id: u64,
) -> Option<ParameterProposalRecord> {
    env.storage().instance().get(&DataKeyB::ParameterProposal(proposal_id))
}

pub fn set_parameter_proposal_record(
    env: &Env,
    proposal_id: u64,
    record: &ParameterProposalRecord,
) {
    env.storage().instance().set(&DataKeyB::ParameterProposal(proposal_id), record);
}

pub fn get_pending_parameter_proposal_ids(env: &Env) -> Vec<u64> {
    env.storage()
        .instance()
        .get(&DataKeyB::PendingParameterProposalIds)
        .unwrap_or_else(|| Vec::new(env))
}

pub fn set_pending_parameter_proposal_ids(env: &Env, ids: &Vec<u64>) {
    env.storage().instance().set(&DataKeyB::PendingParameterProposalIds, ids);
}

pub fn push_pending_parameter_proposal(env: &Env, proposal_id: u64) {
    let mut ids = get_pending_parameter_proposal_ids(env);
    ids.push_back(proposal_id);
    set_pending_parameter_proposal_ids(env, &ids);
}

pub fn remove_pending_parameter_proposal(env: &Env, proposal_id: u64) {
    let ids = get_pending_parameter_proposal_ids(env);
    let mut next = Vec::new(env);
    for i in 0..ids.len() {
        let id = ids.get(i).unwrap();
        if id != proposal_id {
            next.push_back(id);
        }
    }
    set_pending_parameter_proposal_ids(env, &next);
}

pub fn count_pending_parameter_proposals(env: &Env) -> u32 {
    get_pending_parameter_proposal_ids(env).len()
}

pub fn mark_parameter_proposal_status(
    env: &Env,
    proposal_id: u64,
    status: ParameterProposalStatus,
) -> Option<ParameterProposalRecord> {
    let mut record = get_parameter_proposal_record(env, proposal_id)?;
    record.status = status;
    set_parameter_proposal_record(env, proposal_id, &record);
    remove_pending_parameter_proposal(env, proposal_id);
    Some(record)
}

pub fn is_parameter_proposal_expired(proposal: &crate::types::ParameterProposal, now: u64) -> bool {
    let expiry = proposal.proposed_at.saturating_add(proposal.time_lock_secs.saturating_mul(2));
    now > expiry
}

/// Marks expired pending proposals and removes them from the pending index.
pub fn prune_expired_parameter_proposals(env: &Env) {
    let ids = get_pending_parameter_proposal_ids(env);
    let now = env.ledger().timestamp();
    for i in 0..ids.len() {
        let id = ids.get(i).unwrap();
        if let Some(record) = get_parameter_proposal_record(env, id) {
            if record.status == ParameterProposalStatus::Pending
                && is_parameter_proposal_expired(&record.proposal, now)
            {
                mark_parameter_proposal_status(env, id, ParameterProposalStatus::Expired);
            }
        }
    }
}

/// Seeds `count` pending proposals directly in storage for cap tests without
/// replaying the full propose flow (keeps Soroban test snapshots small).
#[cfg(test)]
pub fn test_seed_pending_parameter_proposals(
    env: &Env,
    count: u32,
    proposer: &Address,
    param_key: &Symbol,
    new_value: &Bytes,
) {
    use crate::types::{ParameterProposal, ParameterProposalRecord, ParameterProposalStatus};

    let now = env.ledger().timestamp();
    let time_lock_secs = get_upgrade_delay(env);
    for i in 1..=count {
        let proposal = ParameterProposal {
            param_key: param_key.clone(),
            new_value: new_value.clone(),
            proposer: proposer.clone(),
            proposed_at: now,
            time_lock_secs,
        };
        let record = ParameterProposalRecord { proposal, status: ParameterProposalStatus::Pending };
        set_parameter_proposal_record(env, i as u64, &record);
        push_pending_parameter_proposal(env, i as u64);
    }
    env.storage().instance().set(&DataKeyB::ParameterProposalNextId, &(count as u64 + 1));
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

pub fn set_signer_tier(env: &Env, signer: &Address, bounds: &crate::types::TierBounds) {
    env.storage().instance().set(&DataKey::SignerTier(signer.clone()), bounds);
}

pub fn set_service_threshold(env: &Env, threshold: u32) {
    env.storage().instance().set(&DataKey::ServiceThreshold, &threshold);
}

pub fn get_service_threshold(env: &Env) -> u32 {
    env.storage().instance().get(&DataKey::ServiceThreshold).unwrap_or(1)
}

// ── Escalation / breach count ─────────────────────────────────────────────────

pub fn get_escalation_threshold(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKeyC::EscalationThreshold)
        .unwrap_or(crate::constants::DEFAULT_ESCALATION_THRESHOLD)
}

pub fn set_escalation_threshold(env: &Env, n: u32) {
    env.storage().instance().set(&DataKeyC::EscalationThreshold, &n);
}

pub fn get_breach_count(env: &Env, wallet: &Address, asset_pair: &Symbol) -> u32 {
    let key = DataKeyC::BreachCount(wallet.clone(), asset_pair.clone());
    env.storage().temporary().get(&key).unwrap_or(0)
}

pub fn set_breach_count(env: &Env, wallet: &Address, asset_pair: &Symbol, count: u32) {
    let key = DataKeyC::BreachCount(wallet.clone(), asset_pair.clone());
    env.storage().temporary().set(&key, &count);
}

pub fn clear_breach_count(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let key = DataKeyC::BreachCount(wallet.clone(), asset_pair.clone());
    env.storage().temporary().remove(&key);
}

// ── Model stats ───────────────────────────────────────────────────────────────

pub fn update_model_stats(env: &Env, model_version: u32, score: u32) {
    let key = DataKeyB::ModelStats(model_version);
    let mut stats: ModelVersionStats =
        env.storage().instance().get(&key).unwrap_or(ModelVersionStats {
            model_version,
            submission_count: 0,
            score_sum: 0,
            total_submissions: 0,
            average_score: 0,
        });
    stats.submission_count += 1;
    stats.score_sum += score as u64;
    env.storage().instance().set(&key, &stats);

    let idx_key = DataKeyB::ModelVersionIndex;
    let mut versions: Vec<u32> =
        env.storage().instance().get(&idx_key).unwrap_or_else(|| Vec::new(env));
    if !versions.contains(model_version) && versions.len() < crate::constants::MAX_MODEL_VERSIONS {
        versions.push_back(model_version);
        env.storage().instance().set(&idx_key, &versions);
    }
}

pub fn get_model_stats(env: &Env, model_version: u32) -> Option<ModelVersionStats> {
    env.storage().instance().get(&DataKeyB::ModelStats(model_version))
}

pub fn get_all_model_versions(env: &Env) -> Vec<u32> {
    env.storage().instance().get(&DataKeyB::ModelVersionIndex).unwrap_or_else(|| Vec::new(env))
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

/// Returns the last accepted submission timestamp as `Some(ts)`, or `None` if
/// no submission has ever been recorded for `(wallet, asset_pair)`.
/// Does not extend the storage TTL.
pub fn get_last_submit_time_opt(env: &Env, wallet: &Address, asset_pair: &Symbol) -> Option<u64> {
    let key = DataKey::LastSubmitTime(wallet.clone(), asset_pair.clone());
    env.storage().persistent().get(&key)
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
        .get(&DataKeyC::PairCooldown(asset_pair.clone()))
        .unwrap_or_else(|| get_cooldown_secs(env))
}

pub fn set_pair_cooldown_secs(env: &Env, asset_pair: &Symbol, secs: u64) {
    env.storage().instance().set(&DataKeyC::PairCooldown(asset_pair.clone()), &secs);
}

pub fn clear_pair_cooldown_secs(env: &Env, asset_pair: &Symbol) {
    env.storage().instance().remove(&DataKeyC::PairCooldown(asset_pair.clone()));
}

// ── Adaptive rate limit ───────────────────────────────────────────────────────

pub fn get_adaptive_rate_limit(env: &Env) -> AdaptiveRateLimit {
    env.storage()
        .instance()
        .get(&DataKeyB::AdaptiveRateLimit)
        .unwrap_or(AdaptiveRateLimit { enabled: false, variance_scale: 0 })
}

pub fn set_adaptive_rate_limit(env: &Env, config: &AdaptiveRateLimit) {
    env.storage().instance().set(&DataKeyB::AdaptiveRateLimit, config);
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

pub fn get_unique_wallets_hll(env: &Env, asset_pair: &Symbol) -> Option<HllSketch> {
    let key = DataKeyB::UniqueWalletsHll(asset_pair.clone());
    let sketch: Option<HllSketch> = env.storage().persistent().get(&key);
    if sketch.is_some() {
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }
    sketch
}

pub fn set_unique_wallets_hll(env: &Env, asset_pair: &Symbol, sketch: &HllSketch) {
    let key = DataKeyB::UniqueWalletsHll(asset_pair.clone());
    env.storage().persistent().set(&key, sketch);
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

pub fn get_hll_precision(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKeyB::HllPrecision)
        .unwrap_or(crate::constants::HLL_DEFAULT_PRECISION)
}

pub fn set_hll_precision(env: &Env, precision: u32) {
    env.storage().instance().set(&DataKeyB::HllPrecision, &precision);
}

fn hll_new_sketch(env: &Env, precision: u32) -> HllSketch {
    let mut registers: Vec<u32> = Vec::new(env);
    let len = 1u32 << precision;
    for _ in 0..len {
        registers.push_back(0);
    }
    HllSketch { precision, registers }
}

fn hll_hash_wallet(env: &Env, wallet: &Address) -> [u8; 32] {
    let mut wallet_buf = [0u8; 56];
    let wallet_str = wallet.to_string();
    wallet_str.copy_into_slice(&mut wallet_buf);
    let len = wallet_str.len().min(56) as usize;
    let bytes = soroban_sdk::Bytes::from_slice(env, &wallet_buf[..len]);
    env.crypto().sha256(&bytes).to_bytes().to_array()
}

fn hll_register_index(hash: &[u8; 32], precision: u32) -> u32 {
    let mut index = 0u32;
    for bit in 0..precision as usize {
        let byte = hash[bit / 8];
        let bit_in_byte = 7 - (bit % 8);
        index = (index << 1) | (((byte >> bit_in_byte) & 1) as u32);
    }
    index
}

fn hll_rho(hash: &[u8; 32], precision: u32) -> u32 {
    let mut count = 1u32;
    let mut bit_pos = precision as usize;
    while bit_pos < 256 {
        let byte = hash[bit_pos / 8];
        let bit_in_byte = 7 - (bit_pos % 8);
        if ((byte >> bit_in_byte) & 1) == 1 {
            return count;
        }
        count = count.saturating_add(1);
        bit_pos += 1;
    }
    count
}

fn f64_ln(x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7FF) as i32 - 1023;
    let mantissa = f64::from_bits((bits & 0x000F_FFFF_FFFF_FFFF) | 0x3FF0_0000_0000_0000);
    let y = (mantissa - 1.0) / (mantissa + 1.0);
    let y2 = y * y;
    let mut y_pow = y;
    let mut series = y;
    let mut k = 3.0;
    while k <= 15.0 {
        y_pow *= y2;
        series += y_pow / k;
        k += 2.0;
    }
    2.0 * series + (exp as f64) * core::f64::consts::LN_2
}

fn f64_pow2_neg(r: u32) -> f64 {
    let mut p = 1.0f64;
    for _ in 0..r {
        p *= 2.0;
    }
    1.0 / p
}

fn hll_alpha(precision: u32) -> f64 {
    let m = 1u64 << precision;
    let m_f = m as f64;
    0.7213 / (1.0 + 1.079 / m_f)
}

pub fn hll_estimate(env: &Env, asset_pair: &Symbol) -> u64 {
    let sketch = match get_unique_wallets_hll(env, asset_pair) {
        Some(s) => s,
        None => return 0,
    };
    if sketch.precision == 0 || sketch.registers.is_empty() {
        return 0;
    }
    let m = 1u64 << sketch.precision;
    let mut sum = 0.0f64;
    let mut zeros = 0u32;
    for i in 0..sketch.registers.len() {
        let r = sketch.registers.get(i).unwrap();
        if r == 0 {
            zeros += 1;
        }
        sum += libm::pow(2.0, -(r as f64));
    }
    let estimate = hll_alpha(sketch.precision) * libm::pow(m as f64, 2.0) / sum;
    if zeros > 0 && estimate <= 2.5 * (m as f64) {
        let corrected = (m as f64) * libm::log((m as f64) / (zeros as f64));
        libm::round(corrected) as u64
    } else {
        libm::round(estimate) as u64
    }
}

pub fn hll_update(env: &Env, asset_pair: &Symbol, wallet: &Address) {
    let mut sketch = get_unique_wallets_hll(env, asset_pair)
        .unwrap_or_else(|| hll_new_sketch(env, get_hll_precision(env)));
    let hash = hll_hash_wallet(env, wallet);
    let idx = hll_register_index(&hash, sketch.precision);
    let rank = hll_rho(&hash, sketch.precision);
    if let Some(current) = sketch.registers.get(idx) {
        if rank > current {
            sketch.registers.set(idx, rank);
            set_unique_wallets_hll(env, asset_pair, &sketch);
        }
    }
}

// ── Score trend state ─────────────────────────────────────────────────────────

pub fn get_trend_state(env: &Env, wallet: &Address, asset_pair: &Symbol) -> ScoreTrend {
    let key = DataKeyC::TrendState(wallet.clone(), asset_pair.clone());
    let result: Option<ScoreTrend> = env.storage().persistent().get(&key);
    if result.is_some() {
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }
    result.unwrap_or(ScoreTrend { trend: 0, consecutive: 0 })
}

/// Like [`get_trend_state`] but preserves the distinction between "no trend
/// recorded yet" (`None`) and a stored trend, instead of collapsing the unset
/// case to a default flat trend.
pub fn get_trend_state_opt(env: &Env, wallet: &Address, asset_pair: &Symbol) -> Option<ScoreTrend> {
    let key = DataKeyC::TrendState(wallet.clone(), asset_pair.clone());
    let result: Option<ScoreTrend> = env.storage().persistent().get(&key);
    if result.is_some() {
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }
    result
}

pub fn set_trend_state(env: &Env, wallet: &Address, asset_pair: &Symbol, state: &ScoreTrend) {
    let key = DataKeyC::TrendState(wallet.clone(), asset_pair.clone());
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

// ── Signer nonce tracking ───────────────────────────────────────────────────

pub fn get_signer_nonce(env: &Env, signer: &Address) -> u64 {
    env.storage().instance().get(&DataKey::SignerNonce(signer.clone())).unwrap_or(0)
}

pub fn set_signer_nonce(env: &Env, signer: &Address, nonce: u64) {
    env.storage().instance().set(&DataKey::SignerNonce(signer.clone()), &nonce);
}

pub fn set_gate_callers(env: &Env, callers: &Vec<Address>) {
    env.storage().instance().set(&GateDataKey::GateCallers, callers);
}

pub fn get_gate_callers(env: &Env) -> Vec<Address> {
    env.storage().instance().get(&DataKeyC::GateCallers).unwrap_or_else(|| Vec::new(env))
}

pub fn set_gate_open(env: &Env, open: bool) {
    env.storage().instance().set(&DataKeyC::GateOpen, &open);
}

pub fn get_gate_open(env: &Env) -> bool {
    env.storage().instance().get(&DataKeyC::GateOpen).unwrap_or(true)
}

pub fn get_gate_enforcement_mode(env: &Env) -> bool {
    env.storage().instance().get(&GateDataKey::GateOpen).unwrap_or(false)
}

pub fn set_gate_enforcement_mode(env: &Env, strict: bool) {
    env.storage().instance().set(&GateDataKey::GateOpen, &strict);
}

// ── Time-weighted exponential decay ──────────────────────────────────────────

pub fn get_decay_rate(env: &Env) -> (u64, u64) {
    env.storage().instance().get::<_, (u64, u64)>(&DataKey::DecayRate).unwrap_or((
        crate::constants::DEFAULT_DECAY_LAMBDA_NUM,
        crate::constants::DEFAULT_DECAY_LAMBDA_DEN,
    ))
}

pub fn set_decay_rate(env: &Env, numerator: u64, denominator: u64) {
    env.storage().instance().set(&DataKey::DecayRate, &(numerator, denominator));
}

pub fn set_signer_tier_bounds(env: &Env, signer: &Address, min_score: u32, max_score: u32) {
    env.storage().instance().set(
        &DataKey::SignerTier(signer.clone()),
        &crate::types::TierBounds { min_score, max_score },
    );
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

pub fn get_fee_recipient(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::FeeRecipient)
}

pub fn set_fee_recipient(env: &Env, recipient: &Address) {
    env.storage().instance().set(&DataKey::FeeRecipient, recipient);
}

// ── Score delegation ──────────────────────────────────────────────────────────

pub fn get_score_delegate(env: &Env, sub_wallet: &Address) -> Option<Address> {
    let key = DataKeyC::ScoreDelegate(sub_wallet.clone());
    let result: Option<Address> = env.storage().persistent().get(&key);
    if result.is_some() {
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }
    result
}

pub fn peek_score_delegate(env: &Env, sub_wallet: &Address) -> Option<Address> {
    let key = DataKeyC::ScoreDelegate(sub_wallet.clone());
    env.storage().persistent().get(&key)
}

pub fn set_score_delegate(env: &Env, sub_wallet: &Address, custodian: &Address) {
    let key = DataKeyC::ScoreDelegate(sub_wallet.clone());
    env.storage().persistent().set(&key, custodian);
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

pub fn remove_score_delegate(env: &Env, sub_wallet: &Address) {
    let key = DataKeyC::ScoreDelegate(sub_wallet.clone());
    env.storage().persistent().remove(&key);
}

// ── Adaptive Threshold ─────────────────────────────────────────────────────

pub fn is_adaptive_threshold_enabled(env: &Env) -> bool {
    let result: Option<bool> = env.storage().instance().get(&DataKeyC::AdaptiveThresholdEnabled);
    result.unwrap_or(false)
}

pub fn set_adaptive_threshold_enabled(env: &Env, enabled: bool) {
    env.storage().instance().set(&DataKeyC::AdaptiveThresholdEnabled, &enabled);
}

pub fn get_adaptive_threshold_target_percentile(env: &Env) -> u32 {
    let result: Option<u32> = env.storage().instance().get(&DataKeyC::AdaptiveThresholdTargetPct);
    result.unwrap_or(0)
}

pub fn set_adaptive_threshold_target_percentile(env: &Env, percentile: u32) {
    env.storage().instance().set(&DataKeyC::AdaptiveThresholdTargetPct, &percentile);
}

pub fn get_adaptive_threshold_min_value(env: &Env) -> u32 {
    let result: Option<u32> = env.storage().instance().get(&DataKeyC::AdaptiveThresholdMinValue);
    result.unwrap_or(0)
}

pub fn set_adaptive_threshold_min_value(env: &Env, min: u32) {
    env.storage().instance().set(&DataKeyC::AdaptiveThresholdMinValue, &min);
}

pub fn get_adaptive_threshold_max_value(env: &Env) -> u32 {
    let result: Option<u32> = env.storage().instance().get(&DataKey::AdaptiveThresholdMaxValue);
    result.unwrap_or(100)
}

pub fn set_adaptive_threshold_max_value(env: &Env, max: u32) {
    env.storage().instance().set(&DataKey::AdaptiveThresholdMaxValue, &max);
}

pub fn get_last_computed_threshold(env: &Env) -> u32 {
    let result: Option<u32> = env.storage().instance().get(&DataKey::LastComputedThreshold);
    result.unwrap_or(0)
}

pub fn set_last_computed_threshold(env: &Env, threshold: u32) {
    env.storage().instance().set(&DataKey::LastComputedThreshold, &threshold);
}

pub fn get_adaptive_threshold_config(env: &Env) -> crate::types::AdaptiveThresholdConfig {
    crate::types::AdaptiveThresholdConfig {
        enabled: is_adaptive_threshold_enabled(env),
        target_percentile: get_adaptive_threshold_target_percentile(env),
        min_value: get_adaptive_threshold_min_value(env),
        max_value: get_adaptive_threshold_max_value(env),
        last_computed: get_last_computed_threshold(env),
    }
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

/// Like [`get_historical_max_score`] but returns `None` when no score has ever
/// been recorded for the pair instead of collapsing that case to `0`.
pub fn get_historical_max_score_opt(
    env: &Env,
    wallet: &Address,
    asset_pair: &Symbol,
) -> Option<u32> {
    let key = DataKey::HistoricalMaxScore(wallet.clone(), asset_pair.clone());
    let result: Option<u32> = env.storage().persistent().get(&key);
    if result.is_some() {
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }
    result
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
    let key = DataKeyB::ScoreEmbargo(wallet.clone());
    env.storage().temporary().set(&key, expiry);
    env.storage().temporary().extend_ttl(&key, EMBARGO_TTL_THRESHOLD, EMBARGO_TTL_EXTEND_TO);
}

pub fn remove_embargo(env: &Env, wallet: &Address) {
    let key = DataKeyB::ScoreEmbargo(wallet.clone());
    env.storage().temporary().remove(&key);
}

pub fn is_embargoed(env: &Env, wallet: &Address) -> bool {
    let key = DataKeyB::ScoreEmbargo(wallet.clone());
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
    let key = DataKeyB::ScoreEmbargo(wallet.clone());
    let expiry: Option<EmbargoExpiry> = env.storage().temporary().get(&key);
    match expiry {
        None => false,
        Some(EmbargoExpiry::Indefinite) => true,
        Some(EmbargoExpiry::Until(ts)) => env.ledger().timestamp() <= ts,
    }
}

/// Returns the expiry timestamp of `wallet`'s active embargo, if any.
///
/// - No embargo on record, or an expired timed embargo — `None`.
/// - Indefinite embargo — `None` (there is no timestamp to report).
/// - Active timed embargo — `Some(ts)`.
pub fn get_embargo_expiry(env: &Env, wallet: &Address) -> Option<u64> {
    let key = DataKeyB::ScoreEmbargo(wallet.clone());
    let expiry: Option<EmbargoExpiry> = env.storage().temporary().get(&key);
    match expiry {
        None => None,
        Some(EmbargoExpiry::Indefinite) => None,
        Some(EmbargoExpiry::Until(ts)) => {
            if env.ledger().timestamp() <= ts {
                Some(ts)
            } else {
                None
            }
        }
    }
}

pub fn get_embargoed_wallets(env: &Env) -> Vec<Address> {
    let wallets: Vec<Address> = env
        .storage()
        .temporary()
        .get(&DataKey::EmbargoedWalletIndex)
        .unwrap_or_else(|| Vec::new(env));
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

// ── Active embargo counter ────────────────────────────────────────────────────

pub fn get_active_embargo_count(env: &Env) -> u32 {
    let count: u32 = env.storage().persistent().get(&DataKey::ActiveEmbargoCount).unwrap_or(0);
    if count > 0 {
        env.storage().persistent().extend_ttl(
            &DataKey::ActiveEmbargoCount,
            EMBARGO_TTL_THRESHOLD,
            EMBARGO_TTL_EXTEND_TO,
        );
    }
    count
}

pub fn increment_active_embargo_count(env: &Env) {
    let new_count = get_active_embargo_count(env).saturating_add(1);
    env.storage().persistent().set(&DataKey::ActiveEmbargoCount, &new_count);
    env.storage().persistent().extend_ttl(
        &DataKey::ActiveEmbargoCount,
        EMBARGO_TTL_THRESHOLD,
        EMBARGO_TTL_EXTEND_TO,
    );
}

pub fn decrement_active_embargo_count(env: &Env) {
    let current = get_active_embargo_count(env);
    let new_count = current.saturating_sub(1);
    if new_count == 0 {
        env.storage().persistent().remove(&DataKey::ActiveEmbargoCount);
    } else {
        env.storage().persistent().set(&DataKey::ActiveEmbargoCount, &new_count);
        env.storage().persistent().extend_ttl(
            &DataKey::ActiveEmbargoCount,
            EMBARGO_TTL_THRESHOLD,
            EMBARGO_TTL_EXTEND_TO,
        );
    }
}

pub fn reset_active_embargo_count(env: &Env) {
    env.storage().persistent().remove(&DataKey::ActiveEmbargoCount);
}

// ── Band entry timestamp ──────────────────────────────────────────────────────

/// Returns the ledger timestamp at which `wallet` first entered the high-risk
/// band for `asset_pair`, or `None` when the wallet is not currently in the
/// band (never entered, or the entry time has been cleared on exit). Extends
/// TTL on read so active band memberships keep their entry time alive.
pub fn get_band_entry_time(env: &Env, wallet: &Address, asset_pair: &Symbol) -> Option<u64> {
    let key = DataKeyC::BandEntryTime(wallet.clone(), asset_pair.clone());
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
    let key = DataKeyC::BandEntryTime(wallet.clone(), asset_pair.clone());
    env.storage().temporary().set(&key, &timestamp);
    env.storage().temporary().extend_ttl(&key, BAND_STATE_TTL_THRESHOLD, BAND_STATE_TTL_EXTEND_TO);
}

/// Removes the band entry timestamp for `wallet` / `asset_pair`. Called when
/// the wallet exits the high-risk band so the key is absent whenever the
/// wallet is not in the band.
pub fn clear_band_entry_time(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let key = DataKeyC::BandEntryTime(wallet.clone(), asset_pair.clone());
    env.storage().temporary().remove(&key);
}

// ── Consensus configuration ─────────────────────────────────────────────────

pub fn get_consensus_threshold_k(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKeyB::ConsensusThresholdK)
        .unwrap_or(DEFAULT_CONSENSUS_THRESHOLD_K)
}

pub fn set_consensus_threshold_k(env: &Env, k: u32) {
    env.storage().instance().set(&DataKeyB::ConsensusThresholdK, &k);
}

pub fn get_consensus_epsilon(env: &Env) -> u32 {
    env.storage().instance().get(&DataKeyB::ConsensusEpsilon).unwrap_or(DEFAULT_CONSENSUS_EPSILON)
}

pub fn set_consensus_epsilon(env: &Env, epsilon: u32) {
    env.storage().instance().set(&DataKeyB::ConsensusEpsilon, &epsilon);
}

// ── Adaptive Epsilon (issue #204) ───────────────────────────────────────────

pub fn set_adaptive_epsilon_enabled(env: &Env, enabled: bool) {
    env.storage().instance().set(&DataKeyB::AdaptiveEpsilonEnabled, &enabled);
}

pub fn get_adaptive_epsilon_enabled(env: &Env) -> bool {
    env.storage().instance().get(&DataKeyB::AdaptiveEpsilonEnabled).unwrap_or(false)
}

pub fn set_adaptive_epsilon_bounds(env: &Env, min: u32, max: u32) {
    env.storage().instance().set(&DataKeyB::AdaptiveEpsilonMin, &min);
    env.storage().instance().set(&DataKeyB::AdaptiveEpsilonMax, &max);
}

pub fn get_adaptive_epsilon_min(env: &Env) -> u32 {
    env.storage().instance().get(&DataKeyB::AdaptiveEpsilonMin).unwrap_or(5)
}

pub fn get_adaptive_epsilon_max(env: &Env) -> u32 {
    env.storage().instance().get(&DataKeyB::AdaptiveEpsilonMax).unwrap_or(75)
}

pub fn set_adaptive_epsilon_scale_factor(env: &Env, scale_factor: u32) {
    env.storage().instance().set(&DataKeyB::AdaptiveEpsilonScaleFactor, &scale_factor);
}

pub fn get_adaptive_epsilon_scale_factor(env: &Env) -> u32 {
    env.storage().instance().get(&DataKeyB::AdaptiveEpsilonScaleFactor).unwrap_or(0)
}

// ── Score dispute mechanism ─────────────────────────────────────────────────────

/// Writes (or replaces) the open dispute record for `(wallet, asset_pair)` and
/// refreshes its TTL. Stored in temporary storage so abandoned disputes
/// eventually expire on their own.
pub fn set_dispute(env: &Env, wallet: &Address, asset_pair: &Symbol, dispute: &ScoreDispute) {
    let key = DataKeyB::ScoreDispute(wallet.clone(), asset_pair.clone());
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
    let key = DataKeyB::ScoreDispute(wallet.clone(), asset_pair.clone());
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
    let key = DataKeyB::ScoreDispute(wallet.clone(), asset_pair.clone());
    env.storage().temporary().remove(&key);
}

/// Returns every currently open dispute as `(challenger, asset_pair)` pairs.
/// O(1) storage read — the index is maintained incrementally by
/// `add_to_dispute_index` / `remove_from_dispute_index`.
pub fn get_dispute_index(env: &Env) -> Vec<(Address, Symbol)> {
    let disputes: Vec<(Address, Symbol)> =
        env.storage().persistent().get(&DataKeyB::DisputeIndex).unwrap_or_else(|| Vec::new(env));
    if !disputes.is_empty() {
        env.storage().persistent().extend_ttl(
            &DataKeyB::DisputeIndex,
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
    env.storage().persistent().set(&DataKeyB::DisputeIndex, &disputes);
    env.storage().persistent().extend_ttl(
        &DataKeyB::DisputeIndex,
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
        env.storage().persistent().set(&DataKeyB::DisputeIndex, &disputes);
    }
}

// ── MEV-Resistant Commit-Reveal ──────────────────────────────────────────────

pub fn get_last_global_submission_time(env: &Env) -> u64 {
    env.storage().instance().get(&DataKeyC::LastGlobalSubmissionTime).unwrap_or(0)
}

pub fn set_last_global_submission_time(env: &Env, timestamp: u64) {
    env.storage().instance().set(&DataKeyC::LastGlobalSubmissionTime, &timestamp);
}

pub fn get_quorum_failure_window(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKeyC::QuorumFailureWindow)
        .unwrap_or(DEFAULT_QUORUM_FAILURE_WINDOW_SECS)
}

pub fn set_quorum_failure_window(env: &Env, window_secs: u64) {
    env.storage().instance().set(&DataKeyC::QuorumFailureWindow, &window_secs);
}

pub fn set_consensus_commitment(
    env: &Env,
    model: &Address,
    wallet: &Address,
    asset_pair: &Symbol,
    commitment: &soroban_sdk::BytesN<32>,
) {
    let key = DataKeyC::ConsensusCommitment(model.clone(), wallet.clone(), asset_pair.clone());
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
    let key = DataKeyC::ConsensusCommitment(model.clone(), wallet.clone(), asset_pair.clone());
    env.storage().temporary().get(&key)
}

pub fn set_original_service_threshold(env: &Env, threshold: u32) {
    env.storage().instance().set(&DataKeyC::OriginalServiceThreshold, &threshold);
}

pub fn clear_original_service_threshold(env: &Env) {
    env.storage().instance().remove(&DataKeyC::OriginalServiceThreshold);
}

// ── Finality buffer (pending score commit window) ────────────────────────────

/// Returns the admin-configured finality buffer in seconds, defaulting to `0`
/// (disabled) until `set_finality_buffer` is called.
pub fn get_finality_buffer_secs(env: &Env) -> u64 {
    env.storage().instance().get(&DataKeyC::FinalityBufferSecs).unwrap_or(0)
}

pub fn set_finality_buffer_secs(env: &Env, secs: u64) {
    env.storage().instance().set(&DataKeyC::FinalityBufferSecs, &secs);
}

/// Returns the pending score held for `(wallet, asset_pair)`, if any.
/// Invisible to `get_score` / `query_risk_gate`.
pub fn get_pending_score(
    env: &Env,
    wallet: &Address,
    asset_pair: &Symbol,
) -> Option<PendingScoreEntry> {
    let key = DataKeyB::PendingScore(wallet.clone(), asset_pair.clone());
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
    let key = DataKeyB::PendingScore(wallet.clone(), asset_pair.clone());
    env.storage().persistent().set(&key, entry);
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

/// Removes the pending score for `(wallet, asset_pair)`. No-op if none exists.
pub fn clear_pending_score(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let key = DataKeyB::PendingScore(wallet.clone(), asset_pair.clone());
    env.storage().persistent().remove(&key);
}

// ── Service heartbeat monitor ────────────────────────────────────────────

/// Returns the ledger timestamp of the most recent accepted submission or
/// `ping_heartbeat` call, or `0` if the service has never been active.
pub fn get_last_service_activity(env: &Env) -> u64 {
    env.storage().instance().get(&DataKeyB::LastServiceActivityAt).unwrap_or(0)
}

/// Records `timestamp` as the most recent service activity. Called by
/// `submit_score`, `submit_scores_batch`, and `ping_heartbeat`.
pub fn set_last_service_activity(env: &Env, timestamp: u64) {
    env.storage().instance().set(&DataKeyB::LastServiceActivityAt, &timestamp);
}

/// Returns the admin-configured heartbeat alert threshold (seconds),
/// defaulting to `DEFAULT_HEARTBEAT_ALERT_THRESHOLD_SECS` until
/// `set_heartbeat_alert_threshold` is called.
pub fn get_heartbeat_alert_threshold(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKeyC::ServiceHeartbeatAlertThreshold)
        .unwrap_or(crate::constants::DEFAULT_HEARTBEAT_ALERT_THRESHOLD_SECS)
}

pub fn set_heartbeat_alert_threshold(env: &Env, secs: u64) {
    env.storage().instance().set(&DataKeyC::ServiceHeartbeatAlertThreshold, &secs);
}

/// Returns `true` once a `ServiceSilenceAlertEvent` has been emitted for the
/// current silence window and not yet cleared by a resumed submission.
pub fn is_silent_alert_emitted(env: &Env) -> bool {
    env.storage().instance().get(&DataKeyC::ServiceSilentAlertEmitted).unwrap_or(false)
}

pub fn set_silent_alert_emitted(env: &Env) {
    env.storage().instance().set(&DataKeyC::ServiceSilentAlertEmitted, &true);
}

pub fn clear_silent_alert_emitted(env: &Env) {
    env.storage().instance().remove(&DataKeyC::ServiceSilentAlertEmitted);
}

// ── Failover contract ────────────────────────────────────────────────────────

pub fn set_failover_contract(env: &Env, contract_id: &Address) {
    env.storage().instance().set(&DataKeyB::FailoverContract, contract_id);
}

pub fn get_failover_contract(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKeyB::FailoverContract)
}

// ── Aggregate service pubkey (threshold attestation) ─────────────────────────

pub fn get_aggregate_service_pubkey(env: &Env) -> Option<Bytes> {
    env.storage().instance().get(&DataKeyB::AggregateServicePubKey)
}

pub fn set_aggregate_service_pubkey(env: &Env, pubkey: &Bytes) {
    env.storage().instance().set(&DataKeyB::AggregateServicePubKey, pubkey);
}

// ── Consensus commitment (commit-reveal) ─────────────────────────────────────

pub fn remove_consensus_commitment(
    env: &Env,
    model: &Address,
    wallet: &Address,
    asset_pair: &Symbol,
) {
    let key = DataKeyC::ConsensusCommitment(model.clone(), wallet.clone(), asset_pair.clone());
    env.storage().temporary().remove(&key);
}

pub fn get_reveal_window_secs(env: &Env) -> u64 {
    env.storage().instance().get(&DataKeyB::RevealWindowSecs).unwrap_or(3_600)
}

// ── Signer expiry ─────────────────────────────────────────────────────────────

pub fn check_signer_expired(env: &Env, signer: &Address) -> Result<(), crate::errors::Error> {
    let ttl = get_signer_rotation_ttl(env);
    if ttl == 0 {
        return Ok(());
    }
    if let Some(age) = get_signer_age(env, signer) {
        let grace = get_signer_grace_period(env);
        if age > ttl + grace {
            crate::events::signer_expired(env, signer);
            return Err(crate::errors::Error::UnauthorizedSigner);
        }
        if age > ttl {
            crate::events::signer_expiring(env, signer);
        }
    }
    Ok(())
}

pub fn get_signer_ttl(env: &Env) -> u64 {
    env.storage().instance().get(&DataKeyB::SignerTtl).unwrap_or(0)
}

pub fn set_signer_ttl(env: &Env, ttl_secs: u64) {
    env.storage().instance().set(&DataKeyB::SignerTtl, &ttl_secs);
}

pub fn get_signer_grace_period(env: &Env) -> u64 {
    env.storage().instance().get(&DataKeyB::SignerGracePeriod).unwrap_or(0)
}

pub fn set_signer_grace_period(env: &Env, grace_secs: u64) {
    env.storage().instance().set(&DataKeyB::SignerGracePeriod, &grace_secs);
}

// ── Model version registry ────────────────────────────────────────────────────

pub fn get_model_version_set(env: &Env) -> Vec<u32> {
    env.storage().instance().get(&DataKeyB::AllModelVersions).unwrap_or_else(|| Vec::new(env))
}

pub fn set_model_version_set(env: &Env, versions: &Vec<u32>) {
    env.storage().instance().set(&DataKeyB::AllModelVersions, versions);
}

pub fn is_model_version_registered(env: &Env, version: u32) -> bool {
    get_model_version_set(env).contains(version)
}

pub fn get_model_version_status(env: &Env, version: u32) -> Option<ModelVersionStatus> {
    if !is_model_version_registered(env, version) {
        return None;
    }
    let key = DataKeyB::ModelVersionStatus(version);
    if let Some(status) = env.storage().instance().get(&key) {
        Some(status)
    } else {
        Some(ModelVersionStatus::Active)
    }
}

pub fn set_model_version_status(env: &Env, version: u32, status: ModelVersionStatus) {
    let key = DataKeyB::ModelVersionStatus(version);
    env.storage().instance().set(&key, &status);
}

pub fn get_model_version_executable_after(env: &Env, version: u32) -> u64 {
    let key = DataKeyD::ModelVersionExecutableAfter(version);
    env.storage().instance().get(&key).unwrap_or(0)
}

pub fn set_model_version_executable_after(env: &Env, version: u32, timestamp: u64) {
    let key = DataKeyD::ModelVersionExecutableAfter(version);
    env.storage().instance().set(&key, &timestamp);
}

pub fn get_model_version_description(env: &Env, version: u32) -> Bytes {
    let key = DataKeyD::ModelVersionDescription(version);
    env.storage().instance().get(&key).unwrap_or_else(|| Bytes::new(env))
}

pub fn set_model_version_description(env: &Env, version: u32, description: &Bytes) {
    let key = DataKeyD::ModelVersionDescription(version);
    env.storage().instance().set(&key, description);
}

pub fn is_model_version_active(env: &Env, version: u32) -> bool {
    get_model_version_status(env, version) == Some(ModelVersionStatus::Active)
}

pub fn is_model_version_deprecated(env: &Env, version: u32) -> bool {
    get_model_version_status(env, version) == Some(ModelVersionStatus::Deprecated)
}

pub fn is_model_version_proposed(env: &Env, version: u32) -> bool {
    get_model_version_status(env, version) == Some(ModelVersionStatus::Proposed)
}

pub fn set_model_version_deprecated(env: &Env, version: u32) {
    set_model_version_status(env, version, ModelVersionStatus::Deprecated);
}

// ── Bayesian model posterior weights ─────────────────────────────────────────

pub fn get_model_posterior_weight(env: &Env, version: u32) -> u64 {
    env.storage().instance().get(&DataKeyB::ModelPosteriorWeight(version)).unwrap_or(1_000_000u64)
}

pub fn set_model_posterior_weight(env: &Env, version: u32, weight: u64) {
    env.storage().instance().set(&DataKeyB::ModelPosteriorWeight(version), &weight);
}

// ── Score histogram ───────────────────────────────────────────────────────────

fn get_histogram_vec(env: &Env) -> Vec<u64> {
    env.storage().instance().get(&DataKeyB::ScoreHistogram).unwrap_or_else(|| {
        let mut v = Vec::new(env);
        for _ in 0..10u32 {
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
    let key = DataKeyB::ScoreHistogram;
    let mut histogram = get_histogram_vec(env);
    let bucket = if removed_score >= 100 { 9 } else { removed_score / 10 };
    if histogram.len() >= 10 {
        let count = histogram.get(bucket).unwrap_or(0).saturating_sub(1);
        histogram.set(bucket, count);
        env.storage().instance().set(&key, &histogram);
    }
}

pub fn update_histogram_on_write(env: &Env, previous_score: Option<u32>, new_score: u32) {
    let key = DataKeyB::ScoreHistogram;
    let mut histogram = get_histogram_vec(env);
    if histogram.len() < 10 {
        return;
    }
    if let Some(prev) = previous_score {
        let prev_bucket = if prev >= 100 { 9 } else { prev / 10 };
        let prev_count = histogram.get(prev_bucket).unwrap_or(0).saturating_sub(1);
        histogram.set(prev_bucket, prev_count);
    }
    let new_bucket = if new_score >= 100 { 9 } else { new_score / 10 };
    let new_count = histogram.get(new_bucket).unwrap_or(0).saturating_add(1);
    histogram.set(new_bucket, new_count);
    env.storage().instance().set(&key, &histogram);
}

// ── Verkle commitment ─────────────────────────────────────────────────────────

pub fn get_verkle_commitment_raw(env: &Env) -> [u8; 32] {
    let stored: Option<soroban_sdk::Bytes> =
        env.storage().instance().get(&DataKeyB::VerkleCommitment);
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
    env.storage().instance().set(&DataKeyB::VerkleCommitment, &bytes);
}

pub fn get_verkle_leaf(env: &Env, wallet: &Address, asset_pair: &Symbol) -> Option<[u8; 32]> {
    let key = DataKeyB::VerkleLeaf(wallet.clone(), asset_pair.clone());
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
    let key = DataKeyB::VerkleLeaf(wallet.clone(), asset_pair.clone());
    let bytes = soroban_sdk::Bytes::from_array(env, leaf);
    env.storage().persistent().set(&key, &bytes);
}

// ── Signer lifecycle ─────────────────────────────────────────────────────────

pub fn set_signer_added_at(env: &Env, signer: &Address, timestamp: u64) {
    env.storage().instance().set(&DataKeyB::SignerAddedAt(signer.clone()), &timestamp);
}

pub fn remove_signer_added_at(env: &Env, signer: &Address) {
    env.storage().instance().remove(&DataKeyB::SignerAddedAt(signer.clone()));
}

pub fn get_signer_age(env: &Env, signer: &Address) -> Option<u64> {
    let added_at: Option<u64> =
        env.storage().instance().get(&DataKeyB::SignerAddedAt(signer.clone()));
    added_at.map(|t| env.ledger().timestamp().saturating_sub(t))
}

pub fn set_signer_rotation_ttl(env: &Env, ttl_secs: u64) {
    env.storage().instance().set(&DataKeyB::SignerTtl, &ttl_secs);
}

pub fn get_signer_rotation_ttl(env: &Env) -> u64 {
    env.storage().instance().get(&DataKeyB::SignerTtl).unwrap_or(0)
}

pub fn set_signer_rotation_grace(env: &Env, grace_secs: u64) {
    env.storage().instance().set(&DataKeyB::SignerGracePeriod, &grace_secs);
}

// ── Dispute commit-reveal helpers ────────────────────────────────────────────

pub fn set_dispute_commit(
    env: &Env,
    challenger: &Address,
    wallet: &Address,
    pair: &Symbol,
    hash: &BytesN<32>,
) {
    env.storage()
        .temporary()
        .set(&DataKeyB::DisputeCommit(challenger.clone(), wallet.clone(), pair.clone()), hash);
    env.storage().temporary().set(
        &DataKeyB::DisputeCommitTime(challenger.clone(), wallet.clone(), pair.clone()),
        &env.ledger().timestamp(),
    );
}

pub fn get_dispute_commit(
    env: &Env,
    challenger: &Address,
    wallet: &Address,
    pair: &Symbol,
) -> Option<BytesN<32>> {
    env.storage().temporary().get(&DataKeyB::DisputeCommit(
        challenger.clone(),
        wallet.clone(),
        pair.clone(),
    ))
}

pub fn get_dispute_commit_time(
    env: &Env,
    challenger: &Address,
    wallet: &Address,
    pair: &Symbol,
) -> u64 {
    env.storage()
        .temporary()
        .get(&DataKeyB::DisputeCommitTime(challenger.clone(), wallet.clone(), pair.clone()))
        .unwrap_or(0)
}

pub fn remove_dispute_commit(env: &Env, challenger: &Address, wallet: &Address, pair: &Symbol) {
    env.storage().temporary().remove(&DataKeyB::DisputeCommit(
        challenger.clone(),
        wallet.clone(),
        pair.clone(),
    ));
    env.storage().temporary().remove(&DataKeyB::DisputeCommitTime(
        challenger.clone(),
        wallet.clone(),
        pair.clone(),
    ));
}

pub fn set_reveal_window_secs(env: &Env, secs: u64) {
    env.storage().instance().set(&DataKeyB::RevealWindowSecs, &secs);
}

// ── Issue #285: Decay curve ──────────────────────────────────────────────────

pub fn set_decay_curve(env: &Env, curve: &DecayCurve) {
    env.storage().instance().set(&DataKeyB::DecayCurveConfig, curve);
}

pub fn get_decay_curve(env: &Env) -> DecayCurve {
    env.storage().instance().get(&DataKeyB::DecayCurveConfig).unwrap_or(DecayCurve::Exponential)
}

// ── Issue #283: Dormancy decay ───────────────────────────────────────────────

pub fn set_dormancy_inactivity_secs(env: &Env, secs: u64) {
    env.storage().instance().set(&DataKeyB::DormancyInactivitySecs, &secs);
}

pub fn get_dormancy_inactivity_secs(env: &Env) -> u64 {
    env.storage().instance().get(&DataKeyB::DormancyInactivitySecs).unwrap_or(0)
}

pub fn set_dormancy_decay_fraction_bps(env: &Env, bps: u32) {
    env.storage().instance().set(&DataKeyB::DormancyDecayFractionBps, &bps);
}

pub fn get_dormancy_decay_fraction_bps(env: &Env) -> u32 {
    env.storage().instance().get(&DataKeyB::DormancyDecayFractionBps).unwrap_or(0)
}

pub fn set_decay_checkpoint(env: &Env, wallet: &Address, asset_pair: &Symbol, ts: u64) {
    let key = DataKeyB::DecayCheckpoint(wallet.clone(), asset_pair.clone());
    env.storage().persistent().set(&key, &ts);
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

pub fn get_decay_checkpoint(env: &Env, wallet: &Address, asset_pair: &Symbol) -> u64 {
    let key = DataKeyB::DecayCheckpoint(wallet.clone(), asset_pair.clone());
    env.storage().persistent().get(&key).unwrap_or(0)
}

// ── Issue #284: Finality depth ───────────────────────────────────────────────

pub fn set_finality_depth(env: &Env, ledgers: u32) {
    env.storage().instance().set(&DataKeyB::FinalityDepth, &ledgers);
}

pub fn get_finality_depth(env: &Env) -> u32 {
    env.storage().instance().get(&DataKeyB::FinalityDepth).unwrap_or(0)
}

pub fn set_score_submission_ledger(env: &Env, wallet: &Address, asset_pair: &Symbol, seq: u32) {
    let key = DataKeyB::ScoreSubmissionLedger(wallet.clone(), asset_pair.clone());
    env.storage().persistent().set(&key, &seq);
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

pub fn get_score_submission_ledger(env: &Env, wallet: &Address, asset_pair: &Symbol) -> u32 {
    let key = DataKeyB::ScoreSubmissionLedger(wallet.clone(), asset_pair.clone());
    env.storage().persistent().get(&key).unwrap_or(0)
}

// ── Issue #286: Score breakdown ──────────────────────────────────────────────

pub fn set_score_breakdown(
    env: &Env,
    wallet: &Address,
    asset_pair: &Symbol,
    payload: &SubscorePayload,
) {
    let key = DataKeyB::ScoreBreakdown(wallet.clone(), asset_pair.clone());
    env.storage().persistent().set(&key, payload);
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

pub fn get_score_breakdown(
    env: &Env,
    wallet: &Address,
    asset_pair: &Symbol,
) -> Option<SubscorePayload> {
    let key = DataKeyB::ScoreBreakdown(wallet.clone(), asset_pair.clone());
    env.storage().persistent().get(&key)
}

// ── Per-pair score submission counter ────────────────────────────────────────

/// Increments the running total of successful score submissions for
/// `asset_pair` across all wallets.  Called from every write path
/// (`write_score_with_rate_limit` and `submit_scores_batch`) on a
/// successful write.
pub fn increment_pair_score_count(env: &Env, asset_pair: &Symbol) {
    let key = DataKeyB::PairScoreCount(asset_pair.clone());
    let current: u64 = env.storage().persistent().get(&key).unwrap_or(0);
    env.storage().persistent().set(&key, &(current + 1));
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

/// Returns the total number of successful score submissions ever recorded
/// for `asset_pair` (across all wallets).  Returns `0` before any
/// submission has been accepted for the pair.
pub fn get_pair_score_count(env: &Env, asset_pair: &Symbol) -> u64 {
    let key = DataKeyB::PairScoreCount(asset_pair.clone());
    let result: Option<u64> = env.storage().persistent().get(&key);
    if result.is_some() {
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }
    result.unwrap_or(0)
}

// ── Total unique wallet-pair combinations ever scored ─────────────────────────

/// Increments the global counter of unique `(wallet, asset_pair)`
/// combinations ever scored.  Must be called only on the *first* successful
/// write for a combination — callers check `peek_score` **before** writing
/// to decide whether the combination is new.
pub fn increment_total_wallets_scored(env: &Env) {
    let current: u64 = env.storage().instance().get(&DataKeyB::TotalWalletsScored).unwrap_or(0);
    env.storage().instance().set(&DataKeyB::TotalWalletsScored, &(current + 1));
}

/// Returns the total number of unique `(wallet, asset_pair)` combinations
/// that have ever been successfully scored.  Useful as a high-level
/// protocol-health metric.
pub fn get_total_wallets_scored(env: &Env) -> u64 {
    env.storage().instance().get(&DataKeyB::TotalWalletsScored).unwrap_or(0)
}

// ── Score Momentum Window (issue #289) ───────────────────────────────────────

pub fn set_momentum_window(env: &Env, secs: u64) {
    env.storage().instance().set(&DataKeyB::MomentumWindow, &secs);
}

pub fn get_momentum_window(env: &Env) -> u64 {
    env.storage().instance().get(&DataKeyB::MomentumWindow).unwrap_or(3600)
}

pub fn set_momentum_alert_threshold(env: &Env, threshold: u32) {
    env.storage().instance().set(&DataKeyB::MomentumAlertThreshold, &threshold);
}

pub fn get_momentum_alert_threshold(env: &Env) -> u32 {
    env.storage().instance().get(&DataKeyB::MomentumAlertThreshold).unwrap_or(0)
}

// ── Interpolation Method (issue #290) ────────────────────────────────────────

pub fn set_interpolation_method(env: &Env, method: &InterpolationMethod) {
    env.storage().instance().set(&DataKeyB::InterpolationMethod, method);
}

pub fn get_interpolation_method(env: &Env) -> InterpolationMethod {
    env.storage()
        .instance()
        .get(&DataKeyB::InterpolationMethod)
        .unwrap_or(InterpolationMethod::Linear)
}

// ── Oracle adapter registry (issue: price-feed confidence adjustment) ───────

pub fn get_registered_oracle(env: &Env, asset_pair: &Symbol) -> Option<Address> {
    env.storage().instance().get(&DataKeyD::RegisteredOracle(asset_pair.clone()))
}

pub fn set_registered_oracle(env: &Env, asset_pair: &Symbol, oracle: &Address) {
    env.storage().instance().set(&DataKeyD::RegisteredOracle(asset_pair.clone()), oracle);
}

pub fn remove_registered_oracle(env: &Env, asset_pair: &Symbol) {
    env.storage().instance().remove(&DataKeyD::RegisteredOracle(asset_pair.clone()));
}

/// Returns the ledger timestamp of the last oracle price consultation for
/// `asset_pair`, or `None` if the oracle has never been consulted.
pub fn get_oracle_last_updated(env: &Env, asset_pair: &Symbol) -> Option<u64> {
    env.storage().instance().get(&DataKeyD::OracleLastUpdated(asset_pair.clone()))
}

/// Persists the ledger timestamp of the most recent oracle price consultation.
/// Called by `get_effective_score` each time it successfully invokes the oracle.
pub fn set_oracle_last_updated(env: &Env, asset_pair: &Symbol, ts: u64) {
    env.storage().instance().set(&DataKeyD::OracleLastUpdated(asset_pair.clone()), &ts);
}

/// Removes the last-updated timestamp for `asset_pair` (called by `remove_oracle`
/// so stale metadata does not linger after de-registration).
pub fn remove_oracle_last_updated(env: &Env, asset_pair: &Symbol) {
    env.storage().instance().remove(&DataKeyD::OracleLastUpdated(asset_pair.clone()));
}

/// Returns the admin-configured oracle staleness threshold in seconds.
/// Defaults to `DEFAULT_ORACLE_STALENESS_THRESHOLD_SECS` (1 hour).
pub fn get_oracle_staleness_threshold(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKeyD::OracleStalenessThreshold)
        .unwrap_or(crate::constants::DEFAULT_ORACLE_STALENESS_THRESHOLD_SECS)
}

/// Persists the oracle staleness threshold. Must be > 0.
pub fn set_oracle_staleness_threshold(env: &Env, threshold_secs: u64) {
    env.storage().instance().set(&DataKeyD::OracleStalenessThreshold, &threshold_secs);
}

// ── Epoch sealing (issue #301) ────────────────────────────────────────────────

pub fn is_epoch_open(env: &Env) -> bool {
    env.storage().instance().get(&DataKeyD::EpochOpen).unwrap_or(true)
}

pub fn set_epoch_open(env: &Env, open: bool) {
    env.storage().instance().set(&DataKeyD::EpochOpen, &open);
}

pub fn get_current_epoch(env: &Env) -> u32 {
    env.storage().instance().get(&DataKeyD::CurrentEpoch).unwrap_or(0)
}

pub fn set_current_epoch(env: &Env, epoch_id: u32) {
    env.storage().instance().set(&DataKeyD::CurrentEpoch, &epoch_id);
}

// ── Signer accuracy tracking (rolling MAD) ────────────────────────────────────

pub fn get_signer_accuracy(env: &Env, signer: &Address) -> Option<SignerAccuracyRecord> {
    env.storage().instance().get(&DataKeyD::SignerAccuracy(signer.clone()))
}

pub fn set_signer_accuracy(env: &Env, signer: &Address, record: &SignerAccuracyRecord) {
    env.storage().instance().set(&DataKeyD::SignerAccuracy(signer.clone()), record);
}

pub fn remove_signer_accuracy(env: &Env, signer: &Address) {
    env.storage().instance().remove(&DataKeyD::SignerAccuracy(signer.clone()));
}

pub fn increment_signer_rejection_count(env: &Env, signer: &Address) {
    let key = DataKeyD::SignerRejectionCount(signer.clone());
    let current: u32 = env.storage().instance().get(&key).unwrap_or(0);
    env.storage().instance().set(&key, &(current + 1));
}

// ── Online Welford correlation (issue #268) ───────────────────────────────────

fn welford_key(pair_a: &Symbol, pair_b: &Symbol) -> DataKeyD {
    DataKeyD::WelfordCorrState(pair_a.clone(), pair_b.clone())
}

pub fn get_welford_corr_state(
    env: &Env,
    pair_a: &Symbol,
    pair_b: &Symbol,
) -> Option<WelfordCorrState> {
    env.storage().instance().get(&welford_key(pair_a, pair_b))
}

pub fn set_welford_corr_state(
    env: &Env,
    pair_a: &Symbol,
    pair_b: &Symbol,
    state: &WelfordCorrState,
) {
    env.storage().instance().set(&welford_key(pair_a, pair_b), state);
}

pub fn reset_welford_corr_state(env: &Env, pair_a: &Symbol, pair_b: &Symbol) {
    env.storage().instance().remove(&welford_key(pair_a, pair_b));
}

// ── Admin-set static pair correlation ──────────────────────────────────────────

pub fn get_pair_correlation(env: &Env, pair_a: &Symbol, pair_b: &Symbol) -> i32 {
    env.storage()
        .instance()
        .get(&DataKeyD::PairCorrelation(pair_a.clone(), pair_b.clone()))
        .unwrap_or(0)
}

pub fn set_pair_correlation(env: &Env, pair_a: &Symbol, pair_b: &Symbol, corr: i32) {
    env.storage().instance().set(&DataKeyD::PairCorrelation(pair_a.clone(), pair_b.clone()), &corr);
}

// ── Token-bucket rate limiting (issue #269) ───────────────────────────────────

pub fn get_token_bucket(env: &Env, wallet: &Address, asset_pair: &Symbol) -> Option<TokenBucket> {
    env.storage().instance().get(&DataKeyD::TokenBucket(wallet.clone(), asset_pair.clone()))
}

pub fn set_token_bucket(env: &Env, wallet: &Address, asset_pair: &Symbol, bucket: &TokenBucket) {
    env.storage()
        .instance()
        .set(&DataKeyD::TokenBucket(wallet.clone(), asset_pair.clone()), bucket);
}

pub fn get_burst_capacity(env: &Env) -> u32 {
    env.storage().instance().get(&DataKeyD::BurstCapacity).unwrap_or(1)
}

pub fn set_burst_capacity(env: &Env, capacity: u32) {
    env.storage().instance().set(&DataKeyD::BurstCapacity, &capacity);
}

// ── Wallet clustering ──────────────────────────────────────────────────────────

pub fn get_cluster_boundaries(env: &Env) -> Vec<u32> {
    env.storage().instance().get(&DataKeyD::ClusterBoundaries).unwrap_or_else(|| Vec::new(env))
}

pub fn set_cluster_boundaries(env: &Env, boundaries: &Vec<u32>) {
    env.storage().instance().set(&DataKeyD::ClusterBoundaries, boundaries);
}

pub fn get_wallet_cluster(env: &Env, wallet: &Address) -> Option<u32> {
    env.storage().instance().get(&DataKeyD::WalletCluster(wallet.clone()))
}

pub fn set_wallet_cluster(env: &Env, wallet: &Address, cluster: u32) {
    env.storage().instance().set(&DataKeyD::WalletCluster(wallet.clone()), &cluster);
}

// ── Per-pair 24h score volatility index (issue #270) ──────────────────────────

pub fn get_pair_volatility_state(env: &Env, asset_pair: &Symbol) -> Option<PairVolatilityState> {
    env.storage().instance().get(&DataKeyD::PairVolatilityState(asset_pair.clone()))
}

pub fn set_pair_volatility_state(env: &Env, asset_pair: &Symbol, state: &PairVolatilityState) {
    env.storage().instance().set(&DataKeyD::PairVolatilityState(asset_pair.clone()), state);
}

pub fn get_pair_volatility_window(env: &Env) -> u64 {
    env.storage().instance().get(&DataKeyD::PairVolatilityWindow).unwrap_or(86_400)
}

pub fn set_pair_volatility_window(env: &Env, secs: u64) {
    env.storage().instance().set(&DataKeyD::PairVolatilityWindow, &secs);
}

// ── Flash-loan protection (issue #300) ────────────────────────────────────────

pub fn get_flash_protection_mode(env: &Env) -> FlashProtectionMode {
    env.storage()
        .instance()
        .get(&DataKeyD::FlashProtectionMode)
        .unwrap_or(FlashProtectionMode::Warn)
}

pub fn set_flash_protection_mode(env: &Env, mode: &FlashProtectionMode) {
    env.storage().instance().set(&DataKeyD::FlashProtectionMode, mode);
}

// ── Differential-privacy epsilon (issue #204 privacy model) ───────────────────

pub fn get_dp_epsilon(env: &Env) -> u32 {
    env.storage().instance().get(&DataKeyD::DpEpsilon).unwrap_or(0)
}

pub fn set_dp_epsilon(env: &Env, epsilon_bps: u32) {
    env.storage().instance().set(&DataKeyD::DpEpsilon, &epsilon_bps);
}

// ── Upgrade multisig approvals ─────────────────────────────────────────────────

pub fn get_upgrade_approvals(env: &Env) -> Vec<Address> {
    env.storage().instance().get(&DataKeyD::UpgradeApprovals).unwrap_or_else(|| Vec::new(env))
}

pub fn set_upgrade_approvals(env: &Env, approvals: &Vec<Address>) {
    env.storage().instance().set(&DataKeyD::UpgradeApprovals, approvals);
}

pub fn clear_upgrade_approvals(env: &Env) {
    env.storage().instance().remove(&DataKeyD::UpgradeApprovals);
}

// ── Service pubkey rotation overlap window ────────────────────────────────────

pub fn get_pending_service_pubkey(env: &Env) -> Option<(Bytes, u64)> {
    env.storage().instance().get(&DataKeyD::PendingServicePubKey)
}

pub fn set_pending_service_pubkey(env: &Env, new_key: &Bytes, expiry: u64) {
    env.storage().instance().set(&DataKeyD::PendingServicePubKey, &(new_key.clone(), expiry));
}

pub fn clear_pending_service_pubkey(env: &Env) {
    env.storage().instance().remove(&DataKeyD::PendingServicePubKey);
}

/// Compares a recovered 65-byte uncompressed secp256k1 pubkey against a
/// stored pubkey, which may be either the same 65-byte uncompressed form or
/// the 33-byte compressed form.
pub fn pubkeys_match(recovered: &BytesN<65>, stored: &Bytes) -> bool {
    use subtle::ConstantTimeEq;
    match stored.len() {
        65 => {
            let mut stored_arr = [0u8; 65];
            stored.copy_into_slice(&mut stored_arr);
            recovered.to_array().ct_eq(&stored_arr).unwrap_u8() != 0
        }
        33 => {
            let recovered_arr = recovered.to_array();
            let mut compressed = [0u8; 33];
            compressed[0] = if recovered_arr[64] % 2 == 0 { 0x02 } else { 0x03 };
            compressed[1..33].copy_from_slice(&recovered_arr[1..33]);
            let mut stored_arr = [0u8; 33];
            stored.copy_into_slice(&mut stored_arr);
            compressed.ct_eq(&stored_arr).unwrap_u8() != 0
        }
        _ => false,
    }
}

// ── Rate-limit override audit log ─────────────────────────────────────────────

pub fn get_rate_limit_override_log(env: &Env) -> Vec<RateLimitOverrideEntry> {
    env.storage().instance().get(&DataKeyD::RateLimitOverrideLog).unwrap_or_else(|| Vec::new(env))
}

pub fn append_rate_limit_override_log(env: &Env, entry: &RateLimitOverrideEntry) {
    let mut log = get_rate_limit_override_log(env);
    if log.len() >= crate::constants::MAX_RATE_LIMIT_OVERRIDE_LOG {
        log.remove(0);
    }
    log.push_back(entry.clone());
    env.storage().instance().set(&DataKeyD::RateLimitOverrideLog, &log);
}

// ── IQR-based outlier rejection multiplier ────────────────────────────────────

pub fn get_iqr_rejection_multiplier(env: &Env) -> u32 {
    env.storage().instance().get(&DataKeyD::IqrRejectionMultiplier).unwrap_or(150)
}

// ── Simple time-locked parameter changes ──────────────────────────────────────

pub fn has_pending_param_change(env: &Env, key: &Symbol) -> bool {
    env.storage().instance().has(&DataKeyD::PendingParamChange(key.clone()))
}

pub fn set_pending_param_change(env: &Env, key: &Symbol, proposal: &ParamChangeProposal) {
    env.storage().instance().set(&DataKeyD::PendingParamChange(key.clone()), proposal);
}

pub fn get_pending_param_change(env: &Env, key: &Symbol) -> Option<ParamChangeProposal> {
    env.storage().instance().get(&DataKeyD::PendingParamChange(key.clone()))
}

pub fn clear_pending_param_change(env: &Env, key: &Symbol) {
    env.storage().instance().remove(&DataKeyD::PendingParamChange(key.clone()));
}

pub fn get_param_change_delay(env: &Env) -> u64 {
    crate::constants::DEFAULT_PARAM_CHANGE_DELAY_SECS
}

// ── Query-gate read-ledger tracking (flash-loan protection, issue #300) ──────

pub fn get_gate_read_ledger(env: &Env, wallet: &Address, asset_pair: &Symbol) -> Option<u32> {
    env.storage().temporary().get(&GateDataKey::GateReadLedger(wallet.clone(), asset_pair.clone()))
}

pub fn set_gate_read_ledger(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    let key = GateDataKey::GateReadLedger(wallet.clone(), asset_pair.clone());
    env.storage().temporary().set(&key, &env.ledger().sequence());
    env.storage().temporary().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

pub fn get_gate_query_fee(env: &Env) -> i128 {
    env.storage().instance().get(&GateDataKey::GateQueryFee).unwrap_or(0)
}

pub fn set_gate_query_fee(env: &Env, amount: i128) {
    env.storage().instance().set(&GateDataKey::GateQueryFee, &amount);
}

pub fn get_accumulated_fees(env: &Env) -> i128 {
    env.storage().instance().get(&GateDataKey::AccumulatedFees).unwrap_or(0)
}

// ── #688: Submission provenance snapshots ────────────────────────────────────

pub fn set_submission_provenance(
    env: &Env,
    wallet: &Address,
    asset_pair: &Symbol,
    provenance: &crate::types::SubmissionProvenance,
) {
    let key =
        crate::types::DataKeyE::SubmissionProvenance(wallet.clone(), asset_pair.clone());
    env.storage().persistent().set(&key, provenance);
    env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
}

pub fn get_submission_provenance(
    env: &Env,
    wallet: &Address,
    asset_pair: &Symbol,
) -> Option<crate::types::SubmissionProvenance> {
    let key =
        crate::types::DataKeyE::SubmissionProvenance(wallet.clone(), asset_pair.clone());
    let result: Option<crate::types::SubmissionProvenance> =
        env.storage().persistent().get(&key);
    if result.is_some() {
        env.storage().persistent().extend_ttl(&key, SCORE_TTL_THRESHOLD, SCORE_TTL_EXTEND_TO);
    }
    result
}

#[cfg(test)]
mod test_instrumentation {
    use soroban_sdk::contracttype;

    #[contracttype]
    #[derive(Clone)]
    pub enum TestKey {
        ExtendCount,
    }
}
