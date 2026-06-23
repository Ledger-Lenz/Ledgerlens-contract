---------------------------- MODULE LedgerLens ----------------------------
EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS 
    Wallets,
    Scores,
    COOLDOWN,
    HWM_THRESHOLD,
    FLOOR_VALUE,
    RISK_THRESHOLD

VARIABLES 
    score,
    hwm,
    breach_count,
    last_submit_time,
    embargo_expiry,
    delegate,
    now

vars == <<score, hwm, breach_count, last_submit_time, embargo_expiry, delegate, now>>

\* Initialization
Init ==
    /\ score = [w \in Wallets |-> 0]
    /\ hwm = [w \in Wallets |-> 0]
    /\ breach_count = [w \in Wallets |-> 0]
    /\ last_submit_time = [w \in Wallets |-> 0]
    /\ embargo_expiry = [w \in Wallets |-> 0]
    /\ delegate = [w \in Wallets |-> "None"]
    /\ now = 1

\* Actions
TickTime ==
    /\ now' = now + 1
    /\ UNCHANGED <<score, hwm, breach_count, last_submit_time, embargo_expiry, delegate>>

SubmitScore(w, s) ==
    /\ last_submit_time[w] = 0 \/ now >= last_submit_time[w] + COOLDOWN
    /\ hwm[w] >= HWM_THRESHOLD => s >= FLOOR_VALUE
    /\ score' = [score EXCEPT ![w] = s]
    /\ hwm' = [hwm EXCEPT ![w] = IF s > hwm[w] THEN s ELSE hwm[w]]
    /\ breach_count' = [breach_count EXCEPT ![w] = IF s >= RISK_THRESHOLD THEN breach_count[w] + 1 ELSE 0]
    /\ last_submit_time' = [last_submit_time EXCEPT ![w] = now]
    /\ UNCHANGED <<embargo_expiry, delegate, now>>

SetEmbargo(w, expiry) ==
    /\ embargo_expiry' = [embargo_expiry EXCEPT ![w] = expiry]
    /\ UNCHANGED <<score, hwm, breach_count, last_submit_time, delegate, now>>

LiftEmbargo(w) ==
    /\ embargo_expiry' = [embargo_expiry EXCEPT ![w] = 0]
    /\ UNCHANGED <<score, hwm, breach_count, last_submit_time, delegate, now>>

SetDelegate(sub, cust) ==
    /\ sub /= cust
    /\ delegate[cust] /= sub
    /\ delegate[cust] /= "None" => delegate[delegate[cust]] /= sub
    /\ delegate' = [delegate EXCEPT ![sub] = cust]
    /\ UNCHANGED <<score, hwm, breach_count, last_submit_time, embargo_expiry, now>>

RemoveDelegate(sub) ==
    /\ delegate' = [delegate EXCEPT ![sub] = "None"]
    /\ UNCHANGED <<score, hwm, breach_count, last_submit_time, embargo_expiry, now>>

ResetBreachCount(w) ==
    /\ breach_count' = [breach_count EXCEPT ![w] = 0]
    /\ UNCHANGED <<score, hwm, last_submit_time, embargo_expiry, delegate, now>>

Next ==
    \/ TickTime
    \/ \E w \in Wallets, s \in Scores : SubmitScore(w, s)
    \/ \E w \in Wallets, expiry \in {-1, now+1, now+2} : SetEmbargo(w, expiry)
    \/ \E w \in Wallets : LiftEmbargo(w)
    \/ \E sub \in Wallets, cust \in Wallets : SetDelegate(sub, cust)
    \/ \E sub \in Wallets : RemoveDelegate(sub)
    \/ \E w \in Wallets : ResetBreachCount(w)

\* Invariants (State)
HistoricalMaxMonotonicity == \A w \in Wallets : hwm[w] >= score[w]

EmbargoActive(w) == embargo_expiry[w] = -1 \/ (embargo_expiry[w] > 0 /\ now <= embargo_expiry[w])
EmbargoGateSoundness == \A w \in Wallets : EmbargoActive(w) <=> (embargo_expiry[w] = -1 \/ (embargo_expiry[w] /= 0 /\ now <= embargo_expiry[w]))

IsCyclic == \E w \in Wallets :
    \/ delegate[w] = w
    \/ (delegate[w] /= "None" /\ delegate[delegate[w]] = w)
    \/ (delegate[w] /= "None" /\ delegate[delegate[w]] /= "None" /\ delegate[delegate[delegate[w]]] = w)
DelegationAcyclicity == ~IsCyclic

FloorNeverBypassed == \A w \in Wallets : hwm[w] >= HWM_THRESHOLD => (score[w] >= FLOOR_VALUE \/ score[w] = 0)

\* Action Properties
BreachCounterStateMachine == [][ \A w \in Wallets : (breach_count[w] > 0 /\ breach_count'[w] = 0) => (score'[w] < RISK_THRESHOLD \/ (score'[w] = score[w] /\ hwm'[w] = hwm[w])) ]_vars

CooldownEnforcement == [][ \A w \in Wallets : (last_submit_time'[w] /= last_submit_time[w] /\ last_submit_time[w] /= 0) => now >= last_submit_time[w] + COOLDOWN ]_vars

StateConstraint == now <= 3

Spec == Init /\ [][Next]_vars
=============================================================================
