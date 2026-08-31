# Typed Client Examples

**Tracks issue:** #762  
**Source file:** [`examples/typed_client_examples.rs`](../examples/typed_client_examples.rs)

This document walks through the four canonical LedgerLens integration flows
using the generated `LedgerLensScoreContractClient`. Every snippet is a
copy-pasteable starting point that covers both the success path and the most
important failure modes.

> **Don't read the contract source to integrate.** Use these examples and
> [`docs/interface-spec.md`](interface-spec.md) as your reference.

---

## 1. Score Flow

**Use when:** your off-chain pipeline needs to write scores and your dashboard
or API layer needs to read them back.

### Submit a single score

```rust
use ledgerlens_score::LedgerLensScoreContractClient;
use soroban_sdk::{symbol_short, Address, Env, Vec};

let client = LedgerLensScoreContractClient::new(&env, &contract_id);

client.submit_score(
    &Vec::new(&env),        // signers: empty = single-service mode
    &wallet,                // Address to score
    &symbol_short!("XLM_USDC"), // asset pair (≤9 chars)
    &72,                    // risk score 0-100 (higher = more suspicious)
    &true,                  // benford_flag
    &false,                 // ml_flag
    &env.ledger().timestamp(), // ledger timestamp (must be > 0)
    &88,                    // confidence 0-100
    &1,                     // model_version
    &None,                  // attestation_input (None = no secp256k1 payload proof)
);
```

### Read a score

```rust
let score = client.get_score(&wallet, &symbol_short!("XLM_USDC"));
println!("score={} confidence={}", score.score, score.confidence);
```

### Expected failures

| Situation | Error |
|---|---|
| Wallet has never been scored | `Error::ScoreNotFound` |
| `score > 100` or `confidence > 100` | `Error::InvalidScore` / `Error::InvalidConfidence` |
| Same pair submitted within cooldown window | `Error::RateLimitExceeded` |
| Called before `initialize` | `Error::NotInitialized` |

---

## 2. Gate Flow

**Use when:** another Soroban contract needs to refuse high-risk wallets.

`query_risk_gate` is **infallible**, **never panics**, and **side-effect
free**. Drop it into a guard clause without a `try_*` wrapper.

### Basic swap guard

```rust
use ledgerlens_score::LedgerLensScoreContractClient;
use soroban_sdk::{symbol_short, Address, Env};

fn swap(env: Env, user: Address) -> Result<(), MyError> {
    let client = LedgerLensScoreContractClient::new(&env, &ledgerlens_contract_id);

    // No try_, no ?, no error handling — the gate cannot fail.
    let is_safe = client.query_risk_gate(
        &user,
        &symbol_short!("XLM_USDC"),
        &75, // gate_threshold: score must be strictly below this
    );
    if !is_safe {
        return Err(MyError::HighRiskWallet);
    }
    // ... proceed
    Ok(())
}
```

### Confidence-gated liquidity add

Use `query_risk_gate_with_confidence` when a low-confidence "safe" signal is as
dangerous as a high-risk one (e.g. when moving funds):

```rust
let is_safe = client.query_risk_gate_with_confidence(
    &provider,
    &symbol_short!("XLM_USDC"),
    &75,  // gate_threshold
    &50,  // min_confidence: score.confidence must be >= this
);
if !is_safe {
    return Err(MyError::HighRiskOrLowConfidence);
}
```

### Gate semantics

| Condition | Returns |
|---|---|
| `score < gate_threshold` AND `confidence >= min_confidence` | `true` (safe) |
| `score >= gate_threshold` | `false` (risky) |
| `confidence < min_confidence` | `false` (uncertain, treated as risky) |
| No score exists for wallet | `false` (unknown → fail closed) |
| `gate_threshold > 100` | `false` (impossible to pass) |

---

## 3. History Flow

**Use when:** your dashboard or analytics layer needs to display score trends,
or you need to confirm how many times a wallet has been scored.

### Read score history

```rust
let history = client.get_score_history(&wallet, &symbol_short!("XLM_USDC"));
// Vec<RiskScore>, oldest first. Empty if wallet has never been scored.
for entry in history.iter() {
    println!("ts={} score={}", entry.timestamp, entry.score);
}
```

### Count total submissions

```rust
// Never truncated by the ring-buffer depth.
let total = client.get_score_count(&wallet, &symbol_short!("XLM_USDC"));
```

### Configure ring depth (admin only, time-locked)

```rust
// Reduce the ring to 5 entries (takes effect on the next submission).
client.set_history_max_depth(&admin_signers, &5);
// Read current depth:
let depth = client.get_history_max_depth(); // default: 10
```

**Depth bounds:** `[1, 50]`. Values outside this range are rejected with
`Error::InvalidHistoryDepth`. Decreasing the depth lazily evicts old entries on
the next `submit_score` call for that pair (see
[README § Lazy-truncation behaviour](../README.md)).

---

## 4. Governance Flow

**Use when:** you need to upgrade the contract WASM or rotate the admin service
key.

### Propose and veto an upgrade

```rust
use soroban_sdk::BytesN;

// 1. Propose (starts the time-lock).
let new_hash: BytesN<32> = /* SHA-256 of new WASM */ ...;
client.propose_upgrade(&admin_signers, &new_hash);

// 2. Anyone can inspect the pending proposal during the window.
let proposal = client.get_pending_upgrade();
// proposal.executable_after tells you when it can be executed.

// 3a. If the proposal looks bad — veto it.
client.veto_upgrade(&admin_signers);

// 3b. Or, once the time-lock has elapsed, execute it.
// env.ledger().timestamp() must be >= proposal.executable_after
client.execute_upgrade(&admin_signers);
```

### Rotate the service address

```rust
client.set_service(&new_service_address);
assert_eq!(client.get_service(), new_service_address);
```

### Configure submission cooldown

```rust
// Set to 2 hours (bounds: 60 s – 86 400 s).
client.set_cooldown(&admin_signers, &7_200u64);
```

### Governance errors

| Error | Meaning |
|---|---|
| `Error::UpgradeAlreadyPending` | A proposal is already in flight |
| `Error::NoPendingUpgrade` | `execute_upgrade`/`veto_upgrade` with nothing pending |
| `Error::UpgradeNotReady` | Time-lock has not elapsed yet |
| `Error::InvalidUpgradeDelay` | Delay outside `[172_800, 1_209_600]` seconds |
| `Error::Unauthorized` | Caller is not the admin |

---

## Running the Examples

The examples compile as library crates (they are Soroban contracts, not `main`
binaries):

```bash
cargo build --example typed_client_examples -p ledgerlens-score
```

All tests inside the example are standard `#[test]` functions and run with:

```bash
cargo test --example typed_client_examples -p ledgerlens-score
```

## See Also

- [`docs/interface-spec.md`](interface-spec.md) — canonical function signatures
- [`docs/interface-versioning-policy.md`](interface-versioning-policy.md) — breaking vs non-breaking changes
- [`examples/amm_gate.rs`](../examples/amm_gate.rs) — minimal AMM gate contract
- [`examples/amm_gate_example.rs`](../examples/amm_gate_example.rs) — AMM gate with inline tests
