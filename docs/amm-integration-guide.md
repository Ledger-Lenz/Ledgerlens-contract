# AMM Risk Gate Integration Guide

## Overview

LedgerLens provides real-time, on-chain risk scores for wallet-asset-pair combinations. When integrating into an Automated Market Maker (AMM), the `query_risk_gate` function allows you to enforce risk-based access control before allowing swaps to proceed. This prevents high-risk wallets from trading on your pool without explicit consent, enabling AMMs to serve compliant venues while maintaining DeFi's openness.

**Why it matters:**
- **Compliance:** Automatically block transactions from wallets flagged as high-risk without requiring manual intervention.
- **Risk mitigation:** Reduce exposure to wallets engaged in suspicious patterns (Benford's Law anomalies, ML-detected fraud signals).
- **Composability:** Query risk scores directly from your swap logic without oracles or trust assumptions.

---

## Prerequisites

Before you start, ensure you have:

1. **A Stellar account** funded with XLM (to cover transaction fees on Soroban)
2. **Soroban CLI** installed and configured (`soroban --version` to verify)
3. **The LedgerLens contract ID** for your network (e.g., deployed contract address on Testnet or Mainnet)
4. **Rust toolchain** for Soroban smart contract development

---

## Step 1 — Understand query_risk_gate vs. peek

LedgerLens provides two ways to check wallet risk:

| Aspect | `query_risk_gate` | `peek_effective_score` |
|--------|-------------------|------------------------|
| **Purpose** | Production swap validation | UI previews, off-chain simulation |
| **TTL Extension** | ✗ (no side effects) | ✗ (no side effects) |
| **Embargo Filtering** | ✓ (embargoed = false) | ✓ (checked, no error) |
| **Staleness Filtering** | ✗ (raw score only) | ✓ (applies decay if configured) |
| **Returns** | `bool` (pass/fail) | `Result<EffectiveRiskScore, Error>` |
| **Fee Charged** | ✗ | ✗ |
| **Fail-Closed** | Yes (unknown = false) | Yes (unknown = Err) |

**Choose `query_risk_gate` for swap validation.** It is infallible (no `Result` to handle), side-effect free (does not mutate state), and conservative (unknown wallets return `false`). You call it inside a guard clause and reject the swap if it returns `false`.

---

## Step 2 — Call query_risk_gate from Your Swap Function

Inside your AMM contract's swap entrypoint, after parameter validation, invoke LedgerLens:

```rust
use soroban_sdk::{Address, Symbol, Env, symbol_short};
use ledgerlens_score::LedgerLensScoreContractClient;

#[contractimpl]
impl MyAMM {
    pub fn swap(
        env: Env,
        user: Address,
        from_token: Address,
        to_token: Address,
        amount: i128,
    ) -> Result<i128, AMMError> {
        // 1. Check user's risk gate before proceeding with swap
        let llens_id = Address::from_contract_id(&env, &LEDGERLENS_CONTRACT_ID);
        let client = LedgerLensScoreContractClient::new(&env, &llens_id);
        
        let asset_pair = symbol_short!("XLM_USDC"); // Adjust to your pools
        let gate_threshold = 75; // Adjust based on your risk appetite
        
        if !client.query_risk_gate(&user, &asset_pair, &gate_threshold) {
            return Err(AMMError::UserHighRisk);
        }
        
        // 2. Proceed with swap logic
        let output = self.compute_output(&env, from_token, to_token, amount)?;
        self.execute_swap(&env, &user, from_token, to_token, amount, output)?;
        
        Ok(output)
    }
}
```

**Key details:**
- **Infallible:** `query_risk_gate` returns `bool`, never panics, never returns `Result`.
- **Side-effect free:** Does not extend TTL or charge fees.
- **Conservative:** A wallet with no LedgerLens score returns `false` (treated as risky).
- **Threshold interpretation:** The gate returns `true` only when `score < gate_threshold`. A score of exactly 75 fails the gate if your threshold is 75.

---

## Step 3 — Handle the Gate Result

When `query_risk_gate` returns `false`, reject the swap and return an appropriate error:

```rust
if !client.query_risk_gate(&user, &asset_pair, &gate_threshold) {
    return Err(AMMError::UserHighRisk);
}
// If we reach here, the wallet passed the gate and is safe to proceed.
```

**What causes the gate to return `false`:**
1. No score exists for the wallet and asset pair (unknown wallet — fail closed).
2. The wallet's score is ≥ the `gate_threshold` (too risky).
3. The wallet is embargoed (on a blacklist).
4. The `gate_threshold` or internal confidence floors are invalid (defensive checks).

All of these cases collapse to a single `false` return, so your swap logic can branch simply.

---

## Step 4 — Choose Your Threshold

`gate_threshold` is a caller parameter. Higher-value swaps warrant stricter thresholds:

- **Threshold 75** (default): Blocks wallets in the top 25% of risk. Reasonable for most pools.
- **Threshold 50**: Blocks wallets in the top 50% of risk. Conservative; may reject many wallets.
- **Threshold 90**: Blocks only the highest-risk wallets. Permissive; allows more volume.

LedgerLens's own default threshold is 75. Adjust based on your pool's risk tolerance and total value locked.

---

## Step 5 — (Optional) Use Confidence-Aware Gating

For high-security applications, use `query_risk_gate_with_confidence` to enforce both a maximum risk score and a minimum confidence floor:

```rust
let gate_threshold = 75;
let min_confidence = 60; // Require ≥60% model confidence
let passes_gate = client.query_risk_gate_with_confidence(
    &user,
    &asset_pair,
    &gate_threshold,
    &min_confidence,
);

if !passes_gate {
    return Err(AMMError::UserHighRisk);
}
```

This additional floor prevents wallets with low-confidence scores (meaning the ML model had insufficient data) from bypassing the gate. A score of `(score=30, confidence=5)` is epistemically equivalent to "unknown" and should not be trusted.

---

## Error Handling

`query_risk_gate` is **infallible** — it returns `bool` and never produces an error. All exceptional cases (no score, embargoed wallet, invalid thresholds) collapse to `false`.

If you call `get_score` or `get_effective_score` directly for more detailed diagnostics, handle the result:

```rust
match client.get_score(&user, &asset_pair) {
    Ok(score) => {
        // Manually check: score.score < gate_threshold
        if score.score >= gate_threshold {
            return Err(AMMError::UserHighRisk);
        }
    }
    Err(Error::ScoreNotFound) => {
        // Unknown wallet — apply your fallback logic (e.g., reject or allow)
        return Err(AMMError::UserHighRisk); // Fail closed: reject
    }
    Err(Error::ScoreEmbargoed) => {
        // Wallet is on the embargo list
        return Err(AMMError::UserHighRisk);
    }
    Err(e) => {
        // Other errors (NotInitialized, ContractPaused, etc.)
        return Err(AMMError::SystemError);
    }
}
```

**Why use `query_risk_gate` instead?** It folds all these cases into a simple `bool` so you don't have to handle each error type.

---

## Gas Considerations

- **`query_risk_gate` cost:** ~500–1000 stroops (very cheap). It is a pure read with no storage mutations.
- **Caching:** For hot paths (many swaps in rapid succession), consider caching the gate result per user for a short TTL (e.g., a few ledgers). Re-query when the cache expires.
- **Batch checks:** If you need to validate multiple wallets in a single transaction, query them sequentially or fetch their scores in a batch operation if your AMM supports it.

---

## Example: Complete AMM Swap with Risk Gating

See `examples/amm_gate_example.rs` for a working, compilable Rust snippet that demonstrates:
1. Importing the LedgerLens client.
2. Calling `query_risk_gate` in a swap function.
3. Handling the gate result and returning appropriate errors.
4. A test stub showing the happy path.

---

## Troubleshooting

**"Contract not found"**: Ensure the LedgerLens contract ID is correct and deployed on your network.

**"Service silence alert"**: The LedgerLens service has not submitted updates recently. Decide whether to allow swaps during this window (e.g., fall back to a default threshold) or reject them until the service resumes.

**Gate always returns `false`**: Check that:
1. The wallet has a score in LedgerLens (call `get_score` to verify).
2. The `gate_threshold` is in the valid range 0–100.
3. The wallet is not embargoed.

---

## References

- **Interface specification:** [`docs/interface-spec.md`](interface-spec.md)
- **Score query guide:** [`docs/score-query-guide.md`](score-query-guide.md)
- **Contract source:** `contracts/ledgerlens-score/src/lib.rs`
