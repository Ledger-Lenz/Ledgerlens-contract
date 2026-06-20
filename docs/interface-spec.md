# `ILedgerLensScore` — Composability Interface Specification

**Status:** Stable · **Interface version:** 2 · **Contract:** `LedgerLensScoreContract`

LedgerLens turns off-chain fraud signals (Benford's-Law analysis + an ML
ensemble) into an on-chain, 0–100 risk score per `(wallet, asset_pair)`. The
point of putting it on-chain is **composability**: any Soroban protocol — an
AMM, a lending market, a DEX aggregator — should be able to consult a risk
score inside its own logic without trusting an external oracle.

This document is the canonical, versioned integration contract for those
third-party callers. It defines the function signatures you may rely on, the
exact data layout you decode against, the stability guarantees behind each, and
the recommended ways to wire LedgerLens into your protocol.

> If you are integrating LedgerLens, **program against this document, not
> against the source.** Anything not listed here as stable may change between
> releases.

---

## 1. Canonical functions

These are the functions external contracts are expected to call. Build the
generated `LedgerLensScoreContractClient` against the deployed contract ID and
invoke them like any other cross-contract call.

### 1.1 `query_risk_gate` — the integration primitive

```rust
fn query_risk_gate(
    env: Env,
    wallet: Address,
    asset_pair: Symbol,
    gate_threshold: u32,
) -> bool
```

Returns `true` when `wallet`'s latest score for `asset_pair` is **strictly
below** `gate_threshold` (safe to proceed), and `false` when the score is
`>= gate_threshold` **or no score exists**.

This is the function you should reach for first. Its design is deliberately
defensive so it is safe to call from inside another contract's authorization
path:

- **Infallible.** Returns a plain `bool`, never a `Result`. There is no
  `try_query_risk_gate` to handle.
- **Never panics.** It cannot trap the calling transaction, so it cannot be
  used to grief your protocol's gas or disable your guard clause.
- **Side-effect free.** It is a pure read and does not even extend storage
  TTL — calling it does not mutate LedgerLens state.
- **Conservative on the unknown.** A wallet with no score returns `false`
  (treated as risky). See [§5](#5-security-considerations).

The comparison is strict (`score < gate_threshold`). A score *equal to* the
threshold is **not** safe.

### 1.2 `supports_interface` — capability detection

```rust
fn supports_interface(env: Env, capability: Symbol) -> bool
```

Returns `true` if this deployment supports the named capability. Use it to
feature-detect at runtime instead of hardcoding a contract version. Recognised
capabilities (all `symbol_short!`):

| Capability | Backing functionality                              |
|------------|----------------------------------------------------|
| `score`    | `get_score` / `submit_score`                       |
| `history`  | `get_score_history`                                |
| `batch`    | `submit_scores_batch`                              |
| `gate`     | `query_risk_gate`                                  |
| `aggr`     | `get_aggregate_score` (cross-asset aggregate risk) |

Unrecognised capabilities return `false`.

### 1.3 Direct read functions

| Signature | Returns | Notes |
|-----------|---------|-------|
| `get_score(env, wallet, asset_pair) -> Result<RiskScore, Error>` | latest score | `Err(ScoreNotFound)` if absent |
| `get_score_history(env, wallet, asset_pair) -> Vec<RiskScore>` | up to 10 entries, oldest first | empty `Vec` if none |
| `get_aggregate_score(env, wallet) -> Result<AggregateRiskScore, Error>` | cross-asset weighted view | `Err(ScoreNotFound)` if the wallet has no scores |
| `get_version(env) -> u32` | contract build version | currently `2` (was `1` prior to the `BatchResult` ABI change) |

`get_score` is the right call when you need the full struct (confidence, model
version, flags) rather than a yes/no gate decision. Prefer `query_risk_gate`
for guard clauses precisely because `get_score` *can* return an error you would
then have to handle.

---

## 2. Data layout (`RiskScore`)

Field order is significant: it is part of the XDR serialization that callers in
other languages decode against. **Do not reorder, remove, or change the type of
any field** without a breaking-change release.

```rust
#[contracttype]
pub struct RiskScore {
    pub score: u32,         // overall risk, 0–100 (higher = riskier)
    pub benford_flag: bool, // Benford's-Law engine flagged this entity
    pub ml_flag: bool,      // ML ensemble flagged this entity
    pub timestamp: u64,     // ledger time the score was computed off-chain
    pub confidence: u32,    // model confidence, 0–100
    pub model_version: u32, // detection-pipeline model version
}
```

`AggregateRiskScore` (returned by `get_aggregate_score`) has the following
stable layout:

```rust
#[contracttype]
pub struct AggregateRiskScore {
    pub aggregate_score: u32,    // weighted average of per-pair scores, 0–100
    pub pair_count: u32,         // distinct pairs the wallet has a score for
    pub max_pair_score: u32,     // highest single per-pair score
    pub max_pair: Symbol,        // the pair that produced max_pair_score
    pub benford_flag_count: u32, // pairs with benford_flag set
    pub ml_flag_count: u32,      // pairs with ml_flag set
    pub last_updated: u64,       // ledger time of the newest component score
}
```

### Score scale

`score` and `confidence` are bounded to `0..=100`; submissions outside that
range are rejected (`get_score` never returns an out-of-range value). Treat
higher `score` as higher risk. `confidence` and `model_version` let you reason
about staleness — e.g. ignore a score below some confidence floor, or refresh
when `model_version` advances.

---

## 3. Versioning policy

There are two independent version numbers:

1. **Contract version** — `get_version()` (backed by `CONTRACT_VERSION`,
   currently `2`). Bumped on any breaking ABI change.
2. **Interface version** — the number at the top of this document. It tracks
   the `ILedgerLensScore` surface specifically.

**How callers should detect compatibility:** prefer `supports_interface` over
version comparison. Capability detection is forward-compatible — a newer
deployment that adds capabilities still answers `true` for the ones you depend
on, whereas a hardcoded `get_version() == 1` check would break on an additive
upgrade. Reserve `get_version()` for diagnostics, telemetry, and logging.

A capability symbol, once published, will not be removed or repurposed within
the same interface major version. Removing one is a breaking change and forces
an interface-version bump.

---

## 4. Error code stability

Errors are a `#[contracterror] #[repr(u32)]` enum. **The discriminant values
below are stable** — integrators may match on the numeric code:

| Code | Variant | Meaning |
|-----:|---------|---------|
| 1 | `AlreadyInitialized` | `initialize` called twice |
| 2 | `NotInitialized` | contract not yet initialized |
| 3 | `Unauthorized` | caller is not the required admin/service |
| 4 | `InvalidScore` | `score` outside `0..=100` |
| 5 | `InvalidConfidence` | `confidence` outside `0..=100` |
| 6 | `ScoreNotFound` | no score for this `(wallet, asset_pair)` / wallet |
| 7 | `ContractPaused` | circuit breaker active |
| 8 | `NoPendingAdminTransfer` | no admin transfer in progress |
| 9 | `EmptyBatch` | `submit_scores_batch` called with no entries |
| 10 | `BatchTooLarge` | batch exceeds `MAX_BATCH_SIZE` |
| 11 | `ArithmeticOverflow` | aggregate computation overflowed |
| 30 | `PairPaused` | submission attempted while this `asset_pair` is individually paused via `set_pair_paused` — superseded by `ContractPaused` when the global circuit breaker is also active |

**Guarantees:**

- Existing variants keep their numeric value across releases within an
  interface major version.
- New error variants may be **added** with new, higher discriminants. Callers
  must therefore treat an unrecognised error code defensively (fail closed)
  rather than assuming the set is exhaustive.
- `ScoreNotFound` (6) is the one most integrators handle directly. If you only
  need a go/no-go decision, use `query_risk_gate`, which folds the
  not-found case into a conservative `false` and spares you error handling
  entirely.

---

## 5. Security considerations

- **The gate fails closed.** Because `query_risk_gate` returns `bool`, the
  "no score" case must collapse to a single value — and that value is `false`.
  Unknown wallets are treated as *potentially risky*. If you instead want to
  allow-list unknown wallets, you must make that decision explicitly in your
  own contract; do not assume `query_risk_gate` will ever return `true` for a
  wallet LedgerLens has never seen.
- **`query_risk_gate` cannot be weaponised against you.** It is infallible and
  side-effect free by design, so an attacker cannot craft inputs that make it
  panic, consume unexpected gas, or mutate state to disable your guard.
- **Decide your own threshold.** `gate_threshold` is a caller parameter, not a
  protocol constant. Higher-value actions warrant a lower (stricter) threshold.
  LedgerLens's own default risk threshold is `75`; it is a reasonable starting
  point, not a mandate.
- **Capability removal is breaking.** Treat the capability set as append-only
  within a major version when designing long-lived integrations.

---

## 6. Recommended integration patterns

### 6.1 Gate-on-threshold (the default)

Call `query_risk_gate` inside your guard clause and refuse risky wallets. This
is the pattern shown in [`examples/amm_gate.rs`](../examples/amm_gate.rs):

```rust
let client = LedgerLensScoreContractClient::new(&env, &llens_id);
if !client.query_risk_gate(&user, &symbol_short!("XLM_USDC"), &75) {
    return Err(MyError::HighRiskWallet);
}
// ... proceed with the protected action ...
```

### 6.2 Cache with a TTL

Cross-contract calls cost gas. For hot paths, cache the gate result per wallet
for a short window (e.g. a handful of ledgers) and re-query when the cache
expires. Keep the TTL short: a wallet's score can change the moment the
off-chain pipeline submits an update. Caching trades freshness for cost — never
cache a *safe* verdict longer than you are willing to be wrong about it.

### 6.3 Fallback behaviour when `ScoreNotFound`

With `query_risk_gate` the not-found case is already folded into `false`
(fail closed) — no extra handling required. If you call `get_score` directly,
handle `Err(ScoreNotFound)` explicitly and default to your protocol's
fail-closed branch unless you have a deliberate reason to allow unknown wallets
through.

### 6.4 Feature-detect before using newer functions

If your integration depends on, say, aggregate risk, gate the code path on
`supports_interface(symbol_short!("aggr"))` so it degrades gracefully against
an older deployment instead of trapping on a missing function.

---

## 7. Reference material

- Reference integration: [`examples/amm_gate.rs`](../examples/amm_gate.rs)
- Interface stability tests: `contracts/ledgerlens-score/src/test_interface.rs`
- Contract source: `contracts/ledgerlens-score/src/lib.rs`
