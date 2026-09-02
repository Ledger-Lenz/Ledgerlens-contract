# Cross-Shard Reconciliation Policy Spike

_Date: 2026-08-25_

## Decision

`ledgerlens-aggregator` should keep its current conservative fan-out policies for
user-facing reads, and it should not replace the existing `primary shard wins`
default for configuration-style getters in this PR.

Where a field can legitimately diverge across shards, the preferred long-term
shape is **field-specific, conservative reconciliation or explicit alignment
requirements**, not averaging and not a generic quorum algorithm. For the
current codebase, the right operational answer is:

- keep the existing runtime behavior for `get_decay_rate`
- keep the existing runtime behavior for `get_watchlist_status`, `query_risk_gate`,
  `get_score`, `get_aggregate_score`, `get_score_across_shards`, and
  `contagion_depth_across_shards`
- use `detect_split_brain` plus shard quarantine when shard configuration
  disagrees
- defer any change to config getter reconciliation until the consumer-facing
  behavior is versioned and callers can tolerate a breaking change

This is a deliberate deferment, not a missed optimization.

## ADR Criteria Check

The existing ADR in `docs/adr/aggregator-score-responsibilities.md` says a
capability should move only when all of these hold:

1. It is a pure function of data already exposed by existing per-shard public
   functions.
2. It requires no privileged/admin state.
3. Moving it does not change fail-open/fail-closed behavior for existing
   callers.

### Verdict

- `get_score`, `get_aggregate_score`, `query_risk_gate`, `get_watchlist_status`,
  and `contagion_depth_across_shards` already satisfy the ADR boundary and do
  not need a policy change.
- `get_decay_rate` does not meet criterion 3 for a live change: any automatic
  reconciliation would change downstream effective-score behavior for existing
  callers.
- `get_consensus_threshold_k` is not a live cross-shard read in this checkout;
  if it becomes one in the related fix, it should not silently inherit a
  primary-shard default. The same criterion-3 concern applies.

## Function Classification

| Function | Current policy | Recommended policy | Rationale | Change now? |
| --- | --- | --- | --- | --- |
| `get_score` | Highest score across healthy shards | Keep highest score | Higher score is the stricter, more conservative result for a risk signal. | No |
| `get_aggregate_score` | Highest aggregate score across healthy shards | Keep highest aggregate score | Same reasoning as `get_score`. | No |
| `query_risk_gate` | AND across healthy shards; fail closed on transport/call failure | Keep AND/fail-closed | Any shard disagreeing or failing should not weaken the gate. | No |
| `get_watchlist_status` | OR across shards | Keep OR | Watchlist is a conservative signal; any positive result should surface. | No |
| `get_score_across_shards` | No reconciliation; returns per-shard values | Keep diagnostic fan-out | This is a reporting API, not a policy API. | No |
| `contagion_depth_across_shards` | Maximum depth across shards | Keep maximum | Higher depth is the conservative answer. | No |
| `detect_split_brain` | Majority/canonical diagnostic, not a business reconciliation policy | Keep diagnostic majority rule | It is meant to surface disagreement, not hide it. | No |
| `get_decay_rate` | Primary shard wins | Keep for now; require explicit versioned policy before changing | Any automatic change would alter every downstream effective-score calculation. | Defer |
| `get_consensus_threshold_k` | Hard-coded default in this checkout | If it becomes shard-backed, require explicit alignment or a conservative field rule | A threshold should not be averaged or silently borrowed from a single shard. | Defer |

## Field-Level Policy Guidance

For the configuration values that show up in the split-brain fingerprint, the
safe policy is field-specific:

| Field | Conservative direction | Recommended handling |
| --- | --- | --- |
| `decay_rate` | Faster decay / stricter effective-score aging | Prefer explicit alignment; if reconciliation is unavoidable, use the strictest effective decay rather than an average |
| `staleness_window` | Smaller window | Prefer explicit alignment; if reconciliation is unavoidable, use the minimum window |
| `global_min_confidence` | Higher floor | Prefer explicit alignment; if reconciliation is unavoidable, use the maximum floor |
| `consensus_k` | Higher threshold | Prefer explicit alignment; if reconciliation is unavoidable, use the maximum threshold |
| `consensus_epsilon` | Smaller tolerance | Prefer explicit alignment; if reconciliation is unavoidable, use the minimum epsilon |

The main point is that these fields are not interchangeable. A generic
"average everything" or "quorum everything" policy would blur materially
different security semantics.

## Divergence Detection

The repo already has the right divergence primitive: `detect_split_brain`.
It is bounded, read-only, and already compares the configuration fingerprint
across registered shards.

Recommended operating model:

1. Poll `detect_split_brain` on a stable canary wallet/pair.
2. Treat `Aligned` as healthy.
3. Treat `SplitBrain` and `QuorumLost` as page-worthy signals.
4. Quarantine the outlier shard with `set_shard_health(false)` if the mismatch
   is intentional to isolate or if the shard is known bad.
5. Keep the aggregator hot path simple; do not add quorum math to user-facing
   reads unless there is a versioned requirement to do so.

That gives operators visibility without making the contract itself guess at a
network-wide "correct" configuration.

## Gas And Complexity

The current bounded fan-out model is the right tradeoff:

- `MAX_SHARDS = 10` keeps every traversal bounded.
- `detect_split_brain` is already the natural place to spend extra read budget.
- Weighted or quorum-based reconciliation on-chain would add coordination
  overhead without removing the need for operator alignment.

In practice, the cost of a fuller reconciliation algorithm would be paid on
every consumer read, while the benefit would mostly be diagnostic because the
repo already has a separate drift detector.

## Recommendation

Defer any change to the existing primary-shard default for config getters in
this repo.

Change it only when all of the following are true:

- the field is explicitly versioned as shard-divergent
- the consumer-facing meaning is documented as field-specific
- the new behavior is covered by tests for the exact conservative rule
- downstream callers are prepared for a breaking change in the value they
  receive

Until then, `detect_split_brain` is the correct mechanism for divergence
awareness, and the current shard ordering behavior should remain the default.
