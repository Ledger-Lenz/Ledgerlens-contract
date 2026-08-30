# Score Aggregation Mathematics

This document describes the mathematical formulas, fixed-point representation, and integer arithmetic used in LedgerLens score aggregation. Off-chain simulators must use identical integer arithmetic and truncation behavior to match on-chain results.

---

## Fixed-Point Representation

### Scale Factor

All fractional values in LedgerLens are represented as integers scaled by a fixed multiplier:

```
SCALE = 1,000,000  (10^6)
```

A value represented in fixed-point is scaled by multiplying by `SCALE`. For example:
- 1.0 is represented as `1_000_000`
- 0.5 is represented as `500_000`
- 0.000001 is represented as `1`

### Why Fixed-Point?

Soroban (Stellar's smart contract platform) has no floating-point arithmetic. Fixed-point integer arithmetic is used to approximate decimal values while maintaining determinism across all environments (on-chain Rust, off-chain simulators, indexers).

### Conversion Formulas

**From floating-point to fixed-point:**
```
fixed = float_value * SCALE
```

**From fixed-point to floating-point:**
```
float_value = fixed / SCALE
```

**Key property:** All intermediate computations use integers; the result is truncated (not rounded) to match on-chain behavior. A simulator using rounding will diverge from the contract.

---

## Weighted Average

### Formula

The aggregate risk score is a weighted average of per-pair component scores:

$$\text{aggregate\_score} = \frac{\sum_{i=1}^{n} w_i \cdot s_i}{\sum_{i=1}^{n} w_i}$$

Where:
- $s_i$ = component score for pair $i$ (0–100)
- $w_i$ = weight assigned to pair $i$ (configurable per pair, defaults to 1)
- $n$ = number of distinct asset pairs the wallet has a score for

### Integer Implementation

In the contract (`compute_aggregate_score`), the computation is:

```rust
let mut weighted_sum: u64 = 0;
let mut weight_sum: u64 = 0;

for each (pair, score) in wallet.scores {
    let weight = get_pair_weight(pair);
    let decayed_weight = weight * decay_factor / SCALE;  // If decay is enabled
    
    let product = decayed_weight * score;
    weighted_sum += product;
    weight_sum += decayed_weight;
}

let aggregate_score = (weighted_sum / weight_sum) as u32;
```

**Integer arithmetic notes:**
- `weighted_sum` and `weight_sum` are `u64` to prevent overflow when accumulating across up to 20 pairs.
- Multiplication is checked (`checked_mul`) — overflow returns an error.
- Division truncates toward zero (integer division in Rust).
- The final result is cast to `u32` and is guaranteed to be in 0–100 (since component scores are 0–100 and this is a weighted average).

### Off-Chain Simulation

To match on-chain results exactly:

```python
# Python reference implementation
SCALE = 1_000_000

def aggregate(pairs_and_scores, pair_weights, decay_factors=None):
    """
    pairs_and_scores: list of (pair_symbol, score) tuples
    pair_weights: dict[pair_symbol -> weight]
    decay_factors: dict[pair_symbol -> decay_factor] (each scaled by SCALE)
    """
    if not decay_factors:
        decay_factors = {}
    
    weighted_sum = 0
    weight_sum = 0
    
    for pair, score in pairs_and_scores:
        weight = pair_weights.get(pair, 1)
        decay = decay_factors.get(pair, SCALE)
        
        # Decayed weight: (weight * decay) / SCALE
        decayed_weight = (weight * decay) // SCALE
        
        # Accumulate
        product = decayed_weight * score
        weighted_sum += product
        weight_sum += decayed_weight
    
    if weight_sum == 0:
        raise ValueError("All weights are zero")
    
    # Truncate division
    aggregate_score = weighted_sum // weight_sum
    return aggregate_score
```

---

## Exponential Decay

### Formula

When a score is older than the staleness window (default: 7 days), it is decayed using exponential decay:

$$\text{decay\_factor}(t) = e^{-\lambda \cdot t}$$

Where:
- $t$ = age in seconds since the score was submitted
- $\lambda$ = decay rate (numerator and denominator are configurable separately)
- Result is scaled by `SCALE` for fixed-point arithmetic

### Half-Life Interpretation

If you want scores to decay to half their impact after $T$ seconds:

$$\lambda = \frac{\ln(2)}{T} \approx \frac{0.693}{T}$$

For example, if $T = 30$ days = 2,592,000 seconds:
$$\lambda \approx 0.000000267$$

In fixed-point (scaled by $10^6$): $\lambda_{\text{scaled}} \approx 0.267$

### Integer Approximation (Taylor Series)

Soroban has no `exp()` function. The decay is approximated using a 4-term Taylor series:

$$e^{-x} \approx 1 - x + \frac{x^2}{2} - \frac{x^3}{6} + \frac{x^4}{24}$$

Where $x = \lambda \cdot t$ (scaled by `SCALE`).

**Accuracy:** For $x < 5$, this approximation achieves ~6 decimal places of precision (error < 0.01%).

### Integer Implementation

From `lib.rs` (`decay_fixed` function):

```rust
const SCALE: u64 = 1_000_000;

fn decay_fixed(age_secs: u64, lambda_num: u32, lambda_den: u32) -> u64 {
    if lambda_num == 0 {
        return SCALE;  // No decay
    }
    
    // Compute x_scaled = (num * age_secs * SCALE) / den
    let x_scaled = (lambda_num as u64)
        .checked_mul(age_secs)
        .and_then(|v| v.checked_mul(SCALE))
        .and_then(|v| v.checked_div(lambda_den as u64))
        .unwrap_or(0);
    
    // For large x, decay → 0
    if x_scaled >= 5 * SCALE {
        return 0;
    }
    
    // Taylor series: 1 - x + x²/2 - x³/6 + x⁴/24
    let x = x_scaled as i128;
    let s = SCALE as i128;
    
    let mut result = s;                    // Term 0: 1
    result -= x;                           // Term 1: -x
    result += (x * x) / (2 * s);           // Term 2: +x²/2
    result -= (x * x * x) / (6 * s * s);   // Term 3: -x³/6
    result += (x * x * x * x) / (24 * s * s * s);  // Term 4: +x⁴/24
    
    // Clamp to [0, SCALE]
    if result < 0 {
        0
    } else if result > s {
        SCALE
    } else {
        result as u64
    }
}
```

### Off-Chain Reference

```python
import math

SCALE = 1_000_000

def decay_factor(age_secs, lambda_num, lambda_den):
    """
    Compute the decay factor e^(-lambda * age).
    lambda = lambda_num / lambda_den
    Returns the result scaled by SCALE.
    """
    if lambda_num == 0:
        return SCALE
    
    # Compute x_scaled = (lambda_num * age_secs * SCALE) / lambda_den
    x_scaled = (lambda_num * age_secs * SCALE) // lambda_den
    
    if x_scaled >= 5 * SCALE:
        return 0
    
    # Taylor series approximation
    x = x_scaled
    s = SCALE
    
    result = s                                      # 1
    result -= x                                     # -x
    result += (x * x) // (2 * s)                    # +x²/2
    result -= (x * x * x) // (6 * s * s)            # -x³/6
    result += (x * x * x * x) // (24 * s * s * s)   # +x⁴/24
    
    # Clamp
    result = max(0, min(result, s))
    return result
```

---

## Linear Interpolation

### Formula

When querying a score at a timestamp between two historical entries, linear interpolation is used:

$$\text{score}(t) = s_a + (t - t_a) \cdot \frac{s_b - s_a}{t_b - t_a}$$

Where:
- $(t_a, s_a)$ = earlier history entry (timestamp and score)
- $(t_b, s_b)$ = later history entry
- $t$ = query timestamp

### Integer Implementation

From `lib.rs` (`get_interpolated_score`):

```rust
pub fn get_interpolated_score(
    env: Env,
    wallet: Address,
    asset_pair: Symbol,
    timestamp: u64,
) -> u32 {
    let history = storage::get_score_history(&env, &wallet, &asset_pair);
    
    if history.is_empty() {
        return 0;
    }
    
    // Exact match: return stored value
    for entry in history {
        if entry.timestamp == timestamp {
            return entry.score;
        }
    }
    
    // Extrapolation: clamp to boundaries
    if timestamp <= history.first().timestamp {
        return history.first().score;
    }
    if timestamp >= history.last().timestamp {
        return history.last().score;
    }
    
    // Interpolation: find the bracketing pair
    for i in 0..(history.len() - 1) {
        let a = &history[i];
        let b = &history[i + 1];
        
        if a.timestamp <= timestamp && timestamp <= b.timestamp {
            let dt = (b.timestamp - a.timestamp) as i128;
            if dt == 0 {
                return a.score;
            }
            
            let num = (timestamp - a.timestamp) as i128 * (b.score as i128 - a.score as i128);
            return (a.score as i128 + num / dt) as u32;
        }
    }
    
    history.last().score
}
```

**Integer arithmetic notes:**
- Numerator is `(timestamp - a.timestamp) * (b.score - a.score)` as `i128` to avoid overflow.
- Division truncates (integer division).
- Cast back to `u32` for the result.

---

## Overflow Handling

The contract uses checked arithmetic throughout the hot path to prevent silent overflows:

### Checked Operations

- **`get_aggregate_score`:**
  - `checked_mul(weight, decay_factor)` → error on overflow
  - `checked_div(SCALE)` → error on division by zero
  - `checked_mul(decayed_weight, score)` → error on overflow
  - `checked_add(weighted_sum, product)` → error on overflow
  
- **`decay_fixed`:**
  - `checked_mul` for `lambda_num * age_secs`
  - `checked_mul` for intermediate products
  - Saturating subtraction for negative results

### Error Propagation

When overflow is detected:
- Functions that compose checked operations return `Err(Error::ArithmeticOverflow)`.
- Callers must handle this error.
- On-chain, overflow is visible to integrators as an explicit error, preventing silent failures.

### Off-Chain Simulation

To avoid overflow in Python, use arbitrary-precision integers (Python 3 does this automatically for `int`). In other languages, use 128-bit or 256-bit integers for intermediate calculations.

---

## Staleness and Filtering

### Staleness Window

Scores older than the staleness window (default: `DEFAULT_STALENESS_WINDOW_SECS = 604,800` seconds = 7 days) are considered stale.

### Staleness Filtering in `get_effective_score`

1. Compute age: `age = current_timestamp - score_timestamp`
2. If `age > staleness_window` and `decay_rate != 0`:
   - Apply decay: `effective_score = raw_score * decay_factor(age)`
   - Set `decay_applied = true`
3. Otherwise:
   - `effective_score = raw_score`
   - Set `decay_applied = false`

### Embargo Filtering

Embargoed wallets are checked separately:
- `is_embargoed(wallet)` returns `true` if the wallet is on the embargo list.
- `get_effective_score` returns `Err(ScoreEmbargoed)` if the wallet is embargoed.
- `query_risk_gate` returns `false` if the wallet is embargoed.

---

## Asset-Class Policy Profiles

Different asset categories (stablecoins, volatile assets, thin markets, high-value pairs) warrant different risk-threshold policies. `get_effective_risk_threshold(asset_pair)` resolves the threshold to use for a pair:

1. If the pair has been assigned a class via `set_pair_asset_class(pair, class)` **and** that class has an override via `set_asset_class_policy(class, risk_threshold)`, the class override is returned.
2. Otherwise, the global `risk_threshold` (set via `set_risk_threshold`) is returned.

Lookup is a pure function of on-chain storage — same inputs always produce the same result — and pairs with no assigned class, or classes with no configured override, safely fall back to the global default rather than erroring. See `contracts/ledgerlens-score/src/test_asset_class_policy.rs` for fixtures covering the default-fallback and override-resolution paths.

---

## Precision Limits and Rounding

### Integer Truncation, Not Rounding

All divisions truncate toward zero. For example:
```
7 / 2 = 3  (not 3.5 rounded to 4)
```

This behavior is deterministic and matches across platforms.

### Precision Loss

When computing:
```
aggregate = (weighted_sum / weight_sum)
```

The result is truncated. For example, if the true average is 42.7, the contract returns 42. Off-chain simulators must use the same truncation to match.

### Decimal Precision

Fixed-point representation with `SCALE = 10^6` provides 6 decimal places. Values are stored as integers, so no floating-point rounding errors occur.

---

## Cross-Reference: Formula Documentation in Source Code

The following functions in `contracts/ledgerlens-score/src/lib.rs` reference this document:

- **`get_aggregate_score` (line ~1850):** See [§ Weighted Average](#weighted-average) for the formula and fixed-point implementation notes.
- **`get_effective_score` (line ~1601):** See [§ Staleness and Filtering](#staleness-and-filtering) and [§ Exponential Decay](#exponential-decay) for staleness filtering and decay logic.
- **`get_interpolated_score` (line ~1709):** See [§ Linear Interpolation](#linear-interpolation) for the formula and fixed-point implementation notes.
- **`decay_fixed` (line ~5340):** See [§ Exponential Decay](#exponential-decay) for the Taylor series approximation and fixed-point arithmetic.

---

## Off-Chain Simulation Checklist

When building an off-chain simulator (indexer, backend, analytics):

- [ ] Use integer arithmetic with the same `SCALE = 1,000,000` factor.
- [ ] Implement truncating division (not rounding).
- [ ] Use 64-bit or larger integers for intermediate calculations to prevent overflow.
- [ ] Implement the decay Taylor series with the same 4-term expansion.
- [ ] Handle edge cases: empty wallet lists, all-zero weights, invalid thresholds.
- [ ] Test against on-chain results with known inputs to verify precision.

---

---

## Monotonicity of Aggregate Score Under Pair Reweighting (#721)

When pair weights are changed while input scores remain fixed, the aggregate
score changes in predictable, monotone directions.

### Monotonicity Properties

| Property | Statement |
|---|---|
| M1 | Increasing the weight of a pair whose score is **above** the current aggregate **raises** (or preserves) the aggregate. |
| M2 | Increasing the weight of a pair whose score is **below** the current aggregate **lowers** (or preserves) the aggregate. |
| M3 | Setting all weights to zero is a degenerate case; the contract returns an error when `weight_sum = 0`. |
| M4 | A single pair with a very large weight dominates the aggregate: `agg → score_dominant` as `weight_dominant → ∞`. |
| M5 | If all pairs have the same score `S`, any positive reweighting leaves `agg = S`. |
| M6 | `max_pair_score` always equals the maximum individual score, independent of reweighting. |

### Worked Examples

**Example 1 — raising a high-score pair's weight:**
- Pairs: A (score=80, w=1), B (score=20, w=1) → `agg = floor((80+20)/2) = 50`
- Raise A's weight to 10: `agg = floor((10×80 + 1×20)/11) = floor(820/11) = 74` ✓ (increased)

**Example 2 — raising a low-score pair's weight:**
- Pairs: A (score=20, w=1), B (score=80, w=1) → `agg = 50`
- Raise A's weight to 10: `agg = floor((10×20 + 1×80)/11) = floor(280/11) = 25` ✓ (decreased)

### Test Coverage

Monotonicity properties are verified in
`contracts/ledgerlens-score/src/test_monotonicity_reweight.rs` using
deterministic unit tests with explicit expected values for each property.

---

## Confidence-Floor Semantics: Formal Truth Tables (#722)

The gate function `query_risk_gate_with_confidence` passes only when **all
three** of the following conditions hold simultaneously:

```
PASS  iff  score       >= threshold
       AND confidence  >= query_conf
       AND confidence  >= global_min_confidence
```

Where `global_min_confidence` is the admin-controlled floor set via
`set_global_min_confidence`.

### Table 1 — Score vs Threshold (confidence always passes)

| score | threshold | conf | query_conf | global_floor | result | reason |
|------:|----------:|-----:|-----------:|-------------:|:------:|--------|
|    80 |        70 |   90 |          0 |            0 | PASS   | score > threshold |
|    70 |        70 |   90 |          0 |            0 | PASS   | score == threshold (inclusive boundary) |
|    69 |        70 |   90 |          0 |            0 | FAIL   | score < threshold |
|     0 |         0 |   90 |          0 |            0 | PASS   | both zero |
|   100 |       100 |   90 |          0 |            0 | PASS   | both max |
|     0 |       100 |   90 |          0 |            0 | FAIL   | score 0, threshold max |

### Table 2 — Confidence vs Per-Query Confidence Threshold

| score | threshold | conf | query_conf | global_floor | result | reason |
|------:|----------:|-----:|-----------:|-------------:|:------:|--------|
|    80 |        70 |   80 |         80 |            0 | PASS   | conf == query_conf (inclusive) |
|    80 |        70 |   79 |         80 |            0 | FAIL   | conf one below query_conf |
|    80 |        70 |   81 |         80 |            0 | PASS   | conf above query_conf |
|    80 |        70 |  100 |        100 |            0 | PASS   | conf == query_conf == max |
|    80 |        70 |   99 |        100 |            0 | FAIL   | conf one below max query_conf |
|    80 |        70 |    0 |          0 |            0 | PASS   | both zero |

### Table 3 — Confidence vs Global Minimum Confidence Floor

| score | threshold | conf | query_conf | global_floor | result | reason |
|------:|----------:|-----:|-----------:|-------------:|:------:|--------|
|    80 |        70 |   75 |          0 |           75 | PASS   | conf == global_floor (inclusive) |
|    80 |        70 |   74 |          0 |           75 | FAIL   | conf one below global_floor |
|    80 |        70 |   76 |          0 |           75 | PASS   | conf above global_floor |
|    80 |        70 |    0 |          0 |            0 | PASS   | floor is zero, never blocks |
|    80 |        70 |  100 |          0 |          100 | PASS   | conf == global_floor == max |

### Table 4 — Combined Constraints

| score | threshold | conf | query_conf | global_floor | result | reason |
|------:|----------:|-----:|-----------:|-------------:|:------:|--------|
|    80 |        70 |   85 |         80 |           75 | PASS   | all three conditions pass |
|    65 |        70 |   85 |         80 |           75 | FAIL   | score < threshold |
|    80 |        70 |   79 |         80 |           75 | FAIL   | conf < query_conf |
|    80 |        70 |   74 |         70 |           75 | FAIL   | conf < global_floor |
|    80 |        70 |   74 |         80 |           75 | FAIL   | conf fails both conf checks |
|   100 |       100 |  100 |        100 |          100 | PASS   | all at maximum |

### Configuration Notes

- `global_min_confidence` is set admin-only via `set_global_min_confidence(floor: u32)`.
- Valid range: `[0, 100]`. Setting to 0 disables the floor (never blocks on confidence alone).
- The floor applies retroactively: raising it causes already-submitted scores with lower
  confidence to fail future gate queries without resubmission.

### Test Coverage

Truth tables are verified row-by-row in
`contracts/ledgerlens-score/src/test_confidence_floor_truth_tables.rs`.

---

## Model-Version Risk-Policy Compatibility (#723)

The active risk policy defines an allowlist of approved model versions.
Score submissions that carry an unapproved or retired version are rejected
deterministically at submission time.

### Version Lifecycle

```
                  register_model_version(v, delay)
                           │
                    delay elapsed?
                    ┌─── No ───→  Proposed  (not yet accepted)
                    │
                    └─── Yes ──→  Active    (accepted by risk policy)
                                      │
                              deprecate_model_version(v)
                                      │
                                  Deprecated  (permanently retired)
```

### Compatibility Rules

| Registry state | Submitted version | Outcome |
|---|---|---|
| Empty (no versions registered) | any | ACCEPTED (fallback: no restriction) |
| Non-empty | Active version | ACCEPTED |
| Non-empty | Proposed version (delay not elapsed) | REJECTED |
| Non-empty | Deprecated version | REJECTED |
| Non-empty | Unknown version (never registered) | REJECTED |

### Read API

- `is_model_version_active(version: u32) -> bool` — returns `true` if and only if the version
  is in the Active state. Off-chain tooling should call this before submitting to avoid a
  wasted transaction.
- `get_model_versions() -> Vec<ModelVersionEntry>` — returns the full registry with each
  entry's `version`, `status`, and `metadata` bytes.

### ABI / Storage Notes

- The registry is stored under a persistent storage key (`MODEL_VERSIONS`).
- Deprecation is irreversible: a deprecated version cannot be re-activated.
- The maximum registry size is bounded by `MAX_MODEL_VERSIONS` (defined in `constants.rs`)
  to prevent unbounded storage growth.

### Test Coverage

Model-version policy compatibility is verified in
`contracts/ledgerlens-score/src/test_model_version_policy_compat.rs`.
Existing lifecycle tests live in
`contracts/ledgerlens-score/src/test_model_version.rs`.

---

## Bounded Drift Checks for Consecutive Score Updates (#724)

Consecutive score updates for the same `(wallet, asset_pair)` are checked
against a configurable drift threshold (the "jump threshold").  A score change
whose absolute delta exceeds the threshold is classified as a suspicious jump
and triggers an on-chain event.

### Jump Threshold

| Parameter | Storage function | Description |
|---|---|---|
| `jump_threshold` | `set_jump_threshold(threshold: u32)` | Maximum permitted absolute delta between consecutive scores. Default: 50. |

`get_jump_threshold() -> u32` returns the current threshold.

### Drift Check Logic

```
delta = |new_score - previous_score|

if delta > jump_threshold:
    emit ScoreJumpAnomalyEvent { wallet, pair, prev, new, delta, timestamp }
    increment jump_anomaly_count for (wallet, pair)

# The submission is still stored (fail-soft by default).
# Use is_flagged=true to mark emergency overrides.
```

**Notes:**
- The first submission for a `(wallet, pair)` has no previous score, so it is
  never classified as a drift anomaly.
- The boundary is **exclusive**: `delta == jump_threshold` is accepted without
  an anomaly; `delta == jump_threshold + 1` triggers the anomaly.
- Score drops (decreasing changes) are subject to the same check as increases.

### Jump Stats API

- `get_jump_stats(wallet: Address, pair: Symbol) -> (u32, u64)` — returns
  `(anomaly_count, last_anomaly_timestamp)` for the given wallet/pair.
  Operators can poll this to detect wallets with frequent suspicious jumps.

### Drift Threshold Guidelines

| Threshold value | Behavior |
|---|---|
| 0 | Any change from the previous score triggers an anomaly (maximum sensitivity). |
| 50 (default) | Changes of more than 50 points are flagged. Covers model recalibrations. |
| 100 | Only the most extreme jumps (score goes from one extreme to another) are flagged. |

### ABI / Storage Notes

- `jump_threshold` is configurable post-deploy by admin multisig via
  `set_jump_threshold`.
- `ScoreJumpAnomalyEvent` is emitted on the `jmp_ano` topic with data
  `(prev_score, new_score, abs_delta, threshold, timestamp)`.
- Jump stats are stored per `(wallet, asset_pair)` under a persistent key.

### Test Coverage

Drift boundary conditions are verified in
`contracts/ledgerlens-score/src/test_bounded_drift.rs`, covering within-threshold,
at-boundary, one-above-boundary, drops, first-submission, configurable threshold,
and counter increment cases.

---

## References

- **Interface specification:** [`docs/interface-spec.md`](interface-spec.md)
- **Contract source:** `contracts/ledgerlens-score/src/lib.rs`
- **Constants:** `contracts/ledgerlens-score/src/constants.rs`

