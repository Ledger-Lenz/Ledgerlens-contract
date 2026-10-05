# ADR #1129: Consumer Score-Change Hooks

## Status

Accepted for implementation.

## Context

Consumers need to react to authoritative score changes. Calling a consumer
callback inline from `submit_score` would let a callback trap or consume the
remaining transaction budget, rolling back the score submission itself. The
score contract also shares one Soroban transaction budget with every nested
contract call; Soroban does not provide a per-callee CPU budget or a way to
preempt a callback after a configured amount of work.

## Decision

Hooks use deferred, explicitly requested dispatch. A successful score write
does not call hooks. An authenticated dispatcher later calls
`dispatch_score_change_hooks` for a wallet/pair; it receives the latest
committed score and a monotonically increasing score count. Repeated changes
coalesce to the latest score rather than accumulating an unbounded queue.

Registration requires the consumer contract's authorization. Registrations
are stored in one instance-storage vector, bounded to four hooks per wallet/pair
and 32 hooks globally. Dispatch scans at most that bounded registry and invokes
at most one hook per call. A consumer unregisters with its authorization.
Registration spam is bounded by both hard limits and the required consumer
authorization; no per-registration storage key or unbounded pending queue is
created.

Before dispatch, the contract sets a reentrancy flag. Score submission,
batch submission, buffered commit, dispute correction, score deletion,
contagion score mutation, registration changes, threshold changes, and nested
dispatch are rejected while the flag is set. Callback invocation uses the
Soroban fallible invocation API. A returned error or contract trap increments
that hook's consecutive-failure counter; success resets it. When the counter
reaches the configured threshold exactly, the hook is disabled and a
`hook_off` event is emitted.

## Threat Model

- **Reentrancy:** A malicious hook may call back into the score contract.
  Dispatch is guarded, hook registry/threshold mutation is blocked, and all
  identified authoritative score-mutation entry points reject during dispatch.
- **Expensive hook griefing:** The dispatcher invokes at most one hook, and
  registry scanning is bounded by 32 records. This limits fan-out but cannot
  impose a separate Soroban CPU/memory budget on one hook. A callback that
  exhausts the transaction's host budget can still abort that dispatch
  transaction before its failure counter is committed. Dispatch is separate
  from score submission, so this cannot roll back the already committed score.
- **Registration/storage/resource spam:** Only the consumer can authorize its
  registration and removal. The per-pair and global hard limits bound the
  singleton registry, scan cost, and future dispatch work. Disabled hooks
  continue to occupy capacity until their consumer unregisters them, avoiding
  automatic cleanup work or a second unbounded index.

## Resource Validation

The repository has Criterion benchmarks, including
`contracts/ledgerlens-score/benches/entry_point_budgets.rs`. No hook benchmark
or measured worst-case hook cost is claimed here: `cargo` is unavailable in
the current environment, so neither focused tests nor benchmarks can be run.
The one-hook dispatch ceiling and 32-record registry ceiling are structural
bounds, not measured CPU/memory results.