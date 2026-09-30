------------------------- MODULE RpcIdleShutdown -------------------------
EXTENDS Naturals, TLC
CONSTANT ForgetFlush
VARIABLES flushed,closing,reconnected,done
vars == <<flushed,closing,reconnected,done>>
Init == /\ flushed=FALSE /\ closing=FALSE /\ reconnected=FALSE /\ done=FALSE
Flush == /\ ~flushed /\ flushed'=TRUE /\ UNCHANGED <<closing,reconnected>>
Close == /\ ~closing /\ closing'=TRUE /\ UNCHANGED <<flushed,reconnected>>
Reconnect == /\ ~closing /\ ~reconnected /\ reconnected'=TRUE /\ UNCHANGED <<flushed,closing>>
Next == /\ (Flush \/ Close \/ Reconnect)
        /\ done'=(closing' /\ (flushed' \/ ForgetFlush))
Spec == Init /\ [][Next]_vars
TypeOK == \A x\in {flushed,closing,reconnected,done}: x\in BOOLEAN
WaitForFlush == done=(closing /\ flushed)
=============================================================================
