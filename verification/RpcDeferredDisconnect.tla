----------------------- MODULE RpcDeferredDisconnect -----------------------
EXTENDS Naturals, TLC
CONSTANTS AbortDownstream, LoseAccepted
VARIABLES connected,held,used,again,accepted,failed,calls,provision,healthy
vars == <<connected,held,used,again,accepted,failed,calls,provision,healthy>>
N(x) == IF x THEN 1 ELSE 0
Init == /\ connected=TRUE /\ held=TRUE /\ used=FALSE /\ again=FALSE
        /\ accepted=FALSE /\ failed=FALSE /\ calls=0 /\ provision=TRUE /\ healthy=TRUE
Disconnect == /\ connected /\ connected'=FALSE
              /\ accepted'=(accepted /\ ~LoseAccepted)
              /\ UNCHANGED <<held,used,again,failed,calls,healthy>>
Use == /\ held /\ ~used /\ used'=TRUE /\ accepted'=connected /\ failed'=~connected
       /\ calls'=calls+N(connected)
       /\ healthy'=(healthy /\ ~(AbortDownstream /\ ~connected))
       /\ UNCHANGED <<connected,held,again>>
Again == /\ held /\ used /\ ~again /\ again'=TRUE /\ calls'=calls+N(accepted)
         /\ UNCHANGED <<connected,held,used,accepted,failed,healthy>>
Drop == /\ held /\ held'=FALSE
        /\ UNCHANGED <<connected,used,again,accepted,failed,calls,healthy>>
Next == /\ (Disconnect \/ Use \/ Again \/ Drop)
        /\ provision'=(connected' /\ held' /\ ~used')
Spec == Init /\ [][Next]_vars
TypeOK == /\ \A x\in {connected,held,used,again,accepted,failed,provision,healthy}: x\in BOOLEAN
          /\ calls\in 0..2
FailureIsolation == healthy
AcceptedSurvives == accepted=(used /\ ~failed)
VineLifetime == provision=(connected /\ held /\ ~used)
=============================================================================
