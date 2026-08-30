# `ledgerlens-score` module ownership boundaries

This document records the current ownership boundaries for issue #806 before
further refactors move logic around.

## Current concrete behavior

`ledgerlens-score` exposes one large contract implementation in
`contracts/ledgerlens-score/src/lib.rs`, with supporting modules providing
constants, errors, storage keys, event payloads, governance helpers, and a few
specialized algorithms.

Today, entrypoint methods still orchestrate multiple concerns directly. That is
functional, but it makes ownership easy to blur when adding new features.

## Ownership map

| Area | Current owner | Explicit responsibility | Must not own |
| --- | --- | --- | --- |
| Validation and fail-closed guards | `lib.rs` entrypoints plus `errors.rs`, `constants.rs` | Input bounds, auth checks, interface gating, conservative fallback behavior | Storage encoding details |
| Persistent and instance storage | `storage.rs` plus `types::DataKey*` | Key layout, load/store helpers, default lookup behavior | Policy decisions about whether a call is allowed |
| Events and event payloads | `events.rs` and test-only `event_emission.rs` | Event names, payload shapes, emission helpers | Authorization or state-transition policy |
| Governance / parameter mutation | `parameter_governance.rs` | Proposal bookkeeping and governance-specific state transitions | Generic read APIs unrelated to governance |
| Attestation / cryptographic verification | `lib.rs`, `verkle.rs`, `zk_range_proof.rs` | Signature/proof verification and integrity checks | General admin routing or storage topology |
| Read APIs / integration queries | `lib.rs` entrypoints using `storage.rs` and `types.rs` | Stable read-only contract surface for integrators | Hidden write-side side effects beyond documented audit/event paths |

## Misplaced-logic risks to watch

These are the circular assumptions the current layout still invites:

1. `lib.rs` can silently become the owner of every policy branch because it is
   the only place that sees the full call flow.
2. Storage defaults can accidentally encode business policy if helper names do
   not distinguish “missing value” from “allowed fallback”.
3. Event helpers can become pseudo-policy if callers rely on event emission
   order instead of state invariants.
4. Governance helpers can grow into a second validation layer if parameter
   rules are copied there instead of shared deliberately.

## Practical boundary rules for future changes

- Put auth, fail-closed semantics, and bounded-batch guards at the entrypoint
  that exposes the behavior.
- Put encoding, key evolution, and compatibility shims in `storage.rs`.
- Keep event modules descriptive; they should reflect a state transition, not
  decide whether one is legal.
- Keep cryptographic helper modules pure and reusable; they should not read or
  mutate unrelated contract state.

## Compatibility impact

- Public ABI: unchanged
- Events: unchanged
- Errors: unchanged
- Storage layout: unchanged

## Resource bounds

This is documentation only. It changes no runtime path and no on-chain cost.
