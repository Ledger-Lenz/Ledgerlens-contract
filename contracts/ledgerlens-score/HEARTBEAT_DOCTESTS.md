# Heartbeat monitor rustdoc examples

This change adds runnable `/// examples` (doctests) to four public
functions in `contracts/ledgerlens-score/src/lib.rs` that previously
had doc comments but no copy-pasteable example, following the existing
pattern used throughout the file (e.g. `get_score_trend`,
`get_score_percentile`, `query_risk_gate_relative`).

## Functions documented

- **`is_service_alive`** — shows that a freshly initialized contract
  reports alive before any submission, stays alive right after a
  submission, and flips to not-alive once the default 1-hour heartbeat
  alert threshold (3,600s) elapses with no further activity.
- **`set_heartbeat_alert_threshold`** — shows the admin-only setter
  changing the threshold from its default of 3,600s to 7,200s.
- **`get_heartbeat_alert_threshold`** — shows the default value
  (3,600s) returned on an untouched contract.
- **`ping_heartbeat`** — shows the service account recording a
  liveness heartbeat and `get_last_service_activity` reflecting the
  new ledger timestamp.

## Notes

- All examples build and run as standard doctests (`cargo test --doc`
  / `cargo test`) — no `ignore`/`no_run` attributes were needed.
- No function signatures, behavior, or existing doc text were changed;
  this is additive documentation only.
