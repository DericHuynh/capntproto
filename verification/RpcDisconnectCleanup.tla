---------------------- MODULE RpcDisconnectCleanup ----------------------
EXTENDS Naturals, TLC
CONSTANTS LateDisconnect, StaleRequest, AbortBodyFailure, ForgetShutdown, Explicit, KeepRead
VARIABLES app, exp, closed, dead, observed, request, question,
          calls, finishes, aborts, late, shutdowns, reading
vars == <<app, exp, closed, dead, observed, request, question,
          calls, finishes, aborts, late, shutdowns, reading>>
\* A local capability has application and peer-export ownership. Its destructor
\* releases a pending question and invokes another capability on the same peer.
\* Disconnect also invokes a diagnostic encoder that calls the peer.
\* A separate request was built before disconnect, but has not yet been sent.
Init == /\ app = TRUE /\ exp = TRUE /\ closed = FALSE /\ dead = FALSE
        /\ observed = FALSE /\ request = FALSE /\ question = TRUE
        /\ calls = 0 /\ finishes = 0 /\ aborts = 0 /\ late = 0 /\ shutdowns = 0 /\ reading = TRUE
Change(a,e,c,r,q) ==
    LET dies == ~dead /\ ~a /\ ~e
        callbackSends == dies /\ (~c \/ (LateDisconnect /\ ~closed))
        encoderSends == ~closed /\ c /\ LateDisconnect
        requestSends == ~request /\ r /\ (~c \/ StaleRequest)
        finishSends == (dies /\ (~c \/ (LateDisconnect /\ ~closed)))
    IN /\ app' = a /\ exp' = e /\ closed' = c
       /\ dead' = (~a /\ ~e) /\ request' = r /\ question' = q
       /\ calls' = calls + (IF callbackSends THEN 1 ELSE 0)
                         + (IF requestSends THEN 1 ELSE 0)
                         + (IF encoderSends THEN 1 ELSE 0)
       /\ finishes' = finishes + (IF finishSends THEN 1 ELSE 0)
                     + (IF question /\ ~q /\ ~c THEN 1 ELSE 0)
       /\ aborts' = aborts + (IF ~closed /\ c /\ ~AbortBodyFailure THEN 1 ELSE 0)
       /\ shutdowns' = IF c /\ ~(AbortBodyFailure /\ ForgetShutdown) THEN 1 ELSE 0
       /\ reading' = (~c \/ (Explicit /\ KeepRead))
       /\ late' = late + (IF c /\ (callbackSends \/ requestSends \/ finishSends \/ encoderSends) THEN 1 ELSE 0)
       /\ UNCHANGED observed
DropApp == /\ app /\ Change(FALSE,exp,closed,request,question)
Release == /\ exp /\ ~closed /\ Change(app,FALSE,closed,request,question)
Close == /\ ~closed /\ Change(app,FALSE,TRUE,request,question)
SendRequest == /\ ~request /\ Change(app,exp,closed,TRUE,question)
DropQuestion == /\ question /\ Change(app,exp,closed,request,FALSE)
Observe == /\ dead /\ closed /\ ~observed /\ observed' = TRUE
           /\ UNCHANGED <<app,exp,closed,dead,request,question,calls,finishes,aborts,late,shutdowns,reading>>
Next == DropApp \/ Release \/ Close \/ SendRequest \/ DropQuestion \/ Observe
Spec == Init /\ [][Next]_vars
TypeOK == /\ app \in BOOLEAN /\ exp \in BOOLEAN /\ closed \in BOOLEAN
          /\ dead \in BOOLEAN /\ observed \in BOOLEAN /\ request \in BOOLEAN
          /\ question \in BOOLEAN /\ calls \in 0..3 /\ finishes \in 0..2
          /\ aborts \in 0..1 /\ late \in 0..2 /\ shutdowns \in 0..1 /\ reading \in BOOLEAN
NoTrafficAfterDisconnect == late = 0
CapabilityLifetime == dead = (~app /\ ~exp)
DisconnectedTables == closed => ~exp
SingleAbort == aborts = (IF closed /\ ~AbortBodyFailure THEN 1 ELSE 0)
ReadReleased == closed => ~reading
Shutdown == shutdowns = (IF closed THEN 1 ELSE 0)
Observation == observed => dead /\ closed
=============================================================================
