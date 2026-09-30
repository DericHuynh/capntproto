------------------------- MODULE RpcExceptionTrace -------------------------
EXTENDS Naturals, TLC
CONSTANTS LeakDisabled, StaleEncoder
VARIABLES config, connected, first, second, trace1, trace2, expected1, expected2
vars == <<config,connected,first,second,trace1,trace2,expected1,expected2>>
\* One existing connection and one connection established at any point.
\* Configuration sequence: default off, encoder 1, encoder 2, explicitly off.
\* Each connection emits one exception. Run each wire exception path separately.
Encoder == IF config \in {0,3} THEN 0 ELSE config
Encode == IF LeakDisabled /\ Encoder = 0 THEN 1 ELSE
          IF StaleEncoder /\ Encoder = 2 THEN 1 ELSE Encoder
Init == /\ config = 0 /\ connected = FALSE /\ first = FALSE /\ second = FALSE
        /\ trace1 = 0 /\ trace2 = 0 /\ expected1 = 0 /\ expected2 = 0
Configure == /\ config < 3 /\ config' = config + 1
             /\ UNCHANGED <<connected,first,second,trace1,trace2,expected1,expected2>>
Connect == /\ ~connected /\ connected' = TRUE
           /\ UNCHANGED <<config,first,second,trace1,trace2,expected1,expected2>>
EmitFirst == /\ ~first /\ first' = TRUE /\ trace1' = Encode /\ expected1' = Encoder
             /\ UNCHANGED <<config,connected,second,trace2,expected2>>
EmitSecond == /\ connected /\ ~second /\ second' = TRUE /\ trace2' = Encode /\ expected2' = Encoder
              /\ UNCHANGED <<config,connected,first,trace1,expected1>>
Next == Configure \/ Connect \/ EmitFirst \/ EmitSecond
Spec == Init /\ [][Next]_vars
TypeOK == /\ config \in 0..3 /\ connected \in BOOLEAN /\ first \in BOOLEAN /\ second \in BOOLEAN
          /\ trace1 \in 0..2 /\ trace2 \in 0..2 /\ expected1 \in 0..2 /\ expected2 \in 0..2
CurrentEncoder == trace1 = expected1 /\ trace2 = expected2
NoUnconnectedEmission == second => connected
=============================================================================
