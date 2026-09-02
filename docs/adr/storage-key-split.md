# ADR: DataKey Storage-Key Split Analysis

**Date:** 2026-07-20 · **Status:** Analysis complete — Formalized in [**ADR 0001: Storage-Key Enum Partitioning**](0001-storage-key-enum-split.md)

> [!NOTE]
> This analysis has been formalized into the project's official decision record: [**ADR 0001: Storage-Key Enum Partitioning**](0001-storage-key-enum-split.md). Please refer to ADR 0001 for up-to-date variant budgets and contributor placement rules.

## Context

Storage keys in `contracts/ledgerlens-score/src/types.rs` are split across five
`#[contracttype]` enums:

| Enum | Variants | Purpose |
|------|----------|---------|
| `DataKey` | ~56 | Core contract state (admin, scores, config) |
| `DataKeyB` | ~41 | Extensions: consensus, disputes, decay, HLL, Verkle |
| `DataKeyC` | ~50 | Extensions: gates, thresholds, trends, adaptive params |
| `DataKeyD` | ~21 | Extensions: oracles, epochs, clusters, volatility |
| `GateDataKey` | 6 | Gate-specific state |

**Total:** ~174 distinct storage key variants.

## Investigation

### Is there a Soroban SDK variant-count limit?

No. Soroban `#[contracttype]` enums encode their discriminant as a `u32` in
XDR, supporting up to ~4 billion variants. The SDK does not enforce a
compile-time or runtime limit on variant count.

The practical constraints that likely drove the split are:

1. **WASM binary size** — each variant generates
   serialize/deserialize/match code. With 174 variants in one enum, the
   compiler may generate large dispatch tables that bloat the ~100 KB WASM
   limit.
2. **Compile-time performance** — large `match` statements on a single enum
   slow down compilation.
3. **Developer ergonomics** — a single 174-variant enum is harder to navigate,
   and adding a new variant requires touching every existing match (even
   `unreachable!` arms).

### Can the enums be safely merged?

**Not without a storage migration.** Soroban's `#[contracttype]` encoding uses
a hash of the type *name* as part of the storage-key derivation. Merging two
enums (e.g., adding `GateDataKey::GateCallers` as `DataKey::GateCallers`)
changes the derived key for every existing entry — existing stored data becomes
unreachable through the new key.

### What about deduplication across enums?

There are **23 variant names duplicated** between `DataKeyB` and `DataKeyC`
(identical name and type signature in both). This means the same logical value
(e.g., `DecayRate`) can be stored under two different storage keys depending on
which enum the calling code happened to use. However, removing a variant from
either enum would change the storage key for code that currently references it,
which would orphan the data stored under the removed variant's key.

### Duplicate variant inventory

| Variant | In `DataKeyB`? | In `DataKeyC`? | In `DataKey`? |
|---------|----------------|----------------|---------------|
| `ModelPosteriorWeight(u32)` | ✓ (line 507) | ✓ (line 538) | |
| `SignerAddedAt(Address)` | ✓ (line 522) | ✓ (line 539) | |
| `RevealWindowSecs` | ✓ (line 516) | ✓ (line 553) | |
| `AggregateServicePubKey` | ✓ (line 499) | ✓ (line 558) | |
| `ScoreHistogram` | ✓ (line 520) | ✓ (line 562) | |
| `SignerTtl` | ✓ (line 524) | ✓ (line 564) | |
| `SignerGracePeriod` | ✓ (line 523) | ✓ (line 566) | |
| `DecayRate` | | ✓ (line 568) | ✓ (line 425) |
| `ScoreEntryIndex` | ✓ (line 518) | ✓ (line 571) | |
| `ScoreEntryLastTouchedLedger(Address, Symbol)` | ✓ (line 519) | ✓ (line 572) | |
| `ModelVersionIndex` | ✓ (line 508) | ✓ (line 573) | |
| `DecayCurveConfig` | ✓ (line 502) | ✓ (line 575) | |
| `DecayCheckpoint(Address, Symbol)` | ✓ (line 501) | ✓ (line 577) | |
| `DormancyInactivitySecs` | ✓ (line 504) | ✓ (line 579) | |
| `DormancyDecayFractionBps` | ✓ (line 503) | ✓ (line 581) | |
| `FinalityDepth` | ✓ (line 505) | ✓ (line 583) | |
| `ScoreSubmissionLedger(Address, Symbol)` | ✓ (line 521) | ✓ (line 585) | |
| `ScoreBreakdown(Address, Symbol)` | ✓ (line 517) | ✓ (line 587) | |
| `PairScoreCount(Symbol)` | ✓ (line 512) | ✓ (line 590) | |
| `TotalWalletsScored` | ✓ (line 525) | ✓ (line 593) | |
| `AdaptiveRateLimit` | ✓ (line 498) | ✓ (line 595) | |
| `MomentumWindow` | ✓ (line 511) | ✓ (line 597) | |
| `MomentumAlertThreshold` | ✓ (line 510) | ✓ (line 599) | |
| `InterpolationMethod` | ✓ (line 506) | ✓ (line 601) | |
| `GateCallers` | | ✓ (line 548) | (in `GateDataKey`) |
| `GateOpen` | | ✓ (line 549) | (in `GateDataKey`) |

## Decision

**Leave the five-enum split as-is.** The reasons:

1. **Merging would break storage.** Even partial consolidation (e.g., merging
   `GateDataKey` into `DataKey`) changes the derived storage key for every
   affected variant, orphaning existing data. Soroban contracts have no
   built-in storage-migration mechanism — a safe migration would require a
   multi-step upgrade (read old key, write new key, remove old key) that must
   itself be time-locked and tested, adding significant risk for no functional
   gain.

2. **The duplicates, while untidy, do not cause bugs.** Each enum pair
   (`DataKeyC::DecayRate` and `DataKey::DecayRate`) stores data under a
   *different* storage key because the Soroban SDK incorporates the type name
   into the key derivation. Code that reads `DataKeyC::DecayRate` always gets
   the value that was written with `DataKeyC::DecayRate`. The two copies are
   independent — they do not collide.

3. **Future consolidations should happen at variant-addition time.** New
   storage keys should be added to the most natural existing enum rather than
   creating a new one. The current five enums are already a stable part of the
   contract's ABI; new features should prefer adding variants to `DataKeyD` or
   the existing enums before creating a `DataKeyE`.

4. **A full consolidation into a single enum would likely exceed the WASM
   binary size budget.** A single ~174-variant enum would generate a
   substantially larger dispatch table. Given that the current ~100 KB WASM
   output is already close to Soroban's practical limits, consolidation would
   risk deployment failure.

## Follow-ups

- If a Soroban storage-migration primitive becomes available in a future SDK
  version, revisit this analysis. A safe migration would require:
  1. A temporary read-fallback that checks both old and new keys.
  2. A one-off on-chain migration transaction that reads from the old key and
     writes to the new key for every affected entry.
  3. Removal of the read-fallback after a transition window.
- Before adding new storage-key enums (`DataKeyE`, etc.), prefer adding
  variants to existing enums. If an enum approaches ~60 variants, consider
  splitting along *domain boundaries* (e.g., one enum per subsystem) rather
  than letting the split grow organically as it has to date.

## References

- GitHub issue: [#420](https://github.com/Ledger-Lenz/Ledgerlens-contract/issues/420)
- Source: `contracts/ledgerlens-score/src/types.rs`
