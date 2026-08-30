# Refinement Mapping: Rust State → TLA+ Variables

> **Issue #754** — Document how concrete storage keys and Rust structs in
> `contracts/ledgerlens-score/src/` correspond to the abstract model variables
> in `spec/LedgerLens.tla`.

## Purpose

The TLA+ specification in `LedgerLens.tla` is an *abstract* model: it captures
the essential state-machine behaviour of the LedgerLens Soroban contract using
mathematical variables, without committing to Soroban-specific storage layouts
or XDR encodings.  This document is the **refinement mapping** — the formal
bridge between the abstract model and the concrete implementation — answering
the question:

> "Which Rust storage key / struct field in the running contract corresponds to
> each TLA+ variable, and what is the exact encoding?"

A correct refinement mapping means:
1. Any invariant proved in the abstract model also holds for the concrete
   implementation (by substituting the mapping into the invariant formula).
2. Any counterexample found by TLC can be translated back into a concrete
   Rust test case.
3. When the Rust implementation changes (new storage key, renamed field,
   retyped constant), this document is the checklist for updating the spec.

---

## 1. Scope

The mapping covers the variables that appear in `LedgerLens.tla` as of the
`issue-753-bounded-liveness-checks` commit.  It does **not** cover every
storage key in the contract — only the subset that the abstract model
reasons about.  See [`docs/storage-layout.md`](../docs/storage-layout.md) for
the exhaustive storage reference.

---

## 2. TLA+ Variable → Rust Storage Key Mapping

### 2.1 Core Score State

| TLA+ variable | Type in spec | Rust storage key | Rust type | Storage tier | Notes |
|---|---|---|---|---|---|
| `score[w]` | `Wallet → ℕ` | `DataKey::Score(wallet, asset_pair)` | `RiskScore` | `persistent` | Maps to `RiskScore.score` (u32, 0–100). The abstract model uses a single canonical pair; the concrete implementation keys on `(wallet, asset_pair)`. |
| `hwm[w]` | `Wallet → ℕ` | `DataKey::HistoricalMaxScore(wallet, asset_pair)` | `u32` | `persistent` | The running high-water mark. Never decremented by a submission. |
| `breach_count[w]` | `Wallet → ℕ` | `DataKeyC::BreachCount(wallet, asset_pair)` | `u32` | `persistent` | Consecutive breach counter; reset to 0 on a sub-threshold score, incremented on a ≥ `RISK_THRESHOLD` score. |
| `last_submit_time[w]` | `Wallet → ℕ` | `DataKey::LastSubmitTime(wallet, asset_pair)` | `u64` | `persistent` | Ledger timestamp (`env.ledger().timestamp()`) of the last accepted submission; used to enforce the cooldown. |
| `now` | `ℕ` | `env.ledger().timestamp()` | `u64` | host | Not stored; read from the Soroban host on every invocation. Maps to `now` in the spec's `TickTime` action. |

### 2.2 Embargo State

| TLA+ variable | Type in spec | Rust storage key | Rust type | Storage tier | Notes |
|---|---|---|---|---|---|
| `embargo_expiry[w]` | `Wallet → ℤ` | `DataKeyB::ScoreEmbargo(wallet)` | `EmbargoExpiry` | `temporary` | `EmbargoExpiry::Indefinite` maps to `embargo_expiry[w] = -1` (permanent); `EmbargoExpiry::Until(ts)` maps to `embargo_expiry[w] = ts` (time-bounded); absent key maps to `embargo_expiry[w] = 0` (no embargo). |

The abstract `EmbargoActive(w)` predicate maps to the Rust function
`is_wallet_embargoed(env, wallet)` in `storage.rs`, which performs the
same test: `Indefinite ⟹ true`; `Until(ts) ⟹ now ≤ ts`; absent ⟹ false`.

### 2.3 Delegation State

| TLA+ variable | Type in spec | Rust storage key | Rust type | Storage tier | Notes |
|---|---|---|---|---|---|
| `delegate[w]` | `Wallet → Wallet ∪ {"None"}` | `DataKeyC::ScoreDelegate(wallet)` | `Option<Address>` | `persistent` | `"None"` in the spec maps to `None` (absent key or explicit `None`); a concrete delegate address maps to `Some(address)`. |

The abstract `DelegationAcyclicity` invariant corresponds to the Rust
guard in `set_score_delegate` that checks for depth-bounded cycles via
`MAX_DELEGATION_DEPTH` (5 hops), which is strictly stronger than the
3-hop cycle check in the spec — the spec models depth-3 acyclicity as a
sound under-approximation.

### 2.4 Token-Bucket State

| TLA+ variable | Type in spec | Rust storage key | Rust type | Storage tier | Notes |
|---|---|---|---|---|---|
| `tb_tokens[w]` | `Wallet → ℕ` | `DataKeyD::TokenBucket(wallet, asset_pair)` | `TokenBucket` | `persistent` | `TokenBucket.tokens` (u32) maps to `tb_tokens[w]`. The spec models a single canonical pair; the concrete implementation stores one `TokenBucket` per `(wallet, asset_pair)`. |
| `tb_last_refill[w]` | `Wallet → ℕ` | `DataKeyD::TokenBucket(wallet, asset_pair)` | `TokenBucket` | `persistent` | `TokenBucket.last_refill_ts` (u64) maps to `tb_last_refill[w]`. Co-located with `tokens` in the same struct. |
| `tb_capacity` | `ℕ` | Not a dedicated key; derived from `DataKey::CooldownSecs` | `u64` | `instance` | In the spec, `tb_capacity` is a global constant bounding the burst window. In the Rust implementation the burst capacity is implicit in the token-bucket algorithm: `MAX_CAPACITY` maps to `constants::MAX_COOLDOWN_SECS` as the upper bound on the configured cooldown, while `MIN_CAPACITY = 1` corresponds to the minimum one-token-per-cooldown behaviour enforced by `MIN_COOLDOWN_SECS`. The spec explores `MIN_CAPACITY..MAX_CAPACITY` as a range; the Rust contract enforces `[MIN_COOLDOWN_SECS, MAX_COOLDOWN_SECS]`. |

The abstract `RefillCount(w)` expression maps to the Rust computation in
`token_bucket::try_consume` (in `storage.rs`):

```
// Spec
RefillCount(w) == Min(tb_tokens[w] + (now - tb_last_refill[w]) div COOLDOWN, tb_capacity)

// Rust (simplified)
let elapsed  = now.saturating_sub(bucket.last_refill_ts);
let refills  = elapsed / cooldown_secs;
let refilled = (bucket.tokens + refills).min(capacity);
```

The `saturating_sub` in Rust is the implementation of the spec guard
`RefillAnchorNotInFuture` (INV-TB-4): if `last_refill_ts > now`, the
subtraction returns 0 rather than underflowing.

### 2.5 Consensus Commit-Reveal State

The consensus variables model the `commit_consensus` / `reveal_consensus`
K-of-N flow.  In the Rust implementation these are stored in **temporary**
storage (Soroban temporary entries expire at TTL) rather than persistent
storage — matching the spec's `REVEAL_WINDOW` eviction model.

| TLA+ variable | Type in spec | Rust storage key | Rust type | Storage tier | Notes |
|---|---|---|---|---|---|
| `cc_committed[s]` | `Signer → 𝔹` | `DataKeyC::ConsensusCommitment(service_addr, wallet, asset_pair)` | `BytesN<32>` | `temporary` | Presence of the key encodes `TRUE`; absence encodes `FALSE`. The value is the SHA-256 commitment hash (not the plain-text score, because the Rust contract hides scores at commit time). |
| `cc_commit_time[s]` | `Signer → ℕ` | Implicit in the temporary entry TTL | `u64` (TTL ledger count) | `temporary` | In the Rust implementation the commit time is implicitly encoded as `entry_creation_ledger_sequence`. The spec uses `now - cc_commit_time[s] ≤ REVEAL_WINDOW` to gate reveals; the Rust implementation uses `env.storage().temporary().has(&key)` — the key is automatically evicted by the host when its TTL expires, making `cc_committed[s] = FALSE` after expiry without an explicit delete. |
| `cc_score[s]` | `Signer → ℕ` | Not stored (hash only) | — | — | The spec stores the score in plain-text (no cryptographic hiding needed for structural invariants). The Rust contract stores only `SHA256(score || nonce)` and verifies the pre-image at reveal time. The abstract model's `cc_score[s]` is the *revealed* value; the commitment is opaque in both model and implementation. |
| `cc_revealed[s]` | `Signer → 𝔹` | Implicit: absence of commit key after `reveal_consensus` | — | — | A successful reveal consumes the temporary commit entry (deletes it) and writes the score into the consensus accumulator. The spec's `cc_revealed[s] = TRUE` maps to: commit key absent AND score has been accumulated for signer `s` in the current round's vote tally. |
| `cc_finalized` | `𝔹` | Implicit: `ConsensusReached` check at finalization | — | — | Not stored explicitly; finalization is immediate when K-of-N agreement is detected in `finalize_consensus`. The spec's `cc_finalized = TRUE` maps to the contract state just after `submit_score` writes the consensus result. |
| `cc_final_score` | `ℕ` | `DataKey::Score(wallet, asset_pair)` | `RiskScore.score` | `persistent` | On finalization, the consensus score is written as the new `RiskScore`. `cc_final_score` in the spec maps directly to `RiskScore.score` immediately after `finalize_consensus`. |

---

## 3. Abstract Constants → Rust Constants

| TLA+ constant | Rust constant / config | Source file | Notes |
|---|---|---|---|
| `COOLDOWN` | `DEFAULT_COOLDOWN_SECS` (3600) | `constants.rs` | Default; admin-configurable via `set_cooldown` within `[MIN_COOLDOWN_SECS, MAX_COOLDOWN_SECS]`. Spec uses `COOLDOWN = 1` (unit ticks) to make all arithmetic visible. |
| `HWM_THRESHOLD` | `DEFAULT_SCORE_FLOOR_HWM` (80) | `constants.rs` | Score floor high-water mark; admin-configurable via `set_score_floor_policy`. |
| `FLOOR_VALUE` | `DEFAULT_SCORE_FLOOR_MIN` (20) | `constants.rs` | Score floor minimum; co-configured with HWM. |
| `RISK_THRESHOLD` | `DEFAULT_RISK_THRESHOLD` (75) | `constants.rs` | Default gate threshold; integrators can pass a different `gate_threshold`. |
| `MIN_CAPACITY` | `1` | spec model config | Maps to `MIN_COOLDOWN_SECS = 60`: minimum cooldown ensures at least one token per 60 s. |
| `MAX_CAPACITY` | `3` | spec model config | TLC exploration bound; production equivalent is `MAX_COOLDOWN_SECS = 86400`. |
| `CONSENSUS_K` | `DEFAULT_CONSENSUS_THRESHOLD_K` (2) | `constants.rs` | Minimum agreeing reveals; admin-configurable via `set_consensus_config`. |
| `CONSENSUS_EPSILON` | `DEFAULT_CONSENSUS_EPSILON` (5) | `constants.rs` | Max pairwise score distance for agreement; spec uses 10 to cover failing (0 vs 80) and passing (50/50) cases with `Scores = {0, 50, 80}`. |
| `REVEAL_WINDOW` | `DEFAULT_DISPUTE_REVEAL_WINDOW_SECS` (600) / temporary entry TTL | `constants.rs` | The reveal window in the spec corresponds to the TTL of the temporary storage entry created by `commit_consensus`; when the TTL expires the entry is evicted and `ExpireStaleCommit` fires. |

---

## 4. Abstract Actions → Rust Entry Points

| TLA+ action | Rust entry point | Authorization | Notes |
|---|---|---|---|
| `SubmitScore(w, s)` | `submit_score(...)` / `submit_scores_batch(...)` | `service.require_auth()` or M-of-N | Token-bucket gate in spec maps to `token_bucket::try_consume` in `storage.rs`. Score-floor guard maps to `check_score_floor` in `lib.rs`. |
| `TickTime` | `env.ledger().timestamp()` advances | host | Not a callable entry point; the ledger clock advances between invocations. Modelled as an explicit action in the spec so TLC can interleave it with other actions. |
| `SetBurstCapacity(c)` | `set_cooldown(secs)` | `admin.require_auth()` | In the spec `tb_capacity` is a token count; in Rust the equivalent configuration is `cooldown_secs`, which implicitly controls how fast tokens refill. |
| `SetEmbargo(w, expiry)` | `set_score_embargo(wallet, expiry)` | `admin.require_auth()` | `expiry = -1` → `EmbargoExpiry::Indefinite`; `expiry = ts` → `EmbargoExpiry::Until(ts)`. |
| `LiftEmbargo(w)` | `lift_score_embargo(wallet)` | `admin.require_auth()` | Clears `DataKeyB::ScoreEmbargo(wallet)`. |
| `SetDelegate(sub, cust)` | `set_score_delegate(wallet, delegate)` | `wallet.require_auth()` | Cycle detection uses `MAX_DELEGATION_DEPTH` in Rust; spec models 3-hop acyclicity as a sound under-approximation. |
| `RemoveDelegate(sub)` | `remove_score_delegate(wallet)` | `wallet.require_auth()` | Clears `DataKeyC::ScoreDelegate(wallet)`. |
| `ResetBreachCount(w)` | `reset_breach_count(wallet, asset_pair)` (admin) | `admin.require_auth()` | Direct write of 0 to `DataKeyC::BreachCount`. |
| `CommitConsensus(s, v)` | `commit_consensus(wallet, asset_pair, commitment)` | `signer.require_auth()` | `commitment` is `SHA256(score || nonce)`; score value is hidden until reveal. |
| `RevealConsensus(s)` | `reveal_consensus(wallet, asset_pair, score, nonce)` | `signer.require_auth()` | Verifies pre-image matches the stored commitment hash; enforces reveal-window via temporary entry TTL. |
| `FinalizeConsensus` | `finalize_consensus(wallet, asset_pair)` | permissionless | Triggered once `Cardinality(agreeing_reveals) ≥ CONSENSUS_K`. |
| `ResetConsensusRound` | Implicit: TTL expiry of temporary storage entries | host | A new round starts when all temporary commit/reveal entries have expired or been consumed. |
| `ExpireStaleCommit(s)` | Implicit: Soroban temporary-storage TTL eviction | host | When the TTL elapses the entry is silently removed; a subsequent `reveal_consensus` call finds no commit and returns `RevealWindowExpired`. |

---

## 5. Invariant Correspondence

Each TLA+ invariant in `LedgerLens.cfg` has a direct Rust behavioral
counterpart, tested in the `contracts/ledgerlens-score/src/` test modules.

| TLA+ invariant | Rust behavioral equivalent | Primary test file(s) |
|---|---|---|
| `HistoricalMaxMonotonicity` | `HistoricalMaxScore` key never decreases; only `max(current, new)` is written | `test_score_floor.rs`, `test.rs` |
| `EmbargoGateSoundness` | `is_wallet_embargoed` returns correct boolean for all three expiry states | `test_embargo.rs` |
| `DelegationAcyclicity` | `set_score_delegate` rejects cycles via depth-bounded traversal | `test.rs` |
| `TokensNeverExceedCapacity` (`INV-TB-1`) | `try_consume` clamps `refilled = min(tokens + refills, capacity)` | `test_cooldown.rs`, `test_rate_limit.rs` |
| `TokensNonNegative` (`INV-TB-2`) | `try_consume` only proceeds when `refilled > 0`; stores `refilled - 1` | `test_rate_limit.rs` |
| `CapacityReductionCapsNextBurst` (`INV-TB-3`) | Same as INV-TB-1 | `test_cooldown.rs` |
| `RefillAnchorNotInFuture` (`INV-TB-4`) | `saturating_sub` prevents underflow if clock skews | `test_rate_limit.rs` |
| `CapacityWithinBounds` (`INV-TB-5`) | `set_cooldown` bounded to `[MIN_COOLDOWN_SECS, MAX_COOLDOWN_SECS]` | `test_rate_limit.rs` |
| `FinalScoreRequiresKReveals` (`INV-CR-1`) | `finalize_consensus` counts agreeing reveals; rejects if `< CONSENSUS_K` | `test_consensus.rs` |
| `NoRevealWithoutCommit` (`INV-CR-2`) | `reveal_consensus` checks `env.storage().temporary().has(&commit_key)` | `test_consensus.rs` |
| `RevealOnlyWithinWindow` (`INV-CR-3`) | Temporary entry TTL eviction enforces the window | `test_consensus.rs` |
| `FinalScoreWithinEpsilonOfCluster` (`INV-CR-4`) | `finalize_consensus` computes median of the agreeing cluster | `test_consensus.rs` |
| `CommitTimestampNotInFuture` (`INV-CR-5`) | `commit_consensus` uses `env.ledger().timestamp()` — not caller-supplied | `test_consensus.rs` |
| `ExpiredCommitCannotReveal` (`INV-CR-6`) | Temporary-storage key absent after TTL → `has()` returns false → reveal rejected | `test_consensus.rs` |
| `SubmitEnabledWhenConditionsMet` (`INV-LIVE-1`) | Precondition structure of `submit_score`: token check + floor check + embargo check together are sufficient | `test_rate_limit.rs`, `test_score_floor.rs`, `test_embargo.rs` |
| `ScoreFloorDoesNotBlockAllScores` (`INV-LIVE-2`) | `DEFAULT_SCORE_FLOOR_MIN = 20 < MAX_SCORE = 100`; always an admissible value | `test_score_floor.rs` |

---

## 6. Representation Invariants (Glue Conditions)

For the refinement to be sound, the following *representation invariants*
must hold in every reachable concrete state — they are the conditions under
which the abstract mapping is well-defined.

| Condition | Verified by |
|---|---|
| `RiskScore.score ∈ [0, 100]` | `submit_score` rejects `score > 100` with `InvalidScore` |
| `RiskScore.confidence ∈ [0, 100]` | `submit_score` rejects `confidence > 100` with `InvalidConfidence` |
| `HistoricalMaxScore(w, p) ≥ Score(w, p).score` at all times | `set_score` in `storage.rs` calls `max(old_hwm, new_score)` before writing |
| `TokenBucket.tokens ≥ 0` | `u32` type in Rust; arithmetic only stores `refilled - 1` after checking `refilled > 0` |
| `LastSubmitTime(w, p) ≤ env.ledger().timestamp()` | `submit_score` writes `env.ledger().timestamp()` — always ≤ the current host time |
| Delegation graph has no cycle of depth ≤ `MAX_DELEGATION_DEPTH` | `set_score_delegate` traverses up to `MAX_DELEGATION_DEPTH` steps and rejects if a cycle is detected |

---

## 7. Known Abstractions and Simplifications

The following aspects of the concrete implementation are deliberately
abstracted away in `LedgerLens.tla`; this section documents the gap so that
it is not mistaken for an omission.

| Abstracted away | Reason | Safe? |
|---|---|---|
| **Multi-pair model**: the spec models a single canonical `(wallet, asset_pair)` | Symmetry argument: all pairs are structurally identical; a multi-pair model would multiply the state space without finding new invariant violations | ✅ All per-pair operations are independent; no cross-pair interaction |
| **Cryptographic hiding of committed scores**: spec stores `cc_score[s]` in plain-text | TLA+ has no cryptographic hiding primitive; the invariants being verified (quorum size, epsilon, window) are structural, not confidentiality-based | ✅ The spec's invariants hold regardless of whether the score is hidden |
| **M-of-N multisig authorization**: spec treats `SubmitScore` as a single step | Authorization is orthogonal to state-machine correctness; the `require_auth` precondition is replaced by an unconditional `service` authorization in the abstract model | ✅ The concrete authorization check is strictly stronger |
| **XDR encoding and Soroban SDK types**: spec uses mathematical sets and functions | Storage encoding is irrelevant to behavioral correctness | ✅ Encoding is injective; no aliasing |
| **TTL / storage rent management**: spec does not model archival | Storage rent is an operational concern, not a behavioral correctness concern for the invariants modelled | ⚠️ An archived entry is behaviorally equivalent to `ScoreNotFound`; `get_expiring_entries` / `extend_entry_ttls` exist to prevent archival in practice |
| **Upgrade governance time-lock**: spec does not model WASM upgrades | Upgrade governance is a separate safety property not part of the submission-liveness argument | ✅ The upgrade flow is separately modelled in `test_upgrade.rs` |
| **Attestation secp256k1 verification**: spec does not model signature verification | Cryptographic soundness is assumed; the spec models what happens *given* a valid signature | ✅ Verified separately in `test_attestation.rs` and `test_batch_attestation.rs` |

---

## 8. How to Update This Document

When the Rust implementation changes:

1. **New storage key added**: add a row to the appropriate table in §2 if
   the key corresponds to a spec variable, or note it in §7 if it is
   intentionally abstracted away.

2. **Spec variable renamed**: update the table rows in §2 and the invariant
   correspondence in §5.

3. **New invariant added to `LedgerLens.tla`**: add a row to §5 identifying
   the Rust behavioral equivalent and the test file that covers it.

4. **New abstraction introduced**: add a row to §7 explaining the gap and
   whether it is safe.

5. **Constant value changed**: update the row in §3 and check whether the
   TLC model configuration in `LedgerLens.cfg` needs updating (e.g., if
   `DEFAULT_CONSENSUS_EPSILON` changes, verify that `Scores = {0, 50, 80}`
   still produces both passing and failing epsilon clusters).

---

## 9. References

- `spec/LedgerLens.tla` — The abstract TLA+ specification
- `spec/LedgerLens.cfg` — TLC model-checking configuration
- `spec/README.md` — Invariant catalogue and TLC run instructions
- `contracts/ledgerlens-score/src/types.rs` — `DataKey`, `DataKeyB`, `DataKeyC`, `DataKeyD` enum definitions (all storage keys)
- `contracts/ledgerlens-score/src/storage.rs` — Storage read/write helpers (concrete implementations of abstract `score`, `hwm`, `tb_tokens`, etc.)
- `contracts/ledgerlens-score/src/constants.rs` — All numeric constants referenced in §3
- `docs/storage-layout.md` — Exhaustive storage layout reference (superset of this document)
- `docs/attestation-spec.md` — Secp256k1 attestation specification (abstracted in §7)
- `docs/batch-attestation-spec.md` — Merkle-tree batch attestation specification (abstracted in §7)
