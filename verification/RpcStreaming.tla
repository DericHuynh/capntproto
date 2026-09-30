---------------------------- MODULE RpcStreaming ----------------------------
EXTENDS Naturals, TLC
CONSTANTS Window, ZeroWindowBug, LoseOnDrop, EmptyWaitBug
VARIABLES sent, a1, a2, p1, p2, failed, dropped, waiting, drained, lost
vars == <<sent,a1,a2,p1,p2,failed,dropped,waiting,drained,lost>>
\* Two messages, respectively one and two words. Ack: 0 pending, 1 OK, 2 error.
\* Send promise: 0 not sent, 1 credit granted, 2 blocked, 3 error.
InFlight == (IF sent = 0 THEN 0 ELSE IF sent = 1 THEN 1 ELSE 3)
            - (IF a1 = 1 THEN 1 ELSE 0) - (IF a2 = 1 THEN 2 ELSE 0)
MaxSize == IF sent = 0 THEN 0 ELSE IF sent = 1 THEN 1 ELSE 2
Ready(bytes, max) == (~ZeroWindowBug /\ bytes <= max) \/ bytes < Window + max
AllAcked(x,y) == (sent = 0 \/ x # 0) /\ (sent < 2 \/ y # 0)
Init == /\ sent = 0 /\ a1 = 0 /\ a2 = 0 /\ p1 = 0 /\ p2 = 0 /\ failed = FALSE
        /\ dropped = FALSE /\ waiting = FALSE /\ drained = FALSE /\ lost = FALSE
Send == /\ sent < 2 /\ ~dropped /\ ~waiting /\ sent' = sent + 1
        /\ p1' = IF sent = 0 THEN (IF Ready(1,1) THEN 1 ELSE 2) ELSE p1
        /\ p2' = IF sent = 1 THEN (IF failed THEN 3 ELSE IF Ready(InFlight+2,2) THEN 1 ELSE 2) ELSE p2
        /\ UNCHANGED <<a1,a2,failed,dropped,waiting,drained,lost>>
Ack(first,error) ==
    /\ IF first THEN (sent > 0 /\ a1 = 0) ELSE (sent = 2 /\ a2 = 0)
    /\ a1' = IF first THEN (IF error THEN 2 ELSE 1) ELSE a1
    /\ a2' = IF ~first THEN (IF error THEN 2 ELSE 1) ELSE a2
    /\ failed' = (failed \/ error)
    /\ LET bytes == InFlight - (IF error THEN 0 ELSE IF first THEN 1 ELSE 2)
           Release(p) == IF p # 2 THEN p ELSE IF failed' THEN 3 ELSE IF Ready(bytes,MaxSize) THEN 1 ELSE p
       IN /\ p1' = Release(p1) /\ p2' = Release(p2)
    /\ drained' = (waiting /\ AllAcked(a1',a2'))
    /\ UNCHANGED <<sent,dropped,waiting,lost>>
Wait == /\ ~waiting /\ ~dropped /\ sent \in {0,2} /\ waiting' = TRUE
        /\ drained' = (~EmptyWaitBug /\ AllAcked(a1,a2))
        /\ UNCHANGED <<sent,a1,a2,p1,p2,failed,dropped,lost>>
Drop == /\ ~dropped /\ dropped' = TRUE
        /\ p1' = (IF p1 = 2 THEN 1 ELSE p1) /\ p2' = (IF p2 = 2 THEN 1 ELSE p2)
        /\ lost' = (LoseOnDrop /\ ~AllAcked(a1,a2))
        /\ UNCHANGED <<sent,a1,a2,failed,waiting,drained>>
Next == Send \/ Ack(TRUE,FALSE) \/ Ack(TRUE,TRUE) \/ Ack(FALSE,FALSE) \/ Ack(FALSE,TRUE) \/ Wait \/ Drop
Spec == Init /\ [][Next]_vars
TypeOK == /\ sent \in 0..2 /\ a1 \in 0..2 /\ a2 \in 0..2 /\ p1 \in 0..3 /\ p2 \in 0..3
          /\ failed \in BOOLEAN /\ dropped \in BOOLEAN /\ waiting \in BOOLEAN
          /\ drained \in BOOLEAN /\ lost \in BOOLEAN
FirstSendReady == sent > 0 => p1 = 1
DropPreservesCalls == ~lost
DrainsWhenEmpty == waiting /\ AllAcked(a1,a2) => drained
DrainProvenance == drained => waiting /\ AllAcked(a1,a2)
FailureReleasesBlocked == failed => p1 # 2 /\ p2 # 2
=============================================================================
