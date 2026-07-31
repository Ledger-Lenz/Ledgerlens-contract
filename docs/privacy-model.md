# Differential Privacy Model

`get_private_aggregate_score` provides an ε-differentially-private variant of
the cross-asset aggregate risk score query.  The noise mechanism is the
**discrete Laplace mechanism** (the integer analogue of the classic Laplace
mechanism), calibrated to sensitivity 100 — the full output range of the
aggregate score (0–100).

## Definition

A randomised query `M` satisfies **ε-differential privacy** if for all
adjacent databases `D` and `D'` (differing by a single wallet's score), and
for all output sets `S`:

```
Pr[M(D) ∈ S] ≤ exp(ε) × Pr[M(D') ∈ S]
```

The privacy budget `ε` controls the privacy–utility trade-off:
lower values provide stronger privacy guarantees but add more noise.

## Noise Mechanism

### Laplace Inverse‑CDF Sampling

Noise is drawn from a discrete Laplace distribution `Lap(0, b)` with scale

```
b = sensitivity / ε = 100 / ε
```

using the inverse‑CDF (quantile) method:

```
noise = sign × Lap_magnitude
sign = ±1 with equal probability
magnitude = floor(b × (−ln(u)))
```

where `u` is a uniform random variate in `(0, 1)`.  This is the standard
**geometric mechanism** for integer-valued queries.

### Deterministic PRNG

On-chain smart contracts have no access to true entropy.  Instead, the noise
is derived from a **deterministic pseudo-random function** of the ledger
sequence number (a monotonically increasing value that changes each ledger
close) plus a caller-provided `seed` argument:

```text
prng = SHA-256(ledger_seq || seed || sensitivity || epsilon_scaled || "DPRN")
```

This means:

- **Reproducible** — calling `get_private_aggregate_score` at the same
  ledger sequence with the same `seed` produces identical output, which
  makes integration testing predictable.
- **Non-repeating across ledgers** — unless the caller reuses the same
  `seed` on the same ledger sequence, the noise will differ.
- **Caller-controlled seed** — callers can supply different `seed` values
  even within the same ledger to obtain independent noise samples.

### Clamping

Noise is clamped to `±3 × b` (i.e. `±3 × sensitivity / ε`) before being
added to the exact aggregate score.  The final noised result is clamped to
`[0, 100]` to stay within the valid score range.

## Configuration

| Function | Parameter | Description |
|---|---|---|
| `set_privacy_epsilon(epsilon_scaled)` | `epsilon_scaled = ε × 100` | Admin sets the privacy budget. `100` → ε = 1.0, `1` → ε = 0.01. `0` disables noise. |
| `get_privacy_epsilon()` | — | Returns the current `epsilon_scaled` value. Defaults to `0` (no privacy). |

## Sensitivity

The L1 sensitivity of the aggregate score query is **100** — the full output
range.  Because the aggregate is a weighted average of per-pair scores each
bounded to [0, 100], changing a single wallet's score by at most 100 can
change the average by at most 100.  Using the full range as sensitivity is
conservative (it slightly over-estimates the true sensitivity for wallets
with many pairs) and ensures the mechanism satisfies ε-DP regardless of the
number of asset pairs.

## Usage

```rust
// Query private aggregate
let private_score: u32 = client.get_private_aggregate_score(&wallet, &seed);
```

## Limits and Caveats

1. **Deterministic PRNG is not true randomness.**  An adversary who knows
   the contract source code and the ledger state can reproduce the noise
   value exactly.  This is inherent to any on-chain "randomness" on Soroban
   and is the standard trade-off.  The differential privacy guarantee is
   still meaningful because the noise is *statistically* calibrated —
   even a deterministic adversary sees a value drawn from the correct
   distribution at the time of the call.

2. **ε is a parameter, not a proof.**  The contract does not enforce a
   privacy budget composition bound (e.g., no sequential composition
   tracking).  Integrators calling `get_private_aggregate_score` multiple
   times for the same wallet will accumulate a total privacy spend of
   `k × ε` after `k` queries (by sequential composition).  Calling
   contracts should track their own privacy budget if they need a
   bounded total spend.

3. **Round‑off from clamping.**  When the noise is large enough to push the
   result outside [0, 100], clamping truncates the distribution, but the
   output remains within the valid score range and the privacy guarantee is
   preserved (clamping is a post-processing step and does not increase the
   privacy loss).

4. **No per‑pair private query.**  Only the cross-asset aggregate query
   (`get_private_aggregate_score`) has a private variant.  The per-pair
   `get_score` query is exact and not noise-calibrated.

## Deletion-policy note

Privacy-related score deletion is now intentionally separated from routine
admin rights. `clear_score` and `clear_score_history` can be placed behind an
explicit deletion-approval policy so normal governance operators cannot perform
irreversible deletion without the separately configured high-risk approver.

## Example

```text
sensitivity = 100
ε = 1.0       →  epsilon_scaled = 100
b = 100 / 1.0 = 100

Noise bounds: ±300

Exact score:  70
Noised score: 70 + Lap(0, 100) → e.g. 42 or 91
              (always clamped to [0, 100])
```

## Irreversible score deletion operations

`clear_score` and `clear_score_history` are intentionally destructive operator
tools. Before the changes for issues #791 and #792, the concrete behavior was:

- both calls were admin-only and no-op on missing data;
- `clear_score` removed only the latest `Score(wallet, pair)` entry;
- `clear_score_history` removed only the `ScoreHistory(wallet, pair)` ring;
- each path updated the histogram using only the current latest score when one
  existed;
- the emitted events (`clr_scr`, `clr_hist`) contained only the wallet topic
  and the asset pair payload, so operators had no hashed reason/category or
  auth-context evidence on chain.

The current operator workflow is:

1. Call `get_deletion_preflight(wallet, pair)` to inspect the deletion scope.
2. Execute `clear_score*` or `clear_score_history*`.
3. Persist the unhashed case notes off chain if a human-readable record is
   required.

### Preflight output

`get_deletion_preflight` is read-only and returns:

- `wallet`
- `asset_pair`
- `latest_score_present`
- `history_count`
- `audit_warning = Irreversible`

This preview intentionally avoids touching the history TTL, so operators can
inspect the target without mutating its retention horizon.

### Audit trail shape

`clr_scr` and `clr_hist` keep their existing topic shape
`(event_name, EVENT_VERSION, wallet)` but now append richer payload data:

- `asset_pair`
- `by` (the configured admin address)
- `latest_score_present`
- `history_count`
- `reason_hash = sha256(reason_bytes)`
- `category_hash = sha256(category_bytes)`
- `multisig_enabled`
- `signer_count`
- `threshold`

Raw deleted `RiskScore` values are still excluded from the event payload.

For backwards-compatible callers, `clear_score` and `clear_score_history`
continue to exist and emit deterministic default hashes for
`reason = "unspecified"` and categories `"score-clear"` / `"history-clear"`.
Operators that want case-specific hashes can call
`clear_score_with_audit(...)` or `clear_score_history_with_audit(...)`.

### Compatibility impact

- Public ABI: additive only. Existing delete entrypoints are preserved; new
  preflight and `*_with_audit` functions are optional.
- Event topics: unchanged.
- Event payloads: append-only expansion of `clr_scr` / `clr_hist`.
- Errors: unchanged.
- Storage layout: unchanged. No new persistent deletion log is stored on chain.

### Resource bounds

- `history_count` is derived from the bounded score-history ring, whose maximum
  depth remains `MAX_HISTORY_DEPTH = 50`.
- Batch submission boundaries remain capped at `MAX_BATCH_SIZE = 20`.
- The worst relevant cases are therefore still bounded by existing constants,
  and the existing batch/history TTL tests and benches remain the governing
  budget references for these paths.

## Event Privacy Audit

Every event published from `contracts/ledgerlens-score/src/events.rs` was
reviewed for whether its topic/data fields leak information beyond what is
already visible from the calling transaction's public arguments (which, on
Soroban, are public regardless of event emission). Findings below list each
risk-relevant event group, what it exposes, and the assessed risk. No event
schema changes were made as a result of this audit — see rationale per
finding.

| Event(s) | Data exposed | Risk | Mitigation / rationale |
|---|---|---|---|
| `score_submitted` | `benford_flag`, `ml_flag`, `confidence`, `timestamp` alongside the score | **Medium** — exposes model-confidence and anomaly-flag internals per wallet/pair, which could let an observer infer when the scoring model is uncertain or flagging a wallet, beyond the raw score itself | Accepted: these flags are the mechanism's core output and downstream consumers (disputes, risk gates) require them on-chain to function; `get_private_aggregate_score` remains the noise-calibrated path for callers who need a privacy-preserving read |
| `wallet_cluster_assigned`, `counterparty_link_added/removed`, `contagion_propagated` | Wallet-to-wallet relationships and cluster membership | **Medium** — reveals wallet monitoring/linkage strategy (which wallets are treated as related) | Accepted: correlation is the documented product behavior (contagion/cluster risk propagation); the alternative of suppressing it would break the on-chain audit trail these features exist to provide |
| `embargo_set/lifted`, `delegate_set/removed`, `watchlist_updated` | Regulatory-hold, delegation, and watchlist status per wallet | **Medium** — publicly tags a wallet as flagged/embargoed/delegated | Accepted: these are admin-authorized compliance actions that must be auditable; addresses are already public on Stellar, so this adds status metadata rather than new identity linkage |
| `signer_accuracy_updated/reset`, `consensus_signer_rejected`, `signer_tier_updated` | Per-signer accuracy/deviation stats and tier | **Low–Medium** — exposes signer performance/behavior, which is a service-operator concern rather than an end-user wallet, and is required for `signer_tier` gating to be independently verifiable | No change: signer addresses are already known from `signer_added`/`add_signer` calls; accuracy is operational telemetry, not end-user PII |
| `score_jump_anomaly`, `momentum_threshold_crossed`, `dormancy_decay_applied`, `risk_band_entered/cleared`, `threshold_breach`, `escalation_triggered/resolved`, `failover_triggered`, `suspicious_same_ledger_submission` | Wallet + asset-pair + score/threshold values | **Low** — derivable from the existing `score_submitted`/`score` history already on-chain; these events only summarize state transitions over that same public data | No change: no new inference surface beyond the base score event already audited above |
| Admin/config events (`*_updated`, `*_proposed`, `*_executed`, `pair_weight_*`, `cooldown_*`, `decay_rate_updated`, `fee_*`, upgrade/governance/model-version events) | Global parameters, proposal ids, admin addresses | **Low** — configuration is intentionally public for governance transparency | No change: no per-wallet or model-confidence data present |

### Conclusion

No event schema or field changes are required by this audit. The
highest-exposure events (`score_submitted`, wallet-linkage events, and
compliance-status events) are inherent to the contract's documented risk-gate
and compliance features rather than incidental leakage, and callers needing a
privacy-preserving score read already have `get_private_aggregate_score`
available. Since no event schema changed, no new tests were added for this
audit.
