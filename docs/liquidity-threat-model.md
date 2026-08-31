# Threat model: score manipulation incentives in liquidity protocols

This document focuses on AMM and lending integrations that consume
`query_risk_gate` or `query_risk_gate_with_confidence`.

## Current concrete behavior

- Missing score: gate fails closed
- Embargoed wallet: gate fails closed
- Pending finality-buffer score: downstream gate still sees the prior live
  score, or no score at all if none was committed yet
- Low-confidence score: `query_risk_gate_with_confidence` fails closed when
  `score.confidence < max(call_min_confidence, global_min_confidence)`
- Raw `query_risk_gate` does not itself encode a confidence floor

## Attack scenarios

### 1. Timing attack against finality windows

Scenario:

1. Attacker obtains or submits a low-risk score.
2. Score is held behind a non-zero `finality_buffer`.
3. Attacker attempts to trade or borrow before the score is committed.

Impact:

- If the integrator incorrectly assumes pending scores are already live, it may
  admit activity based on nonexistent state.

Current mitigation:

- The contract fails closed for unknown wallets and does not expose pending
  scores through the normal risk-gate path.

Tests:

- `amm_swap_rejected_while_safe_score_is_still_pending_finality`
- `lending_borrow_rejected_while_safe_score_is_still_pending_finality`

### 2. Confidence-floor evasion

Scenario:

1. Attacker obtains a low raw score with weak model confidence.
2. Protocol uses `query_risk_gate` instead of the confidence-aware variant.

Impact:

- Risk gate may admit an economically important action on low-quality evidence.

Current mitigation:

- `query_risk_gate_with_confidence`
- `global_min_confidence`
- Existing composability tests for low-confidence rejection

Operator guidance:

- AMMs with compliance or sanctions concerns should still prefer the
  confidence-aware gate.
- Lending protocols should not use raw `query_risk_gate` for credit decisions.

### 3. Whitewashing via rapid re-score

Scenario:

1. High-risk wallet receives a historically high score.
2. Compromised signer attempts to overwrite it with an artificially safe score.

Impact:

- Borrow or LP admission can occur before manual review.

Current mitigation:

- `score_floor_policy`
- `cooldown`
- `adaptive_rate_limit`
- multisig / threshold attestation controls

Residual risk:

- If operators leave the score floor disabled and allow aggressive cooldown
  settings, the window for exploitation expands materially.

### 4. Stale-safe-score carry trade

Scenario:

1. Wallet had a previously safe score during benign behavior.
2. Behavior deteriorates off-chain, but no fresh score arrives.
3. Integrator continues trusting an old score without appropriate freshness
   expectations.

Impact:

- Capital access granted against outdated evidence.

Current mitigation:

- `staleness_window`
- `get_effective_score` for integrations that explicitly care about stale-score
  penalty semantics

Follow-up note:

- `query_risk_gate` itself is a threshold gate, not a freshness policy engine.
  Integrators with strict freshness requirements should document an additional
  freshness check in their own flow.

## Recommended integration posture

- AMM swaps:
  - acceptable to use raw gate only when the venue tolerates lower assurance
  - prefer non-zero `finality_buffer` plus clear operator review
- Liquidity provision:
  - use confidence-aware gate and non-zero global confidence floor
- Lending:
  - use confidence-aware gate
  - keep `score_floor_policy` enabled
  - keep cooldown and adaptive rate-limit settings conservative

## Compatibility impact

- No existing gate ABI changed
- The new tests only document and lock in fail-closed timing semantics

