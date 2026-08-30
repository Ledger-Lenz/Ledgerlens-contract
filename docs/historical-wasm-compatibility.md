# Historical WASM compatibility

This suite freezes a production-shaped `ledgerlens-score` release WASM and
executes it against the consumers compiled from the current workspace. It is a
regression gate for the stable `ILedgerLensScore` surface, not a replacement
for upgrade or migration testing.

## Fixture and trust assumptions

The fixture at
`tests/fixtures/historical/ledgerlens-score-v3-8336828.wasm` is the locked
Rust 1.85 release build of commit
`8336828159b7e7ff05d018200ce7f7a385bdade5`. That revision is the final v3
main-line contract before the v4 storage and ABI changes and already contains
the AMM and lending consumers under test. The adjacent fixture-specific
`Cargo.lock` removes dependency-index drift from reproduction.

The historical source used `u8::is_multiple_of` before its Rust 1.87
stabilization. The pinned build therefore injects only the compiler feature
attribute with `RUSTC_BOOTSTRAP=1`; it does not patch source. Rust 1.85 is
used with Binaryen 131 to canonicalize and optimize the output while explicitly
lowering reference types, multivalue, and bulk memory for the Soroban SDK 21
host. The exact compiler and optimizer commands are pinned in the manifest.
The reproducibility script creates a detached worktree at the pinned commit,
copies the fixture-specific lockfile from the current checkout, runs that exact
build, checks both SHA-256 digests, compares the bytes, and removes the
worktree:

```sh
scripts/reproduce-historical-wasm.sh
```

It requires Rust 1.85 with `wasm32-unknown-unknown` and Binaryen 131. It does
not edit the caller's branch or its `Cargo.lock`.

Its adjacent manifest pins:

- the full source commit and contract version;
- Rust and Soroban SDK versions;
- the exact build command, SHA-256 digest, and byte size;
- maximum accepted fixture and contract-spec sizes;
- the consumers and stable capabilities covered by the suite.

The binary is repository data and is never fetched during tests. Tests fail
closed on an empty, oversized, malformed, hash-mismatched, or ABI-mismatched
fixture.

## Compatibility contract

The reviewed ABI golden covers the entry points that current consumers call:

- `query_risk_gate`
- `query_risk_gate_with_confidence`
- `supports_interface`
- `initialize` and `get_version` for deployment diagnostics
- the historical and current `RiskScore` representations

Compatibility is tested in both directions:

1. Current `mock-amm` and `mock-lending` contracts call the historical WASM.
2. A client generated from the historical WASM calls the current native
   contract.

The suite intentionally changes no public type, event, error discriminant, or
storage key. A golden diff must be reviewed as an ABI change; updating the
fixture merely to make CI green is prohibited.

The proposed representation is therefore the current representation with no
ABI change. The v3 golden has six `RiskScore` fields; the current golden has
four additional fields (`benford_score`, `ml_score`, `network_score`, and
`commitment`). Gate consumers remain compatible because they exchange only a
boolean. A consumer that directly decodes the historical six-field
`RiskScore` must keep its v3 binding or migrate explicitly; that boundary is
recorded instead of being treated as silently compatible.

## Authorization and state transitions

Fixture deployment uses a fresh test environment and mock authorization only
for setup. Consumer gate calls remain public read paths and preserve fail-closed
behavior:

- missing score: rejected;
- score at or above the threshold: rejected;
- confidence below the consumer floor: rejected;
- valid low-risk, sufficiently confident score: accepted.

The active-service gate path is read-only: the resource test proves that
persistent state, temporary state, and events do not change. The historical
contract's existing `check_service_silence` behavior is intentionally
preserved: if a service crosses its configured silence threshold, the first
gate call may set the bounded silence-alert flag and emit its diagnostic event.
This suite does not add any other read-path write.

## Size and resource boundaries

Both the WASM envelope and encoded `contractspecv0` section have explicit
zero, one, maximum, and maximum-plus-one tests.

| Resource | Observed baseline | Enforced bound |
|---|---:|---:|
| Fixture WASM | 213,850 bytes | 524,288 bytes |
| Encoded contract spec | 112,028 bytes | 262,144 bytes |
| Direct-score AMM gate CPU | 13,242,050 instructions | 25,000,000 instructions |
| Delegated AMM gate CPU | 13,362,830 instructions | 25,000,000 instructions |
| Silence-transition AMM gate CPU | 13,342,786 instructions | 25,000,000 instructions |
| Worst measured AMM gate memory | 4,056,627 bytes | 4,500,000 bytes |
| Logical storage lookups | 10 direct / 12 delegated | 12 |
| Active/delegated persistent writes | 0 | 0 |
| Temporary ledger writes | 0 | 0 |
| Silence transition instance writes | 1 one-shot flag | 1 |
| Silence transition events | 1 / 144 payload bytes | 1 / 1,024 payload bytes |

The stable gate path has bounded work and no input collection. In the
worst supported direct-score case it performs three consumer configuration
lookups and seven fixed contract configuration/status/score lookups. The
one-level historical delegation fallback adds at most two more lookups; there
is no data-dependent loop. Inputs are one address, one bounded Soroban symbol,
one `u32` threshold, and one `i128` amount (the resource test uses
`i128::MAX`), with no input collection.

The suite measures the direct-score, delegated fallback, and first
service-silence transition separately. The silence event boundary counts the
XDR-encoded topics plus data; a repeated call in the same silence window is
asserted to produce neither another write nor another event.

Run the evidence locally:

```sh
cargo test --locked -p composability-tests --test historical_wasm_compat -- --nocapture
```

## Failure diagnosis and recovery

- **Hash or size mismatch:** the binary changed. Rebuild the pinned commit with
  the pinned toolchain, compare hashes, and do not update the manifest until
  the provenance difference is understood.
- **Golden ABI mismatch:** inspect the spec diff. Restore the stable signature,
  or follow `docs/interface-versioning-policy.md` before accepting a breaking
  representation.
- **Consumer behavior mismatch:** treat it as a release blocker. Check
  fail-closed threshold/confidence semantics and cross-contract invocation
  errors.
- **Resource regression:** inspect newly added storage reads, events, or
  data-dependent work before changing a ceiling.

Rollback does not mutate the historical fixture. Rebuild and redeploy the
pinned source commit, verify its manifest digest, then route consumers back to
the previous contract ID. Because the compatibility suite performs no
migration, no fixture-driven downgrade writes are required.
