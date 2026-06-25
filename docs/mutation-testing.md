# Mutation Testing Strategy for LedgerLens Contract

## Overview

Mutation testing introduces deliberate logic errors into the source code and measures whether the existing test suite catches them. A high "kill rate" (percentage of caught mutations) indicates robust test coverage for critical logic.

For the LedgerLens score registry, mutation testing focuses on score-modification functions and validation logic, where logic bugs could lead to incorrect risk assessments or bypassed security controls.

## Target Functions (>= 95% Kill Rate)

These functions have the highest impact on contract security and correctness:

### 1. **submit_score** (lib.rs:~300-860)
- **What it tests:** Score submission, velocity cap enforcement, cooldown checks, override logic
- **Critical mutations to kill:**
  - Velocity cap comparison operators (`<` → `<=`, `>` → `>=`)
  - Cooldown arithmetic (`saturating_add` → `saturating_sub`)
  - Override flag checks (removed or inverted `is_velocity_cap_overridden`)
  - Off-by-one in time calculations

### 2. **apply_velocity_cap** (lib.rs, if extracted)
- **What it tests:** Velocity cap arithmetic and time-based allowance
- **Critical mutations to kill:**
  - Comparison operators in cap checks
  - Arithmetic operators in delta calculations (`-` → `+`, `*` → `/`)
  - Zero/boundary conditions

### 3. **check_cooldown** (lib.rs:~338-340, ~820-821, ~1170-1171)
- **What it tests:** Cooldown elapsed time validation
- **Critical mutations to kill:**
  - Comparison operators in timestamp checks (`<` ↔ `<=`, `>` ↔ `>=`)
  - `now < last_submit + cooldown` becoming `now > last_submit + cooldown`
  - Removal of `last_submit != 0` guard

### 4. **Score Floor Enforcement** (test_score_floor.rs)
- **What it tests:** Minimum floor based on historical max
- **Critical mutations to kill:**
  - Comparison operators for floor checks
  - High-water-mark update logic

### 5. **Authorization Checks** (all `require_auth` calls)
- **What it tests:** Admin multisig, signer validation
- **Critical mutations to kill:**
  - Removed or inverted `require_auth` calls
  - Corrupted signer set checks
  - Threshold comparisons (`<` → `<=`)

## Running Mutation Tests

### Prerequisites
```bash
cargo install cargo-mutants
```

### Full Baseline (1500+ mutants, ~2-4 hours)
```bash
cd contracts/ledgerlens-score
cargo mutants --jobs 4
```

### Targeted Testing by Function
```bash
# Test only velocity cap logic
cargo mutants --jobs 4 --file "src/lib.rs" --pattern "velocity|cooldown"

# Test only a specific file
cargo mutants --jobs 4 --file "src/lib.rs"

# Limit to 100 mutants for quick validation
cargo mutants --jobs 4 --file "src/test_velocity_cap.rs" | head -200
```

### Generating Reports
```bash
# Save full output to file
cargo mutants --jobs 4 2>&1 | tee mutation-report.txt

# Filter for surviving mutants only
cargo mutants --jobs 4 2>&1 | grep "SURVIVED"

# Count results
cargo mutants --jobs 4 2>&1 | grep -c "KILLED"
cargo mutants --jobs 4 2>&1 | grep -c "SURVIVED"
```

## Interpreting Results

Each mutant is reported with one of three statuses:

- **KILLED** ✓ : Test suite caught the mutation (desired)
- **SURVIVED** ✗ : No test caught the mutation (indicates gap)
- **UNVIABLE** ⚠ : Mutation caused compile error (usually OK)

Example output:
```
KILLED   src/lib.rs:340: < → > in cooldown check
SURVIVED src/lib.rs:831: < → <= in velocity cap check  ← Test gap detected!
UNVIABLE src/lib.rs:500: (mutation caused compile error)
```

## Addressing Surviving Mutants

When cargo-mutants reports a surviving mutant:

1. **Identify the mutation location and type** — The report shows the line number and operator changed
2. **Add a targeted test** — Write a test that would fail under that specific mutation
3. **Place the test** — Add to `src/test_velocity_cap.rs` for cap/cooldown logic, `src/test.rs` for general logic
4. **Verify the kill** — Re-run cargo-mutants to confirm the test kills that mutant

Example: A surviving mutant at `lib.rs:340: now < last_submit + cooldown` becomes `now <= last_submit + cooldown`:

```rust
#[test]
fn test_cooldown_boundary_exact_elapsed_time() {
    // Set up: submit at time 100, cooldown = 60 seconds
    let (env, client, admin, _service) = initialized();
    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    client.submit_score(&Vec::new(&env), &wallet, &asset_pair, &50, ...);
    
    // Attempt resubmission at exactly now = 160 (100 + 60)
    // A mutation `<` → `<=` would reject this; test ensures it's accepted
    env.ledger().with_mut(|l| l.timestamp = 160);
    
    client.submit_score(&Vec::new(&env), &wallet, &asset_pair, &55, ...);
    assert_eq!(client.get_score(&wallet, &asset_pair).score, 55);
}
```

## Known Limitations

### 1. WASM Target Incompatibility
Soroban contracts compile to `wasm32-unknown-unknown`, but cargo-mutants works with the native `rlib` target. This means:
- Some WASM-specific code paths are not mutated
- `env` API calls may not mutate as expected
- Test this contract's native lib target, not the WASM binary

### 2. Long Runtime
Full mutation runs on 1500+ mutants take 2-4 hours. For faster feedback:
- Test a single file with `--file "src/test_velocity_cap.rs"`
- Limit to core functions with `--pattern` matching
- Run during off-hours in CI

### 3. Mutation Scope
Some mutations are not meaningful for this codebase:
- Changes to panic messages
- Changes to event data fields (no runtime effect)
- Reordering of independent statements

## CI Integration (TODO)

Mutation testing is not yet required in CI. To add it:

1. Create a scheduled CI job (e.g., nightly) that runs:
   ```bash
   cargo mutants --jobs 4 --file "src/lib.rs" > mutants.out 2>&1
   ```

2. Archive `mutants.out` as a build artifact for review

3. Fail if kill rate < 95% for target functions:
   ```bash
   KILLED=$(grep -c "KILLED" mutants.out)
   SURVIVED=$(grep -c "SURVIVED" mutants.out)
   RATE=$(( KILLED * 100 / (KILLED + SURVIVED) ))
   if [ $RATE -lt 95 ]; then exit 1; fi
   ```

## Best Practices

1. **Run before major refactors** — Mutation tests verify that refactored logic is still correct
2. **Focus on arithmetic and comparisons** — These are the most common sources of logic bugs
3. **Test boundary conditions** — Off-by-one errors are common mutation survivors
4. **Document surviving mutants** — If a mutation is intentionally unkillable, document why
5. **Use proptest alongside mutation testing** — Property tests verify invariants; mutation tests verify test strength

## References

- [cargo-mutants documentation](https://mutants.rs/)
- [Mutation Testing Wikipedia](https://en.wikipedia.org/wiki/Mutation_testing)
- [LedgerLens CONTRIBUTING.md](../CONTRIBUTING.md) — Mutation Testing section
