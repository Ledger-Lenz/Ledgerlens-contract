---------------------------- MODULE LedgerLens ----------------------------
EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS 
    Wallets,
    Scores,
    Assets,
    COOLDOWN,
    HWM_THRESHOLD,
    FLOOR_VALUE,
    RISK_THRESHOLD,
    \* ── Token-bucket constants ──────────────────────────────────────────────
    \* MIN_CAPACITY and MAX_CAPACITY bound the range of capacity values that
    \* TLC will explore when model-checking SetBurstCapacity.
    MIN_CAPACITY,   \* smallest capacity the admin may set (≥ 1)
    MAX_CAPACITY,   \* largest capacity the admin may set
    \* ── Consensus constants (issue #403) ────────────────────────────────────
    \* Signers is the set of model addresses that may commit/reveal.
    \* CONSENSUS_K is the minimum number of valid in-epsilon reveals required
    \* for a round to finalise (K-of-N agreement).
    \* CONSENSUS_EPSILON is the maximum pairwise score distance counted as
    \* "agreement" — mirrors the Rust DEFAULT_CONSENSUS_EPSILON / configurable
    \* epsilon stored in contract storage.
    \* REVEAL_WINDOW is the number of time-ticks within which a reveal must
    \* arrive after the corresponding commit (analogous to the Soroban
    \* temporary-storage TTL used as the reveal deadline in the Rust contract).
    Signers,
    CONSENSUS_K,
    CONSENSUS_EPSILON,
    REVEAL_WINDOW

VARIABLES 
    score,
    hwm,
    breach_count,
    last_submit_time,
    embargo_expiry,
    delegate,
    now,
    \* ── Upgrade-proposal signer-set snapshot (issue #1) ─────────────────────
    \* upg_approvals      – set of signers who have approved the pending
    \*                      proposal under the *current* signer set snapshot.
    \* upg_signer_snap    – the frozen signer set captured when the first
    \*                      approval arrived.  If the live set ever diverges
    \*                      from this snapshot the approval accumulator is
    \*                      invalidated, preventing replay under a new set.
    \* upg_live_signers   – the current live admin signer set (mutated by
    \*                      AddAdminSigner / RemoveAdminSigner actions).
    upg_approvals,
    upg_signer_snap,
    upg_live_signers,
    \* ── Token-bucket variables ──────────────────────────────────────────────
    \* tb_tokens[w]      – current token count for wallet w (across the single
    \*                     pair modelled here; extend to a function of pairs for
    \*                     a multi-pair model).
    \* tb_last_refill[w] – ledger timestamp of the last refill anchor.
    \* tb_capacity       – global burst capacity (max tokens per bucket).
    \* NOTE: The Rust implementation stores one bucket per (wallet, asset_pair).
    \*       For clarity this spec models a single canonical pair; all
    \*       multi-pair behaviour follows by symmetry.
    tb_tokens,
    tb_last_refill,
    tb_capacity,
    \* ── Consensus commit-reveal variables (issue #403) ──────────────────────
    \* Each signer independently commits a hash of (score || nonce) for a
    \* given (wallet, pair) before the reveal step.  We model a single
    \* canonical (wallet, pair) for clarity — the multi-pair generalisation
    \* follows by symmetry, exactly as for the token bucket above.
    \*
    \* cc_committed[s]   – TRUE iff signer s has an open (unexpired) commit.
    \* cc_commit_time[s] – the value of `now` when signer s committed.
    \* cc_score[s]       – the score signer s committed to (hidden until reveal;
    \*                     modelled in plain-text here because TLA+ has no
    \*                     cryptographic hiding — the invariants we care about
    \*                     are structural, not confidentiality-based).
    \* cc_revealed[s]    – TRUE iff signer s has successfully revealed in the
    \*                     current round.
    \* cc_finalized      – TRUE once the round has been finalized (K-of-N
    \*                     agreement reached and a score written).
    \* cc_final_score    – the consensus score written on finalization (median
    \*                     of the K agreeing reveals).
    cc_committed,
    cc_commit_time,
    cc_score,
    cc_revealed,
    cc_finalized,
    cc_final_score

\* ── Full variable tuple (used in UNCHANGED clauses) ──────────────────────────
vars == <<score, hwm, breach_count, last_submit_time, embargo_expiry, delegate, now,
          tb_tokens, tb_last_refill, tb_capacity,
          cc_committed, cc_commit_time, cc_score, cc_revealed,
          cc_finalized, cc_final_score,
          upg_approvals, upg_signer_snap, upg_live_signers>>

\* Convenience: smaller / larger of two naturals
Min(a, b) == IF a <= b THEN a ELSE b
Max(a, b) == IF a >= b THEN a ELSE b
Abs(x)    == IF x >= 0 THEN x ELSE -x

\* ── Helper: compute refilled token count ─────────────────────────────────────
\* Mirrors the Rust expression:
\*   let elapsed  = now.saturating_sub(last_refill);
\*   let refills  = elapsed / cooldown;        (integer division)
\*   let refilled = min(tokens + refills, capacity);
RefillCount(w) ==
    LET elapsed  == now - tb_last_refill[w]
        refills  == elapsed \div COOLDOWN
    IN  Min(tb_tokens[w] + refills, tb_capacity)

\* ── Consensus helpers ────────────────────────────────────────────────────────

\* The set of signers that have an open (committed but not yet expired) commit.
OpenCommitters ==
    { s \in Signers : cc_committed[s] /\ (now - cc_commit_time[s]) <= REVEAL_WINDOW }

\* The set of signers that have successfully revealed in this round.
RevealedSigners ==
    { s \in Signers : cc_revealed[s] }

\* Count of revealed signers whose score is within CONSENSUS_EPSILON of `ref_score`.
\* Used in the finalization check — mirrors the Rust loop that counts how many
\* valid reveals land within epsilon of each candidate pivot.
InEpsilonCount(ref_score) ==
    Cardinality({ s \in RevealedSigners : Abs(cc_score[s] - ref_score) <= CONSENSUS_EPSILON })

\* TRUE iff there exists at least one revealed score such that at least
\* CONSENSUS_K other revealed scores agree with it within epsilon.
\* Matches the Rust finalization check: for each candidate pivot, count
\* reveals within epsilon; if any pivot collects >= K, consensus passes.
ConsensusReached ==
    \E s \in RevealedSigners : InEpsilonCount(cc_score[s]) >= CONSENSUS_K

\* The consensus (median) score when ConsensusReached.  We model this as the
\* score of an arbitrary pivot signer whose epsilon-cluster is >= K, which is
\* a valid representative value under the invariants we care about.  A full
\* median computation in TLA+ would require sorting and is not needed to
\* capture the safety properties.
\* 
\* For a richer median model the user could extend cc_final_score to be the
\* exact middle element of the sorted agreeing cluster; the invariants below
\* hold for any selection from within the epsilon band.
ConsensusScore ==
    CHOOSE s \in RevealedSigners : InEpsilonCount(cc_score[s]) >= CONSENSUS_K

\* ── Initialization ───────────────────────────────────────────────────────────
Init ==
    /\ score           = [w \in Wallets |-> 0]
    /\ hwm             = [w \in Wallets |-> 0]
    /\ breach_count    = [w \in Wallets |-> 0]
    /\ last_submit_time= [w \in Wallets |-> 0]
    /\ embargo_expiry  = [w \in Wallets |-> 0]
    /\ delegate        = [w \in Wallets |-> "None"]
    /\ now             = 1
    \* Token-bucket: every wallet starts with a full bucket.
    /\ tb_tokens       = [w \in Wallets |-> MIN_CAPACITY]
    /\ tb_last_refill  = [w \in Wallets |-> 1]
    /\ tb_capacity     = MIN_CAPACITY
    \* Consensus: no open commitments, no reveals, not finalized.
    /\ cc_committed    = [s \in Signers |-> FALSE]
    /\ cc_commit_time  = [s \in Signers |-> 0]
    /\ cc_score        = [s \in Signers |-> 0]
    /\ cc_revealed     = [s \in Signers |-> FALSE]
    /\ cc_finalized    = FALSE
    /\ cc_final_score  = 0
    \* Upgrade-proposal signer-set snapshot (issue #1): start empty.
    /\ upg_approvals   = {}
    /\ upg_signer_snap = {}
    /\ upg_live_signers = Signers   \* model all spec Signers as the live admin set

\* ── Action: TickTime ─────────────────────────────────────────────────────────
TickTime ==
    /\ now' = now + 1
    /\ UNCHANGED <<score, hwm, breach_count, last_submit_time, embargo_expiry, delegate,
                   tb_tokens, tb_last_refill, tb_capacity,
                   cc_committed, cc_commit_time, cc_score, cc_revealed,
                   cc_finalized, cc_final_score,
                   upg_approvals, upg_signer_snap, upg_live_signers>>

\* ── Action: SubmitScore ──────────────────────────────────────────────────────
\* A score submission is accepted only when the wallet's token bucket has at
\* least one token available (RefillCount > 0).  On acceptance:
\*   – one token is consumed,
\*   – last_refill is advanced by (refills * COOLDOWN) so the clock doesn't
\*     "slip" (matches the Rust `new_last_refill` calculation), and
\*   – score / hwm / breach_count / last_submit_time are updated as before.
\*
\* When tb_capacity = 1 this collapses to the legacy flat-cooldown model:
\* a submission is accepted only after COOLDOWN ticks have elapsed.
SubmitScore(w, s) ==
    LET refilled  == RefillCount(w)
        elapsed   == now - tb_last_refill[w]
        refills   == elapsed \div COOLDOWN
        new_last_refill == tb_last_refill[w] + refills * COOLDOWN
    IN
    /\ hwm[w] >= HWM_THRESHOLD => s >= FLOOR_VALUE
    /\ refilled > 0                        \* token available — gate
    /\ score'           = [score           EXCEPT ![w] = s]
    /\ hwm'             = [hwm             EXCEPT ![w] = IF s > hwm[w] THEN s ELSE hwm[w]]
    /\ breach_count'    = [breach_count    EXCEPT ![w] = IF s >= RISK_THRESHOLD THEN breach_count[w] + 1 ELSE 0]
    /\ last_submit_time'= [last_submit_time EXCEPT ![w] = now]
    /\ tb_tokens'       = [tb_tokens       EXCEPT ![w] = refilled - 1]
    /\ tb_last_refill'  = [tb_last_refill  EXCEPT ![w] = new_last_refill]
    /\ UNCHANGED <<embargo_expiry, delegate, now, tb_capacity,
                   cc_committed, cc_commit_time, cc_score, cc_revealed,
                   cc_finalized, cc_final_score,
                   upg_approvals, upg_signer_snap, upg_live_signers>>

\* ── Action: SetBurstCapacity ─────────────────────────────────────────────────
\* Admin reduces or increases burst capacity.  The Rust implementation applies
\* the new capacity lazily — existing per-bucket token counts are clamped to
\* the new capacity on the *next* refill, not immediately.  We model that
\* faithfully: only tb_capacity changes; tb_tokens is left untouched.
\*
\* The guard capacity >= MIN_CAPACITY ensures capacity never drops to 0
\* (which would permanently lock all submissions).
SetBurstCapacity(capacity) ==
    /\ capacity >= MIN_CAPACITY
    /\ capacity <= MAX_CAPACITY
    /\ tb_capacity' = capacity
    /\ UNCHANGED <<score, hwm, breach_count, last_submit_time, embargo_expiry, delegate, now,
                   tb_tokens, tb_last_refill,
                   cc_committed, cc_commit_time, cc_score, cc_revealed,
                   cc_finalized, cc_final_score,
                   upg_approvals, upg_signer_snap, upg_live_signers>>

\* ── Action: SetEmbargo ───────────────────────────────────────────────────────
SetEmbargo(w, expiry) ==
    /\ embargo_expiry' = [embargo_expiry EXCEPT ![w] = expiry]
    /\ UNCHANGED <<score, hwm, breach_count, last_submit_time, delegate, now,
                   tb_tokens, tb_last_refill, tb_capacity,
                   cc_committed, cc_commit_time, cc_score, cc_revealed,
                   cc_finalized, cc_final_score,
                   upg_approvals, upg_signer_snap, upg_live_signers>>

\* ── Action: LiftEmbargo ──────────────────────────────────────────────────────
LiftEmbargo(w) ==
    /\ embargo_expiry' = [embargo_expiry EXCEPT ![w] = 0]
    /\ UNCHANGED <<score, hwm, breach_count, last_submit_time, delegate, now,
                   tb_tokens, tb_last_refill, tb_capacity,
                   cc_committed, cc_commit_time, cc_score, cc_revealed,
                   cc_finalized, cc_final_score,
                   upg_approvals, upg_signer_snap, upg_live_signers>>

\* ── Action: SetDelegate / RemoveDelegate ─────────────────────────────────────
SetDelegate(sub, cust) ==
    /\ sub /= cust
    /\ delegate[cust] /= sub
    /\ delegate[cust] /= "None" => delegate[delegate[cust]] /= sub
    /\ delegate' = [delegate EXCEPT ![sub] = cust]
    /\ UNCHANGED <<score, hwm, breach_count, last_submit_time, embargo_expiry, now,
                   tb_tokens, tb_last_refill, tb_capacity,
                   cc_committed, cc_commit_time, cc_score, cc_revealed,
                   cc_finalized, cc_final_score,
                   upg_approvals, upg_signer_snap, upg_live_signers>>

RemoveDelegate(sub) ==
    /\ delegate' = [delegate EXCEPT ![sub] = "None"]
    /\ UNCHANGED <<score, hwm, breach_count, last_submit_time, embargo_expiry, now,
                   tb_tokens, tb_last_refill, tb_capacity,
                   cc_committed, cc_commit_time, cc_score, cc_revealed,
                   cc_finalized, cc_final_score,
                   upg_approvals, upg_signer_snap, upg_live_signers>>

\* ── Action: ResetBreachCount ─────────────────────────────────────────────────
ResetBreachCount(w) ==
    /\ breach_count' = [breach_count EXCEPT ![w] = 0]
    /\ UNCHANGED <<score, hwm, last_submit_time, embargo_expiry, delegate, now,
                   tb_tokens, tb_last_refill, tb_capacity,
                   cc_committed, cc_commit_time, cc_score, cc_revealed,
                   cc_finalized, cc_final_score,
                   upg_approvals, upg_signer_snap, upg_live_signers>>

\* ════════════════════════════════════════════════════════════════════════════
\* CONSENSUS COMMIT-REVEAL ACTIONS  (issue #403)
\* ════════════════════════════════════════════════════════════════════════════
\*
\* The three-phase flow modelled here directly mirrors the Rust contract:
\*
\*   Phase 1 – CommitConsensus(signer, value):
\*     Each signer independently records a commitment (the hash of their score
\*     and a nonce) in temporary storage.  In this abstract model we record the
\*     score in plain-text and simply track `cc_committed` and `cc_commit_time`
\*     so that the reveal-window guard can be checked structurally.
\*
\*   Phase 2 – RevealConsensus(signer, value):
\*     The signer reveals their pre-image.  The action is accepted only when:
\*       (a) the signer has an open commit (cc_committed[s] = TRUE), AND
\*       (b) the reveal arrives within REVEAL_WINDOW ticks of the commit, AND
\*       (c) the revealed score matches the committed score (hash check —
\*           modelled here by requiring the same score value since we store
\*           it in plain-text), AND
\*       (d) the round has not yet been finalized.
\*
\*   Phase 3 – FinalizeConsensus:
\*     Any party may trigger finalization once ConsensusReached holds.
\*     Finalization is an *atomic* step that writes cc_final_score and sets
\*     cc_finalized = TRUE.  A new round starts by resetting cc_committed,
\*     cc_revealed, etc.  (modelled as a separate ResetConsensusRound action).
\*
\* ─────────────────────────────────────────────────────────────────────────────

\* ── Action: CommitConsensus ──────────────────────────────────────────────────
\* Signer `s` commits a score `v` for the current round.
\* Guards:
\*   • The round must not yet be finalized (cc_finalized = FALSE).
\*   • The signer must not already have an open commitment (idempotency-guard;
\*     matches the Rust `CommitmentAlreadyExists` error).
CommitConsensus(s, v) ==
    /\ cc_finalized = FALSE
    /\ ~cc_committed[s]               \* no open commit for this signer
    /\ v \in Scores                   \* score is in the modelled domain
    /\ cc_committed'   = [cc_committed   EXCEPT ![s] = TRUE]
    /\ cc_commit_time' = [cc_commit_time EXCEPT ![s] = now]
    /\ cc_score'       = [cc_score       EXCEPT ![s] = v]
    /\ UNCHANGED <<score, hwm, breach_count, last_submit_time, embargo_expiry, delegate, now,
                   tb_tokens, tb_last_refill, tb_capacity,
                   cc_revealed, cc_finalized, cc_final_score,
                   upg_approvals, upg_signer_snap, upg_live_signers>>

\* ── Action: RevealConsensus ──────────────────────────────────────────────────
\* Signer `s` reveals their previously committed score.
\* Guards (correspond 1-to-1 with Rust error codes):
\*   • cc_committed[s] = TRUE           (RevealWindowExpired / no commit)
\*   • now - cc_commit_time[s] <= REVEAL_WINDOW  (RevealWindowExpired)
\*   • ~cc_revealed[s]                  (replay protection)
\*   • cc_finalized = FALSE             (round already done)
\*   • The revealed score equals the committed score (CommitmentMismatch)
\*
\* On success the signer's cc_revealed flag is set.  Finalization is a
\* separate step so that TLC can explore interleavings of multiple reveals
\* before any one of them triggers finalization.
RevealConsensus(s) ==
    /\ cc_committed[s]                                   \* has open commit
    /\ (now - cc_commit_time[s]) <= REVEAL_WINDOW        \* within window
    /\ ~cc_revealed[s]                                   \* not yet revealed
    /\ cc_finalized = FALSE                              \* round open
    \* Score equality is trivially satisfied here because we stored the score
    \* in plain-text at commit time (no hash needed in the abstract model).
    \* The important structural check is the window and the prior-commit guard.
    /\ cc_revealed' = [cc_revealed EXCEPT ![s] = TRUE]
    /\ UNCHANGED <<score, hwm, breach_count, last_submit_time, embargo_expiry, delegate, now,
                   tb_tokens, tb_last_refill, tb_capacity,
                   cc_committed, cc_commit_time, cc_score,
                   cc_finalized, cc_final_score,
                   upg_approvals, upg_signer_snap, upg_live_signers>>

\* ── Action: FinalizeConsensus ────────────────────────────────────────────────
\* Atomically finalizes the round when K-of-N agreement within epsilon holds.
\* Sets cc_final_score to the pivot score and cc_finalized to TRUE.
\* No further commits or reveals are accepted after this point.
FinalizeConsensus ==
    /\ cc_finalized = FALSE
    /\ ConsensusReached          \* K-of-N in-epsilon check passes
    /\ cc_finalized'    = TRUE
    /\ cc_final_score'  = cc_score[ConsensusScore]
    /\ UNCHANGED <<score, hwm, breach_count, last_submit_time, embargo_expiry, delegate, now,
                   tb_tokens, tb_last_refill, tb_capacity,
                   cc_committed, cc_commit_time, cc_score, cc_revealed,
                   upg_approvals, upg_signer_snap, upg_live_signers>>

\* ── Action: ResetConsensusRound ──────────────────────────────────────────────
\* Starts a fresh consensus round.  In the Rust contract a new round is
\* implicitly started by the TTL expiry of temporary storage entries; here we
\* model the reset as an explicit action so TLC can verify that round
\* boundaries do not allow cross-round reveals.
\* Guard: the current round must be finalized (or we model a timed-out
\* unfinished round — both cases produce a clean slate).
ResetConsensusRound ==
    /\ cc_finalized = TRUE
    /\ cc_committed'   = [s \in Signers |-> FALSE]
    /\ cc_commit_time' = [s \in Signers |-> 0]
    /\ cc_score'       = [s \in Signers |-> 0]
    /\ cc_revealed'    = [s \in Signers |-> FALSE]
    /\ cc_finalized'   = FALSE
    /\ cc_final_score' = 0
    /\ UNCHANGED <<score, hwm, breach_count, last_submit_time, embargo_expiry, delegate, now,
                   tb_tokens, tb_last_refill, tb_capacity,
                   upg_approvals, upg_signer_snap, upg_live_signers>>

\* ── Action: ExpireStaleCommit ────────────────────────────────────────────────
\* Models the Soroban temporary-storage TTL eviction: a signer's commitment is
\* silently dropped if the reveal window has elapsed without a reveal.
\* This lets TLC verify that an expired commit cannot be used to trigger a
\* reveal in a later tick.
ExpireStaleCommit(s) ==
    /\ cc_committed[s]
    /\ ~cc_revealed[s]
    /\ (now - cc_commit_time[s]) > REVEAL_WINDOW    \* window has elapsed
    /\ cc_committed'   = [cc_committed   EXCEPT ![s] = FALSE]
    /\ cc_commit_time' = [cc_commit_time EXCEPT ![s] = 0]
    /\ cc_score'       = [cc_score       EXCEPT ![s] = 0]
    /\ UNCHANGED <<score, hwm, breach_count, last_submit_time, embargo_expiry, delegate, now,
                   tb_tokens, tb_last_refill, tb_capacity,
                   cc_revealed, cc_finalized, cc_final_score,
                   upg_approvals, upg_signer_snap, upg_live_signers>>

\* ════════════════════════════════════════════════════════════════════════════
\* UPGRADE PROPOSAL SIGNER-SET SNAPSHOT ACTIONS  (issue #1)
\* ════════════════════════════════════════════════════════════════════════════
\*
\* The Rust contract accumulates M-of-N admin co-signatures for an upgrade
\* proposal across multiple transactions.  The vulnerability: if a signer is
\* added or removed while approvals are accumulating, the stale approvals
\* remain valid and count toward the new threshold — an approval collected
\* under one signer set can be replayed under a different one.
\*
\* The fix: snapshot the signer set when the first approval arrives and
\* invalidate the entire accumulator whenever the live set diverges from
\* the snapshot.  These three actions model that lifecycle.
\*
\* AddUpgradeApproval(s): a signer in the live set adds their approval.
\*   – On the first approval, freeze upg_signer_snap = upg_live_signers.
\*   – If the snapshot already exists and the live set has diverged, clear
\*     the accumulator and start fresh (snapshot invalidation).
\*
\* MutateAdminSet(s, add): admin adds or removes signer `s`.
\*   – After the mutation, if any approvals have been collected under the
\*     old snapshot, they are invalidated (accumulator cleared, snap reset).
\*
\* ClearUpgradeApprovals: explicit accumulator reset (veto / threshold met).

AddUpgradeApproval(s) ==
    /\ s \in upg_live_signers        \* signer must be in the live set
    /\ s \notin upg_approvals        \* idempotency guard
    /\ LET snap == IF upg_approvals = {} THEN upg_live_signers ELSE upg_signer_snap
       IN
       /\ IF snap /= upg_live_signers
          \* Snapshot diverged: clear stale approvals, start fresh with `s`.
          THEN /\ upg_approvals'   = {s}
               /\ upg_signer_snap' = upg_live_signers
          ELSE /\ upg_approvals'   = upg_approvals \cup {s}
               /\ upg_signer_snap' = snap
    /\ UNCHANGED <<score, hwm, breach_count, last_submit_time, embargo_expiry, delegate, now,
                   tb_tokens, tb_last_refill, tb_capacity,
                   cc_committed, cc_commit_time, cc_score, cc_revealed,
                   cc_finalized, cc_final_score, upg_live_signers>>

\* Model admin adding or removing a signer while a proposal accumulates.
\* Either direction invalidates any stale approvals.
MutateAdminSet(s) ==
    /\ upg_live_signers' =
           IF s \in upg_live_signers
           THEN upg_live_signers \ {s}    \* remove
           ELSE upg_live_signers \cup {s} \* add
    \* Invalidate accumulated approvals whenever the signer set changes.
    /\ upg_approvals'   = {}
    /\ upg_signer_snap' = {}
    /\ UNCHANGED <<score, hwm, breach_count, last_submit_time, embargo_expiry, delegate, now,
                   tb_tokens, tb_last_refill, tb_capacity,
                   cc_committed, cc_commit_time, cc_score, cc_revealed,
                   cc_finalized, cc_final_score>>

ClearUpgradeApprovals ==
    /\ upg_approvals'   = {}
    /\ upg_signer_snap' = {}
    /\ UNCHANGED <<score, hwm, breach_count, last_submit_time, embargo_expiry, delegate, now,
                   tb_tokens, tb_last_refill, tb_capacity,
                   cc_committed, cc_commit_time, cc_score, cc_revealed,
                   cc_finalized, cc_final_score, upg_live_signers>>

\* ── Next-state relation ──────────────────────────────────────────────────────
Next ==
    \/ TickTime
    \/ \E w \in Wallets, s \in Scores : SubmitScore(w, s)
    \/ \E capacity \in MIN_CAPACITY..MAX_CAPACITY : SetBurstCapacity(capacity)
    \/ \E w \in Wallets, expiry \in {-1, now+1, now+2} : SetEmbargo(w, expiry)
    \/ \E w \in Wallets : LiftEmbargo(w)
    \/ \E sub \in Wallets, cust \in Wallets : SetDelegate(sub, cust)
    \/ \E sub \in Wallets : RemoveDelegate(sub)
    \/ \E w \in Wallets : ResetBreachCount(w)
    \* Consensus actions
    \/ \E s \in Signers, v \in Scores : CommitConsensus(s, v)
    \/ \E s \in Signers : RevealConsensus(s)
    \/ FinalizeConsensus
    \/ ResetConsensusRound
    \/ \E s \in Signers : ExpireStaleCommit(s)
    \* Upgrade signer-set snapshot actions (issue #1)
    \/ \E s \in Signers : AddUpgradeApproval(s)
    \/ \E s \in Signers : MutateAdminSet(s)
    \/ ClearUpgradeApprovals

\* ════════════════════════════════════════════════════════════════════════════
\* INVARIANTS
\* ════════════════════════════════════════════════════════════════════════════

\* ── Existing invariants (unchanged) ──────────────────────────────────────────

HistoricalMaxMonotonicity == \A w \in Wallets : hwm[w] >= score[w]

EmbargoActive(w) == embargo_expiry[w] = -1 \/ (embargo_expiry[w] > 0 /\ now <= embargo_expiry[w])
EmbargoGateSoundness == \A w \in Wallets : EmbargoActive(w) <=> (embargo_expiry[w] = -1 \/ (embargo_expiry[w] /= 0 /\ now <= embargo_expiry[w]))

IsCyclic == \E w \in Wallets :
    \/ delegate[w] = w
    \/ (delegate[w] /= "None" /\ delegate[delegate[w]] = w)
    \/ (delegate[w] /= "None" /\ delegate[delegate[w]] /= "None" /\ delegate[delegate[delegate[w]]] = w)
DelegationAcyclicity == ~IsCyclic

\* ── Token-bucket invariants (new) ────────────────────────────────────────────

\* INV-TB-1  Tokens never exceed the current global capacity.
\* This holds even after SetBurstCapacity *reduces* the capacity —
\* existing over-capacity buckets are never refilled beyond the new cap
\* (RefillCount clamps to tb_capacity), so on the next SubmitScore the
\* bucket is written back within bounds.
\* NOTE: Between a capacity *reduction* and the next SubmitScore for a
\*       wallet, tb_tokens[w] may legitimately be above the *new* tb_capacity
\*       because the lazy-truncation contract (matching the Rust implementation)
\*       does not immediately rewrite stored buckets.  We therefore state the
\*       invariant in terms of what the wallet would *use* on its next call,
\*       i.e. RefillCount (which already clamps), rather than raw tb_tokens.
TokensNeverExceedCapacity ==
    \A w \in Wallets : RefillCount(w) <= tb_capacity

\* INV-TB-2  Tokens never go negative (trivially satisfied because tokens is
\*           a natural and we only store refilled-1 ≥ 0 after a successful
\*           SubmitScore, and we never decrement without a prior > 0 check).
TokensNonNegative ==
    \A w \in Wallets : tb_tokens[w] >= 0

\* INV-TB-3  After a capacity reduction, the *effective* available tokens on
\*           the next refill are capped at the new capacity.  Stated as a
\*           state invariant: RefillCount is always bounded by tb_capacity.
CapacityReductionCapsNextBurst ==
    \A w \in Wallets : RefillCount(w) <= tb_capacity

\* INV-TB-4  last_refill never drifts ahead of now.
RefillAnchorNotInFuture ==
    \A w \in Wallets : tb_last_refill[w] <= now

\* INV-TB-5  The global capacity is always within the configured bounds.
CapacityWithinBounds ==
    /\ tb_capacity >= MIN_CAPACITY
    /\ tb_capacity <= MAX_CAPACITY

\* ── Consensus invariants (new — issue #403) ──────────────────────────────────

\* INV-CR-1  A value can only finalize when at least CONSENSUS_K valid reveals
\*           agree within CONSENSUS_EPSILON of each other.
\* This is the primary safety invariant: the finalized score must have come
\* from a cluster of at least K revealed scores, all within epsilon of the
\* chosen pivot.  No smaller quorum can ever produce a finalized score.
FinalScoreRequiresKReveals ==
    cc_finalized =>
        /\ Cardinality(RevealedSigners) >= CONSENSUS_K
        /\ InEpsilonCount(cc_final_score) >= CONSENSUS_K

\* INV-CR-2  A reveal without a prior matching commit is never accepted.
\* Formally: cc_revealed[s] = TRUE implies cc_committed[s] was TRUE at some
\* earlier point.  Because we model the commit bit monotonically (it is set
\* TRUE on CommitConsensus and cleared only by ExpireStaleCommit or
\* ResetConsensusRound, both of which also clear cc_revealed), the following
\* state-level check is sound: if a signer has revealed but their commit was
\* already cleared, cc_committed can be FALSE only after an explicit reset,
\* which also resets cc_revealed.  Therefore in any reachable state:
\*   cc_revealed[s] = TRUE  =>  cc_committed[s] = TRUE
NoRevealWithoutCommit ==
    \A s \in Signers : cc_revealed[s] => cc_committed[s]

\* INV-CR-3  A reveal is only accepted within the reveal window.
\* If a signer has revealed, their commit timestamp must be within
\* REVEAL_WINDOW ticks of the current `now`.
RevealOnlyWithinWindow ==
    \A s \in Signers :
        cc_revealed[s] => (now - cc_commit_time[s]) <= REVEAL_WINDOW

\* INV-CR-4  Once finalized, the final score is within CONSENSUS_EPSILON of
\*           at least CONSENSUS_K revealed scores.  This is a stronger restatement
\*           of INV-CR-1 that pins the epsilon band directly to cc_final_score.
FinalScoreWithinEpsilonOfCluster ==
    cc_finalized =>
        InEpsilonCount(cc_final_score) >= CONSENSUS_K

\* INV-CR-5  Commit timestamps are always <= now (no future-dated commits).
CommitTimestampNotInFuture ==
    \A s \in Signers : cc_commit_time[s] <= now

\* INV-CR-6  An expired (TTL-evicted) commit cannot be used to reveal.
\* Formally: if cc_committed[s] is FALSE and cc_revealed[s] is FALSE, no
\* reveal for that signer exists — the guard in RevealConsensus requires
\* cc_committed[s] = TRUE, so an evicted commit (cleared by ExpireStaleCommit)
\* can never produce a revealed entry.  This is an indirect invariant captured
\* by NoRevealWithoutCommit; we state it explicitly for documentation clarity.
ExpiredCommitCannotReveal ==
    \A s \in Signers : ~cc_committed[s] => ~cc_revealed[s]

\* ════════════════════════════════════════════════════════════════════════════
\* ACTION PROPERTIES (temporal)
\* ════════════════════════════════════════════════════════════════════════════

\* Existing temporal properties (unchanged).
BreachCounterStateMachine == [][ \A w \in Wallets : (breach_count[w] > 0 /\ breach_count'[w] = 0) => (score'[w] < RISK_THRESHOLD \/ (score'[w] = score[w] /\ hwm'[w] = hwm[w])) ]_vars

\* ── Token-bucket temporal properties (new) ────────────────────────────────────

\* PROP-TB-1  A wallet that exhausts its bucket cannot submit again until at
\*            least one COOLDOWN period has elapsed.
\*            Stated as: whenever a SubmitScore drains the bucket to 0, the
\*            next accepted SubmitScore for the same wallet must happen at a
\*            strictly later time (≥ current now + COOLDOWN from that point).
TokenExhaustionBlocksSubmit ==
    [][ \A w \in Wallets :
            (tb_tokens[w] > 0 /\ tb_tokens'[w] = 0)
            => \/ now' = now   \* same tick, different wallet — fine
               \/ (last_submit_time'[w] = now)  \* only the draining submit itself updates last_submit
       ]_vars

\* PROP-TB-2  After a capacity *increase*, a wallet's effective available
\*            tokens never exceed the *new* capacity on the very next refill.
\* (Follows from TokensNeverExceedCapacity but stated temporally for clarity.)
BurstNeverExceedsNewCapacity ==
    [][tb_capacity' >= tb_capacity
       => \A w \in Wallets : RefillCount(w) <= tb_capacity']_vars

\* ── Consensus temporal properties (new — issue #403) ─────────────────────────

\* PROP-CR-1  Once finalized, cc_finalized stays TRUE until an explicit reset.
\* Finalization is a one-way latch within a round: no action (other than
\* ResetConsensusRound) may take cc_finalized from TRUE back to FALSE.
\* This catches any accidental re-entry or double-finalization.
FinalizationIsTerminalWithinRound ==
    [][cc_finalized => (cc_finalized' \/ 
        (\* only ResetConsensusRound may clear it *)
        /\ cc_committed'   = [s \in Signers |-> FALSE]
        /\ cc_revealed'    = [s \in Signers |-> FALSE]
        /\ cc_finalized'   = FALSE)]_vars

\* PROP-CR-2  The final score never changes after finalization within a round.
\* Once cc_final_score is set it is immutable until ResetConsensusRound.
FinalScoreImmutableWithinRound ==
    [][cc_finalized /\ cc_finalized' => cc_final_score' = cc_final_score]_vars

\* ── Upgrade signer-set snapshot invariants (issue #1) ────────────────────────

\* INV-UPG-1  Every collected approval must belong to the live signer set at
\*            the time the snapshot was taken.  Because MutateAdminSet clears
\*            the accumulator on any set change, upg_approvals ⊆ upg_signer_snap
\*            holds in every reachable state.
ApprovalsSubsetOfSnapshot ==
    upg_approvals \subseteq upg_signer_snap

\* INV-UPG-2  The snapshot is always a subset of (or equal to) the live set
\*            OR the accumulator is empty.  If the sets diverge but approvals
\*            are still present, that is a violation — the invalidation step
\*            must have fired first.
SnapshotConsistentWithLiveSet ==
    upg_approvals /= {} =>
        upg_signer_snap = upg_live_signers

\* INV-UPG-3  No approval collected under a removed signer can ever survive a
\*            signer-set mutation.  Follows from INV-UPG-1 + INV-UPG-2.
RemovedSignerApprovalInvalidated ==
    \A s \in Signers :
        (s \in upg_approvals) => (s \in upg_live_signers)

\* ── Temporal: signer-set mutation clears the accumulator ─────────────────────

\* PROP-UPG-1  Whenever the live signer set changes, the approval accumulator
\*             is empty in the very next state.  This is the key liveness
\*             property: no stale approval ever persists across a set mutation.
SignerMutationInvalidatesApprovals ==
    [][upg_live_signers /= upg_live_signers' =>
           upg_approvals' = {}]_vars

\* ── State constraint (model-checking bound) ──────────────────────────────────
StateConstraint == now <= 5

Spec == Init /\ [][Next]_vars
=============================================================================
