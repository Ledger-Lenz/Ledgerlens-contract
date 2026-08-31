# Glossary

Precise definitions for terms used throughout this repo's code, docs, and README — written
against the actual implementation, not the general/aspirational meaning a term might carry
elsewhere in Web3 or fraud-detection contexts. Several terms below (**shard**, **finality**,
**attestation**, **pause**) are flagged specifically because their meaning *here* is narrower or
different from what a reader might assume from general blockchain usage — that gap is exactly
what causes the ambiguity this document exists to remove.

Entries are alphabetical. Each one cites its source-of-truth file so this glossary can't silently
drift from the code the way a purely prose description could.

---

### Admin

The address (or, under M-of-N multisig, a threshold of an admin signer set) authorised to change
contract configuration — pausing, thresholds, signer sets, upgrades, model version lifecycle. Set
at `initialize` and rotatable via a two-step transfer (`transfer_admin` / `accept_admin_transfer`
/ `cancel_admin_transfer`). Distinct from the **service** account, which submits scores but cannot
change configuration. See [README.md § Contract Functions](../README.md#contract-functions),
[`docs/governance.md`](governance.md).

### Aggregate score

The cross-asset risk summary for a wallet, computed **live** (never cached) as a weighted average
of every `RiskScore` the wallet has across all asset pairs, via `get_aggregate_score`. Pair
weights default to `1` and are configurable per-pair via `set_pair_weight`; a weight of `0`
excludes that pair from the aggregate entirely. Not to be confused with **consensus score**
(§ Consensus), which combines *multiple submitters'* view of a *single* wallet/pair, not one
wallet's view across pairs.

### Attestation

An umbrella term covering **three distinct, related mechanisms** — check which one a given doc or
function is actually about before assuming:

1. **Score attestation** (`ScoreAttestation`) — a single secp256k1 signature over one score
   payload, submitted alongside `submit_score`. Proves the payload wasn't altered in transit
   between the off-chain pipeline and the transaction that carries it. See
   [`docs/attestation-spec.md`](attestation-spec.md).
2. **Threshold attestation** (`ThresholdAttestation`) — a t-of-n scheme where multiple signers
   each attest, and the contract requires a configurable threshold of valid signatures. See
   [`docs/threshold-attestation-spec.md`](threshold-attestation-spec.md).
3. **Batch attestation** — a single secp256k1 signature over a domain-separated Merkle root
   covering an entire `submit_scores_batch_attested` batch, verified per-entry via
   `verify_merkle_proof` against that root. See
   [`docs/batch-attestation-spec.md`](batch-attestation-spec.md).

All three answer "did the off-chain pipeline vouch for this payload," which is a different
question from **authorization** (§ Authorization) — a payload can be correctly authorised
(`require_auth` passed) and still carry no attestation, or vice versa be attested but sent by an
unauthorised relayer.

### Authorization

Soroban's `require_auth()` mechanism proving a transaction was signed by a specific address (or
threshold of addresses, for the M-of-N paths). This proves *who sent the transaction*, not
*whether its contents are correct* — see the note under **Attestation** above for why both exist.
Every state-mutating admin/service function requires it; every read/gate function
(`get_score`, `query_risk_gate*`) deliberately does not — see
[`docs/invariants.md` § 2](invariants.md#2-no-panic-reads).

### Bounded storage

The invariant that every contract-managed collection (signer sets, gate caller list, dispute
index, model version registry, etc.) has a hard `MAX_*` ceiling enforced by a typed `Error`
rather than growing indefinitely. Fully catalogued in
[`docs/invariants.md` § 3](invariants.md#3-bounded-storage).

### Capability / `supports_interface`

A `Symbol` an integrator can pass to `supports_interface` to feature-detect whether a deployed
contract instance supports a given piece of functionality (e.g. `gate` for `query_risk_gate`,
`cgate` for the confidence-gated variant) without hardcoding a **contract version** (§ below)
number. Once published, a capability symbol is never removed or repurposed — see
[`docs/interface-versioning-policy.md`](interface-versioning-policy.md).

### Confidence

A `u32` in `[0, 100]` submitted alongside every score, representing the off-chain model's
certainty in that specific score — **not** a property of the wallet, and not the same thing as
the **score** itself (a low-confidence score can still be numerically high-risk). Gate functions
compare confidence against a floor (`min_confidence` and/or `global_min_confidence`, whichever is
higher) to reject decisions the model itself wasn't sure about — see
[README.md § Composability § Gated liquidity provision](../README.md#gated-liquidity-provision).

### Consensus (score)

An on-chain median computed from multiple model-attested submissions for the same
`(wallet, asset_pair)` via `commit_consensus` / `reveal_consensus`, gated by a configurable
`k`-of-`n` threshold and an epsilon tolerance for outlier rejection
(`DEFAULT_CONSENSUS_THRESHOLD_K`, `DEFAULT_CONSENSUS_EPSILON` in `constants.rs`). Distinct from
**aggregate score** (§ above), which combines one wallet's scores *across pairs*, not multiple
submitters' scores for *one* pair.

### Contract version / interface version

`CONTRACT_VERSION` (currently `4`, in `constants.rs`) and the interface version in
[`docs/interface-spec.md`](interface-spec.md) are incremented together and always match — one
integer identifying which generation of the ABI a deployed instance implements. Prefer
**capability** detection (§ above) over comparing this number directly; see
[`docs/interface-versioning-policy.md` § 5](interface-versioning-policy.md#5-programmatic-detection).

### Decay

Time-weighted exponential reduction applied to a stored score's effective value as it ages,
configurable via `set_decay_rate` (a `(numerator, denominator)` fixed-point rate) and surfaced
through `get_effective_score`. A decayed score is a *view*, not a mutation — the underlying stored
`RiskScore` is untouched; only what `get_effective_score` returns changes over time.

### Delegate (score delegation)

A wallet (the "custodian") that another wallet (the "sub-wallet") designates via
`set_score_delegate` to stand in for it when no direct score exists — `query_risk_gate` and
`get_score` both fall through to the delegate's score if the queried wallet has none of its own.
Bounded by `MAX_DELEGATION_DEPTH` to prevent unbounded or circular delegation chains.

### Embargo

An admin-set flag (`set_score_embargo` / `lift_score_embargo`) on a specific wallet that makes
every read/gate function treat it as "no signal available" — `query_risk_gate` returns `false`
(fail-closed), `get_aggregate_score` excludes it — regardless of what score is actually stored.
Used to block a wallet under active investigation without deleting its history. Bounded by
`MAX_EMBARGOED_WALLETS`.

### Fail-closed

The rule that every gate function resolves *uncertain* conditions (no score, embargoed wallet,
paused contract with no fresh failover, out-of-range parameters) to **deny**, never to *allow*.
See [`docs/invariants.md` § 1](invariants.md#1-fail-closed-gates) for the exhaustive list of
conditions and which return `false`.

### Failover

A secondary `ledgerlens-score` contract instance an admin can configure
(`set_failover_contract`) that `query_risk_gate*` falls back to *only* while the primary is
paused — and only if the secondary's score for that wallet/pair is fresher than
`FAILOVER_STALENESS_WINDOW`. Not related to **shard** (§ below): a shard is a peer the aggregator
reads from during normal operation; a failover contract is a fallback used only during an outage.

### Finality (buffer)

**Not** blockchain/ledger-close finality — in this codebase, "finality buffer" is an
admin-configurable escrow/hold window (`set_finality_buffer`, seconds, `0` = disabled) that a
`submit_score` payload sits in as a `PendingScore` before `commit_pending_score` promotes it to
live storage. While pending, the admin can inspect (`get_pending_score`) or discard
(`cancel_pending_score`) it — it is invisible to `get_score` / `query_risk_gate` the entire time.
This is a review-and-cancel window over *score submissions*, unrelated to Stellar ledger
consensus finality.

### Gate / gate threshold

"Gate" refers to `query_risk_gate`, `query_risk_gate_with_confidence`, and
`query_risk_gate_relative` collectively — the infallible (except for the last one; see
[`docs/invariants.md` § 1](invariants.md#1-fail-closed-gates)), side-effect-free functions
designed to be called from inside another protocol's guard clause. "Gate threshold" is the
score-vs-threshold cutoff passed to the first two: the gate returns `false` when the wallet's
score is **at or above** it.

### Hysteresis (risk band)

A margin (`set_hysteresis_margin`) that keeps a wallet gated as risky for a period after its score
drops back below the gate threshold, to prevent an attacker from oscillating a score just under
and over the line to slip through repeatedly. While "in the band," `query_risk_gate` returns
`false` even if the current raw score would otherwise pass.

### Model version

An admin-registered `u32` identifying which off-chain ML model produced a given score, submitted
with every `submit_score` call. Lifecycle: `Proposed` → `Active` → `Deprecated` (irreversible —
there is deliberately no re-activation path once retired, so a retired model can never silently
start being trusted again). Bounded by `MAX_MODEL_VERSIONS` (20). Not a synonym for
**contract version** (§ above), which identifies the *contract's* ABI generation, not the
*scoring model's*.

### No-panic reads

The invariant that every function reachable in a read/query context must never panic — errors
surface as `Result::Err`, `Option::None`, or `false`, never an unwinding panic. See
[`docs/invariants.md` § 2](invariants.md#2-no-panic-reads).

### Oracle staleness

The age threshold (`set_oracle_staleness_threshold`, default 1 hour) beyond which a secondary
score source's last update is considered too old to trust for confidence adjustment in
`get_effective_score`. Distinct from — but conceptually parallel to —
`FAILOVER_STALENESS_WINDOW` (§ Failover), which gates whether a *failover contract's* score is
fresh enough to use at all.

### Pause

Two distinct scopes share this word — check which one a doc means:

1. **Global pause** (`pause` / `unpause` / `is_paused`) — a contract-wide circuit breaker
   blocking all state-mutating calls (`submit_score`, batch variants, `withdraw_fees`,
   `set_decay_rate`, etc.). This is also what triggers **failover** (§ above) in gate functions.
2. **Per-pair pause** (`set_pair_paused` / `is_pair_paused` / `get_paused_pairs`) — freezes
   submissions for one specific asset pair without affecting any other pair or global reads.
   Bounded by `MAX_PAUSED_PAIRS` (50).

### Rent

Soroban's ledger-space storage cost model — entries incur fees based on footprint size and TTL
duration. This contract's rent-management strategy (per-tier TTL thresholds, `peek_*` read paths
that don't extend TTL, the tracked-entry sweep index) is fully specified in
[`docs/storage-layout.md`](storage-layout.md); "rent" in this repo always refers to that Soroban
mechanic, never to a LedgerLens-specific fee (see **Score gate fee**, distinct concept, below).

### Score (risk score)

A `u32` in `[0, 100]` produced by the off-chain LedgerLens pipeline for one `(wallet, asset_pair)`
combination, where higher means more likely fraudulent/manipulated. Stored as part of a
`RiskScore` struct alongside `benford_flag`, `ml_flag`, **confidence** (§ above), and `timestamp`.
"Score" alone, without qualification, always means this per-pair value — see **Aggregate score**
and **Consensus** above for the two ways multiple scores get combined into something else.

### Score gate fee

An optional per-call fee (`set_gate_query_fee`, in fee-token stroops) charged on `query_risk_gate`
invocations, tracked via `get_accumulated_fees` and withdrawn with `withdraw_fees`. `0` disables
fee collection. Unrelated to Soroban **rent** (§ above) — this is a LedgerLens-level charge on
integrators, not a network storage cost.

### Service

The address (or M-of-N signer set) authorised to submit scores via `submit_score` /
`submit_scores_batch*`, rotatable by the admin via `set_service`. Distinct from **admin** (§
above): the service can write scores but cannot change contract configuration.

### Shard

**Not** blockchain state sharding. In `ledgerlens-aggregator`, a "shard" is simply another
deployed `ledgerlens-score` contract instance registered via `add_shard` (bounded by
`MAX_SHARDS`), which must advertise the required **capabilities** (`gate`, `score`, `aggr`) via
`supports_interface` before it's accepted. The aggregator reads across all registered shards and
applies a `ConflictPolicy` (`HighestScore`, `MostRecent`, ...) when they disagree — see
[`docs/aggregator-conflict-resolution.md`](aggregator-conflict-resolution.md) and
[`docs/interface-spec.md`](interface-spec.md). The term describes a *horizontal peer relationship
between separate contract deployments*, not a partition of one contract's own state.

### Watchlist

An admin-maintained flag (`set_watchlist`) on individual wallets, independent of **score** and
**embargo** (§ above) — a wallet can be watchlisted without ever having been scored or embargoed.
`ledgerlens-aggregator`'s watchlist check is conservative: a wallet is reported as watchlisted if
*any* registered shard reports it as such.

---

## Terms deliberately not duplicated here

Some concepts have their own dedicated deep-dive doc rather than a glossary stub, to avoid two
sources of truth drifting apart:

- **Errors** (every `Error` variant, code, and trigger condition) — [`docs/errors.md`](errors.md).
- **Storage keys, tiers, and TTL mechanics** — [`docs/storage-layout.md`](storage-layout.md).
- **What counts as a breaking ABI change** — [`docs/interface-versioning-policy.md`](interface-versioning-policy.md).
- **Repository invariants** (fail-closed, no-panic, bounded storage, event/error stability) —
  [`docs/invariants.md`](invariants.md).
