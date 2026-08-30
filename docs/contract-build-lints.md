# Contract-only build linting

This repository has two different compilation surfaces:

- native host builds used for unit tests and local tooling
- `wasm32-unknown-unknown` contract builds that are actually deployable

Issue #808 is about drift between those surfaces: code can look covered in native
tests while still being unreachable or dead in the deployable WASM build.

## What CI checks now

`tools/check_contract_build_lints.sh` runs warning-denied WASM-target checks:

- `RUSTFLAGS=-Dwarnings cargo check -p ledgerlens-score --lib --target wasm32-unknown-unknown --release`
- `RUSTFLAGS=-Dwarnings cargo check -p ledgerlens-aggregator --lib --target wasm32-unknown-unknown --release`

This keeps the check scoped to contract crates and turns Rust warnings,
including `dead_code`, into hard failures for deployable builds.
Using `cargo check` is intentional: the aggregator imports the score contract's
generated client and shared types, whose dependency RLIB also contains the
score contract's WASM exports. Linking both contracts into one artifact would
create duplicate exports; each deployable contract is linked by its own build
job.

## Intentional host-only exceptions

The following code is intentionally native-only and is therefore excluded from
the contract-only lint check instead of being treated as a deployable dead-code
failure:

- `#[cfg(test)]` modules under `contracts/ledgerlens-score/src/`
- the native-only `event_causality` audit/replay helper, which uses host heap
  collections and is not invoked by contract entry points
- compatibility catalogs in `constants`, `events`, `governance_actions`,
  `storage`, `types`, and `zk_range_proof`; these retain legacy identifiers,
  storage accessors, proof-generation helpers, and test-visible types even
  when a particular build has no live caller
- private numerical/accumulator helpers retained for compatibility tests and
  planned feature paths; each is annotated individually in `lib.rs`
- `contracts/ledgerlens-aggregator/src/test.rs`
- doctest and shell-test harness code used only to verify host tooling

Those paths are still expected to stay purposeful. They are not part of the
WASM artifact, so CI documents them as exceptions rather than forcing them
through the deployable dead-code gate.

## Compatibility impact

- Public ABI: unchanged
- Events: unchanged
- Errors: unchanged
- Storage layout: unchanged

## Resource bounds

The check builds exactly two contract libraries once each for the wasm target.
It adds compile time but no runtime cost and no on-chain resource impact.
