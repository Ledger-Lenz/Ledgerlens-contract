# Configuration drift detection

LedgerLens now defines a stable operator-facing configuration manifest for
deployed `ledgerlens-score` instances and ships a deterministic drift checker
through the `replay` tool.

## Stable manifest fields

- `contract_version`
- `paused`
- `risk_threshold`
- `jump_threshold`
- `staleness_window`
- `upgrade_delay`
- `cooldown`
- `service_threshold`
- `admin_threshold`
- `consensus_threshold_k`
- `consensus_epsilon`
- `reveal_window`
- `finality_buffer`
- `heartbeat_alert_threshold`
- `oracle_staleness_threshold`

## Workflow

1. Capture the approved manifest in JSON.
2. Query the live deployment and materialize the same JSON object.
3. Run:

```bash
cargo run -p replay --manifest-path tools/replay/Cargo.toml -- \
  config-drift approved.json observed.json
```

4. Treat any `drift`, `missing_observed_field`, `unexpected_observed_field`,
   `unknown_approved_field`, or `unknown_observed_field` entry as an operator
   review item.

## Compatibility notes

- No on-chain storage layout changed.
- No contract ABI or event changed.
- The drift checker is off-chain only and reads JSON snapshots.
- The supported manifest field list is exported from the Rust crate so the
  tool and contract documentation stay aligned.
