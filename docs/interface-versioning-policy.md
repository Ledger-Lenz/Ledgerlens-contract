# `ILedgerLensScore` — Interface Versioning & Migration Policy

**Status:** Adopted · **Effective:** v3.0.0+

## 1. Purpose

`ILedgerLensScore` is the canonical composability surface that external Soroban
protocols (AMMs, lending markets, DEX aggregators) integrate against. Every
breaking change to this surface requires integrators to update and re-deploy
their contracts — an expensive, error-prone process with no warning.

This document defines a predictable policy for numbering, announcing, and
migrating between interface versions so that integrators can plan ahead and
that no breaking change ships silently.

## 2. Version Numbering

The interface version is a single incrementing integer at the top of
[`docs/interface-spec.md`](interface-spec.md).

| Version | Contract `CONTRACT_VERSION` | Notable changes |
|---------|-----------------------------|-----------------|
| 1 | 1 | Initial release: `submit_score`, `get_score`, `query_risk_gate`. |
| 2 | 2 | Added `submit_scores_batch` (return type `BatchResult`), payload attestation, `supports_interface`. |
| 3 | 3 | Added Merkle-root batch attestation, consensus scoring, score floor, embargo, hysteresis, pair pausing, M-of-N admin signers, `query_risk_gate_with_confidence`. |

Interface version and `CONTRACT_VERSION` are incremented together; they always
match.

## 3. Breaking vs. Non-Breaking Changes

### 3.1 Breaking changes (require a version bump)

- Removing or renaming any function listed in §1 of `interface-spec.md`.
- Changing the return type of any listed function.
- Adding a required parameter to any listed function.
- Reordering, removing, or changing the type of any field in `RiskScore`,
  `AggregateRiskScore`, or any other `#[contracttype]` struct that integrators
  decode directly.
- Changing the numeric value of an error-code discriminant published in the
  interface spec.
- Removing a capability symbol from `supports_interface`.

### 3.2 Non-breaking changes (no bump required)

- Adding a new function (integrators who do not call it are unaffected).
- Adding an optional parameter to an existing function (existing call sites
  continue to compile).
- Adding a new field to a response struct **if** the client binding is
  generated from the latest interface definition (Soroban deserialisation
  tolerates trailing fields from newer versions — see §4).
- Adding new error variants with larger discriminant values.
- Adding a new capability symbol to `supports_interface`.
- Performance optimisations, bug fixes, and test additions that do not change
  the ABI.

### 3.3 What constitutes a "new field" vs. a changed struct

Because Soroban's XDR deserialisation is **append-only tolerant** — a client
generated against interface v2 can deserialize a struct produced by v3 as long
as the v2 fields are at their original offsets — adding fields to the **end**
of a struct is non-breaking. Reordering, removing, or inserting fields at any
position other than the end is breaking.

## 4. Announcement & Migration Timeline

```
  T-30 days (or earlier)           T-0 (deploy)
       │                               │
       │  CHANGELOG entry (UNRELEASED)  │
       │  + interface-spec.md updated   │
       │  + `supports_interface`        │
       │    allows feature-detect       │
       │                               │
       ▼                               ▼
  ┌──────────────────────┐    ┌──────────────────────┐
  │  Notice period        │    │  Breaking change      │
  │  (new version         │    │  ships on-chain       │
  │   announced but not   │    │                       │
  │   yet deployed)       │    │                       │
  └──────────────────────┘    └──────────────────────┘
```

1. **A breaking change is first announced as an `Unreleased` entry in
   `CHANGELOG.md`** with a clear "Migration Guide" subsection listing the exact
   code changes required. The `interface-spec.md` is also updated to reflect
   the new signatures.

2. **A minimum 30-day notice period** elapses between the `Unreleased`
   changelog entry and the deployment of the breaking change to `mainnet`.
   During this window:
   - Integrators can audit the diff, update their clients, and run their test
     suites against the `testnet` deployment.
   - The `testnet` deployment is updated first — integrators are expected to
     validate against it.

3. **On deploy:** the `CHANGELOG.md` entry moves from `Unreleased` to a dated
   release section. `supports_interface` on the new deployment returns `true`
   for the new capabilities and continues answering `true` for all prior
   capabilities.

## 5. Programmatic Detection

Integrators are expected to use `supports_interface` rather than comparing
version integers:

```rust
// ✅ Recommended: feature-detect at runtime
let client = LedgerLensScoreContractClient::new(&env, &contract_id);
if client.supports_interface(&symbol_short!("gate")) {
    // This deployment supports query_risk_gate.
}
```

```rust
// ⚠️ Avoid: version-number comparison
let ver = client.get_version();
if ver >= 2 { /* ... */ }  // Fragile: what if v4 removes nothing you use?
```

`supports_interface` is an append-only capability registry within a major
interface version. Once a capability symbol is published, it is never removed
or repurposed without a breaking release and the 30-day notice period above.
The current capability table is in [`docs/interface-spec.md §1.3`](interface-spec.md#13-supports_interface--capability-detection).

## 6. Old Version Support

Once a new interface version is deployed to `mainnet`:

- The previous version's functions remain callable **for the lifetime of the
  deployed contract instance** — Soroban contracts are immutable after
  deployment, so a v2 contract continues answering v2 calls forever.
- Integrators must **opt in** to a new version by pointing their client at the
  new contract ID. There is no "coexistence window" on a single contract
  instance — it serves one interface version for its entire life.
- For contract **upgrades** (WASM replacement): the new WASM may change the
  interface. The 30-day notice period still applies, and `supports_interface`
  on the upgraded contract reflects the new version's capabilities. Old
  capability symbols that were removed are not honoured by the new WASM.

## 7. Cross-References

- Interface function signatures and capability table:
  [`docs/interface-spec.md`](interface-spec.md)
- Upgrade governance (time-lock, veto):
  [`README.md § Upgrade Governance`](../README.md#upgrade-governance)
- Migration guides for past breaking changes:
  [`CHANGELOG.md`](../CHANGELOG.md)
- Contributing guidelines (what counts as `types.rs`-breaking):
  [`CONTRIBUTING.md`](../CONTRIBUTING.md)

## 8. Deprecation Policy

How old functions, return shapes, error codes, and capability symbols are
supported, warned about, and eventually removed is specified separately in
[`docs/deprecation-policy.md`](deprecation-policy.md). The key points:

- Deprecated public symbols remain callable for at least **2 full major
  interface versions** before removal.
- Removal follows the same **30-day notice period** as any other breaking
  change (§4 above).
- Every deprecated symbol must have at least one test in
  `src/test_deprecation_compat.rs` pinning its current behaviour.
- `supports_interface("old_cap")` returns `false` on a deployment that has
  removed the cap, and `true` on deployments still in the deprecation window.
- The sunset checklist in `docs/deprecation-policy.md §6` must be completed
  before any removal lands on mainnet.
