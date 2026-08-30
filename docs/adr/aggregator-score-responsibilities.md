# ADR: Aggregator vs. Score Contract Responsibilities

**Date:** 2026-07-26 · **Status:** Accepted — clarifies current boundaries, no code changes required

## Context

`ledgerlens-score` (`contracts/ledgerlens-score/src/lib.rs`, ~10.5k lines, 271
public functions) and `ledgerlens-aggregator`
(`contracts/ledgerlens-aggregator/src/lib.rs`, ~385 lines, 15 public
functions) have grown asymmetrically. It is not documented anywhere which
contract *should* own a given piece of behavior, which makes it easy for new
features to land in the wrong layer.

## Current responsibilities

**`ledgerlens-score` (per-shard contract)**
- Source of truth for wallet/asset-pair risk scores: `submit_score`,
  `submit_scores_batch`, `submit_scores_batch_attested`, consensus
  commit/reveal (`commit_consensus`, `reveal_consensus`,
  `submit_consensus_score`).
- All score-derived computation: interpolation, decay, variance, portfolio
  VaR, pair correlation, adaptive thresholds, privacy-preserving aggregates.
- All persisted state for the above (storage keys in `types.rs`), plus
  governance primitives that gate writes to that state: multisig admin,
  parameter timelock, rate limiting, embargo, pause.
- Its own risk-gate evaluation: `query_risk_gate` (single-shard opinion).

**`ledgerlens-aggregator` (fan-out/registry contract)**
- Shard registry: `add_shard`, `remove_shard`, `get_shards`.
- Cross-shard read fan-out: `get_score`, `get_aggregate_score`,
  `get_score_across_shards`, `contagion_depth_across_shards`.
- Cross-shard gate composition: `query_risk_gate`, which calls every healthy
  shard's own `query_risk_gate` and ANDs the results, failing closed on any
  unreachable/erroring shard (see `aggregator/src/lib.rs:196-222` and
  `tests/composability/tests/aggregator_shard_pause.rs`, issue #411).
- Shard-health bookkeeping: `get_last_shard_failure`.

## Ambiguity this ADR resolves

1. **`query_risk_gate` exists on both contracts with different semantics.**
   The score contract's version answers "does this wallet pass the gate on
   *this* shard's data." The aggregator's version answers "does this wallet
   pass the gate on *every* shard," and is explicitly documented as mirroring
   the per-shard check. This is intentional, not duplication to remove: the
   aggregator has no scoring data of its own, so it can only ever be a
   composition layer over shard-level gate decisions. Any change to
   fail-open/fail-closed semantics in the score contract's `query_risk_gate`
   must be mirrored in the aggregator's composition logic, or the two will
   silently diverge.
2. **Governance/config primitives belong to the score contract, not the
   aggregator.** Parameter timelocks, rate limits, embargo, and admin
   multisig all gate *writes* to score state, which only the score contract
   holds. The aggregator has no equivalent governance surface today because
   it holds no mutable risk state beyond the shard registry — `add_shard`/
   `remove_shard` are admin-gated but not timelocked. This is a deliberate
   asymmetry, not an oversight: the shard registry is low-frequency
   infrastructure config, not a risk parameter.
3. **New score-derived analytics (e.g. portfolio VaR, pair correlation)
   belong in `ledgerlens-score`,** even when the primary consumer is
   cross-shard, because the underlying data and its access controls
   (embargo, privacy epsilon, rate limits) live there. The aggregator should
   only add a cross-shard *composition* of an already-existing per-shard
   primitive, never a new computation.

## Future migration criteria

Move a capability from `ledgerlens-score` to `ledgerlens-aggregator` (or a new
contract) only when **all** of the following hold:
- The capability is a pure function of data already exposed by existing
  per-shard public functions (no new storage reads inside the score
  contract).
- It requires no privileged/admin state — the aggregator's registry has no
  timelock/multisig machinery today, so anything requiring that governance
  must stay in `ledgerlens-score` until such machinery is added there too.
- Moving it does not change fail-open/fail-closed behavior for existing
  callers (see the risk-gate precedent from issue #411).

Move a capability from `ledgerlens-aggregator` into `ledgerlens-score` only if
it stops being a cross-shard composition and becomes single-shard state.

## Compatibility impact

Documentation only. No public ABI, event, error, or storage changes.
