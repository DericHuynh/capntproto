---------------------- MODULE HandoffCounterBoundary -----------------------
EXTENDS Naturals
CONSTANTS Bound, Kind, Fault
\* Proxy mode represents the last Bound slots of the cumulative drain counter.
\* Direct mode starts in Direct with Bound invocations left. Rust replay adds
\* u64::MAX-Bound to the relevant counter; keys/cryptography are not modeled.
VARIABLES phase, pending, drained, direct, admitted, revoked,
          result, expected, event, steps
vars == <<phase,pending,drained,direct,admitted,revoked,result,expected,event,steps>>
Init == /\ phase=(IF Kind="proxy" THEN 0 ELSE 3)
        /\ pending=0 /\ drained=0 /\ direct=0 /\ admitted=0 /\ revoked=0
        /\ result=0 /\ expected=0 /\ event=0 /\ steps=0
Enqueue ==
    /\ steps<5
    /\ LET normal == phase=0 /\ revoked=0 /\ pending+drained<Bound
           allow == IF Fault="unreserved" THEN phase=0 /\ revoked=0 /\ pending<Bound
                    ELSE normal
       IN /\ pending'=pending+(IF allow THEN 1 ELSE 0)
          /\ admitted'=admitted+(IF allow THEN 1 ELSE 0)
          /\ result'=(IF allow THEN 1 ELSE 0) /\ expected'=(IF normal THEN 1 ELSE 0)
    /\ event'=1 /\ steps'=steps+1
    /\ UNCHANGED <<phase,drained,direct,revoked>>
Drain ==
    /\ steps<5
    /\ pending'=(IF pending>0 THEN pending-1 ELSE pending)
    /\ drained'=(IF pending>0 /\ Fault#"lostDrain" THEN drained+1 ELSE drained)
    /\ result'=(IF pending>0 THEN 1 ELSE 0) /\ expected'=result'
    /\ event'=2 /\ steps'=steps+1
    /\ UNCHANGED <<phase,direct,admitted,revoked>>
Provide ==
    /\ steps<5
    /\ LET allow == phase=0 /\ revoked=0
       IN /\ phase'=(IF allow THEN 1 ELSE phase)
          /\ result'=(IF allow THEN 1 ELSE 0) /\ expected'=result'
    /\ event'=3 /\ steps'=steps+1
    /\ UNCHANGED <<pending,drained,direct,admitted,revoked>>
Accept(authorized) ==
    /\ steps<5
    /\ LET normal == authorized /\ phase#0 /\ revoked=0
           allow == authorized /\ phase#0 /\ (revoked=0 \/ Fault="revokedAccept")
       IN /\ phase'=(IF allow /\ phase=1 THEN 2 ELSE phase)
          /\ result'=(IF allow THEN 1 ELSE 0) /\ expected'=(IF normal THEN 1 ELSE 0)
    /\ event'=(IF authorized THEN 4 ELSE 5) /\ steps'=steps+1
    /\ UNCHANGED <<pending,drained,direct,admitted,revoked>>
Lift ==
    /\ steps<5
    /\ LET normal == phase=2 /\ pending=0 /\ revoked=0
           allow == phase=2 /\ revoked=0 /\ (pending=0 \/ Fault="earlyLift")
       IN /\ phase'=(IF allow THEN 3 ELSE phase)
          /\ result'=(IF allow THEN 1 ELSE 0) /\ expected'=(IF normal THEN 1 ELSE 0)
    /\ event'=6 /\ steps'=steps+1
    /\ UNCHANGED <<pending,drained,direct,admitted,revoked>>
Direct ==
    /\ steps<5 /\ Kind="direct"
    /\ LET normal == phase=3 /\ revoked=0 /\ direct<Bound
           allow == phase=3 /\ revoked=0 /\ (direct<Bound \/ Fault="wrappingDirect")
       IN /\ direct'=(IF allow THEN (direct+1) % (Bound+1) ELSE direct)
          /\ result'=(IF allow THEN 1 ELSE 0) /\ expected'=(IF normal THEN 1 ELSE 0)
    /\ event'=7 /\ steps'=steps+1
    /\ UNCHANGED <<phase,pending,drained,admitted,revoked>>
Revoke ==
    /\ steps<5 /\ revoked'=1
    /\ result'=(IF revoked=0 THEN 1 ELSE 0) /\ expected'=result'
    /\ event'=8 /\ steps'=steps+1
    /\ UNCHANGED <<phase,pending,drained,direct,admitted>>
Next == Enqueue \/ Drain \/ Provide \/ Accept(TRUE) \/ Accept(FALSE) \/ Lift \/ Direct \/ Revoke
Spec == Init /\ [][Next]_vars
TypeOK == /\ phase\in 0..3 /\ pending\in 0..(Bound+1) /\ drained\in 0..(Bound+1)
          /\ direct\in 0..Bound /\ admitted\in 0..(Bound+1) /\ revoked\in 0..1
          /\ result\in 0..1 /\ expected\in 0..1 /\ event\in 0..8 /\ steps\in 0..5
DrainCapacity == pending+drained<=Bound
Conservation == pending+drained=admitted
Embargo == phase=3 => pending=0
ExpectedOutcome == result=expected
=============================================================================
