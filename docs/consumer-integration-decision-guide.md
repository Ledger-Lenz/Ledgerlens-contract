# Consumer Integration Decision Guide

_Integration decision points for protocol risk owners (issue #720)._

This document maps protocol risk appetite to concrete LedgerLens configuration
choices: threshold, confidence floor, fallback behavior, and pause policy.  It
is addressed to operators and developers who are integrating LedgerLens into an
AMM or lending contract and need to choose settings that match their risk
tolerance.

---

## 1. Quick Reference — Configuration Parameters

| Parameter | What it controls | Configured on |
|---|---|---|
| `gate_threshold` (0–100) | Maximum risk score a wallet may have to pass the gate. Lower = stricter. | Consumer contract |
| `min_confidence` (0–100) | Minimum confidence LedgerLens must have in the score. Lower = more permissive. | Consumer contract |
| Failover contract address | Secondary LedgerLens deployment consulted when the primary is paused. | Primary LedgerLens (admin-only) |
| `FAILOVER_STALENESS_WINDOW` | How old a failover score may be before it is treated as missing (3 600 s). | Hard-coded in LedgerLens |
| Embargo | Wallet-level override: always `false` regardless of score. | Primary LedgerLens (admin-only) |
| Pause circuit breaker | Global pause: all gates return `false` unless a healthy failover is configured. | Primary LedgerLens (admin-only) |

---

## 2. AMM Examples

### 2.1 Permissive AMM (retail/open trading)

Risk appetite: accept the widest possible range of wallets while blocking only
the clearest high-risk signals.

```rust
// Consumer-side settings passed at initialize / set_liquidity_gate_config
const GATE_THRESHOLD: u32 = 90;  // only block wallets scored 90–100
const MIN_CONFIDENCE: u32 = 30;  // accept scores with low certainty
```

**Trade-off:** Wallets with moderately elevated risk (score 75–89) pass the
gate.  Low-confidence scores are accepted, meaning signals backed by sparse
data are treated as conclusive.  Suitable for open venues where compliance
requirements are minimal.

**Explicitly unsafe configuration:** setting `gate_threshold = 100` allows
every scored wallet, but wallets with _no_ score still fail closed (the
gate returns `false`).  This is distinct from an allowlist — unknown wallets
are always blocked regardless of threshold.

---

### 2.2 Standard AMM (regulated or semi-permissioned trading)

Risk appetite: block wallets with a meaningfully elevated risk score and require
a reasonable confidence level before trusting the signal.

```rust
const GATE_THRESHOLD: u32 = 75;  // recommended default; mirrors mock-amm fixture
const MIN_CONFIDENCE: u32 = 50;  // score must be backed by moderate certainty
```

**Trade-off:** Some legitimate wallets with sparse transaction history will be
blocked because their confidence is below 50.  This is the intended behavior:
treating an uncertain signal as "not safe" is more conservative than treating
it as "probably safe."

**Pause behavior:** If the LedgerLens primary is paused and no failover is
configured, the gate returns `false` for every wallet.  During scheduled
maintenance windows, configure a failover secondary to avoid blocking all swaps.

---

### 2.3 Strict AMM (compliance-first venue)

Risk appetite: only wallets with a conclusive low-risk score may trade; any
uncertainty is treated as a rejection.

```rust
const GATE_THRESHOLD: u32 = 60;
const MIN_CONFIDENCE: u32 = 80;  // require high certainty before allowing
```

**Trade-off:** Significantly more wallets are blocked.  New wallets (no score)
and wallets whose scores were submitted by fewer than the quorum of service
signers (low confidence) are blocked entirely.  Suitable for regulated venues
with strict KYC/AML obligations.

---

## 3. Lending Protocol Examples

Lending markets have asymmetric risk: a bad borrow causes a delayed loss (at
liquidation time) rather than an immediate fee.  This justifies a lower
`gate_threshold` and a higher `min_confidence` than an equivalent AMM.

### 3.1 Standard Lending Market

```rust
const GATE_THRESHOLD: u32 = 75;
const MIN_CONFIDENCE: u32 = 60;  // stricter than the equivalent AMM
```

**Rationale:** Use `query_risk_gate_with_confidence` rather than
`query_risk_gate` so that a wallet with a technically-passing score but
low-quality data is still blocked.  The mock-lending contract demonstrates
this: a wallet with `score = 10, confidence = 20` is rejected even though
`10 < 75`.

### 3.2 Conservative Lending Market (over-collateralized, institutional)

```rust
const GATE_THRESHOLD: u32 = 50;
const MIN_CONFIDENCE: u32 = 85;
```

**Explicitly unsafe configurations to avoid:**

- Setting `min_confidence = 0`: accepts scores with zero supporting evidence.
  A wallet that was scored once from a single low-quality data source passes
  exactly the same as one scored by a full consensus round.
- Omitting the confidence check (using `query_risk_gate` instead of
  `query_risk_gate_with_confidence`): the confidence floor is bypassed entirely,
  defeating the signal-quality gate.

---

## 4. Fallback and Pause Settings

### 4.1 No failover configured (simplest, most conservative)

When the primary is paused (circuit breaker tripped), every gate call returns
`false`.  All swaps and borrows are blocked until the operator calls `unpause`.

**When to use:** Small deployments, protocols that can tolerate a brief
operational pause, or protocols where a wrong-direction error (allowing a
bad actor) is worse than a false-positive (blocking a legitimate user).

### 4.2 Failover secondary configured

```
primary.set_failover_contract(admin_signers, secondary_contract_id)
```

When the primary is paused, gate calls consult the secondary's `get_score_opt`.
The secondary score is accepted only when:

1. The secondary returns a score (wallet has been scored on that deployment).
2. The score's `timestamp` is within `FAILOVER_STALENESS_WINDOW` (3 600 s = 1 hour).

If either condition fails, the gate returns `false` (fail closed), not `true`.

**Operational requirement:** The secondary must be kept in sync with the primary.
A failover deployment that is days behind offers no useful coverage and will
fail closed for all wallets after the staleness window expires.

**When to use:** High-availability protocols (AMMs with significant liquidity)
where brief pauses would cause meaningful lost revenue or user friction.

### 4.3 Behavior matrix

| Primary state | Failover configured | Secondary score | Gate result |
|---|---|---|---|
| Active | Any | Any | Normal (score-based) |
| Paused | No | — | `false` (fail closed) |
| Paused | Yes | Fresh, low-risk | `true` (failover pass) |
| Paused | Yes | Fresh, high-risk | `false` (failover reject) |
| Paused | Yes | Stale (> 3 600 s) | `false` (fail closed) |
| Paused | Yes | Missing (not scored) | `false` (fail closed) |

---

## 5. Embargo Behavior

An embargoed wallet always produces `false` from both gate functions,
regardless of its stored score or the configured threshold and confidence
values.  Embargoes are wallet-global: they apply across all asset pairs.

**Implication for operators:** Lifting an embargo re-exposes the wallet's
stored score to the gate.  If the score has decayed or was submitted while
the wallet was embargoed, it may have changed.  Audit the current score before
lifting an embargo in production.

---

## 6. ABI, Event, Error, and Storage Compatibility Notes

The functions covered by this guide have been stable since interface version 2
(`get_version() >= 4`):

| Function | Interface capability symbol | Notes |
|---|---|---|
| `query_risk_gate` | `gate` | Infallible, side-effect free |
| `query_risk_gate_with_confidence` | `cgate` | Infallible, side-effect free |
| `is_paused` | (read from storage) | Always safe to poll |
| `set_failover_contract` | (admin write) | Requires admin multi-sig |
| `supports_interface` | (introspection) | Safe to cache the result |

Consumers should probe `supports_interface("cgate")` before calling
`query_risk_gate_with_confidence`.  If it returns `false`, fall back to
`query_risk_gate` with a stricter threshold and accept that confidence is not
enforced at the gate level.

---

## 7. Resource Usage

Both gate functions are pure reads that do not extend TTL.  Each cross-contract
call consumes a fixed, bounded instruction budget:

- `query_risk_gate`: single storage read + embargo check + score comparison.
- `query_risk_gate_with_confidence`: same as above plus confidence comparison.
- Failover path (primary paused): one additional cross-contract `invoke_contract`
  to the secondary.

These calls are safe to invoke on every user-triggered transaction.  No
caching, batching, or rate limiting is required at the consumer level.

---

## 8. Integration Checklist

Before going live, verify:

- [ ] `gate_threshold` reflects your protocol's risk appetite (see §2–3).
- [ ] `min_confidence` is set to a non-zero value if you use `cgate`.
- [ ] You call `query_risk_gate_with_confidence` (not `query_risk_gate`) if
      signal quality matters to your protocol.
- [ ] You have decided on a pause policy (§4) and configured failover if needed.
- [ ] Your contract rejects operations when the gate returns `false`, not only
      when it returns an error.  The gate is infallible; there is no error to
      catch — only the boolean result.
- [ ] You have tested the "wallet never scored" path and confirmed it blocks.
- [ ] You understand that embargoed wallets are blocked regardless of your
      threshold (§5).
- [ ] You have probed `supports_interface("gate")` and `supports_interface("cgate")`
      on your target deployment and confirmed both return `true`.
