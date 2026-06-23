# Replay Harness

A deterministic replay tool for regression testing the LedgerLens contract with real Stellar mainnet trade history.

## Overview

The replay harness reads a Horizon API snapshot (NDJSON format) and feeds it through the off-chain detection pipeline simulation and contract submission functions. It validates:

- **No panics**: all contract calls complete without panicking
- **Score range**: all accepted scores remain in [0, 100]
- **Rate limits**: repeated submissions for the same (wallet, pair) are rate-limited as expected
- **Numerical stability**: no arithmetic overflow in aggregate score computations
- **Determinism**: identical input produces identical contract state and call sequences

## Snapshot Format

Each line is JSON:
```json
{"wallet":"wallet_id","asset_pair":"XLM_USDC","trades":[{"price":0.12},{"price":0.13}]}
```

- `wallet` (string): Horizon account ID or identifier
- `asset_pair` (string): asset pair symbol (e.g., "XLM_USDC")
- `trades` (array, optional): trade records with `price` field

## Building

```bash
cargo build -p replay
```

## Running

```bash
cargo run -p replay --manifest-path tools/replay/Cargo.toml
```

The binary expects `testdata/reference.ndjson` relative to the current working directory.

## Testing

Integration tests verify replay logic and contract call safety:

```bash
cargo test -p replay
```

## Sample Data

`testdata/reference.ndjson` contains 25 entries across 20 wallets and 5 asset pairs, exercising:
- Empty trade lists
- Single and multi-trade entries
- Repeated wallets across multiple pairs
- Realistic price ranges

## CI Integration

The workflow `.github/workflows/replay-regression.yml` runs the replay harness on every PR to detect regressions in score submissions or contract behavior.

