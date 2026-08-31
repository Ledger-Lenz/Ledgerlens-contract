# Repository Invariants

This document lists the behaviors in `ledgerlens-score` that are **non-negotiable**: a PR that
weakens one of them needs an explicit design discussion, not just a normal review pass. It
exists because this contract's primary value is being a dependency other protocols embed
directly inside their own guard clauses (see [README.md § Composability](../README.md#composability))
— a subtle regression here doesn't just break this repo, it silently breaks every AMM, lending
market, or aggregator that composes on it.

Each invariant below states the rule, points at exactly where it's implemented, and links the
tests/CI that currently enforce it. Where no such enforcement exists yet, that's called out
explicitly as a gap rather than implied to be covered — per this repo's contribution rules, an
undocumented gap is preferable to a false sense of coverage.

This document doesn't replace the deeper specs it cites — [`docs/interface-versioning-policy.md`](interface-versioning-policy.md),
[`docs/storage-layout.md`](storage-layout.md), [`docs/errors.md`](errors.md) — it's the short,
scannable index a contributor should check *before* touching `lib.rs`.

---

## 1. Fail-closed gates

**Rule:** every function whose job is to answer "is this wallet safe to interact with" must
resolve every uncertain condition to **deny**, never to **allow**. "Uncertain" includes: no
score exists, the wallet is embargoed, it's inside a hysteresis risk band, the primary contract
is paused and no fresh failover data exists, or supplied parameters are out of range.

**Where implemented:** `query_risk_gate_with_confidence` in
[`lib.rs`](../contracts/ledgerlens-score/src/lib.rs) (`query_risk_gate` is a thin wrapper around
it). Reading the function top to bottom, every early return on an uncertain path is `false`:

| Condition | Returns |
|---|---|
| Strict gate enforcement on and caller not allow-listed | `false` |
| `gate_threshold > 100` or `min_confidence > 100` | `false` |
| Contract paused, no failover contract configured | `false` |
| Contract paused, failover configured but its score is missing or older than `FAILOVER_STALENESS_WINDOW` | `false` |
| Wallet embargoed (`peek_is_embargoed`) | `false` |
| Wallet inside a hysteresis risk band (`peek_risk_band_state`) | `false` |
| No score for `(wallet, asset_pair)` and no delegate has one either | `false` |
| Score's `confidence` is below the effective floor (`max(min_confidence, global_min_confidence)`) | `false` |

**Tests:** this is one of the best-covered invariants in the repo —
[`test_embargo.rs`](../contracts/ledgerlens-score/src/test_embargo.rs) (`test_query_risk_gate_false_when_embargoed`,
`test_query_risk_gate_false_when_embargoed_and_no_score`),
[`test_hysteresis.rs`](../contracts/ledgerlens-score/src/test_hysteresis.rs) (`test_query_risk_gate_returns_false_when_in_band_despite_low_score`,
`test_query_risk_gate_no_score_still_conservative`),
[`test_failover.rs`](../contracts/ledgerlens-score/src/test_failover.rs) (`test_gate_returns_false_when_paused_with_no_secondary`,
`test_stale_secondary_score_fails_closed`),
[`test_confidence_gate.rs`](../contracts/ledgerlens-score/src/test_confidence_gate.rs) (`test_confidence_gate_no_score_returns_false`,
`test_confidence_gate_gate_threshold_above_100_returns_false`, `test_confidence_gate_min_confidence_above_100_returns_false`),
[`test_gate_enforcement.rs`](../contracts/ledgerlens-score/src/test_gate_enforcement.rs) (`test_strict_mode_unlisted_caller_returns_false`).

**⚠️ Known exception — `query_risk_gate_relative` does not follow this pattern.** Unlike
`query_risk_gate` / `query_risk_gate_with_confidence` (which return a plain `bool` and are
infallible), `query_risk_gate_relative` returns `Result<bool, Error>` — it can return
`Err(Error::InvalidThreshold)` or propagate `Err(Error::ScoreNotFound)` from
`get_score_percentile`. An integrator who calls `try_query_risk_gate_relative` and doesn't
explicitly treat every `Err` branch as "deny" can accidentally fail *open*. This function is also
absent from [`docs/interface-spec.md`](interface-spec.md)'s formal `ILedgerLensScore` listing and
from the `supports_interface` doc-comment's capability table (though its `rgate` capability
symbol is present in the actual `supports_interface` match arms — see §4's gap note below).
Tested in [`test_histogram.rs`](../contracts/ledgerlens-score/src/test_histogram.rs), but not
flagged anywhere as a deliberately different contract from the other two gates. **This is
documentation of existing behavior, not a proposal to change the signature** — changing it to an
infallible `bool` would itself be a breaking ABI change requiring the 30-day notice process in
`docs/interface-versioning-policy.md`. Flagging as a gap for a follow-up issue: either document
this divergence prominently in `interface-spec.md` and the function's own doc comment, or
(separately, with a migration plan) bring it in line with the other two gates.

---

## 2. No-panic reads

**Rule:** any function reachable by another contract in a read/query context — `get_score`,
`get_score_opt`, `query_risk_gate`, `query_risk_gate_with_confidence`, and the `peek_*` storage
helpers they call — must never panic, for any input. A panic in a cross-contract call traps the
*caller's* transaction; since these functions exist specifically to be called from inside another
protocol's guard clause, a crafted input that panics one of them is a denial-of-service primitive
against every integrator, not just LedgerLens itself. (This is distinct from `require_auth()`
panics on state-*mutating* admin/service functions — those are Soroban's own, intentional
auth-failure mechanism, not a violation of this invariant. The gate/read functions in scope here
take no `admin_signers`/`service` parameter and call no `require_auth()` at all — they are
deliberately permissionless.)

**Where implemented:**
- `get_score` returns `Result<RiskScore, Error>` via `.ok_or(Error::ScoreNotFound)` — no `unwrap`/`expect`/`panic!`.
- `peek_score`, `peek_score_delegate`, `peek_is_embargoed` in [`storage.rs`](../contracts/ledgerlens-score/src/storage.rs)
  contain no `unwrap`/`expect`/`panic!`.
- `peek_risk_band_state` uses `.unwrap_or(false)` — a safe default, not a panic risk.
- `query_risk_gate_with_confidence` itself contains no `unwrap`/`expect`/`panic!` in its body.

**Tests:** [`error_coverage.rs`](../contracts/ledgerlens-score/src/error_coverage.rs) exercises
every declared error path (see §4), which indirectly proves those paths return typed errors
rather than panicking. There is **no dedicated adversarial/fuzz test for the read/gate path**
analogous to [`test_fuzz_submit_score.rs`](../contracts/ledgerlens-score/src/test_fuzz_submit_score.rs),
which runs thousands of deterministically-seeded inputs against `submit_score` specifically. Given
the gate functions are the primary cross-contract attack surface, **this is a real coverage gap**:
recommend a `test_fuzz_query_risk_gate.rs` modeled on the existing fuzz harness, covering
combinations of `gate_threshold`/`min_confidence` at and beyond their `[0, 100]` bounds, paused +
failover on/off, embargoed + non-embargoed, and hysteresis-band + cleared states. Tracked here as
a gap rather than added in this PR, since a fuzz suite of that scope is its own focused change,
not a byproduct of a documentation pass.

---

## 3. Bounded storage

**Rule:** no contract collection may grow without a hard, enforced ceiling. Every list, set, or
index that grows from user/service/admin action has a `MAX_*` constant, and the write path that
would exceed it returns a typed `Error` instead of silently succeeding.

**Where implemented — [`constants.rs`](../contracts/ledgerlens-score/src/constants.rs) is the
single source of truth for every cap:**

| Constant | Value | Guards |
|---|---:|---|
| `MAX_SERVICE_SIGNERS` | 10 | `Error::ServiceSetFull` |
| `MAX_ADMIN_SIGNERS` | 5 | `Error::AdminSetFull` |
| `MAX_GATE_CALLERS` | 20 | gate caller allow-list |
| `MAX_BATCH_SIZE` | 20 | `Error::BatchTooLarge` (`submit_scores_batch`) |
| `BATCH_READ_MAX` | 50 | batch score reads |
| `MAX_HISTORY_DEPTH` | 50 | ring-buffer cap on per-wallet score history |
| `MAX_PAUSED_PAIRS` | 50 | `Error::PausedPairIndexFull` (alias of `ServiceSetFull`) |
| `MAX_COUNTERPARTY_LINKS_PER_WALLET` | 50 | `Error::CounterpartyLinkFull` |
| `MAX_DELEGATION_DEPTH` | 5 | bounds delegate-chain traversal (DoS via circular/deep delegation) |
| `MAX_EMBARGOED_WALLETS` | 100 | keeps `revoke_all_embargoes` inside one tx's resource budget |
| `MAX_MODEL_VERSIONS` | 20 | `Error::ModelVersionRegistryFull` |
| `MAX_OPEN_DISPUTES` | 100 | `Error::DisputeIndexFull` |
| `MAX_DISPUTES_PER_ACTOR` | 5 | `Error::ActorDisputeLimitExceeded` |
| `MAX_TRACKED_SCORE_ENTRIES` / `MAX_EXPIRING_ENTRIES_PER_CALL` | 500 / 100 | bounds per-call TTL-sweep work |
| `MAX_PENDING_PARAMETER_PROPOSALS` | 10 | `Error::TooManyPendingParameterProposals` |
| `MAX_RATE_LIMIT_OVERRIDE_LOG` | 100 | rate-limit override audit log |
| `MAX_MERKLE_PROOF_DEPTH` | 30 | rejects oversized Merkle proofs before verification |
| `MAX_WALLET_PAIRS` | 20 | bulk-operation input size |

**Rent/TTL bounding:** all persistent and temporary keys use explicit `(threshold, extend_to)`
TTL pairs rather than indefinite retention — fully catalogued per-key in
[`docs/storage-layout.md`](storage-layout.md). That document also explains why gate/read paths use
the `peek_*` no-TTL-extension variants (§2 above) — bounding *write footprint*, not just entry
count, on the hot read path.

**Resource usage — worst case:** [`docs/wasm-size-budget.md`](wasm-size-budget.md) tracks binary
size (`./scripts/wasm-size-report.sh`); `MAX_EMBARGOED_WALLETS` / `MAX_TRACKED_SCORE_ENTRIES` /
`MAX_EXPIRING_ENTRIES_PER_CALL` exist specifically to bound the *execution* cost of the bulk
operations that iterate them (`revoke_all_embargoes`, TTL-sweep) to a single transaction's budget
— see the doc comments at each constant's definition in `constants.rs` for the specific operation
each one bounds. There is currently no dedicated benchmark asserting the worst case (e.g. an
`MAX_OPEN_DISPUTES`-sized dispute index, or a full `MAX_HISTORY_DEPTH` ring buffer) stays under a
specific CPU-instruction budget — `contracts/ledgerlens-score/benches/` has benchmarks for some
hot paths but not an exhaustive one per cap. Flagging as a gap: a benchmark suite that fills each
bounded collection to its `MAX_*` and asserts the operation's `env.budget()` cost stays within a
committed ceiling would make "bounded" verifiable rather than just structurally true.

**Tests:** cap-enforcement is covered by dedicated cases in
[`error_coverage.rs`](../contracts/ledgerlens-score/src/error_coverage.rs) for every `*Full`/`*TooLarge`
error variant (one test per variant, see §4), plus targeted suites such as
[`test_bulk_signer_tier.rs`](../contracts/ledgerlens-score/src/test_bulk_signer_tier.rs),
[`test_batch_watchlist.rs`](../contracts/ledgerlens-score/src/test_batch_watchlist.rs), and
[`test_ttl_rent_manager.rs`](../contracts/ledgerlens-score/src/test_ttl_rent_manager.rs).

---

## 4. Append-only event/error stability

**Rule:** once an error discriminant or an event's topic shape ships to `mainnet`, it is part of
the deployed contract's binary ABI. Existing discriminants and topic layouts may never be
renumbered, renamed, reordered, or removed — only appended to.

### 4a. Errors

**Where implemented:** [`errors.rs`](../contracts/ledgerlens-score/src/errors.rs)'s `Error` enum
is hard-capped at 50 variants (a Soroban XDR spec limit). Because of that ceiling, new *semantic*
error names are added as `pub const` aliases inside `impl Error` that map to an **existing**
discriminant (e.g. `pub const PairPaused: Error = Error::ContractPaused;`) rather than as new enum
variants — this is the established, deliberate pattern for adding meaning without consuming the
remaining discriminant budget or touching a value already in use.

**Enforced by, in order of how early a violation is caught:**
1. **CI, at PR time:** [`tools/check_error_discriminants.sh`](../tools/check_error_discriminants.sh),
   run by the `error-discriminants` job in [`.github/workflows/ci.yml`](../.github/workflows/ci.yml).
   It diffs `errors.rs`'s enum body between the PR base and head ref and fails the build if any
   existing discriminant was renumbered, renamed, or removed. New discriminants and new aliases
   always pass.
2. **Test suite:** [`error_coverage.rs`](../contracts/ledgerlens-score/src/error_coverage.rs) has
   exactly one dedicated regression test per `Error` variant (50 tests for 50 variants, one
   `#[test] fn test_error_<name>()` each) asserting the exact code is returned from the real
   trigger condition — not just that the variant exists.
3. **Docs lock:** the same file's `test_errors_md_lock_against_discriminants` test parses
   [`docs/errors.md`](errors.md) and `errors.rs` and asserts every documented code matches the
   real discriminant, so the human-readable reference can't silently drift from the source of
   truth.

This is the most rigorously enforced invariant in the repo — three independent layers, two of
which run in CI. Use it as the template if extending equivalent coverage to events (next).

### 4b. Events

**Where implemented:** [`events.rs`](../contracts/ledgerlens-score/src/events.rs) defines
`EVENT_VERSION: u32 = 1`, documented as: append a field to a payload without bumping it; bump it
if you change a field's meaning, order, or remove one. Every event-emitting function publishes
`EVENT_VERSION` as the second element of its topic tuple, e.g.
`(symbol_short!("score"), EVENT_VERSION, wallet.clone(), asset_pair.clone())`.

**Enforced by:** [`event_emission.rs`](../contracts/ledgerlens-score/src/event_emission.rs)'s
`test_all_events_carry_schema_version`, which asserts every event captured during the test carries
`EVENT_VERSION` at topic index 1.

**⚠️ Known gap, found while writing this document — `pair_weight_reset` violates the rule it's
supposed to be tested against.** Compare in `events.rs`:

```rust
pub fn pair_weight_updated(env: &Env, asset_pair: &Symbol, weight: u32) {
    env.events().publish((symbol_short!("pw_upd"), EVENT_VERSION, asset_pair.clone()), weight);
}

pub fn pair_weight_reset(env: &Env, asset_pair: &Symbol) {
    env.events().publish((symbol_short!("pw_rst"), asset_pair.clone()), ());
}
```

`pair_weight_reset`'s topic tuple has 2 elements (name, pair) where every sibling event has 3
(name, `EVENT_VERSION`, ...). This went undetected because
`test_all_events_carry_schema_version` only calls `initialize` and `set_watchlist` — it never
triggers `bulk_reset_pair_weight` (the only caller of `pair_weight_reset`), so the assertion loop
never sees this event. **The invariant is real and tested; the test's *coverage* of which events
it actually observes is the gap.**

A regression test proving this (`test_pair_weight_reset_missing_version_topic` in
`event_emission.rs`, added in this PR) calls `set_pair_weight` then `bulk_reset_pair_weight` and
asserts the resulting `pw_rst` event carries `EVENT_VERSION` at topic index 1 — **this test is
expected to fail against the current `pair_weight_reset` implementation**, which is the point: it
turns a discovered violation into a tracked, deterministic regression rather than a paragraph
someone has to take on faith.

**Deliberately not fixed in this PR.** Adding a topic to `pair_weight_reset` changes its shape
from `(name, pair)` to `(name, EVENT_VERSION, pair)` — anything decoding it by position today
would break. That's exactly the class of change `docs/interface-versioning-policy.md` requires a
`CHANGELOG.md` migration entry and a 30-day notice period for. Recommend a dedicated follow-up
issue scoped to: fix `pair_weight_reset`, decide whether the fix is silent (patch, since this
specific event realistically has near-zero existing integrators) or goes through the full notice
process, and broaden `test_all_events_carry_schema_version` to invoke every event-emitting
function so this class of gap can't recur silently for the *next* new event either.

**No CI-level diff check exists for event topic shape**, unlike §4a's `check_error_discriminants.sh`
for errors. Flagging as a second, related gap: an analogous script (or an extension of the
existing one) that fails a PR if an event function's topic tuple arity or field order changes
relative to the base ref would close this at the same layer errors are already protected at.

### 4c. Interface / capability registry

**Where implemented:** `supports_interface` in [`lib.rs`](../contracts/ledgerlens-score/src/lib.rs)
is documented in [`docs/interface-versioning-policy.md`](interface-versioning-policy.md) as an
append-only capability registry — once a symbol is published, it's never removed or repurposed.

**⚠️ Known gap, found while writing this document.** The `supports_interface` function's own
match arms recognise 16 capability symbols (`score`, `history`, `hpag`, `batch`, `gate`, `aggr`,
`count`, `var`, `batch_attested`, `cgate`, `histogram`, `rgate`, `emb`, `cons`, `pr_rd`, `dprv`),
but the doc-comment table directly above it lists only 11 — `hpag`, `var`, `histogram`, `rgate`,
and `dprv` are live, callable capabilities with no documented meaning anywhere in `lib.rs`,
[`docs/interface-spec.md`](interface-spec.md), or the README. An integrator has no way to look up
what those five actually gate. This doesn't violate the append-only *stability* rule (nothing was
removed or renumbered), but it undermines the stated purpose of `supports_interface` — letting
integrators feature-detect — for exactly the newest five capabilities. Recommend a focused
follow-up issue to document all five (cross-referencing the functions each one maps to) in both
the in-code table and `interface-spec.md`; out of scope here since it requires figuring out and
verifying intent for five separate capabilities, not a documentation-format fix.

---

## Summary: gaps found while writing this document

None of the following weaken an invariant that's currently relied upon — they're places where
the *documentation or coverage* of an already-real rule has drifted. Recommend each become its
own tracked follow-up issue rather than being folded into future unrelated PRs:

1. `query_risk_gate_relative` is fallible (`Result<bool, Error>`) unlike its two sibling gates,
   isn't in `interface-spec.md`, and isn't flagged as an exception anywhere (§1).
2. No fuzz/adversarial test suite targets the read/gate path, unlike `submit_score` (§2).
3. No benchmark asserts worst-case resource cost when a bounded collection is filled to its
   `MAX_*` cap (§3).
4. `pair_weight_reset` doesn't carry `EVENT_VERSION`, contradicting the invariant §4b describes —
   now covered by a deliberately-failing regression test added in this PR.
5. There's no CI diff-check for event topic-shape stability, unlike the one that exists for error
   discriminants (§4b).
6. Five `supports_interface` capability symbols (`hpag`, `var`, `histogram`, `rgate`, `dprv`) are
   live but undocumented in both the in-code table and `docs/interface-spec.md` (§4c).
