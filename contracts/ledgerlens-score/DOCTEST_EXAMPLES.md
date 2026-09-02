# Service pubkey rustdoc examples

Adds runnable `# Examples` blocks to the four service-pubkey admin/query
functions in `contracts/ledgerlens-score/src/lib.rs`, following the existing
pattern used by `get_score_trend`, `get_score_percentile`, and
`query_risk_gate_relative` (test `Env`, `mock_all_auths`, register the
contract, call through `LedgerLensScoreContractClient`).

- `set_service_pubkey`: sets a 33-byte compressed pubkey and confirms it via
  `get_service_pubkey`.
- `get_service_pubkey`: shows the `Error::ServicePubkeyNotSet` case before any
  key is configured, then the success case after `set_service_pubkey`.
- `rotate_service_pubkey`: rotates instantly (`overlap_secs = 0`) and confirms
  the new key is active immediately.
- `get_pending_service_pubkey`: shows `None` with no rotation in flight, then
  the pending `(key, expiry)` tuple after rotating with a 3600s overlap
  window.

No function signatures, behavior, or existing doc text were changed —
documentation-only, additive changes, one commit per function on branch
`docs/service-pubkey-doctests`.
