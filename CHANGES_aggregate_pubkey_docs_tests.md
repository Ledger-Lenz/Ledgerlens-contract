# Aggregate service pubkey: docs + test coverage

Branch: `feat/aggregate-pubkey-docs-and-tests` (4 commits, 81 lines changed)

## Issue 1 — rustdoc example for `get_aggregate_service_pubkey`
`contracts/ledgerlens-score/src/lib.rs`: added a fenced `/// ```rust` example
above `pub fn get_aggregate_service_pubkey`, showing the `ServicePubkeyNotSet`
error case (via `try_get_aggregate_service_pubkey`) and the happy path after
`set_aggregate_service_pubkey` is called. Additive doc only, no behavior change.

## Issue 2 — unit test for `get_pending_aggregate_pubkey`
`contracts/ledgerlens-score/src/test_aggregate_key_rotation.rs`: added
`test_get_pending_aggregate_pubkey_none_before_any_rotation`, asserting the
function returns `None` on a freshly initialized contract that has never
called `set_aggregate_service_pubkey` or `rotate_aggregate_service_pubkey`.
Checked `test_aggregate_key_rotation.rs` and `test_threshold_attestation.rs`
first — existing coverage only reads this function after a rotation has
already started or resolved, never in the pre-rotation state.

## Issue 3 — unit test for `rotate_aggregate_service_pubkey`
`contracts/ledgerlens-score/src/test_aggregate_key_rotation.rs`: added
`test_rotate_aggregate_service_pubkey_before_initialize_fails_not_initialized`,
asserting the call fails with `Error::NotInitialized` (and leaves no pending
key) when invoked before the contract has an admin. Checked the same two test
files — all existing rotation tests build on a `setup()` helper that always
calls `initialize()` first, so this boundary was untested.

## Issue 4 — rustdoc example for `set_aggregate_service_pubkey`
`contracts/ledgerlens-score/src/lib.rs`: added a fenced `/// ```rust` example
above `pub fn set_aggregate_service_pubkey`, showing initialization, setting a
33-byte SEC-1 pubkey, and reading it back via `get_aggregate_service_pubkey`.
Additive doc only, no behavior change.

## Notes
- Not built or tested locally per task instructions; examples/tests follow
  the exact conventions of neighboring code (`set_consensus_config`,
  `get_score_percentile`, `test_admin_rotation.rs`'s `try_`/`Err(Ok(..))`
  assertion pattern) so they should compile and pass as-is.
