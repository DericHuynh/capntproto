------------------------- MODULE RpcNativeStreamGate -------------------------
EXTENDS Naturals, TLC
CONSTANTS Server, Early, BadPreface
VARIABLES auth, opened, failed, published, event
vars == <<auth,opened,failed,published,event>>
Init == /\ auth=FALSE /\ opened=FALSE /\ failed=FALSE /\ published=FALSE /\ event=0
Authenticate == /\ ~auth /\ ~failed /\ auth'=TRUE /\ event'=1 /\ UNCHANGED <<opened,failed,published>>
Open == /\ auth /\ ~opened /\ ~failed /\ opened'=TRUE /\ event'=2 /\ UNCHANGED <<auth,failed,published>>
Bad == /\ Server /\ auth /\ ~opened /\ ~failed /\ failed'=TRUE /\ event'=3
       /\ opened'=BadPreface /\ published'=BadPreface /\ UNCHANGED auth
Publish == /\ ~published /\ ((auth /\ opened /\ ~failed) \/ Early) /\ published'=TRUE /\ event'=4 /\ UNCHANGED <<auth,opened,failed>>
Next == Authenticate \/ Open \/ Bad \/ Publish
Spec == Init /\ [][Next]_vars
TypeOK == /\ auth\in BOOLEAN /\ opened\in BOOLEAN /\ failed\in BOOLEAN /\ published\in BOOLEAN /\ event\in 0..4
AuthenticatedPublication == published => (auth /\ opened /\ ~failed)
=============================================================================
