-------------------------- MODULE RpcTailRouting --------------------------
EXTENDS Naturals, TLC
CONSTANT LostRequest
VARIABLES resolved, sent, used, route
vars == <<resolved,sent,used,route>>
\* A server has built an outgoing tail request on an unresolved imported promise.
\* Check the specific race where the promise resolves locally before tail_call.
Init == /\ resolved = FALSE /\ sent = FALSE /\ used = FALSE /\ route = 0
Resolve == /\ ~resolved /\ resolved' = TRUE /\ UNCHANGED <<sent,used,route>>
Send == /\ resolved /\ ~sent /\ sent' = TRUE
        /\ route' = (IF LostRequest THEN 2 ELSE 1) /\ UNCHANGED <<resolved,used>>
Use == /\ sent /\ route = 1 /\ ~used /\ used' = TRUE /\ UNCHANGED <<resolved,sent,route>>
Next == Resolve \/ Send \/ Use
Spec == Init /\ [][Next]_vars
TypeOK == /\ resolved \in BOOLEAN /\ sent \in BOOLEAN /\ used \in BOOLEAN /\ route \in 0..2
ResolvedRequestForwarded == sent => route = 1
CapabilityProvenance == used => (sent /\ resolved /\ route = 1)
=============================================================================
