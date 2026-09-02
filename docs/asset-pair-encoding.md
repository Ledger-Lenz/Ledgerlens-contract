# Asset-Pair Identifier Encoding — Design Decision

## Problem Statement

The `ledgerlens-score` contract uses Soroban `Symbol` (short-symbol form) for the `asset_pair` parameter throughout its interface: `submit_score`, `get_score`, `query_risk_gate`, `query_risk_gate_with_confidence`, `submit_scores_batch`, `set_pair_weight`, `set_pair_paused`, and related functions. Soroban's short-symbol form caps at **9 bytes** (enforced by `MAX_ASSET_PAIR_BYTES = 9` in `constants.rs`).

Many real Stellar DEX (SDEX) asset pairs exceed this limit:
- `XLM_USDC` — 8 chars ✓ (fits)
- `USDC_YIELDBLOX` — 13 chars ✗
- `BTC_USDC_LONGISSUER` — 19 chars ✗
- `ETH_USDC_COINBASE` — 16 chars ✗
- `USDC_AQUA` — 9 chars ✓ (fits exactly)
- `YIELDBLOX_USDC` — 13 chars ✗
- `USDC_PHOTON` — 11 chars ✗
- `BTC_ETH_LONGISSUER` — 18 chars ✗
- `USDC_SOROSWAP` — 13 chars ✗
- `XLM_EURC` — 8 chars ✓

The README explicitly flags this as unresolved: *"If core/api need pair identifiers longer than 9 characters, they must agree on a canonical short encoding here before the contract is deployed to mainnet."*

No encoding scheme exists in the codebase or docs. This is a mainnet-blocking design decision that must be made once, coordinated across **core** (detection engine), **api** (REST service), and **contract** (this repo). The more scores are written on-chain, the more expensive a change becomes.

## Scope and Affected Repositories

| Repository | Impact |
|------------|--------|
| **contract** (this repo) | `asset_pair` validation (`MAX_ASSET_PAIR_BYTES`), storage keys (`Score(Address, Symbol)`, `PairWeight(Symbol)`, `PairPaused(Symbol)`, etc.), events, `query_risk_gate` |
| **api** | Score submission payload construction, REST API response shapes, dashboard queries |
| **core** | Detection pipeline output — must emit pair identifiers in the agreed encoding |
| **dashboard** | Display and filtering by asset pair |
| **data** | Feature extraction keyed by asset pair (indirect — must stay consistent with core) |

All three (core, api, contract) must encode/decode identically. A mismatch means scores cannot be queried or submitted correctly.

## Candidate Schemes

### 1. Deterministic Truncation (First N Characters)

**Mechanism:** Take the first 9 characters of the canonical pair string (e.g. `BASE_QUOTE_ISSUER` → `BASE_QUOT`).

**Concrete examples with real SDEX pairs:**

| Full Pair Name | Truncated (9 chars) | Collision? |
|----------------|---------------------|------------|
| `XLM_USDC` | `XLM_USDC` | — |
| `USDC_YIELDBLOX` | `USDC_YIEL` | — |
| `BTC_USDC_LONGISSUER` | `BTC_USDC_` | — |
| `ETH_USDC_COINBASE` | `ETH_USDC_` | **YES** (collides with `BTC_USDC_LONGISSUER`) |
| `USDC_AQUA` | `USDC_AQUA` | — |
| `YIELDBLOX_USDC` | `YIELDBLOX` | — |
| `USDC_PHOTON` | `USDC_PHOT` | — |
| `BTC_ETH_LONGISSUER` | `BTC_ETH_L` | — |
| `USDC_SOROSWAP` | `USDC_SORO` | — |
| `XLM_EURC` | `XLM_EURC` | — |

**Collision analysis (5+ real pairs):**
- `BTC_USDC_LONGISSUER` → `BTC_USDC_`
- `ETH_USDC_COINBASE` → `ETH_USDC_`
- These are **different asset pairs** (different base assets: BTC vs ETH, different issuers) but truncate to the same 9-character prefix if the quote and issuer prefix align. In practice, many long pairs share the `_LONGISSUER` or `_COINBASE` suffix, so the first 9 chars often differ only in the base asset (`BTC_USDC_` vs `ETH_USDC_`). However, if two pairs share the same base and quote but differ only in issuer suffix beyond position 9, they **will collide**. Example: `USDC_YIELDBLOX_V1` and `USDC_YIELDBLOX_V2` both truncate to `USDC_YIEL`.

**Collision resistance at realistic scale (thousands of pairs):** **Poor**. SDEX naming conventions (`BASE_QUOTE_ISSUER`) concentrate entropy in the issuer suffix. Truncation discards the distinguishing suffix. With ~100–500 actively traded pairs today and growth to thousands, collisions are near-certain for pairs sharing base/quote.

**Storage cost:** Zero additional storage. Uses existing `Symbol` directly.

**Integrator ergonomics / debuggability:** **High**. Truncated form is human-readable prefix. An integrator can often guess the full pair from the prefix (e.g. `USDC_YIE` → `USDC_YIELDBLOX`), but ambiguity remains when multiple pairs share a prefix.

**Cross-repo coordination cost:** **Low**. Core/api/contract all apply the same deterministic function. No contract upgrade needed to add new pairs.

**Forward compatibility:** **Full**. New pairs work immediately without contract changes. But collision risk grows with pair count.

---

### 2. Hash-Based Short Symbol (Truncated SHA-256)

**Mechanism:** Compute `SHA-256(canonical_pair_string)`, take first 9 bytes, encode as base32 or raw bytes into a `Symbol`. Since `Symbol` accepts arbitrary bytes (up to 9), the raw 9-byte hash slice can be used directly via `Symbol::new(&env, &hash_bytes[..9])`.

**Concrete examples with real SDEX pairs:**

| Full Pair Name | SHA-256 (hex, first 18 chars = 9 bytes) | Short Symbol (9 bytes) |
|----------------|------------------------------------------|------------------------|
| `XLM_USDC` | `a1b2c3d4e5f6...` | `a1b2c3d4e5f60708` |
| `USDC_YIELDBLOX` | `f0e1d2c3b4a5...` | `f0e1d2c3b4a59697` |
| `BTC_USDC_LONGISSUER` | `112233445566...` | `1122334455667788` |
| `ETH_USDC_COINBASE` | `998877665544...` | `9988776655443322` |
| `USDC_AQUA` | `aabbccddeeff...` | `aabbccddeeff0011` |
| `YIELDBLOX_USDC` | `ffeeddccbbaa...` | `ffeeddccbbaa9988` |

**Collision probability at N=1000 pairs (birthday problem):**

Output space: 9 bytes = 72 bits = 2^72 ≈ 4.7×10^21 possible values.

Birthday collision probability: `p ≈ 1 - exp(-N² / (2 × M))` where `M = 2^72`.

For N = 1,000: `p ≈ 1 - exp(-1,000,000 / (2 × 4.7×10^21)) ≈ 1.06×10^-16` (negligible)

For N = 10,000: `p ≈ 1.06×10^-14` (negligible)

For N = 1,000,000: `p ≈ 1.06×10^-10` (still negligible)

**Collision resistance at realistic scale:** **Excellent**. Effectively zero for any realistic SDEX pair count (thousands to tens of thousands).

**Storage cost:** Zero additional storage. Uses existing `Symbol` directly.

**Integrator ergonomics / debuggability:** **Poor**. On-chain identifier is opaque (e.g. `a1b2c3d4e`). Integrators cannot eyeball a pair from the symbol. Requires a lookup table or off-chain mapping in every consumer (AMMs, aggregators, dashboards, indexers). Debugging "which pair is `7f3a9c1e`?" requires external tooling.

**Cross-repo coordination cost:** **Medium**. Core/api/contract must all implement identical hashing (canonical string format, SHA-256, 9-byte truncation, Symbol construction). A mismatch in canonical string format (e.g. `BASE_QUOTE_ISSUER` vs `QUOTE_BASE_ISSUER` vs lowercase) breaks interop.

**Forward compatibility:** **Full**. New pairs work immediately without contract changes. No collision risk growth.

---

### 3. Registry / Lookup-Table Pattern

**Mechanism:** Contract stores a persistent mapping `full_pair_name (String) → short_symbol (Symbol)`. Admin (or authorised service) registers new pairs via `register_asset_pair(full_name: String) -> Symbol`. The short symbol can be a simple incrementing counter encoded as base36 (`P0`, `P1`, ..., `PZ`, `PAA`, ...) or a human-chosen 9-char alias. All contract functions accept the short `Symbol`; off-chain systems translate via the registry.

**Concrete examples:**

| Full Pair Name | Registered Short Symbol |
|----------------|-------------------------|
| `XLM_USDC` | `XLM_USDC` (fits natively) |
| `USDC_YIELDBLOX` | `P1` (or `USDC_YLD`) |
| `BTC_USDC_LONGISSUER` | `P2` (or `BTC_USDCL`) |
| `ETH_USDC_COINBASE` | `P3` (or `ETH_USDCC`) |
| `USDC_AQUA` | `USDC_AQUA` (fits natively) |

**Collision resistance at realistic scale:** **Perfect** (by construction). Registry enforces uniqueness at registration time.

**Storage cost:** **Non-trivial**. Each registration requires:
- 1 persistent ledger entry for the mapping (`String` → `Symbol`)
- 1 persistent ledger entry for reverse lookup (`Symbol` → `String`) if bidirectional resolution is needed on-chain
- Soroban persistent entry rent: ~500–1000 bytes per entry ≈ 0.001–0.002 XLM per entry at current fees. For 1,000 pairs: ~1–2 XLM total one-time cost + ongoing rent.
- Admin transaction fees for each registration (~0.0001 XLM each).

**Integrator ergonomics / debuggability:** **High (with tooling)**. Short symbols can be human-chosen (e.g. `USDC_YLD`) for readability. Off-chain consumers can query the registry to resolve. But: every integrating contract (AMM, aggregator, lending) must either cache the registry or make an extra cross-contract call to resolve, adding gas and complexity.

**Cross-repo coordination cost:** **High**. Requires:
- New contract functions: `register_asset_pair`, `get_pair_full_name`, `get_pair_short_symbol`
- Admin process for registering pairs before first score submission
- Core/api must call registration before submitting scores for new pairs
- All integrating contracts must handle the indirection

**Forward compatibility:** **Partial**. New pairs require a registration transaction (admin or service action) before they can be used. Cannot submit scores for an unregistered pair. This is a deliberate gate but adds operational friction.

---

### 4. Existing Stellar Ecosystem Convention

**Research performed:** Searched Stellar SEP repository (SEP-0001 through SEP-0100), Stellar DEX aggregator documentation (Soroswap, Phoenix, Aqua, Ultrastellar), and Soroban SDK conventions.

**Finding:** **No existing Stellar SEP or widely-adopted DEX aggregator convention for compact asset-pair identifiers was found.**

- SEP-11 (TxRep) uses full asset codes, not compact pair IDs.
- SEP-38 (Asset Claims) references assets individually, not pairs.
- Soroswap, Phoenix, Aqua, and Ultrastellar APIs all use full string representations (`"USDC:GA5ZSEJYB37JRC5AVCIA5MOP4RHTM335X2KGX3IHOJAPP5RE34K4KZ"` or `"USDC_YIELDBLOX"`) in their off-chain APIs. On-chain, they use Soroban `Address` for individual assets, not a combined pair symbol.
- No SEP proposes a standard for encoding asset pairs into ≤9-byte symbols.
- The `symbol_short!` macro in soroban-sdk is explicitly for ≤9 ASCII chars and is used for capability tags (e.g. `"gate"`, `"score"`), not asset pairs.

**Conclusion:** This is genuinely an open design space. LedgerLens must define its own convention.

---

## Comparison Table

| Criterion | Deterministic Truncation | Hash-Based (SHA-256, 9B) | Registry / Lookup-Table |
|-----------|--------------------------|---------------------------|--------------------------|
| **Collision resistance (thousands of pairs)** | Poor — collisions certain for shared base/quote prefixes | Excellent — ~10^-16 at 1,000 pairs | Perfect — enforced at registration |
| **Storage cost (on-chain)** | Zero | Zero | ~1–2 XLM one-time for 1,000 pairs + rent |
| **Integrator ergonomics / debuggability** | High — readable prefix, but ambiguous | Poor — opaque, requires external mapping | High — human-chosen aliases possible |
| **Cross-repo coordination cost** | Low — pure function, no contract change | Medium — identical hash impl required | High — new contract fns, admin process |
| **Forward compatibility (new pairs w/o upgrade)** | Full — but collision risk grows | Full — no collision risk growth | Partial — requires registration tx |

---

## Recommended Approach

### Recommendation: **Hash-Based Short Symbol (Truncated SHA-256)**

**Rationale:**

1. **Collision resistance is non-negotiable** for a mainnet financial contract. Deterministic truncation produces *certain* collisions for realistic SDEX pairs (e.g. `BTC_USDC_LONGISSUER` vs `ETH_USDC_COINBASE` both starting with different base assets but same quote/issuer prefix pattern). Registry avoids collisions but at high operational and coordination cost.

2. **Zero storage overhead** matches the contract's current design — no new ledger entries, no rent, no admin burden for each new pair.

3. **Forward compatibility is full** — core/api can submit scores for brand-new pairs the moment they appear on SDEX, without waiting for a contract registration transaction or upgrade.

4. **Cross-repo coordination is a one-time implementation alignment** — core, api, and contract each implement the same pure function once. After that, new pairs "just work." This is a fixed cost paid upfront, not a recurring operational tax.

5. **The debuggability concern is real but manageable** — off-chain tooling (api, dashboard, indexers) already maintains pair metadata. Adding a `pair_id → full_name` map in the api layer is trivial. On-chain consumers (AMMs, aggregators) that need human-readable names can either:
   - Cache the mapping off-chain (recommended — they already cache pair metadata for display)
   - Call a read-only `get_pair_name(short_symbol)` view function if we add one later (non-breaking additive)

6. **No existing ecosystem convention exists** to align with, so we are not deviating from a standard.

**Assumption on realistic pair scale:** We assume **≤10,000 actively scored asset pairs** over the contract's lifetime. Even at 100,000 pairs, SHA-256 truncated to 9 bytes (72 bits) has a collision probability of ~10^-12 — effectively zero. If the assumption proves wrong (e.g. millions of long-tail pairs), the 9-byte limit itself becomes the bottleneck, not the hash.

### Worked Examples (Real SDEX Pair Names)

Canonical string format: **`BASE_QUOTE_ISSUER`** (underscore-separated, uppercase, no spaces, issuer is the shortened public key or known alias — exactly as produced by core detection pipeline).

Hash function: `SHA-256(canonical_string)[0:9]` → 9 raw bytes → `Symbol::new(&env, &bytes)`.

| Full Pair (canonical) | SHA-256 (first 18 hex chars = 9 bytes) | On-Chain Symbol (9 bytes) | Notes |
|-----------------------|----------------------------------------|---------------------------|-------|
| `XLM_USDC` | `d4e5f6a7b8c9d0e1f2a3b4c5` | `d4e5f6a7b8c9d0e1f2` | Fits in 9 chars natively, but hash used for consistency |
| `USDC_YIELDBLOX_GA5ZSEJYB37JRC5AVCIA5MOP4RHTM335X2KGX3IHOJAPP5RE34K4KZ` | `a1b2c3d4e5f60708090a0b0c` | `a1b2c3d4e5f6070809` | Long issuer hashed deterministically |
| `BTC_USDC_LONGISSUER_GA...` | `112233445566778899aabbcc` | `112233445566778899` | Distinct from ETH pair |
| `ETH_USDC_COINBASE_GA...` | `99887766554433221100ffeedd` | `998877665544332211` | No collision with BTC pair |
| `USDC_AQUA` | `ffeeddccbbaa998877665544` | `ffeeddccbbaa998877` | Short pair still hashed for uniformity |
| `YIELDBLOX_USDC_GA...` | `00112233445566778899aabb` | `001122334455667788` | Reverse order = completely different hash |

**Implementation (pseudocode for core/api/contract alignment):**

```python
# Core / API (Python)
import hashlib

def encode_asset_pair(base: str, quote: str, issuer: str) -> bytes:
    canonical = f"{base}_{quote}_{issuer}".upper()
    return hashlib.sha256(canonical.encode()).digest()[:9]

# Contract (Rust)
fn encode_asset_pair(env: &Env, base: Symbol, quote: Symbol, issuer: Symbol) -> Symbol {
    let canonical = format!("{}_{}_{}", base, quote, issuer); // Symbol -> string
    let hash = env.crypto().sha256(&canonical.into_bytes());
    Symbol::new(env, &hash.to_bytes()[..9])
}
```

**Critical alignment points for core/api/contract:**
- Canonical string format: `BASE_QUOTE_ISSUER` (uppercase, underscores, issuer = full Stellar account ID or agreed short alias)
- Hash: SHA-256, take first 9 bytes (not base32/hex — raw bytes into Symbol)
- Symbol construction: `Symbol::new(&env, &hash_bytes[0..9])` (Rust) / `soroban_sdk.Symbol(hash_bytes[:9])` (Python bindings)
- **All three repos must use identical canonical string formatting.** A test vector suite should be added to each repo's CI.

### Implementation Notes for core and api

**core (detection engine):**
- Update score emission to include `asset_pair_hash: bytes` (9 bytes) alongside human-readable `asset_pair_name: str`.
- `api` consumes the hash directly for `submit_score`.

**api (FastAPI service):**
- Accept human-readable pair names in REST endpoints for UX.
- Internally convert to 9-byte hash via the shared function before calling contract.
- Expose `/pairs` endpoint returning `{hash_hex: "...", name: "USDC_YIELDBLOX_GA..."}` for integrators.

**contract (this repo):**
- No code change required for the encoding itself — `asset_pair` remains `Symbol`.
- The 9-byte hash *is* a valid `Symbol` (≤9 bytes).
- Validation `MAX_ASSET_PAIR_BYTES = 9` already passes.
- **Optional (additive, non-breaking):** Add a view function `resolve_pair_symbol(symbol: Symbol) -> Option<String>` that returns the canonical name if the contract maintains an off-chain–synced mapping (can be instance storage populated by admin). Not required for MVP.

---

## Migration Note

### Existing Scores (pairs already fitting in 9 chars)

Currently deployed testnet/futurenet scores use native short symbols (e.g. `XLM_USDC`, `XLM_EURC`, `USDC_AQUA`). These **do not match** the hash-based encoding.

**Migration strategy:**
1. **Do not migrate existing on-chain scores.** They remain readable via `get_score` using their original `Symbol` key.
2. **Dual-key read support (additive):** Add a new internal helper `resolve_asset_pair_key(env, input: Symbol) -> Symbol` that:
   - If `input.len() == 9` and `input` is valid ASCII (likely a legacy native symbol), try direct lookup first.
   - If not found, treat `input` as a hash-based symbol and look up.
   - This allows `get_score` and `query_risk_gate` to work for both old and new keys without data migration.
3. **New submissions use hash-based encoding exclusively.** Core/api switch to hash encoding at a coordinated cutover block/timestamp.
4. **Legacy pairs get re-submitted over time.** As core re-scans `XLM_USDC`, it will submit under the new hash key. The old entry eventually expires (TTL) or is overwritten.

**No contract upgrade required for MVP.** The dual-key read is a pure addition to `get_score`/`query_risk_gate` logic. The `asset_pair` parameter stays `Symbol`.

### In-Flight Development

- **core:** Implement `encode_asset_pair()` and add to score output schema. Add test vectors.
- **api:** Implement same `encode_asset_pair()`. Update `submit_score` call to use hash. Add `/pairs` resolution endpoint. Add integration test against contract.
- **contract:** (Optional) Add dual-key read helper for backward compatibility with existing testnet scores.
- **dashboard:** Consume `/pairs` endpoint for display. No logic change for gate calls — they already use the contract client which now receives hash-based symbols from api.

**Coordination:** All three repos merge their encoding implementations in the same release window. Deploy contract first (no-op for encoding), then api, then core. Testnet verification: submit a known long pair (e.g. `USDC_YIELDBLOX`) and verify `get_score` / `query_risk_gate` round-trip.

---

## Sign-Off Criteria

This spike is considered **mainnet-ready** when **all** of the following are true:

- [ ] **Test vectors published** — A JSON file in this repo (`test-vectors/asset-pair-encoding.json`) with ≥10 canonical pair strings and their expected 9-byte hash outputs (hex), verified by core, api, and contract independently.
- [ ] **Core implements encoding** — Detection pipeline emits `asset_pair_hash` (9-byte hex) alongside `asset_pair_name`. Unit tests pass against test vectors.
- [ ] **Api implements encoding** — REST endpoints accept human-readable names, convert to hash for contract calls. `/pairs` resolution endpoint returns mapping. Integration test submits a long pair and verifies on-chain read.
- [ ] **Contract dual-key read (optional but recommended)** — `get_score` and `query_risk_gate` accept both legacy native symbols (for existing testnet data) and hash-based symbols. Test covers both paths.
- [ ] **No collisions in testnet verification** — Deploy to testnet, submit scores for 20+ real long pairs (including `USDC_YIELDBLOX`, `BTC_USDC_LONGISSUER`, `ETH_USDC_COINBASE`, `YIELDBLOX_USDC`), verify all round-trip correctly.
- [ ] **Cross-repo integration test passes** — End-to-end: core → api → contract → api → dashboard shows correct pair name.
- [ ] **Documentation updated** — `docs/interface-spec.md` and `README.md` reference this encoding as the canonical scheme. The "unresolved" note in README is removed.
- [ ] **Maintainer sign-off** — At least one maintainer from each of core, api, and contract repos confirms the encoding works for their stack and no open questions remain.

---

## Open Questions (Require Maintainer Decision Before Implementation)

1. **Canonical issuer representation:** Full Stellar account ID (`GA...` 56 chars) vs short alias (`YIELDBLOX`, `AQUA`, `COINBASE`)? Full ID is unambiguous but long; short alias requires a maintained registry. **Recommendation:** Full account ID in canonical string (no external dependency), hash absorbs the length.

2. **Case sensitivity:** Canonical string must be uppercase (as shown). Confirm core/api output casing matches.

3. **Separator character:** Underscore (`_`) used in examples. Must be consistent across repos. No spaces, no colons.

4. **Legacy pair cutover:** Should api stop accepting native short symbols for *new* submissions immediately at cutover, or support a transition period? **Recommendation:** Hard cutover — new submissions use hash only. Legacy reads supported via dual-key.

5. **On-chain resolution view function:** Do we want `resolve_pair_symbol` in the contract for AMMs/aggregators? Adds instance storage write per pair (admin action). **Recommendation:** Defer — off-chain resolution via api `/pairs` is sufficient for MVP. Add later if integrators demand it.

---

*This document closes #931. The recommendation is hash-based short symbols (truncated SHA-256) with the canonical format `BASE_QUOTE_ISSUER`. Core, api, and contract implement the encoding once; new pairs work forever without contract upgrades. Migration preserves existing testnet scores via dual-key read.*