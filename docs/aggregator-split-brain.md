# Aggregator Split-Brain Detection

This document defines the observable contract for detecting conflicting
configuration across `ledgerlens-aggregator` shards.

## PR Checklist

Entry points affected:

- `detect_split_brain(wallet, asset_pair) -> SplitBrainReport`
- `set_shard_health(shard, healthy) -> Result<(), Error>`
- `get_shard_health(shard) -> Result<bool, Error>`
- `supports_interface("sbrain")`
- `supports_interface("health")`

Storage keys affected:

- Existing `ShardHealth(Address)` is now writable through `set_shard_health`.
- No existing storage key is renamed or retyped.
- No score-contract storage key is changed.

Events affected:

- `sh_health` is emitted when an admin marks a shard healthy or unhealthy.

Errors affected:

- No new aggregator error discriminants.
- Existing `ShardNotRegistered` is reused for health operations on unknown
  shards.

Tests affected:

- `contracts/ledgerlens-aggregator/src/test.rs`
- `tests/composability/tests/aggregator_fanout.rs`

## Design

`detect_split_brain` probes every registered shard in registration order, capped
by `MAX_SHARDS = 10`. It never writes storage. Each healthy shard is classified
with a bounded reason code:

- `Aligned`: shard is available and matches the canonical configuration.
- `ConfigMismatch`: shard is available but its configuration fingerprint does
  not match the canonical fingerprint.
- `Unavailable`: shard call failed or returned malformed data.
- `Stale`: shard has score data for the requested wallet/pair, but that score
  is stale under the shard's own staleness rule.
- `Unhealthy`: operator has quarantined the shard through `set_shard_health`.

The configuration fingerprint is:

- decay rate numerator
- decay rate denominator
- staleness window
- global minimum confidence
- consensus `k`
- consensus epsilon

Missing score data for the diagnostic wallet/pair is not considered stale. This
keeps configuration checks usable before a canary wallet has been scored.
Soroban does not expose an in-contract timeout primitive; timed-out, trapped,
or otherwise failed cross-contract invocations are all classified as
`Unavailable`.

## Quorum and Tie-Breaking

The required quorum is a strict majority of healthy registered shards:

```text
required_quorum = healthy_count / 2 + 1
```

The canonical fingerprint is the fingerprint with the highest count among
available, non-stale shards. If two fingerprints have the same count, the
lexicographically lowest fingerprint is selected only to make diagnostics
deterministic; the report still returns `QuorumLost` when the selected
fingerprint does not meet `required_quorum`.

Statuses:

- `NoShards`: no registered shards.
- `Aligned`: canonical fingerprint meets quorum and no available shard differs.
- `SplitBrain`: canonical fingerprint meets quorum, but at least one available
  shard differs.
- `QuorumLost`: no fingerprint reaches the required quorum.

## Trust Assumptions

- The aggregator admin controls shard registration and health quarantine.
- A shard can lie about capabilities at registration. Runtime probes still
  classify missing or malformed configuration methods as `Unavailable`.
- Shards are independent deployments and can diverge through delayed parameter
  changes, signer mistakes, or interrupted recovery.

## Authorization Boundaries

`detect_split_brain` is permissionless and read-only. It does not modify
`LastShardFailure`, TTLs, shard health, or score state.

`set_shard_health` is admin-only. It is the recovery switch for quarantining a
known-bad shard without removing it from the registry. `get_shard_health` is
permissionless but validates that the shard is registered.

## State Transitions

```text
registered healthy shard
  -> detect_split_brain classifies aligned, mismatched, stale, or unavailable
  -> admin set_shard_health(false) quarantines it
  -> detect_split_brain reports Unhealthy and excludes it from quorum
  -> admin set_shard_health(true) restores it to quorum calculation
```

`add_shard` remains atomic: it validates authorization, duplicate/self-reference
checks, shard limit, and capability support before storing the new shard list.
`remove_shard` remains atomic: it writes the new shard list and removes the
associated `ShardHealth` override in the same invocation.

## Failure Modes and Recovery

| Failure mode | Detection | Recovery |
| --- | --- | --- |
| Shards disagree on configuration | `SplitBrain` with `ConfigMismatch` diagnostics | quarantine divergent shard or align its score-contract configuration |
| No configuration reaches majority | `QuorumLost` | stop relying on aggregator result until enough shards are restored |
| Shard unavailable or malformed | `Unavailable` | inspect shard deployment, RPC health, and interface version |
| Shard score is stale for canary wallet/pair | `Stale` | resume score publication or mark shard unhealthy |
| Bad health update | `sh_health` event and `get_shard_health` | admin restores the previous value |

## Resource Bounds

Worst-case work is bounded by `MAX_SHARDS = 10`.

- Shard traversals: at most 10.
- Returned diagnostics: at most 10.
- Configuration calls per healthy shard: one score existence check, one stale
  check only when score data exists, and four configuration reads.
- Quorum counting: at most 10 x 10 fingerprint comparisons.
- Persistent writes from `detect_split_brain`: zero.

## Usage

```rust
let report = aggregator.detect_split_brain(&wallet, &symbol_short!("XLM_USDC"));
if report.status != SplitBrainStatus::Aligned {
    // fail closed or route to fallback shard policy
}
```

Operator quarantine:

```rust
aggregator.set_shard_health(&shard_id, &false);
let healthy = aggregator.get_shard_health(&shard_id);
assert!(!healthy);
```
