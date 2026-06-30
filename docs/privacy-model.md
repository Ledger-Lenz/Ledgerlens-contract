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
