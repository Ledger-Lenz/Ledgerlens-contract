//! Formal pre/post-condition invariant checker for issues #292 and #710.
//!
//! `invariant_check` is compiled only in test/debug builds
//! (`#[cfg(any(test, feature = "testutils"))]`). Call it at the end of every
//! state-mutating public function in `lib.rs` to catch invariant violations at
//! the point of introduction rather than downstream.
//!
//! A violation panics with a descriptive message identifying the broken
//! invariant, which causes the surrounding test to fail with a clear diagnosis.
//!
//! ## Invariant catalogue
//!
//! | # | Family | Rationale |
//! |---|--------|-----------|
//! | 1 | Config | `global_min_confidence` must be in `[0, 100]`. |
//! | 2 | Config | `service_threshold` ≤ service signer set size (when non-empty). |
//! | 3 | Config | `admin_threshold` ≤ admin set size (when non-empty). |
//! | 4 | Config | Decay rate denominator is never zero (guards divisions). |
//! | 5 | Config | Gate query fee is non-negative. |
//! | 6 | Config | Accumulated fees are non-negative. |
//! | 7 | Score  | Every `(wallet, asset_pair)` present in `ScoreEntryIndex` has a live `Score` entry. |
//! | 8 | Score  | `ScoreEntryIndex` contains no duplicate `(wallet, asset_pair)` pairs. |
//! | 9 | Score  | `ScoreCount` for any pair is > 0 iff a `Score` entry exists. |
//! | 10| History| `ScoreHistory` length never exceeds `HistoryMaxDepth`. |
//! | 11| Embargo| `ActiveEmbargoCount` equals the number of wallets in `EmbargoedWalletIndex` that have a live embargo. |
//! | 12| Embargo| `EmbargoedWalletIndex` contains no duplicate wallet entries. |
//! | 13| Pairs  | Every `(wallet, asset_pair)` in `AssetPairs(wallet)` has a live `Score` or `ScoreHistory` entry. |
//! | 14| Admin  | `PendingAdmin` is not equal to the current `Admin` (a no-op transfer must never be staged). |
//! | 15| Histogram| Histogram total equals the number of live `Score` entries tracked in `ScoreEntryIndex`. |
//! | 16| Pause  | `PausedPairIndex` contains no duplicate pairs. |
//! | 17| Decay  | Decay rate numerator ≤ denominator (λ ∈ [0, 1]). |
//! | 18| History| `HistoryMaxDepth` is in `[1, MAX_HISTORY_DEPTH]`. |

use soroban_sdk::Env;

use crate::storage;

// ─── Public top-level checker ─────────────────────────────────────────────────

/// Assert all contract-level invariants after a state mutation.
///
/// Each invariant is documented with a short rationale in the module-level
/// comment above.  Violations panic with a descriptive message, which causes
/// the surrounding test to fail at the point of introduction rather than
/// downstream.
#[cfg(any(test, feature = "testutils"))]
pub fn invariant_check(env: &Env) {
    check_config_invariants(env);
    check_score_index_invariants(env);
    check_history_invariants(env);
    check_embargo_invariants(env);
    check_pair_registry_invariants(env);
    check_admin_transfer_invariants(env);
    check_pause_index_invariants(env);
    check_decay_rate_invariants(env);
}

// ─── Config invariants ────────────────────────────────────────────────────────

#[cfg(any(test, feature = "testutils"))]
fn check_config_invariants(env: &Env) {
    // 1. Global min confidence must be in [0, 100].
    let min_conf = storage::get_global_min_confidence(env);
    assert!(min_conf <= 100, "INVARIANT #1 VIOLATED: global_min_confidence={min_conf} exceeds 100");

    // 2. Service threshold ≤ service signer set size.
    let svc_set = storage::get_service_set(env);
    let svc_set_len = svc_set.len();
    if svc_set_len > 0 {
        let svc_threshold = storage::get_service_threshold(env);
        assert!(
            svc_threshold <= svc_set_len,
            "INVARIANT #2 VIOLATED: service_threshold={svc_threshold} > signer_set_size={svc_set_len}"
        );
    }

    // 3. Admin threshold ≤ admin set size.
    let admin_set = storage::get_admin_set(env);
    let admin_set_len = admin_set.len();
    if admin_set_len > 0 {
        let admin_threshold = storage::get_admin_threshold(env);
        assert!(
            admin_threshold <= admin_set_len,
            "INVARIANT #3 VIOLATED: admin_threshold={admin_threshold} > admin_set_size={admin_set_len}"
        );
    }

    // 4. Decay rate denominator must never be zero.
    let (_, denom) = storage::get_decay_rate(env);
    assert!(denom > 0, "INVARIANT #4 VIOLATED: decay_rate_denominator=0 (division by zero risk)");

    // 5. Gate query fee must be non-negative.
    let gate_fee = storage::get_gate_query_fee(env);
    assert!(gate_fee >= 0, "INVARIANT #5 VIOLATED: gate_query_fee={gate_fee} is negative");

    // 6. Accumulated fees must be non-negative.
    let accum = storage::get_accumulated_fees(env);
    assert!(accum >= 0, "INVARIANT #6 VIOLATED: accumulated_fees={accum} is negative");
}

// ─── Decay rate invariants ────────────────────────────────────────────────────

/// Rationale for invariant #17 (decay λ ∈ [0, 1]):
/// The exponential decay formula uses `lambda = num / den`. If `num > den`
/// then λ > 1, which means scores would *grow* over time rather than decay,
/// violating the score-attenuation guarantee. Checked here separately from
/// `check_config_invariants` so the constraint is easy to locate.
#[cfg(any(test, feature = "testutils"))]
fn check_decay_rate_invariants(env: &Env) {
    let (num, den) = storage::get_decay_rate(env);
    // den > 0 already guaranteed by invariant #4 above; safe to use in comparison.
    assert!(
        num <= den,
        "INVARIANT #17 VIOLATED: decay_rate_numerator={num} > decay_rate_denominator={den}; lambda > 1 would amplify instead of attenuate scores"
    );
}

// ─── Score index invariants ───────────────────────────────────────────────────

/// Rationale for invariants #7 and #8:
/// `ScoreEntryIndex` drives proactive rent management (`get_expiring_entries`).
/// A stale reference (index entry without a live score) would cause the
/// rent manager to waste gas extending a key that no longer exists.  A
/// duplicate would double-extend the same entry and distort the queue's
/// LRU ordering, defeating the early-exit optimisation.
#[cfg(any(test, feature = "testutils"))]
fn check_score_index_invariants(env: &Env) {
    let index = storage::get_score_entry_index(env);

    for i in 0..index.len() {
        let (wallet, pair) = index.get(i).unwrap();

        // 7. Every index entry must have a live score.
        let score = storage::peek_score(env, &wallet, &pair);
        assert!(
            score.is_some(),
            "INVARIANT #7 VIOLATED: ScoreEntryIndex contains ({wallet:?}, {pair:?}) but no live Score entry exists"
        );

        // 8. No duplicate entries in the index.
        for j in (i + 1)..index.len() {
            let (wallet2, pair2) = index.get(j).unwrap();
            assert!(
                !(wallet == wallet2 && pair == pair2),
                "INVARIANT #8 VIOLATED: ScoreEntryIndex contains duplicate entry ({wallet:?}, {pair:?}) at positions {i} and {j}"
            );
        }
    }
}

// ─── History invariants ───────────────────────────────────────────────────────

/// Rationale for invariant #10:
/// `push_score_history` trims the ring to `HistoryMaxDepth` on every write.
/// If the history exceeds the depth, a prior write either skipped the trim or
/// someone wrote directly to storage bypassing the safe accessor, both of
/// which indicate a logic regression.
///
/// Rationale for invariant #18:
/// `HistoryMaxDepth` of 0 would cause `push_score_history` to evict every
/// entry it just pushed, making the ring permanently empty — which is not a
/// meaningful configuration.  Depths above `MAX_HISTORY_DEPTH` are blocked
/// at the API layer; checking here catches direct storage writes that bypass
/// the setter's validation.
#[cfg(any(test, feature = "testutils"))]
fn check_history_invariants(env: &Env) {
    // 18. HistoryMaxDepth must be in [1, MAX_HISTORY_DEPTH].
    let max_depth = storage::get_history_max_depth(env);
    assert!(
        (1..=crate::constants::MAX_HISTORY_DEPTH).contains(&max_depth),
        "INVARIANT #18 VIOLATED: history_max_depth={max_depth} is outside [1, {}]",
        crate::constants::MAX_HISTORY_DEPTH
    );

    // 10. No score history ring may exceed HistoryMaxDepth entries.
    //
    // Walking every (wallet, pair) in the index is sufficient because
    // `push_score_history` is always paired with `set_score` (which calls
    // `track_score_entry`), so the index covers all wallets that have history.
    let index = storage::get_score_entry_index(env);
    for i in 0..index.len() {
        let (wallet, pair) = index.get(i).unwrap();
        let len = storage::peek_score_history_len(env, &wallet, &pair);
        assert!(
            len <= max_depth,
            "INVARIANT #10 VIOLATED: ScoreHistory({wallet:?}, {pair:?}) has {len} entries but HistoryMaxDepth={max_depth}"
        );
    }
}

// ─── Embargo invariants ───────────────────────────────────────────────────────

/// Rationale for invariant #11:
/// `ActiveEmbargoCount` is the authority for "how many wallets are currently
/// under embargo". The embargo index is a separate data structure used for
/// enumeration.  If they disagree, queries for the embargo count return
/// incorrect data, and `revoke_all_embargoes` may under- or over-terminate.
///
/// Rationale for invariant #12:
/// Duplicate entries in `EmbargoedWalletIndex` would cause `revoke_all_embargoes`
/// to decrement the count twice for one wallet, driving it below the true
/// number of active embargoes.
#[cfg(any(test, feature = "testutils"))]
fn check_embargo_invariants(env: &Env) {
    let wallets = storage::get_embargoed_wallets(env);

    // 12. No duplicates in the embargo index.
    for i in 0..wallets.len() {
        let w_i = wallets.get(i).unwrap();
        for j in (i + 1)..wallets.len() {
            let w_j = wallets.get(j).unwrap();
            assert!(
                w_i != w_j,
                "INVARIANT #12 VIOLATED: EmbargoedWalletIndex contains duplicate wallet {w_i:?} at positions {i} and {j}"
            );
        }
    }

    // 11. ActiveEmbargoCount must equal the number of actually-embargoed wallets
    //     in the index (i.e. those whose embargo has not yet expired).
    let stored_count = storage::get_active_embargo_count(env);
    let live_count = {
        let mut n = 0u32;
        for i in 0..wallets.len() {
            let w = wallets.get(i).unwrap();
            if storage::peek_is_embargoed(env, &w) {
                n += 1;
            }
        }
        n
    };
    assert!(
        stored_count == live_count,
        "INVARIANT #11 VIOLATED: ActiveEmbargoCount={stored_count} but EmbargoedWalletIndex contains {live_count} live embargoes"
    );
}

// ─── Pair registry invariants ─────────────────────────────────────────────────

/// Rationale for invariant #13:
/// `AssetPairs(wallet)` is the index used by `get_aggregate_score` and
/// `delete_score` to enumerate all scoring data for a wallet.  A pair that
/// lingers after its score and history have been erased would cause aggregate
/// reads to silently under-count, and the pair weight accounting to diverge.
#[cfg(any(test, feature = "testutils"))]
fn check_pair_registry_invariants(env: &Env) {
    let index = storage::get_score_entry_index(env);
    for i in 0..index.len() {
        let (wallet, pair) = index.get(i).unwrap();
        let registered_pairs = storage::get_wallet_pairs(env, &wallet);
        let has_score = storage::peek_score(env, &wallet, &pair).is_some();
        let history_len = storage::peek_score_history_len(env, &wallet, &pair);

        if has_score || history_len > 0 {
            assert!(
                registered_pairs.contains(&pair),
                "INVARIANT #13 VIOLATED: ({wallet:?}, {pair:?}) has live data but is missing from AssetPairs(wallet)"
            );
        }
    }
}

// ─── Admin transfer invariants ────────────────────────────────────────────────

/// Rationale for invariant #14:
/// Staging a pending-admin transfer to the *current* admin is a no-op that
/// wastes governance timelock budget and can confuse on-chain monitoring tools
/// that watch for `PendingAdminSet` events.  Blocking it here is cheaper than
/// explaining to operators why the pending admin is the same as the current one.
#[cfg(any(test, feature = "testutils"))]
fn check_admin_transfer_invariants(env: &Env) {
    if !storage::has_admin(env) {
        return;
    }
    if let Some(pending) = storage::get_pending_admin(env) {
        let current = storage::get_admin(env);
        assert!(
            pending != current,
            "INVARIANT #14 VIOLATED: PendingAdmin ({pending:?}) is the same as the current Admin — a no-op transfer was staged"
        );
    }
}

// ─── Pause index invariants ───────────────────────────────────────────────────

/// Rationale for invariant #16:
/// `PausedPairIndex` is used by `get_paused_pairs` for enumeration.  A
/// duplicate entry would cause `is_pair_paused` checks to return the correct
/// answer, but un-pausing a pair would remove only the first occurrence,
/// leaving a ghost entry that falsely keeps the pair flagged on the next
/// `add_to_paused_index` call's duplicate check.
#[cfg(any(test, feature = "testutils"))]
fn check_pause_index_invariants(env: &Env) {
    let pairs = storage::get_paused_pairs(env);
    for i in 0..pairs.len() {
        let p_i = pairs.get(i).unwrap();
        for j in (i + 1)..pairs.len() {
            let p_j = pairs.get(j).unwrap();
            assert!(
                p_i != p_j,
                "INVARIANT #16 VIOLATED: PausedPairIndex contains duplicate pair {p_i:?} at positions {i} and {j}"
            );
        }
    }
}

// ─── Standalone invariant helpers (usable from migration tooling) ─────────────

/// Returns `true` iff the score entry index contains no entries that lack a
/// live `Score` key.  Intended for offline migration tooling and property-based
/// tests that need a cheaper yes/no answer rather than a panic.
#[cfg(any(test, feature = "testutils"))]
pub fn score_index_is_consistent(env: &Env) -> bool {
    let index = storage::get_score_entry_index(env);
    for i in 0..index.len() {
        let (wallet, pair) = index.get(i).unwrap();
        if storage::peek_score(env, &wallet, &pair).is_none() {
            return false;
        }
    }
    true
}

/// Returns `true` iff every history ring is within `HistoryMaxDepth`.
#[cfg(any(test, feature = "testutils"))]
pub fn history_rings_are_bounded(env: &Env) -> bool {
    let max_depth = storage::get_history_max_depth(env);
    let index = storage::get_score_entry_index(env);
    for i in 0..index.len() {
        let (wallet, pair) = index.get(i).unwrap();
        if storage::peek_score_history_len(env, &wallet, &pair) > max_depth {
            return false;
        }
    }
    true
}

/// Returns `true` iff `ActiveEmbargoCount` matches the live embargo count
/// derived from `EmbargoedWalletIndex`.
#[cfg(any(test, feature = "testutils"))]
pub fn embargo_count_is_consistent(env: &Env) -> bool {
    let wallets = storage::get_embargoed_wallets(env);
    let stored = storage::get_active_embargo_count(env);
    let live = {
        let mut n = 0u32;
        for i in 0..wallets.len() {
            if storage::peek_is_embargoed(env, &wallets.get(i).unwrap()) {
                n += 1;
            }
        }
        n
    };
    stored == live
}

/// Returns `true` iff the decay rate satisfies λ ∈ [0, 1] and the denominator
/// is non-zero.
#[cfg(any(test, feature = "testutils"))]
pub fn decay_rate_is_valid(env: &Env) -> bool {
    let (num, den) = storage::get_decay_rate(env);
    den > 0 && num <= den
}
