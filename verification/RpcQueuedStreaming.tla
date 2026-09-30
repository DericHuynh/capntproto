------------------------- MODULE RpcQueuedStreaming -------------------------
EXTENDS Naturals, TLC
CONSTANTS WaitForAck, EarlyFinish
VARIABLES sent, resolved, dropped, acked, ready, finished
vars == <<sent,resolved,dropped,acked,ready,finished>>
Init == /\ sent = FALSE /\ resolved = FALSE /\ dropped = FALSE
        /\ acked = FALSE /\ ready = FALSE /\ finished = FALSE
Send == /\ ~sent /\ sent' = TRUE /\ ready' = (resolved /\ ~WaitForAck)
        /\ UNCHANGED <<resolved,dropped,acked,finished>>
Resolve == /\ ~resolved /\ resolved' = TRUE /\ ready' = (sent /\ ~WaitForAck)
           /\ UNCHANGED <<sent,dropped,acked,finished>>
Drop == /\ sent /\ ~dropped /\ dropped' = TRUE
        /\ finished' = (finished \/ EarlyFinish)
        /\ UNCHANGED <<sent,resolved,acked,ready>>
Ack == /\ sent /\ resolved /\ ~acked /\ acked' = TRUE
       /\ ready' = TRUE /\ finished' = TRUE
       /\ UNCHANGED <<sent,resolved,dropped>>
Next == Send \/ Resolve \/ Drop \/ Ack
Spec == Init /\ [][Next]_vars
TypeOK == /\ sent \in BOOLEAN /\ resolved \in BOOLEAN /\ dropped \in BOOLEAN
          /\ acked \in BOOLEAN /\ ready \in BOOLEAN /\ finished \in BOOLEAN
CreditBeforeAck == ready = (sent /\ resolved)
RetainUntilAck == finished = acked
=============================================================================
