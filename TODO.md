# TODO: Finality buffer + pending score admin cancellation

## Step 0: Repo understanding
- [x] Read existing score submission/read paths, storage keys model, errors, and tests.

## Step 1: Add new contract types + storage keys
- [x] Update `contracts/ledgerlens-score/src/types.rs`:
  - [x] Add `PendingScoreEntry`
  - [x] Add `DataKey::FinalityBufferSecs` and `DataKey::PendingScore(Address, Symbol)`

## Step 2: Implement storage helpers
- [x] Update `contracts/ledgerlens-score/src/storage.rs`:
  - [x] get/set `FinalityBufferSecs`
  - [x] set/get/clear pending score entries

## Step 3: Add events
- [x] Update `contracts/ledgerlens-score/src/events.rs`:
  - [x] ScorePendingEvent helper (`score_pending`)
  - [x] ScoreCommittedEvent helper (`score_committed`)
  - [x] ScorePendingCancelledEvent helper (`score_pending_cancelled`)
  - [x] (bonus, matches existing admin-config event convention) `finality_buffer_updated`

## Step 4: Add errors
- [x] Update `contracts/ledgerlens-score/src/errors.rs`:
  - [x] Add `NoPendingScore`, `InvalidFinalityBuffer`, `FinalityWindowNotElapsed`

## Step 5: Wire contract API + behavior
- [x] Update `contracts/ledgerlens-score/src/lib.rs`:
  - [x] Export `PendingScoreEntry`
  - [x] Add admin functions `set_finality_buffer`, `get_finality_buffer`
  - [x] Add read `get_pending_score`
  - [x] Modify `submit_score` routing when buffer > 0
  - [x] Add public `commit_pending_score`
  - [x] Add admin multisig `cancel_pending_score`
  - Note: `submit_scores_batch` is intentionally left untouched — the issue's
    "Behaviour" section only specifies `submit_score` routing to pending vs.
    live. If batch submissions should also respect the finality buffer,
    that's a follow-up, not part of this issue's stated scope.

## Step 6: Tests
- [x] Add `contracts/ledgerlens-score/src/test_finality_buffer.rs` covering:
  - [x] buffer disabled: immediate live score (existing tests unchanged)
  - [x] buffer set: submit writes pending only; `get_score` / `query_risk_gate` unaffected
  - [x] commit before window: fails with `FinalityWindowNotElapsed`
  - [x] commit after window: live score appears, pending cleared
  - [x] commit is permissionless (no auth required)
  - [x] cancel before window: pending removed and never commits
  - [x] second submission replaces (not queues) the pending entry
  - [x] `set_finality_buffer` bounds + admin-only gating
  - [x] event assertions for all three new events

## Step 7: Run verification
- [x] `cargo test -p ledgerlens-score` — 208 passed, 0 failed
- [x] `cargo test --doc -p ledgerlens-score` for the 5 new functions — all pass
  - Note: a number of *pre-existing* doc examples elsewhere in `lib.rs`
    (e.g. `pause`, `unpause`, `set_cooldown`, `submit_score` itself) were
    already broken before this change — they call `.unwrap()` on client
    methods that don't return `Result`, or rely on an implicit `Vec` import.
    Left untouched as out of scope for this issue.
