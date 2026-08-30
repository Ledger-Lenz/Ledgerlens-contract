# Contributing to LedgerLens Contract

Thanks for your interest in improving the LedgerLens on-chain risk score registry.

## Opening an Issue

Pick the template that matches what you're actually doing — each one asks for the specific
evidence that class of task needs, not a generic description box:

| Task class | Template | What it's for |
|---|---|---|
| Implementation | [`🔧 Implementation task`](.github/ISSUE_TEMPLATE/implementation-task.yml) | Adding or changing contract behavior. |
| Testing | [`🧪 Testing task`](.github/ISSUE_TEMPLATE/testing-task.yml) | Closing a coverage gap without changing behavior. |
| Documentation | [`📚 Documentation task`](.github/ISSUE_TEMPLATE/documentation-task.yml) | README/CONTRIBUTING/docs additions or corrections. |
| Security review | [`🔒 Security review task`](.github/ISSUE_TEMPLATE/security-review-task.yml) | Auditing a specific property across a defined set of code paths. |
| Benchmark | [`📊 Benchmark task`](.github/ISSUE_TEMPLATE/benchmark-task.yml) | Measuring and recording resource cost, especially at a worst-case bound. |

Every template requires an explicit **Compatibility impact** statement (even "None") and links
back to [`docs/invariants.md`](docs/invariants.md) — read that first regardless of which template
you use.

Touching governance, cryptography, storage, upgrades, or composability? Work through
[`docs/review-checklists.md`](docs/review-checklists.md) — short, actionable per-category gates
reviewers will hold your PR to.

## Getting Started

1. Install the Rust toolchain (stable) and the `wasm32-unknown-unknown` target:
   ```bash
   rustup target add wasm32-unknown-unknown
   ```
2. Fork the repo and create a feature branch off `main`.
3. Make your changes inside `contracts/ledgerlens-score/`.

## Before Opening a Pull Request

Run the same checks CI runs:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --target wasm32-unknown-unknown --release
```

## Guidelines

- **Read [`docs/invariants.md`](docs/invariants.md) before touching `lib.rs`.** It lists the
  behaviors that are non-negotiable — fail-closed gates, no-panic reads, bounded storage, and
  append-only event/error stability — with pointers to exactly what enforces each one. If your
  change would weaken any of them, that needs a design discussion before a PR, not after.
- Keep `contracts/ledgerlens-score/src/types.rs` changes minimal and deliberate — `RiskScore` and `DataKey` are shared, cross-repo data contracts (see [README.md § Organization Architecture](README.md#organization-architecture)). Any field/shape change is breaking for the `api`, `core`, and `dashboard` repos and must be coordinated.
- Add or update tests in `src/test.rs` for any behavioral change.
- For replay or forensic workflow changes, add or update tests in `tools/replay/src/main.rs` and keep the evidence bundle deterministic and stable across reruns.
- Keep error codes in `errors.rs` stable; append new variants rather than reordering or removing existing ones, since their numeric values are part of the deployed contract's ABI. This is enforced in CI by the `error-discriminants` job (`tools/check_error_discriminants.sh`), which fails the build if a PR renames, removes, or renumbers any discriminant that already existed on the base branch. New discriminants and new `pub const` aliases are always fine — prefer an alias over renumbering when you need a new name for an existing error.
- Update `README.md` if you change contract function signatures, events, or the deployment flow in `deploy.sh`.
- Use terms as defined in [`docs/glossary.md`](docs/glossary.md) consistently — e.g. don't call a `ledgerlens-aggregator` peer a "node" or a "partition" when the established term is **shard**; don't use "finality" to mean ledger-close finality when this repo's docs mean the finality *buffer* (a score-submission hold window). If you introduce a genuinely new concept, add it to the glossary in the same PR rather than letting a new term go undefined.
- **Interface-breaking changes** (see [`docs/interface-versioning-policy.md`](docs/interface-versioning-policy.md) for what counts as breaking) require a minimum 30-day notice period between the `Unreleased` changelog entry and mainnet deployment. The announcement must include a migration guide in `CHANGELOG.md`.

## Submitting a Pull Request

- Describe what changed and why.
- Note any cross-repo coordination needed (e.g. "requires `api` to update its `RiskScore` schema").
- Ensure all CI checks pass.
- **If your change touches governance, cryptography, storage, upgrades, or composability**, work
  through the matching checklist in [`docs/review-checklists.md`](docs/review-checklists.md)
  before requesting review — reviewers will be checking against it, so a PR description that
  addresses each item up front gets reviewed faster than one that doesn't.
