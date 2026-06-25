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

### Mutation Testing with cargo-mutants

Mutation testing introduces deliberate logic errors ("mutants") into the code and checks whether the test suite catches them. A high kill rate (>= 95%) indicates that tests would catch real logic bugs like wrong operators, off-by-one errors, and inverted comparisons.

**Install cargo-mutants:**
```bash
cargo install cargo-mutants
```

**Run mutation testing on core functions:**

cargo-mutants works with the native Rust library target, not the WASM build. Run it from the contract directory:

```bash
cd contracts/ledgerlens-score

# Test the entire lib with default 256 cases per mutant
cargo mutants --jobs 4

# Test a specific file (e.g., core scoring functions)
cargo mutants --jobs 4 --file "src/lib.rs"

# Generate a report with surviving mutants
cargo mutants --jobs 4 2>&1 | tee mutants-report.txt
```

**Interpreting results:**

- `KILLED`: The test suite caught this mutant (good — the test kills the mutation)
- `SURVIVED`: A mutant was introduced but no test caught it (bad — indicates under-tested logic)
- `UNVIABLE`: The mutant caused a compile error (usually OK — means the mutation is syntactically invalid)

**Critical functions to achieve >= 95% kill rate:**
- `submit_score()` — velocity cap and cooldown validation
- `get_effective_score()` — decay and confidence floor logic
- `apply_velocity_cap()` — comparison operators and arithmetic
- `check_cooldown()` — timestamp comparisons
- Score floor enforcement logic

**When a mutant survives:**
1. Understand what the mutant changed (the report shows the diff)
2. Write a test that would kill that mutant
3. Add the test to `src/test_velocity_cap.rs`, `src/test.rs`, or a new test module
4. Verify the new test kills the mutant: `cargo mutants --jobs 4 | grep "SURVIVED"`

**Known limitations:**
- Soroban contracts use `wasm32-unknown-unknown` build target; cargo-mutants works with the native `rlib` target
- Full mutation run on 1000+ mutants takes 2-4 hours; test against specific files or functions during development
- Some Soroban-specific code (e.g., `require_auth`, `env` calls) may not mutate as expected

**CI integration:**
Mutation testing is not yet in the CI pipeline (see TODO in `.github/workflows/ci.yml`). Run manually before submitting PRs that modify core scoring logic. Target a minimum 95% kill rate in score-modification and validation functions.

## Submitting a Pull Request

- Describe what changed and why.
- Note any cross-repo coordination needed (e.g. "requires `api` to update its `RiskScore` schema").
- Ensure all CI checks pass.
