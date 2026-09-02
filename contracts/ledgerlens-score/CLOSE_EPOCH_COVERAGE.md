# Unit test coverage: close_epoch / get_current_epoch / get_reveal_window / set_reveal_window

Adds one focused unit test per function, each covering a boundary or
failure-path scenario that existing tests did not already exercise.

## Issue 1 — `close_epoch` (lib.rs ~line 6008)

**Gap:** every existing `close_epoch` call in `test_epoch.rs`,
`test_admin_multisig.rs`, and `test_embargo.rs` runs through a `setup()`
helper that always calls `initialize()` first, so the `NotInitialized`
branch (`!storage::has_admin(&env)`) was never exercised.

**Test added:** `test_close_epoch_before_initialize_fails` in
`src/test_epoch.rs` — registers the contract without initializing it,
calls `try_close_epoch`, and asserts `Err(Ok(Error::NotInitialized))`.

## Issue 2 — `get_current_epoch` (lib.rs ~line 6020)

**Gap:** `test_epoch_transitions` in `test_epoch.rs` asserts
`get_current_epoch() == 0` immediately after `close_epoch`, but the epoch
id was already `0` at that point, so the assertion can't distinguish
"reset to 0" from "left unchanged." No test confirms the id survives a
`close_epoch` call once a non-zero epoch has been opened.

**Test added:** `test_get_current_epoch_persists_after_close_of_nonzero_epoch`
in `src/test_epoch.rs` — opens epoch 7, closes it, and asserts
`get_current_epoch()` still returns `7` (close_epoch only flips
`EpochOpen`, it never resets the stored epoch id).

## Issue 3 — `get_reveal_window` (lib.rs ~line 5703)

**Gap:** checked `test_consensus.rs` and `test_public_error_snapshots.rs`
— every existing reference to the reveal window calls
`set_reveal_window(1)` before reading it, so the storage-layer default
(`3_600` seconds, from `storage::get_reveal_window_secs`) was never
asserted directly.

**Test added:** `test_get_reveal_window_default_before_any_set` in
`src/test_consensus.rs` — reads `get_reveal_window()` right after
`setup()` (no prior `set_reveal_window` call) and asserts it equals
`3_600`.

## Issue 4 — `set_reveal_window` (lib.rs ~line 5692)

**Gap:** checked `test_admin_multisig.rs` — it exercises the
`InsufficientAdminSigners` quorum-enforcement path for `set_risk_threshold`
and `pause`, but never for `set_reveal_window`.

**Test added:**
`test_set_reveal_window_insufficient_signers_after_multisig_configured` in
`src/test_consensus.rs` — configures a 2-of-2 admin quorum, calls
`try_set_reveal_window` with only one signer, asserts
`Err(Ok(Error::InsufficientAdminSigners)))`, and confirms the reveal
window value was left unchanged at its default.

## Files touched

- `contracts/ledgerlens-score/src/test_epoch.rs`
- `contracts/ledgerlens-score/src/test_consensus.rs`
