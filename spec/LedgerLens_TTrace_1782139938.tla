---- MODULE LedgerLens_TTrace_1782139938 ----
EXTENDS Sequences, TLCExt, Toolbox, Naturals, TLC, LedgerLens

_expression ==
    LET LedgerLens_TEExpression == INSTANCE LedgerLens_TEExpression
    IN LedgerLens_TEExpression!expression
----

_trace ==
    LET LedgerLens_TETrace == INSTANCE LedgerLens_TETrace
    IN LedgerLens_TETrace!trace
----

_inv ==
    ~(
        TLCGet("level") = Len(_TETrace)
        /\
        delegate = ([W1 |-> "W3", W2 |-> "W1", W3 |-> "W2"])
        /\
        last_submit_time = ([W1 |-> 0, W2 |-> 0, W3 |-> 0])
        /\
        breach_count = ([W1 |-> 0, W2 |-> 0, W3 |-> 0])
        /\
        score = ([W1 |-> 0, W2 |-> 0, W3 |-> 0])
        /\
        embargo_expiry = ([W1 |-> 0, W2 |-> 0, W3 |-> 0])
        /\
        now = (1)
        /\
        hwm = ([W1 |-> 0, W2 |-> 0, W3 |-> 0])
    )
----

_init ==
    /\ last_submit_time = _TETrace[1].last_submit_time
    /\ now = _TETrace[1].now
    /\ breach_count = _TETrace[1].breach_count
    /\ hwm = _TETrace[1].hwm
    /\ delegate = _TETrace[1].delegate
    /\ score = _TETrace[1].score
    /\ embargo_expiry = _TETrace[1].embargo_expiry
----

_next ==
    /\ \E i,j \in DOMAIN _TETrace:
        /\ \/ /\ j = i + 1
              /\ i = TLCGet("level")
        /\ last_submit_time  = _TETrace[i].last_submit_time
        /\ last_submit_time' = _TETrace[j].last_submit_time
        /\ now  = _TETrace[i].now
        /\ now' = _TETrace[j].now
        /\ breach_count  = _TETrace[i].breach_count
        /\ breach_count' = _TETrace[j].breach_count
        /\ hwm  = _TETrace[i].hwm
        /\ hwm' = _TETrace[j].hwm
        /\ delegate  = _TETrace[i].delegate
        /\ delegate' = _TETrace[j].delegate
        /\ score  = _TETrace[i].score
        /\ score' = _TETrace[j].score
        /\ embargo_expiry  = _TETrace[i].embargo_expiry
        /\ embargo_expiry' = _TETrace[j].embargo_expiry

\* Uncomment the ASSUME below to write the states of the error trace
\* to the given file in Json format. Note that you can pass any tuple
\* to `JsonSerialize`. For example, a sub-sequence of _TETrace.
    \* ASSUME
    \*     LET J == INSTANCE Json
    \*         IN J!JsonSerialize("LedgerLens_TTrace_1782139938.json", _TETrace)

=============================================================================

 Note that you can extract this module `LedgerLens_TEExpression`
  to a dedicated file to reuse `expression` (the module in the 
  dedicated `LedgerLens_TEExpression.tla` file takes precedence 
  over the module `LedgerLens_TEExpression` below).

---- MODULE LedgerLens_TEExpression ----
EXTENDS Sequences, TLCExt, Toolbox, Naturals, TLC, LedgerLens

expression == 
    [
        \* To hide variables of the `LedgerLens` spec from the error trace,
        \* remove the variables below.  The trace will be written in the order
        \* of the fields of this record.
        last_submit_time |-> last_submit_time
        ,now |-> now
        ,breach_count |-> breach_count
        ,hwm |-> hwm
        ,delegate |-> delegate
        ,score |-> score
        ,embargo_expiry |-> embargo_expiry
        
        \* Put additional constant-, state-, and action-level expressions here:
        \* ,_stateNumber |-> _TEPosition
        \* ,_last_submit_timeUnchanged |-> last_submit_time = last_submit_time'
        
        \* Format the `last_submit_time` variable as Json value.
        \* ,_last_submit_timeJson |->
        \*     LET J == INSTANCE Json
        \*     IN J!ToJson(last_submit_time)
        
        \* Lastly, you may build expressions over arbitrary sets of states by
        \* leveraging the _TETrace operator.  For example, this is how to
        \* count the number of times a spec variable changed up to the current
        \* state in the trace.
        \* ,_last_submit_timeModCount |->
        \*     LET F[s \in DOMAIN _TETrace] ==
        \*         IF s = 1 THEN 0
        \*         ELSE IF _TETrace[s].last_submit_time # _TETrace[s-1].last_submit_time
        \*             THEN 1 + F[s-1] ELSE F[s-1]
        \*     IN F[_TEPosition - 1]
    ]

=============================================================================



Parsing and semantic processing can take forever if the trace below is long.
 In this case, it is advised to uncomment the module below to deserialize the
 trace from a generated binary file.

\*
\*---- MODULE LedgerLens_TETrace ----
\*EXTENDS IOUtils, TLC, LedgerLens
\*
\*trace == IODeserialize("LedgerLens_TTrace_1782139938.bin", TRUE)
\*
\*=============================================================================
\*

---- MODULE LedgerLens_TETrace ----
EXTENDS TLC, LedgerLens

trace == 
    <<
    ([delegate |-> [W1 |-> "None", W2 |-> "None", W3 |-> "None"],last_submit_time |-> [W1 |-> 0, W2 |-> 0, W3 |-> 0],breach_count |-> [W1 |-> 0, W2 |-> 0, W3 |-> 0],score |-> [W1 |-> 0, W2 |-> 0, W3 |-> 0],embargo_expiry |-> [W1 |-> 0, W2 |-> 0, W3 |-> 0],now |-> 1,hwm |-> [W1 |-> 0, W2 |-> 0, W3 |-> 0]]),
    ([delegate |-> [W1 |-> "None", W2 |-> "W1", W3 |-> "None"],last_submit_time |-> [W1 |-> 0, W2 |-> 0, W3 |-> 0],breach_count |-> [W1 |-> 0, W2 |-> 0, W3 |-> 0],score |-> [W1 |-> 0, W2 |-> 0, W3 |-> 0],embargo_expiry |-> [W1 |-> 0, W2 |-> 0, W3 |-> 0],now |-> 1,hwm |-> [W1 |-> 0, W2 |-> 0, W3 |-> 0]]),
    ([delegate |-> [W1 |-> "None", W2 |-> "W1", W3 |-> "W2"],last_submit_time |-> [W1 |-> 0, W2 |-> 0, W3 |-> 0],breach_count |-> [W1 |-> 0, W2 |-> 0, W3 |-> 0],score |-> [W1 |-> 0, W2 |-> 0, W3 |-> 0],embargo_expiry |-> [W1 |-> 0, W2 |-> 0, W3 |-> 0],now |-> 1,hwm |-> [W1 |-> 0, W2 |-> 0, W3 |-> 0]]),
    ([delegate |-> [W1 |-> "W3", W2 |-> "W1", W3 |-> "W2"],last_submit_time |-> [W1 |-> 0, W2 |-> 0, W3 |-> 0],breach_count |-> [W1 |-> 0, W2 |-> 0, W3 |-> 0],score |-> [W1 |-> 0, W2 |-> 0, W3 |-> 0],embargo_expiry |-> [W1 |-> 0, W2 |-> 0, W3 |-> 0],now |-> 1,hwm |-> [W1 |-> 0, W2 |-> 0, W3 |-> 0]])
    >>
----


=============================================================================

---- CONFIG LedgerLens_TTrace_1782139938 ----
CONSTANTS
    Wallets = { "W1" , "W2" , "W3" }
    Scores = { 0 , 20 , 50 , 80 , 100 }
    COOLDOWN = 1
    HWM_THRESHOLD = 80
    FLOOR_VALUE = 20
    RISK_THRESHOLD = 50

INVARIANT
    _inv

CHECK_DEADLOCK
    \* CHECK_DEADLOCK off because of PROPERTY or INVARIANT above.
    FALSE

INIT
    _init

NEXT
    _next

CONSTANT
    _TETrace <- _trace

ALIAS
    _expression
=============================================================================
\* Generated on Mon Jun 22 15:52:19 WAT 2026