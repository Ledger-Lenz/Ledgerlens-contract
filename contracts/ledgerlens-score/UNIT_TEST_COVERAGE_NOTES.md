# Unit test coverage additions — execute_upgrade / flash protection / is_epoch_open

This note documents four small, targeted unit tests added to close direct
coverage gaps called out against `contracts/ledgerlens-score/src/lib.rs`,
per CONTRIBUTING.md's requirement that public `lib.rs` functions get direct
unit coverage in `src/test_*.rs`, not just incidental coverage from
integration-style tests.

## Issue 1 — `execute_upgrade`

**File checked for existing coverage:** `src/test_upgrade.rs` (256 lines,
14 existing `#[test]` functions covering propose/veto/execute happy paths,
`UpgradeNotReady`, `NoPendingUpgrade`, and delay-bounds errors). Every test
in that file calls `setup()`, which always calls `client.initialize(...)`
first — no test ever calls `execute_upgrade` against a contract with no
admin set yet, so the `Error::NotInitialized` branch at the top of the
function was untested.

**Test added:** `test_execute_upgrade_before_initialize_rejected` —
registers the contract without calling `initialize`, calls
`try_execute_upgrade`, and asserts `Err(Ok(Error::NotInitialized))`.

## Issue 2 — `get_flash_protection_mode`

**File checked for existing coverage:** `src/test_flash_protection.rs`
(117 lines, 4 existing tests). Every test that reads the mode back
(`test_same_ledger_reject_mode_blocks_submission`) does so only *after*
calling `set_flash_protection_mode(Reject)` first — the function's default
return value on a freshly-initialized contract (before any admin ever sets
a mode) was never asserted.

**Test added:** `test_get_flash_protection_mode_default_before_any_set` —
calls `setup()` (initializes, never sets a mode) and asserts
`get_flash_protection_mode() == FlashProtectionMode::Warn`, matching
`storage::get_flash_protection_mode`'s `unwrap_or(FlashProtectionMode::Warn)`
fallback.

## Issue 3 — `set_flash_protection_mode`

**File checked for existing coverage:** same `src/test_flash_protection.rs`
as above. All four existing tests call `setup()` first, so the
`Error::NotInitialized` guard (checked before the admin-auth check) was
never exercised.

**Test added:** `test_set_flash_protection_mode_before_initialize_rejected`
— registers the contract without calling `initialize`, calls
`try_set_flash_protection_mode`, and asserts
`Err(Ok(Error::NotInitialized))`.

## Issue 4 — `is_epoch_open`

**File checked for existing coverage:** `src/test_epoch.rs` (94 lines,
4 existing tests covering close/open/re-open transitions). All of them call
`setup()`, which initializes the contract before checking `is_epoch_open()`.
Unlike `open_epoch`/`close_epoch`, `is_epoch_open` has no `NotInitialized`
guard — it reads straight from storage, which falls back to `true` via
`unwrap_or(true)` — and that pre-initialize default was never pinned down by
a test.

**Test added:** `test_is_epoch_open_defaults_true_before_initialize` —
registers the contract without calling `initialize` and asserts
`client.is_epoch_open() == true`.

## Result

`cargo test -p ledgerlens-score` (whole workspace) passes with these four
new tests included; no existing test files or production code were
modified.
