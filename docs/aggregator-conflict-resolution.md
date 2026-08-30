# Aggregator Conflict Resolution Policy

When scores are replicated across multiple shards, the aggregator must resolve
conflicts between potentially different values from each shard. The
`ConflictPolicy` enum controls which strategy is used.

Configuration conflicts are handled separately from score conflicts. Use
[`aggregator-split-brain.md`](aggregator-split-brain.md) and
`detect_split_brain(wallet, asset_pair)` to detect shard configuration drift
before relying on a fan-out read.

## Policy Variants

### `HighestScore` (default)

Pick the shard whose score (or aggregate score) is numerically highest.

- `get_score`: selects the `RiskScore` with the largest `score`
- `get_aggregate_score`: selects the `AggregateRiskScore` with the largest
  `aggregate_score`

This is the default policy (used if no policy has been explicitly set).

### `MostRecent`

Pick the shard whose timestamp is newest.

- `get_score`: selects the `RiskScore` with the largest `timestamp`
- `get_aggregate_score`: selects the `AggregateRiskScore` with the largest
  `last_updated`

Useful when score freshness matters more than magnitude (e.g. a rapidly changing
market).

## API

### `set_conflict_resolution_policy(Env, ConflictPolicy) -> Result<(), ScoreError>`

Sets the active policy. Requires admin authorization. Returns
`ScoreError::NotInitialized` if the contract has not been initialized.

### `get_conflict_resolution_policy(Env) -> ConflictPolicy`

Returns the current policy, or `ConflictPolicy::HighestScore` if none has been
set.

## Example

```rust
// Set to MostRecent
client.set_conflict_resolution_policy(&ConflictPolicy::MostRecent);

// get_score now returns the score from the shard with the newest timestamp
let score = client.get_score(&wallet, &pair);

// Switch back to HighestScore
client.set_conflict_resolution_policy(&ConflictPolicy::HighestScore);
```
