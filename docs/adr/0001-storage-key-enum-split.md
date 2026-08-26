# ADR 0001: Storage-Key Enum Partitioning (`DataKey` Family Split)

**Date:** 2026-08-26 · **Status:** Accepted — Formalizes existing storage key partitioning and contributor guidelines

---

## Context

Storage keys in `contracts/ledgerlens-score/src/types.rs` are partitioned across multiple `#[contracttype]` enums (`DataKey`, `DataKeyB`, `DataKeyC`, `DataKeyD`, `DataKeyE`, and `GateDataKey`) rather than a single unified `DataKey` enum.

As the contract expanded to support multi-sig governance, commit-reveal consensus, disputes, decay curves, Verkle trees, hyperloglog cardinality, risk gates, volatility tracking, and post-incident recovery snapshots, the number of distinct storage keys grew to nearly 200.

This Architecture Decision Record (ADR) establishes the technical justification for the storage-key enum split, details the constraints imposed by the Stellar Soroban platform, records current enum capacities and variant inventories, and defines an unambiguous decision rule for contributors adding new storage keys.

---

## Technical Drivers & Soroban Constraints

### 1. WASM Bytecode Size Budget & Codegen Overhead
Soroban contracts execute within a WebAssembly (WASM) virtual machine environment with strict memory, CPU gas metering, and contract bytecode size limits (~100 KB target budget).

When an enum is annotated with `#[contracttype]`, the `soroban-sdk` proc-macros generate extensive boilerplate:
- Serialization and deserialization routines to and from Soroban `Val` types.
- Contract XDR specification type exports (`__SPEC_XDR_TYPE_*`).
- Exhaustive `match` dispatch tables and conversion functions.

A single monolithic enum containing ~200 variants generates massive dispatch tables and bloated WASM code sections. Splitting the storage keys across smaller, focused enums prevents excessive codegen bloat and keeps the compiled binary safely within deployment limits.

### 2. Compilation Performance & Compiler Diagnostics
Large enums with dozens of tuple variants (e.g. `(Address, Symbol)`, `(Address, Address, Symbol)`) substantially increase Rust/LLVM compilation times and stack usage during code generation. Partitioning enums into domain-sized groups (~50 variants max) keeps compilation times manageable and makes compiler error messages significantly clearer.

### 3. On-Chain Storage Key Derivation & Immutability
In the Soroban SDK, storage keys are derived deterministically by hashing both the **enum type identifier** (e.g., `Symbol::new(env, "DataKey")` vs `Symbol::new(env, "DataKeyB")`) and the variant tag/payload.

**Critical Consequence:**
- Merging existing enums (e.g., moving a variant from `DataKeyB` to `DataKey`) alters the derived on-chain storage key hash.
- Any data written under the old key becomes permanently unreachable (orphaned) under the new key.
- Soroban does not provide automated on-chain storage migration primitives. A migration would require deploying a temporary read-fallback dual-query layer and executing costly batch-rewrite transactions.
- Therefore, **existing enum variants and their host enum assignments are strictly immutable on deployed networks**.

---

## Enum Inventory & Capacity Budget

### Size / Variant-Count Budget
- **Recommended Upper Bound:** **50–60 variants** per `#[contracttype]` enum.
- Enums reaching ~50 variants are considered closed to new features, and subsequent storage keys must be routed to the next active expansion enum.

### Current Utilization Table

| Enum | Current Variants | Target Ceiling | Status | Primary Purpose / Subsystem Domain |
| :--- | :---: | :---: | :---: | :--- |
| `DataKey` | **50** | 50–60 | **Full / Stable** | Core state (Admin, scores, core config, thresholds, rate-limits, floor policy) |
| `DataKeyB` | **49** | 50–60 | **Full / Stable** | Consensus commit-reveal, disputes, parameter proposals, HLL, Verkle |
| `DataKeyC` | **50** | 50–60 | **Full / Stable** | Risk gates, adaptive thresholds, momentum, decay curves, privacy epsilon |
| `DataKeyD` | **39** | 50–60 | **Active Expansion** | Oracles, epochs, volatility, alert acks, recovery snapshots, frozen state |
| `DataKeyE` | **1** | 50–60 | **Next Expansion** | Submission provenance and future high-cardinality extensions |
| `GateDataKey` | **6** | 20 | **Specialized** | Gatekeeper subsystem configuration and fee accounting |
| **Total** | **195** | — | — | **Overall Contract Storage Surface** |

---

## Cross-Enum Duplicates & Historical Context

There are 23 variant names that historically appear in both `DataKeyB` and `DataKeyC` (e.g., `ModelPosteriorWeight`, `SignerAddedAt`, `RevealWindowSecs`, `DecayRate`, `ScoreHistogram`, `TotalWalletsScored`, `MomentumWindow`, `InterpolationMethod`, etc.).

### Why They Exist
During parallel development of earlier contract milestones (issues #204, #275, #289, #290), several features introduced variants into `DataKeyB` and `DataKeyC` independently before formal module boundaries were documented.

### Why They Must Remain
Because Soroban incorporates the type name into the storage key hash:
- `DataKeyB::DecayRate` and `DataKeyC::DecayRate` write to completely distinct on-chain storage locations.
- Code calling `storage.get(&DataKeyB::DecayRate)` reads only the value written by `DataKeyB::DecayRate`.
- Removing or deduplicating either variant would orphan any on-chain state stored under that key.
- **Rule:** Never delete or alter existing duplicate variants. New features must never introduce new duplicates across enums.

---

## Contributor Decision Guide: Where Should a New Key Go?

When adding a new storage key in `contracts/ledgerlens-score/src/types.rs`, follow this decision rule:

```
[Need to add a new storage key]
               │
               ▼
 Is it part of a dedicated subsystem with its own enum? (e.g. GateDataKey)
       ├──► YES ──► Add to the subsystem enum (e.g. GateDataKey)
       │
       └──► NO (General/Extension Key)
               │
               ▼
       Check DataKeyD variant count (currently 39 / 50)
               │
               ├──► Count < 50 ──► Add new variant to DataKeyD
               │
               └──► Count >= 50 ──► Add new variant to DataKeyE (currently 1 / 50)
                                      (or create domain enum if introducing a major subsystem)
```

### Practical Rules Checklist for PRs:
1. **Prefer `DataKeyD` for immediate additions**: `DataKeyD` currently holds 39 variants and has capacity for ~11 more variants before reaching the 50-variant threshold.
2. **Use `DataKeyE` when `DataKeyD` hits 50**: Once `DataKeyD` reaches 50 variants, new general storage keys must be added to `DataKeyE`.
3. **Dedicated Subsystem Enums**: If introducing an entirely new, self-contained subsystem with >5 distinct keys (e.g., an independent ZK verification module or collateral escrow), create a dedicated `#[contracttype]` enum (e.g., `ZkDataKey`) rather than polluting general keys.
4. **Never Mutate Existing Variants**:
   - Do not rename or remove existing variants in `DataKey`, `DataKeyB`, `DataKeyC`, `DataKeyD`, `DataKeyE`, or `GateDataKey`.
   - Do not move variants between enums.
   - Do not reorder variant fields or change payload types.

---

## Compatibility Impact

Documentation and architectural guidance only. No on-chain storage keys, function signatures, error discriminants, or contract ABIs are modified by this record.

---

## References

- Implementation: [`contracts/ledgerlens-score/src/types.rs`](../../contracts/ledgerlens-score/src/types.rs)
- Storage Layout Reference: [`docs/storage-layout.md`](../storage-layout.md)
- Code Review Checklist: [`docs/review-checklists.md`](../review-checklists.md)
- Issue Reference: [#416](https://github.com/Ledger-Lenz/Ledgerlens-contract/issues/416), [#420](https://github.com/Ledger-Lenz/Ledgerlens-contract/issues/420)
