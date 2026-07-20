# Aggregator Error Codes

The `ledgerlens-aggregator` contract defines its own `Error` enum (annotated
with `#[contracterror]`) instead of reusing error codes from
`ledgerlens-score`. This gives each aggregator-specific failure condition its
own stable, meaningful numeric code.

## Error Variants

| Code | Variant | Meaning |
|------|---------|---------|
| 1 | `AlreadyInitialized` | `initialize` has already been called |
| 2 | `NotInitialized` | `initialize` has not yet been called |
| 3 | `ShardNotFound` | The given shard is not registered (e.g. `remove_shard`, `set_shard_health` for an unregistered address) |
| 4 | `ShardAlreadyExists` | Duplicate shard registration attempt in `add_shard` |
| 5 | `ShardSetFull` | Shard list has reached `MAX_SHARDS` (10) |
| 6 | `SelfShard` | Attempted to register the aggregator's own address as a shard |
| 7 | `ScoreNotFound` | No score was found across any healthy shard for the given wallet/pair (or wallet for aggregate) |

## Append-only numbering

New variants must be appended with the next available integer code. Never
reorder, remove, or rename existing variants — their numeric values are part
of the deployed contract's ABI.
