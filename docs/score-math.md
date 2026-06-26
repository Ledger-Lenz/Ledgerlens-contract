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

## References

- **Interface specification:** [`docs/interface-spec.md`](interface-spec.md)
- **Contract source:** `contracts/ledgerlens-score/src/lib.rs`
- **Constants:** `contracts/ledgerlens-score/src/constants.rs`

