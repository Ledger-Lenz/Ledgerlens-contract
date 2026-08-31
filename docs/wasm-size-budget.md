# WASM Size Budget and Analysis Guide

This document establishes the baseline WebAssembly (WASM) binary size breakdown for the `ledgerlens-score` contract and documents local reproduction steps for tracking size regression.

## Baseline Overview

- **Target Contract**: `ledgerlens-score`
- **WASM Binary Path**: `target/wasm32-unknown-unknown/release/ledgerlens_score.wasm`
- **Total Binary Size**: 442,107 bytes (~442 KB)

---

## Top 10 Shallow Size Contributors (`twiggy top`)

The shallow size measures the raw byte size directly attributable to an item in the WASM binary.

| Rank | Item | Shallow Size (Bytes) | Shallow % | Description |
|------|------|----------------------|-----------|-------------|
| 1 | `custom section 'contractspecv0'` | 178,232 | 40.31% | Soroban contract XDR specification metadata section |
| 2 | `data[0]` | 15,421 | 3.49% | Static data segment (string literals, error message tables) |
| 3 | `code[952]` | 8,743 | 1.98% | Compiled function code block |
| 4 | `code[51]` | 4,954 | 1.12% | Compiled function code block |
| 5 | `code[53]` | 4,795 | 1.08% | Compiled function code block |
| 6 | `code[46]` | 4,576 | 1.04% | Compiled function code block |
| 7 | `code[319]` | 4,311 | 0.98% | Compiled function code block |
| 8 | `code[993]` | 3,559 | 0.81% | Compiled function code block |
| 9 | `code[441]` | 2,994 | 0.68% | Compiled function code block |
| 10 | `code[683]` | 2,180 | 0.49% | Compiled function code block |

---

## Top 10 Retained Size / Dominator Tree (`twiggy dominators`)

The retained size measures the size of an item plus all items in the call graph that are kept alive exclusively by it.

| Rank | Item / Subtree Node | Retained Size (Bytes) | Retained % |
|------|---------------------|-----------------------|------------|
| 1 | `export "verify_score_range_proof"` | 17,717 | 4.01% |
| 2 | `⤷ code[1441]` | 17,689 | 4.00% |
| 3 | `⤷ code[1074]` | 17,669 | 4.00% |
| 4 | `⤷ code[952]` | 17,395 | 3.93% |
| 5 | `⤷ code[323]` | 1,133 | 0.26% |
| 6 | `⤷ code[326]` | 809 | 0.18% |
| 7 | `⤷ code[346]` | 601 | 0.14% |
| 8 | `⤷ code[343]` | 531 | 0.12% |
| 9 | `⤷ code[328]` | 511 | 0.12% |
| 10 | `⤷ code[342]` | 97 | 0.02% |

---

## Local Reproduction Steps

### Prerequisites

Install `twiggy` using Cargo:

```bash
cargo install twiggy
```yaml

### Running the Analysis Script

Run the automated reporting script:

```bash
# Generate report for top 10 items to stdout:
./scripts/wasm-size-report.sh --top 10

# Generate report for top 20 items to a specific markdown file:
./scripts/wasm-size-report.sh --top 20 --output docs/wasm-size-report-latest.md

### Manual Analysis Commands

You can also run `twiggy` directly against the release binary:

```bash
# 1. Build the release WASM target
cargo build --target wasm32-unknown-unknown --release -p ledgerlens-score

# 2. View top shallow size items
twiggy top -n 10 target/wasm32-unknown-unknown/release/ledgerlens_score.wasm

# 3. View top retained size / dominator tree
twiggy dominators -r 10 target/wasm32-unknown-unknown/release/ledgerlens_score.wasm
```yaml

---

## CI Integration

The WASM size report is automatically executed in GitHub Actions (`.github/workflows/ci.yml`) on pull requests and main branch builds. The resulting analysis document is uploaded as a build artifact named `ledgerlens-score-wasm-size-report`.
