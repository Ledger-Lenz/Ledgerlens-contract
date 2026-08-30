# Critical state-transition scenario matrix

This matrix records the concrete behavior of the `ledgerlens-score` and
`ledgerlens-aggregator` contracts at the time of issues #779, #780, #781, and
#782.
It is an executable-coverage index: every row names the deterministic test
that protects the transition, or an explicit bounded follow-up where mutation
coverage is not yet automated.

## Matrix

| Path | Starting state and actor | Successful transition | Boundary/adversarial behavior | Deterministic coverage |
|---|---|---|---|---|
| Score initialization | No admin; nominated admin authorizes | Stores admin, service, and zero audit root exactly once | Missing nominated-admin authorization traps and rolls back all writes; a second call returns `AlreadyInitialized` | `test::test_initialize`, `test::test_initialize_twice_fails`, `test::test_initialize_requires_nominated_admin_and_rolls_back` |
| Aggregator initialization | No admin; nominated admin authorizes | Stores admin exactly once | Missing nominated-admin authorization traps and leaves `get_admin` at `NotInitialized`; duplicate returns `AlreadyInitialized` | `ledgerlens-aggregator::test_initialize`, `test_initialize_twice_fails`, `test_initialize_requires_nominated_admin_and_rolls_back` |
| Single submission | Initialized, unpaused, authorized service, valid bounded fields | Stores the complete score and updates bounded indexes/history | Global/pair pause, missing auth, invalid score/confidence/timestamp, cooldown, attestation, and signer quorum reject before a live score is accepted | `test_submit_and_get_score`, `test_paused_blocks_submission`, `test_pair_pause::*`, `test_multisig_service::*`, `test_attestation::*`, `test_cooldown::*` |
| Batch submission | Same as single submission; `1..=MAX_BATCH_SIZE` entries | Returns one indexed result per input and processes within the fixed cap | Empty and over-limit batches reject; per-entry failures are explicit and cannot silently become successes | `test_batch_*`, `test_batch_attestation::*`; batch resource benches in `contracts/ledgerlens-score/benches` |
| Oversized symbol / byte input | Any caller supplies an oversized `asset_pair`, score commitment, or dispute replay byte payload | Valid 9-byte symbols, 32-byte commitments, and bounded replay inputs continue through the normal path | `asset_pair > 9` rejects before auth fan-out, hashing, shard traversal, or storage mutation; malformed commitment bytes reject before submission writes; oversized dispute commit/reveal payloads reject without clearing the stored commitment; aggregator queries fail closed without shard fan-out | `test_submit_score_rejects_oversized_asset_pair_without_mutation`, `test_submit_score_rejects_oversized_commitment_bytes_without_mutation`, `test_submit_scores_batch_rejects_oversized_pair_per_entry`, `test_commit_dispute_bond_rejects_oversized_preimage_without_mutation`, `test_open_dispute_rejects_oversized_reveal_input_without_clearing_commitment`, aggregator `test_oversized_asset_pair_fails_closed_before_shard_fanout` |
| Admin governance | Initialized; current single admin or valid M-of-N admin set authorizes | Mutations update the named configuration and emit the matching event | Unknown/duplicate signers, insufficient quorum, invalid bounds, and unauthorized calls reject | `test_admin_multisig::*`, `test_parameter_governance::*`, `test_param_timelock::*`, `event_emission::*` |
| Admin transfer | Current admin nominates; nominated address authorizes acceptance | Pending admin becomes current and pending slot is cleared | No pending transfer errors; cancellation preserves old admin; overwrite is explicit | `test_admin_transfer::*` |
| Global pause | Initialized; admin quorum authorizes | `pause` sets global flag; `unpause` clears it | Writes fail with `ContractPaused`; reads remain available; repeated state assignments do not grow storage | `test_chaos_pause::*`, `test_is_paused::*`, `tests/composability/tests/integration_lifecycle.rs::test_paused_state_blocks_submission` |
| Pair pause | Initialized; admin authorizes | Only the selected pair is frozen and bounded index is updated | Duplicate pause is idempotent; index is capped; unrelated pairs and reads remain available | `test_pair_pause::*` |
| Upgrade | Admin quorum proposes one hash; ledger time reaches committed deadline | Proposal is stored, executable only at/after deadline, then cleared on execute or veto | Missing proposal, duplicate proposal, early execution, invalid delay, service-only authority, and insufficient admin quorum reject | `test_upgrade::*`, `test_upgrade_multisig::*`, `upgrade_smoke::*` |
| Deletion | Initialized; admin quorum authorizes | `clear_score` removes live score; `clear_score_history` removes history; each emits an audit event | Missing entries are idempotent; clearing one representation or pair does not erase another | `test_clear_score_*`, `test_clear_score_history_*`, `test_gdpr_accumulator::*`, `test_histogram::*` |
| Direct query | Any caller/contract | Existing, non-embargoed data is returned without mutation | Unknown/embargoed data returns the documented error/absence; risk gates fail closed | `test_query_helpers::*`, `test_confidence_gate::*`, `test_gate_enforcement::*`, `test_embargo::*` |
| Aggregated query | At least one compatible registered shard | Conservative max/AND/OR policy is applied as documented | Zero shards and shard call failures fail closed; fan-out is capped by `MAX_SHARDS = 10`; incompatible shards reject at registration | `aggregator_fallback_gate.rs`, `aggregator_fanout.rs`, `aggregator_shard_pause.rs`, aggregator unit tests |

## Mutation-resistant assertion policy

High-value transition tests must assert the observable post-state and the exact
contract error, not merely that a call completed. Authorization tests must also
assert rollback: after a rejected privileged call, getters must still report
the pre-transition state. Event tests must assert schema version and, for
security-sensitive transitions, the event topic/data values.

The initialization regression tests deliberately satisfy this policy: deleting
either new `admin.require_auth()` call makes the unauthorized call succeed,
the first assertion fail, and the rollback assertions observe attacker-chosen
state. Replacing the authorization target with `service`, the invoker, or a
contract address is also caught.

The oversized-input regressions use the same pattern: they assert the exact
error or fail-closed return value, then assert that no live score, wallet-pair
index entry, shard-failure marker, or dispute progression was created as a
side effect. Removing the guard, moving it after hashing/fan-out, or clearing
state on the rejected dispute-reveal path causes deterministic failures.

For local mutation analysis, target the two initializer functions and the
listed critical tests with `cargo-mutants`. Mutation analysis was not executed
for this change because the requested delivery explicitly excludes test runs.

## Compatibility and resource impact

- **Public ABI:** unchanged. Function names, arguments, return values, error
  discriminants, and interface capability symbols are unchanged.
- **Events:** unchanged. Successful initialization still emits no event.
- **Storage:** unchanged. No key or value layout changes and no migration.
- **Authorization behavior:** intentionally tightened. Both initializers now
  require authorization from the nominated admin.
- **Malformed-input behavior:** intentionally tightened. Oversized asset-pair
  symbols and malformed oversized byte payloads now reject at the contract
  boundary, using `InvalidAttestation` on `Result` APIs and conservative
  `false`/empty outputs on infallible query APIs.
- **Resources:** one bounded host `require_auth` operation was added to each
  one-time initializer, and constant-time length checks were added before
  expensive auth, hashing, proof parsing, or shard fan-out. No new loop,
  collection, or persistent entry was added. Existing worst-case bounds remain
  `MAX_BATCH_SIZE`, `BATCH_READ_MAX`, `MAX_ASSET_PAIR_BYTES = 9`,
  `MAX_SCORE_COMMITMENT_BYTES = 32`, bounded dispute replay inputs, bounded
  per-pair history, and `MAX_SHARDS = 10`.
