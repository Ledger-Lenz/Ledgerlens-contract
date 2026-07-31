# LedgerLens TLA+ Specification

This directory contains a formal specification of the LedgerLens smart contract's state machine written in TLA+. The specification models score writes, the embargo gate, breach counter, risk band state, the delegation chain, the **adaptive rate-limit token bucket** (issue #405), and the **M-of-N consensus commit-reveal flow** (issue #403).

## Invariants Modelled

The following critical invariants are encoded and verified by TLC.

### Existing Invariants

1. **Historical Max Monotonicity**: `hwm` never decreases — the high-water mark is a running maximum.
2. **Embargo Gate Soundness**: The embargo gate correctly blocks score writes when an embargo is active (permanent or time-bounded).
3. **Breach Counter State Machine**: The breach counter correctly increments on threshold crossings and resets on clean submissions or explicit admin resets.
4. **Delegation Acyclicity**: No cyclical score delegation loops exist up to depth 3.
5. **Score Floor Enforcement**: Wallets that have crossed `HWM_THRESHOLD` cannot have their scores forced below `FLOOR_VALUE`.

### Token-Bucket Invariants (issue #405)

6. **TokensNeverExceedCapacity** (`INV-TB-1`): The effective token count seen by the next `SubmitScore` call (computed by `RefillCount`) never exceeds the current global capacity `tb_capacity`. This holds both under normal operation and immediately after a capacity *reduction* — the lazy-truncation contract (bucket state is clamped on the next read, not eagerly rewritten) means raw stored tokens may temporarily exceed the new cap, but `RefillCount` always clamps to `tb_capacity`, so no wallet can burst above the new limit.

7. **TokensNonNegative** (`INV-TB-2`): Stored token counts are always ≥ 0. Because `SubmitScore` only proceeds when `RefillCount > 0`, and then stores `refilled - 1 ≥ 0`, this is structurally guaranteed — the invariant makes it machine-checkable.

8. **CapacityReductionCapsNextBurst** (`INV-TB-3`): Mirrors `INV-TB-1` and is stated separately for clarity: after `SetBurstCapacity` reduces the capacity, the *effective* tokens available on the next refill are bounded by the new capacity. This directly catches the class of off-by-one bugs where a burst larger than the new capacity is allowed right after a capacity reduction.

9. **RefillAnchorNotInFuture** (`INV-TB-4`): `tb_last_refill[w] ≤ now` at all times. If this were violated, `elapsed` would underflow and the refill count would be computed incorrectly, potentially granting extra tokens.

10. **CapacityWithinBounds** (`INV-TB-5`): `tb_capacity` is always within `[MIN_CAPACITY, MAX_CAPACITY]`. This ensures `SetBurstCapacity` can never lock wallets permanently (capacity = 0) or open the bucket arbitrarily wide.

### Consensus Commit-Reveal Invariants (new — issue #403)

These invariants model the `commit_consensus` / `reveal_consensus` K-of-N agreement-within-epsilon flow from `contracts/ledgerlens-score/src/lib.rs`.

11. **FinalScoreRequiresKReveals** (`INV-CR-1`): A value can only be written as the consensus result when at least `CONSENSUS_K` valid reveals exist **and** at least `CONSENSUS_K` of those revealed scores lie within `CONSENSUS_EPSILON` of the final score. This is the primary safety invariant: no smaller quorum can produce a finalized score.

12. **NoRevealWithoutCommit** (`INV-CR-2`): A reveal is only recorded for a signer that has an open (non-expired) commitment. In any reachable state, `cc_revealed[s] = TRUE` implies `cc_committed[s] = TRUE`. This catches the entire class of replay / pre-image attacks where a reveal is injected without a prior commit.

13. **RevealOnlyWithinWindow** (`INV-CR-3`): A reveal is only accepted if it arrives within `REVEAL_WINDOW` ticks of the corresponding commit. If a signer has revealed, `now - cc_commit_time[s] ≤ REVEAL_WINDOW` holds. This verifies that expired commitments (modelling Soroban temporary-storage TTL eviction) are permanently rejected.

14. **FinalScoreWithinEpsilonOfCluster** (`INV-CR-4`): A stronger restatement of `INV-CR-1`: once finalized, the written `cc_final_score` is within `CONSENSUS_EPSILON` of at least `CONSENSUS_K` revealed scores. This directly pins the epsilon band to the committed result rather than relying on the existence check alone.

15. **CommitTimestampNotInFuture** (`INV-CR-5`): `cc_commit_time[s] ≤ now` for all signers. Rules out time-travel commits that could extend the reveal window artificially.

16. **ExpiredCommitCannotReveal** (`INV-CR-6`): A signer whose commit has been TTL-evicted (`cc_committed[s] = FALSE`) has no reveal recorded (`cc_revealed[s] = FALSE`). This is a direct consequence of `INV-CR-2` but is stated explicitly to document the eviction contract.

### Token-Bucket Temporal Properties (issue #405)

17. **TokenExhaustionBlocksSubmit** (`PROP-TB-1`): When a `SubmitScore` drains the bucket to 0, only that very submission is accepted at `now`; subsequent submissions for the same wallet are blocked until tokens refill (at least one `COOLDOWN` tick must elapse).

18. **BurstNeverExceedsNewCapacity** (`PROP-TB-2`): After a capacity *increase*, the effective available tokens on the next refill still never exceed the new (higher) capacity. Upward-direction companion to `INV-TB-3`.

### Consensus Temporal Properties (new — issue #403)

19. **FinalizationIsTerminalWithinRound** (`PROP-CR-1`): Once `cc_finalized` is set to `TRUE`, it stays `TRUE` until an explicit `ResetConsensusRound` action. No action other than the round reset may take `cc_finalized` from `TRUE` back to `FALSE`. Catches any accidental re-entry or double-finalization bug.

20. **FinalScoreImmutableWithinRound** (`PROP-CR-2`): Once `cc_final_score` is written it is immutable within the round: if `cc_finalized` holds in both the current and next state, `cc_final_score` does not change. Prevents a late reveal from silently overwriting an already-committed consensus result.

### Bounded Liveness Properties (new — issue #753)

These properties prove that a valid submission is *not* blocked forever. Under the bounded state constraint (`now ≤ 10`), there are always enough ticks to drain the cooldown and refill the token bucket, after which `SubmitScore` becomes enabled. Permanent embargoes are explicitly excluded — the liveness argument applies only to time-bounded blocks.

21. **SubmitEnabledWhenConditionsMet** (`INV-LIVE-1`): Whenever (a) no embargo is active, (b) the score-floor policy is satisfied, and (c) at least one token is available, the `SubmitScore` action is *enabled* (its precondition holds). This is the structural half of the liveness argument: once the cooldown has elapsed and no policy block is in effect, the submission can proceed.

22. **ScoreFloorDoesNotBlockAllScores** (`INV-LIVE-2`): If the floor policy is active for a wallet (historical max ≥ `HWM_THRESHOLD`), there always exists at least one score in the modelled `Scores` set that is ≥ `FLOOR_VALUE`. This ensures the floor policy never makes every possible submission inadmissible — a valid score is always available.

23. **CooldownExpiryEnablesSubmission** (`PROP-LIVE-1`): In every state where a wallet has no token and is not embargoed, time advances by one tick (or the token was already available). Combined with the token-bucket invariants, this proves the bucket refills within `COOLDOWN` ticks after exhaustion.

24. **BoundedEmbargoEventuallyLifts** (`PROP-LIVE-2`): For time-bounded embargoes (`embargo_expiry[w] > 0`), once `now` advances past `embargo_expiry[w]` the wallet is no longer embargoed. Permanent embargoes (`embargo_expiry[w] = -1`) are excluded — they are the "pause remains" case and are intentionally out of scope for the liveness argument.

25. **BoundedLivenessSubmissionAccepted** (`PROP-LIVE-3`): In every step where all three preconditions hold (no embargo, policy-compliant score, token available, last submit time < now), either the submission is accepted in that step (`last_submit_time'[w] = now`) or the state is unchanged — proving no state can indefinitely prevent an otherwise-enabled submission.

## Variables

| Variable | Type | Description |
|---|---|---|
| `score` | `Wallet → ℕ` | Latest submitted risk score |
| `hwm` | `Wallet → ℕ` | Historical high-water mark (running maximum score) |
| `breach_count` | `Wallet → ℕ` | Consecutive breach counter |
| `last_submit_time` | `Wallet → ℕ` | Ledger timestamp of last accepted submission |
| `embargo_expiry` | `Wallet → ℤ` | Embargo expiry timestamp (0 = none, −1 = permanent) |
| `delegate` | `Wallet → Wallet ∪ {"None"}` | Delegation mapping |
| `now` | `ℕ` | Monotonically advancing ledger timestamp |
| `tb_tokens` | `Wallet → ℕ` | Current token count per wallet bucket |
| `tb_last_refill` | `Wallet → ℕ` | Last-refill anchor timestamp per wallet |
| `tb_capacity` | `ℕ` | Global burst capacity (max tokens per bucket) |
| `cc_committed` | `Signer → 𝔹` | Open-commit flag per signer (consensus round) |
| `cc_commit_time` | `Signer → ℕ` | Timestamp of each signer's commit |
| `cc_score` | `Signer → ℕ` | Committed score value per signer (plain-text in the abstract model) |
| `cc_revealed` | `Signer → 𝔹` | Successful-reveal flag per signer |
| `cc_finalized` | `𝔹` | TRUE once the current consensus round has been finalized |
| `cc_final_score` | `ℕ` | The consensus score written on finalization |

## Actions

### Core Score Actions

| Action | Description |
|---|---|
| `TickTime` | Advance `now` by one tick |
| `SubmitScore(w, s)` | Submit score `s` for wallet `w` (token-bucket gated) |
| `SetBurstCapacity(c)` | Admin sets global burst capacity |
| `SetEmbargo(w, e)` | Place an embargo on wallet `w` |
| `LiftEmbargo(w)` | Lift the embargo on wallet `w` |
| `SetDelegate(sub, cust)` | Assign a delegation from `sub` to `cust` |
| `RemoveDelegate(sub)` | Remove `sub`'s delegation |
| `ResetBreachCount(w)` | Admin resets breach counter for wallet `w` |

### Consensus Actions (new — issue #403)

| Action | Description |
|---|---|
| `CommitConsensus(s, v)` | Signer `s` commits score `v` for the current round |
| `RevealConsensus(s)` | Signer `s` reveals their committed score (window-gated) |
| `FinalizeConsensus` | Atomically finalizes the round when K-of-N epsilon agreement holds |
| `ResetConsensusRound` | Resets all consensus state to begin a new round |
| `ExpireStaleCommit(s)` | Models Soroban TTL eviction: clears an expired uncommitted commit |

## Model-Check Results

The model was checked with TLC using the configuration in [`LedgerLens.cfg`](LedgerLens.cfg).

### Model parameters

| Constant | Value | Rationale |
|---|---|---|
| `Wallets` | `{"W1", "W2"}` | Two wallets give sufficient pair-interaction coverage |
| `Scores` | `{0, 50, 80}` | Covers below-floor, at-threshold, and above-threshold; also produces passing (50/50) and failing (0 vs 80) epsilon checks |
| `Assets` | `{}` | Not used by the current single-pair model |
| `COOLDOWN` | `1` | Unit cooldown makes all time arithmetic directly visible |
| `HWM_THRESHOLD` | `80` | Matches default production value |
| `FLOOR_VALUE` | `20` | Matches default production value |
| `RISK_THRESHOLD` | `50` | Mid-range threshold |
| `MIN_CAPACITY` | `1` | Minimum legal capacity (legacy flat-cooldown behaviour) |
| `MAX_CAPACITY` | `3` | Upper exploration bound; 3 tokens exposes multi-burst paths |
| `Signers` | `{"S1", "S2", "S3"}` | 3 signers: sufficient to exercise K=2 majority/minority boundary (1-of-3 should fail, 2-of-3 should pass) |
| `CONSENSUS_K` | `2` | Minimum agreeing reveals; matches the K-of-N threshold explored in `test_consensus.rs` |
| `CONSENSUS_EPSILON` | `10` | Score distance budget; scores {0, 50, 80} ensure both passing (50 and 50) and failing (0 vs 80) epsilon clusters exist in the state space |
| `REVEAL_WINDOW` | `2` | Two ticks model the TTL boundary; with `now ≤ 5` this covers within-window, at-boundary, and past-window reveals |
| `StateConstraint` | `now ≤ 10` | Increased from 5 to 10 (issue #753): covers ≥ 2 full COOLDOWN cycles, ≥ 1 full commit-reveal-finalize-reset cycle, and leaves enough headroom for a subsequent policy-compliant submission — exercising all five new liveness properties |

### Outcome

**No invariant violations found.** All 18 invariants and 8 temporal properties (including the 6 new consensus invariants, 2 new consensus temporal properties, 2 new liveness invariants, and 3 new bounded liveness temporal properties) held across all reachable states within the `now ≤ 10` bound.

To reproduce:

```bash
# Download TLA+ Tools if not already present
curl -L -o tla2tools.jar \
  https://github.com/tlaplus/tlaplus/releases/download/v1.8.0/tla2tools.jar

# Run TLC
java -jar tla2tools.jar -config LedgerLens.cfg -depth 10 LedgerLens.tla
```

Expected output: `Model checking completed. No error has been found.`

### Bugs the new invariants are designed to catch

| Invariant | Vulnerability class caught |
|---|---|
| `INV-CR-1` / `INV-CR-4` | Consensus finalizing on fewer than K reveals; epsilon check bypassed |
| `INV-CR-2` / `INV-CR-6` | Reveal injected without a prior commit; pre-image forgery |
| `INV-CR-3` | Reveal accepted after the reveal window expires (TTL eviction race) |
| `INV-CR-5` | Future-dated commit extending the reveal window beyond `REVEAL_WINDOW` |
| `PROP-CR-1` | Double finalization; re-entry into a finalized round |
| `PROP-CR-2` | Late reveal silently overwriting an already-committed consensus result |

### Invariant violations and bug reports

Any invariant violation TLC produces should be converted into a Rust regression test targeting `contracts/ledgerlens-score/src/` and filed as a bug against the Rust implementation — **not** silently patched in the spec alone. The spec must remain a faithful model of the implemented behaviour, not an idealised version of it.

During development of this extension (issues #405, #403), no violations were found in the token-bucket or consensus invariants.

## How to Install and Run TLC

TLC is the official model checker for TLA+ specifications. You can run TLC from the command line using Java.

### Prerequisites

You must have Java installed (JRE 11+ recommended).

**On Ubuntu/Debian:**
```bash
sudo apt install default-jre
```

**On macOS:**
```bash
brew install openjdk
```

### Running TLC

1. Download the TLA+ Tools (`tla2tools.jar`) if it isn't already present:
   ```bash
   cd spec
   curl -L -o tla2tools.jar https://github.com/tlaplus/tlaplus/releases/download/v1.8.0/tla2tools.jar
   ```

2. Run the TLC model checker on the specification using the configuration file:
   ```bash
   java -jar tla2tools.jar -config LedgerLens.cfg -depth 8 LedgerLens.tla
   ```

### Output

TLC will explore all possible states up to a depth of 8 state transitions.
- If it prints **"No errors"**, all specified invariants hold in all reachable states up to depth 8.
- If it encounters an invariant violation, it will print an **Error Trace** detailing the exact sequence of actions that led to the failure. This trace should be converted into a Rust unit test to confirm and patch the vulnerability in the smart contract.
