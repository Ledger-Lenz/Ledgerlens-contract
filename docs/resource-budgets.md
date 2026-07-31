# Per-Entry-Point Soroban Resource Budgets

> **Issue #756** — Define explicit CPU, memory, and event-size budgets for
> all public entry points.

## Overview

Every public entry point in `ledgerlens-score` is measured by the Criterion
benchmark suite in
`contracts/ledgerlens-score/benches/entry_point_budgets.rs`.

Budgets are expressed as **soroban-sdk test-environment units**:

| Metric | Unit |
|---|---|
| CPU | `env.budget().cpu_instruction_cost()` (abstract instruction count) |
| Memory | `env.budget().memory_bytes_cost()` (bytes allocated on the host heap) |

These units are deterministic across runs and provide stable regression
anchors for CI.

Run the benchmarks:

```bash
cargo bench -p ledgerlens-score --bench entry_point_budgets
```

---

## Entry-Point Budget Reference

### Read-only (no state mutation)

| Entry point | Scenario | Relative cost |
|---|---|---|
| `get_score` | Score exists | Low — single persistent read + TTL extend |
| `get_score` | Score not found | Very low — read miss |
| `get_score_count` | After one submission | Low — single instance read |
| `get_score_history` | Empty ring | Low — single persistent read |
| `get_score_history` | Full ring (depth 10) | Low-medium — 10-entry Vec deserialize |
| `get_aggregate_score` | 3 scored pairs | Medium — O(pairs) weighted average |
| `query_risk_gate` | Score below threshold | Low — read + one comparison |
| `query_risk_gate` | Score at/above threshold | Low — same path, early return |
| `query_risk_gate` | No score (fail-closed) | Very low — read miss |
| `query_risk_gate_with_confidence` | Low confidence blocked | Low — read + two comparisons |
| `supports_interface` | Any capability | Very low — instance read + string match |
| `get_cooldown` | — | Very low — single instance read |
| `get_admin` | — | Very low — single instance read |
| `get_expiring_entries` | Empty index | Very low — read miss |
| `get_expiring_entries` | 50 tracked entries | Low — index scan, no TTL writes |

### Write (state mutation)

| Entry point | Scenario | Relative cost |
|---|---|---|
| `initialize` | Fresh contract | Low — 2 instance writes |
| `submit_score` | First submission | Medium — persistent write + index + TTL |
| `submit_score` | Subsequent (after cooldown) | Medium — same + history ring push |
| `submit_score` | Rate-limited rejection | Low — reads only, no writes |
| `submit_scores_batch` | Batch size 1 | Medium — equivalent to one `submit_score` |
| `submit_scores_batch` | Batch size 20 (max) | High — 20× O(n) |
| `set_cooldown` | — | Very low — single instance write |
| `set_service` | — | Very low — instance write + event |
| `set_history_max_depth` | — | Very low — proposal write |
| `override_rate_limit` | After one submission | Low — persistent delete + event |
| `set_pair_paused` | Pause a pair | Low — persistent write + index update |
| `set_score_floor_policy` | Enable floor | Very low — instance write |
| `extend_entry_ttls` | 1 entry | Low — 1 `extend_ttl` call |
| `extend_entry_ttls` | 20 entries | Medium — 20 `extend_ttl` calls O(n) |

---

## Budget Regression Policy

CI fails if a measured cost exceeds the approved baseline by more than:
- **10%** for read-only paths
- **20%** for write paths

To update a baseline after a deliberate change, re-run the bench, record the
new numbers in this document, and document the delta in the PR description.

---

## Worst-Case Paths

| Entry point | Worst-case trigger | Complexity |
|---|---|---|
| `submit_scores_batch` | 20 unique wallets, max batch | O(n) in batch size |
| `get_aggregate_score` | `MAX_WALLET_PAIRS` (20) pairs | O(pairs) |
| `get_score_history` | Ring at `MAX_HISTORY_DEPTH` (50) after depth reduction | O(depth) |
| `extend_entry_ttls` | `MAX_EXPIRING_ENTRIES_PER_CALL` (100) entries | O(n) |
| `get_expiring_entries` | `MAX_TRACKED_SCORE_ENTRIES` (500) in index | O(entries) early-exit |

For batch worst-case profiles (rejected, attested, mixed) see
[`benches/batch_worst_case_profiles.rs`](../contracts/ledgerlens-score/benches/batch_worst_case_profiles.rs)
and issue #757.

---

## ABI / Compatibility Notes

- Adding a new persistent write inside an existing entry point increases its
  budget; document the delta here and adjust the CI tolerance if needed.
- Soroban-SDK version bumps may shift all numbers proportionally; re-baseline
  on every SDK upgrade.
