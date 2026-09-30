--------------------------- MODULE RpcDebugInfo ---------------------------
EXTENDS Naturals
CONSTANT Fault
\* A membrane around a promise, plus an independent lazy reconnecting client.
\* Delivery of a fulfiller's result does not itself poll the Rust promise.
\* Observations must describe existing state without changing its lifecycle.
VARIABLES offered, resolved, driven, revoked, connected, attempts, connects,
          diag, connDiag, wrapper, event
vars == <<offered,resolved,driven,revoked,connected,attempts,connects,
          diag,connDiag,wrapper,event>>
Init == /\ offered=0 /\ resolved=0 /\ driven=0 /\ revoked=0 /\ connected=0
        /\ attempts=0 /\ connects=0 /\ diag=0 /\ connDiag=0 /\ wrapper=0 /\ event=0
Offer(value) == /\ offered=0 /\ offered'=value /\ event'=value
                /\ UNCHANGED <<resolved,driven,revoked,connected,attempts,connects,
                               diag,connDiag,wrapper>>
Drive == /\ offered#0 /\ resolved=0 /\ resolved'=1 /\ driven'=1 /\ event'=3
         /\ UNCHANGED <<offered,revoked,connected,attempts,connects,diag,connDiag,wrapper>>
Revoke == /\ revoked=0 /\ revoked'=1 /\ event'=4
          /\ UNCHANGED <<offered,resolved,driven,connected,attempts,connects,diag,connDiag,wrapper>>
Connect == /\ connected=0 /\ attempts<2 /\ connected'=1
           /\ attempts'=attempts+1 /\ connects'=connects+1 /\ event'=5
           /\ UNCHANGED <<offered,resolved,driven,revoked,diag,connDiag,wrapper>>
Reset == /\ connected=1 /\ connected'=0 /\ event'=6
         /\ UNCHANGED <<offered,resolved,driven,revoked,attempts,connects,diag,connDiag,wrapper>>
Expected == IF revoked=1 THEN 30 ELSE IF resolved=0 THEN 10
            ELSE IF offered=1 THEN 20 ELSE 21
Inspect == /\ event'=7
           /\ diag'=IF Fault="hideError" /\ Expected=21 THEN 20 ELSE Expected
           /\ connDiag'=connected
           /\ wrapper'=IF Fault="stripMembrane" THEN 0 ELSE 1
           /\ resolved'=IF Fault="drivePromise" /\ offered#0 THEN 1 ELSE resolved
           /\ connects'=IF Fault="lazyConnect" /\ connected=0 THEN connects+1 ELSE connects
           /\ UNCHANGED <<offered,driven,revoked,connected,attempts>>
Next == Offer(1) \/ Offer(2) \/ Drive \/ Revoke \/ Connect \/ Reset \/ Inspect
Spec == Init /\ [][Next]_vars
TypeOK == /\ offered\in 0..2 /\ resolved\in 0..1 /\ driven\in 0..1 /\ revoked\in 0..1
          /\ connected\in 0..1 /\ attempts\in 0..2 /\ connects\in 0..3
          /\ diag\in {0,10,20,21,30} /\ connDiag\in 0..1 /\ wrapper\in 0..1 /\ event\in 0..7
ResolutionRequiresDrive == resolved=driven
NoConnectOnInspect == connects=attempts
WrapperVisible == event=7 => wrapper=1
AccurateDescription == event=7 => diag=Expected /\ connDiag=connected
=============================================================================
