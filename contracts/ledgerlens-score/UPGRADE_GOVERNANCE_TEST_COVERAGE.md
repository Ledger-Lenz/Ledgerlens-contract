# Upgrade governance: direct unit test coverage

This documents four unit tests added to close gaps in direct coverage (per
CONTRIBUTING.md) for the upgrade-governance functions in
`contracts/ledgerlens-score/src/lib.rs`.

## Files checked before writing new tests

- `contracts/ledgerlens-score/src/test_upgrade.rs`
- `contracts/ledgerlens-score/src/test_upgrade_multisig.rs`
- `contracts/ledgerlens-score/src/test_param_timelock.rs`
- `contracts/ledgerlens-score/src/test_governance_action_registry.rs`
- `contracts/ledgerlens-score/src/test_public_error_snapshots.rs`

Every existing test that touches `veto_upgrade`, `set_upgrade_delay`,
`get_upgrade_approval_count`, or `get_pending_upgrade` goes through the
`setup()` / `setup_multisig()` helpers, which call `initialize` (and, for
multisig tests, also configure admin signers/threshold) before ever invoking
the function under test. None of them exercised these functions against a
freshly-registered, never-initialized contract, and none re-read
`get_pending_upgrade` after a *rejected* mutation to confirm state was left
untouched. Those were the genuine gaps closed here.

## What was added

1. **`veto_upgrade`** (`test_upgrade.rs::test_veto_upgrade_before_initialize_rejected`)
   Calls `veto_upgrade` on a contract that was registered but never
   `initialize`d. Asserts the result is `Error::NotInitialized`, not
   `Error::NoPendingUpgrade` — proving the `has_admin` guard runs first.

2. **`set_upgrade_delay`** (`test_upgrade.rs::test_set_upgrade_delay_before_initialize_rejected`)
   Calls `set_upgrade_delay` with a well-formed, in-bounds delay on an
   uninitialized contract. Asserts `Error::NotInitialized` is returned
   instead of the value being accepted or rejected as
   `Error::InvalidUpgradeDelay` — proving the `has_admin` check precedes
   bounds validation.

3. **`get_upgrade_approval_count`** (`test_upgrade_multisig.rs::test_approval_count_zero_before_initialize`)
   Calls the getter on an uninitialized contract. Asserts it returns `0`
   without panicking, since the function reads storage directly and never
   checks `has_admin`.

4. **`get_pending_upgrade`** (`test_upgrade.rs::test_get_pending_upgrade_unchanged_after_rejected_double_propose`)
   Proposes an upgrade, attempts a second (rejected) proposal, then reads
   `get_pending_upgrade` and asserts every field (`new_wasm_hash`,
   `proposed_at`, `executable_after`, `proposed_by`) still matches the
   *original* proposal — proving a rejected `UpgradeAlreadyPending` call
   leaves stored state untouched.

Each test asserts on the specific `Result`/`Error` variant or field values
expected for its scenario, not just that the call doesn't panic.
