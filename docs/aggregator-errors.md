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
| 3 | `SelfReference` | Attempted to register the aggregator's own address as a shard |
| 4 | `ShardAlreadyRegistered` | Duplicate shard registration attempt in `add_shard` |
| 5 | `ShardLimitReached` | Shard list has reached `MAX_SHARDS` (10) |
| 6 | `ShardNotRegistered` | The given shard is not registered (for `remove_shard`, `set_shard_health`, or `get_shard_health`) |
| 7 | `IncompatibleInterface` | Candidate shard does not advertise the capabilities required by the aggregator |

Split-brain detection reuses existing discriminants and adds no new error code.

## Split-Brain Diagnostics

`detect_split_brain` does not return `Error`; it returns a `SplitBrainReport`
with reason-coded shard diagnostics. See
[`aggregator-split-brain.md`](aggregator-split-brain.md).

## Append-only numbering

New variants must be appended with the next available integer code. Never
reorder, remove, or rename existing variants — their numeric values are part
of the deployed contract's ABI.
