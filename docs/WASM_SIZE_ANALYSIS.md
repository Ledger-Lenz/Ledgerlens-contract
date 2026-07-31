# WASM Size Analysis and Attribution

## Overview

WASM size analysis identifies performance regressions and tracks binary growth by contract module and feature area. This documentation describes the size tracking infrastructure, how to interpret results, and how CI uses these measurements.

## Size Categories

Each module or feature is classified into one of these categories:

- **Critical**: Essential contract functionality (must ship)
- **Core**: Primary business logic
- **Feature**: Optional or secondary features
- **Test**: Test utilities and debugging code
- **Other**: Miscellaneous code

## Attribution by Module

The `WasmBinaryAnalysis` structure breaks down binary size by:

1. **Module attribution**: Size contribution from each contract module
   - `ledgerlens_score`: Core scoring engine
   - `ledgerlens_aggregator`: Shard aggregation logic
   - Each module size is tracked as a percentage of total

2. **Feature attribution**: Size contribution from feature flags
   - `testutils`: Testing infrastructure
   - `oracle_staleness`: Oracle freshness checking
   - `rate_limiting`: Rate limit enforcement

## Regression Detection

The system detects size regressions by comparing previous and current binaries:

### Severity Levels

- **Negligible**: < 1% increase
- **Minor**: 1-5% increase
- **Moderate**: 5-10% increase
- **Severe**: > 10% increase

### Review Requirements

CI automatically flags binaries for review when:
- Total binary size changes > 2%
- Any module has a **Severe** regression
- Multiple **Moderate** regressions exist

## Interpreting CI Reports

Example CI output:

```
WASM Size Analysis
==================
Total: 500KB → 520KB (+4.0%)
Status: REVIEW_REQUIRED (increased 4.0%)

Regressions:
  score module:        250KB → 260KB (+4.0%) [MODERATE]
  
Improvements:
  validator module:     40KB → 38KB (-5.0%)
```

## Performance Benchmarks

Typical binary sizes (ledgerlens-score):
- **Unoptimized**: ~800KB
- **Optimized (release)**: ~280KB
- **Post-compression (gzip)**: ~90KB

Expected compression ratio: ~68% (gzip)

## Using the Analysis API

### Creating an Analysis

```rust
use replay::wasm_analysis::{WasmBinaryAnalysis, SizeCategory};

let mut analysis = WasmBinaryAnalysis::new(280_000); // 280KB binary
analysis.add_module("score", 150_000, SizeCategory::Core);
analysis.add_module("validator", 50_000, SizeCategory::Feature);
analysis.add_feature("oracle_staleness", 30_000, SizeCategory::Feature);
```

### Detecting Regressions

```rust
use replay::wasm_analysis::compare_binaries;

let comparison = compare_binaries(previous_analysis, current_analysis);
if comparison.requires_review {
    println!("Changes require review:");
    for regression in &comparison.regressions {
        println!("  {}: +{} bytes", regression.name, regression.increase_bytes);
    }
}
```

### Sorting by Impact

```rust
// Largest modules first
let modules = analysis.modules_by_size();
for module in modules.iter().take(5) {
    println!("{}: {:.1}%", module.name, module.percentage);
}
```

## Resource Constraints

WASM size analysis is bounded by:
- Binary size: No inherent limit; practical maximum ~1MB
- Module count: No limit (scales linearly)
- Comparison time: O(n) where n = module count
- Memory usage: O(n) for storage

## Compatibility

### Storage Changes

None. WASM analysis is a build-time and CI-time utility; no on-chain storage is affected.

### API Changes

The analysis API may add new size categories or severity levels in future releases.

## Future Enhancements

Potential future work:
- Per-function size attribution (requires debug info)
- Instruction-level cost estimates
- Historical trend analysis
- Automated optimization suggestions
- Differential binary analysis
