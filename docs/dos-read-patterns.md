# Resource Denial-of-Service Tests — Hostile Read Patterns

**Tracks issue:** #760  
**Test file:** [`contracts/ledgerlens-score/src/test_dos_read_patterns.rs`](../contracts/ledgerlens-score/src/test_dos_read_patterns.rs)

This document describes the hostile read patterns exercised in the DoS test
suite and explains the bounded behaviour that each test verifies.

---

## Background

Read-only functions in Soroban contracts can be called by anyone — including
hostile callers who want to:

- **Probe for panics** (a panic in a cross-contract read traps the caller's
  transaction).
- **Exhaust CPU/memory budgets** via unbounded loops or oversized return values.
- **Discover undefined behaviour** at boundary inputs (`0`, `u32::MAX`, empty
  collections).

Because `query_risk_gate` and `get_score` are called from inside other
protocols' guard clauses, a panic or trap there disables the caller's risk
check — a potential attack vector. Every read path must prove it returns a
documented value or a documented error, never a panic.

---

## Patterns and Bounds

### Pattern 1 — Repeated queries on the same wallet

**File:** `test_dos_read_patterns::repeated_query_same_wallet_does_not_panic`

200 consecutive `get_score` calls on the same `(wallet, pair)`. Each call is
`O(1)` (a single storage lookup). The test verifies every result is identical
and no panic occurs.

**Bound:** `O(1)` per call, no state mutation.

---

### Pattern 2 — Missing scores for many distinct wallets

**File:** `test_dos_read_patterns::query_unscored_wallets_returns_not_found_for_all`

100 distinct wallets that have never been scored. Each `try_get_score` call
returns `ScoreNotFound`; each `query_risk_gate` call returns `false`. No panic.

**Bound:** `O(1)` per call. "Not found" is a first-class documented result, not
an exception.

---

### Pattern 3 — Gate boundary thresholds

**File:** `test_dos_read_patterns::gate_boundary_thresholds_are_safe`

Tests `gate_threshold=0`, `gate_threshold=u32::MAX`, and `gate_threshold=101`.

- `threshold=0`: impossible to satisfy (score must be `< 0`). Always `false`.
- `threshold=u32::MAX`: outside the valid 0–100 range, so all wallets fail closed.
- `threshold=101`: outside the valid 0–100 range, so all wallets fail closed.

**Bound:** `O(1)`. No arithmetic overflow (score is a `u32`, comparison is
safe).

---

### Pattern 4 — History for missing wallet

**File:** `test_dos_read_patterns::get_score_history_missing_wallet_returns_empty`

`get_score_history` for a wallet that has never been scored. Returns an empty
`Vec` — no error, no panic.

**Bound:** `O(1)` (storage miss, returns empty `Vec`).

---

### Pattern 5 — Aggregate score with no pairs

**File:** `test_dos_read_patterns::get_aggregate_score_no_pairs_returns_not_found`

`get_aggregate_score` for a wallet with no scored pairs returns
`ScoreNotFound`. No panic.

**Bound:** `O(1)`.

---

### Pattern 6 — `get_expiring_entries` over cap

**File:** `test_dos_read_patterns::get_expiring_entries_is_bounded_at_cap`

Requesting `max_entries=200` when the hard cap is `MAX_EXPIRING_ENTRIES_PER_CALL`
(100). The result is silently capped at 100.

**Bound:** At most 100 entries returned per call, regardless of how large
`max_entries` is.

---

### Pattern 7 — Score count for never-scored wallet

**File:** `test_dos_read_patterns::get_score_count_never_scored_returns_zero`

`get_score_count` for a wallet that has never been scored returns `0`. No error,
no panic. Documented initial value.

**Bound:** `O(1)`.

---

### Pattern 8 — Batch read with mixed scored/unscored wallets

**Files:** `test_dos_read_patterns::batch_read_mixed_wallets_returns_per_entry_results`,
`test_dos_read_patterns::batch_read_over_limit_returns_batch_too_large`

A batch of 20 queries (10 scored + 10 unscored). Each entry returns a
`BatchScoreResult` with `found=true` or `found=false` — no missing entries, no
panic.

Requesting more than `BATCH_READ_MAX` (50) entries returns `BatchTooLarge`
immediately without processing any entries.

**Bound:** `O(N)` where `N ≤ BATCH_READ_MAX` (50).

---

### Pattern 9 — Unknown capability symbol

**File:** `test_dos_read_patterns::supports_interface_unknown_capability_returns_false`

`supports_interface` with arbitrary unknown symbols returns `false` — no panic,
no error.

**Bound:** `O(1)`.

---

### Pattern 10 — `get_pending_upgrade` when none

**File:** `test_dos_read_patterns::get_pending_upgrade_when_none_returns_error`

`get_pending_upgrade` with no upgrade in flight returns `NoPendingUpgrade` —
not a panic.

**Bound:** `O(1)`.

---

### Pattern 11 — Confidence gate with extreme thresholds

**Files:** `test_dos_read_patterns::confidence_gate_min_confidence_u32_max_returns_false`,
`test_dos_read_patterns::confidence_gate_threshold_zero_never_passes`

- `min_confidence=u32::MAX`: can never be satisfied (confidence ≤ 100). Always
  `false`, no overflow.
- `gate_threshold=0`: can never be satisfied. Always `false`.

**Bound:** `O(1)`. `max(min_confidence, global_min_confidence)` is computed in
safe `u32` arithmetic.

---

### Pattern 12 — Default read-only state

**Files:** multiple `*_default_*` tests

All read-only accessors (`get_global_min_confidence`, `get_history_max_depth`,
`get_cooldown`, `is_paused`, `get_paused_pairs`, `is_pair_paused`) return their
documented defaults before any admin action. None panic.

**Bound:** `O(1)` each.

---

## ABI Impact

None. These are read-only functions — no new storage keys, no new error
variants, no changes to return types.

## Resource Usage Summary

| Function | Complexity | Notes |
|---|---|---|
| `get_score` | O(1) | Single storage read |
| `try_get_score` | O(1) | Returns error instead of panic |
| `query_risk_gate` | O(1) | Bool, never panics |
| `query_risk_gate_with_confidence` | O(1) | Bool, never panics |
| `get_score_history` | O(depth) | Depth ≤ 50 |
| `get_aggregate_score` | O(pairs) | Pairs ≤ MAX_WALLET_PAIRS (20) |
| `get_score_count` | O(1) | Counter read |
| `get_expiring_entries` | O(min(N, 100)) | Hard-capped at 100 |
| `get_scores_batch` | O(min(N, 50)) | Hard-capped at BATCH_READ_MAX (50) |
| `supports_interface` | O(1) | Symbol lookup |
| `get_pending_upgrade` | O(1) | Storage read |
