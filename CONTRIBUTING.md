# Contributing to LedgerLens Contract

Thanks for your interest in improving the LedgerLens on-chain risk score registry.

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

- Keep `contracts/ledgerlens-score/src/types.rs` changes minimal and deliberate — `RiskScore` and `DataKey` are shared, cross-repo data contracts (see [README.md § Organization Architecture](README.md#organization-architecture)). Any field/shape change is breaking for the `api`, `core`, and `dashboard` repos and must be coordinated.
- Add or update tests in `src/test.rs` for any behavioral change.
- Keep error codes in `errors.rs` stable; append new variants rather than reordering or removing existing ones, since their numeric values are part of the deployed contract's ABI.
- Update `README.md` if you change contract function signatures, events, or the deployment flow in `deploy.sh`.

## Testing

### Running Tests

Unit and property-based tests can be run with:
```bash
cargo test -p ledgerlens-score
```

### Property-Based Tests with proptest

The contract includes property-based tests for score velocity cap invariants. These are found in `src/test_velocity_cap_prop.rs`.

Run the property-based tests alone:
```bash
cargo test -p ledgerlens-score test_velocity_cap_prop
```

By default, proptest runs 256 test cases per property. To run extended testing locally:
```bash
PROPTEST_CASES=10000 cargo test -p ledgerlens-score test_velocity_cap_prop
```

In CI, `PROPTEST_CASES` is set to `10000` for comprehensive coverage. Each proptest iteration creates a fresh Soroban `Env` to ensure no state bleed between test cases.

## Submitting a Pull Request

- Describe what changed and why.
- Note any cross-repo coordination needed (e.g. "requires `api` to update its `RiskScore` schema").
- Ensure all CI checks pass.
