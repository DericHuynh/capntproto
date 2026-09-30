------------------------- MODULE RpcVariableWindow -------------------------
EXTENDS Naturals
CONSTANT Fault
\* Two ordered sends of one/two words; out-of-order success/error acks,
\* one shared-window change, credit cancellation, drain and controller drop.
\* Numeric scalar state allows fresh graph replay through the public Rust API.
VARIABLES sent,a1,a2,p1,p2,failed,dropped,waiting,drained,lost,window,changed,event
vars == <<sent,a1,a2,p1,p2,failed,dropped,waiting,drained,lost,window,changed,event>>
InFlight == (IF sent=0 THEN 0 ELSE IF sent=1 THEN 1 ELSE 3)
            - (IF a1=1 THEN 1 ELSE 0) - (IF a2=1 THEN 2 ELSE 0)
MaxSize == IF sent=0 THEN 0 ELSE IF sent=1 THEN 1 ELSE 2
Ready(bytes,max,w) == bytes<=max \/ bytes<max+w
CurrentWindow == IF Fault="staleWindow" THEN 1 ELSE window
AllAcked(x,y) == (sent=0 \/ x#0) /\ (sent<2 \/ y#0)
Init == /\ sent=0 /\ a1=0 /\ a2=0 /\ p1=0 /\ p2=0 /\ failed=0
        /\ dropped=0 /\ waiting=0 /\ drained=0 /\ lost=0
        /\ window=1 /\ changed=0 /\ event=0
Send == /\ sent<2 /\ dropped=0 /\ waiting=0 /\ sent'=sent+1 /\ event'=1
        /\ p1'=IF sent=0 THEN 1 ELSE p1
        /\ p2'=IF sent=1 THEN
                    IF failed=1 THEN 3 ELSE IF Ready(InFlight+2,2,CurrentWindow) THEN 1 ELSE 2
                ELSE p2
        /\ UNCHANGED <<a1,a2,failed,dropped,waiting,drained,lost,window,changed>>
Ack(first,error) ==
    /\ (IF first THEN sent>0 /\ a1=0 ELSE sent=2 /\ a2=0)
    /\ event'=IF first THEN (IF error THEN 3 ELSE 2) ELSE (IF error THEN 5 ELSE 4)
    /\ a1'=IF first THEN (IF error THEN 2 ELSE 1) ELSE a1
    /\ a2'=IF ~first THEN (IF error THEN 2 ELSE 1) ELSE a2
    /\ failed'=IF failed=1 \/ error THEN 1 ELSE 0
    /\ LET bytes == InFlight - (IF error THEN 0 ELSE IF first THEN 1 ELSE 2)
           Release(p) == IF p#2 THEN p ELSE IF failed'=1 THEN 3
                         ELSE IF Ready(bytes,MaxSize,CurrentWindow) THEN 1 ELSE p
       IN /\ p1'=Release(p1) /\ p2'=Release(p2)
    /\ drained'=IF waiting=1 /\ AllAcked(a1',a2') THEN 1 ELSE 0
    /\ UNCHANGED <<sent,dropped,waiting,lost,window,changed>>
Change(w) == /\ changed=0 /\ dropped=0 /\ window'=w /\ changed'=1 /\ event'=6
             /\ UNCHANGED <<sent,a1,a2,p1,p2,failed,dropped,waiting,drained,lost>>
CancelCredit == /\ p2=2 /\ p2'=4 /\ event'=7
                /\ lost'=IF Fault="cancelAck" /\ a2=0 THEN 1 ELSE lost
                /\ UNCHANGED <<sent,a1,a2,p1,failed,dropped,waiting,drained,window,changed>>
Wait == /\ waiting=0 /\ dropped=0 /\ sent\in {0,2} /\ waiting'=1 /\ event'=8
        /\ drained'=IF AllAcked(a1,a2) THEN 1 ELSE 0
        /\ UNCHANGED <<sent,a1,a2,p1,p2,failed,dropped,lost,window,changed>>
Drop == /\ dropped=0 /\ dropped'=1 /\ event'=9
        /\ p1'=IF p1=2 THEN 1 ELSE p1
        /\ p2'=IF p2=2 THEN 1 ELSE p2
        /\ lost'=IF Fault="dropAck" /\ ~AllAcked(a1,a2) THEN 1 ELSE lost
        /\ UNCHANGED <<sent,a1,a2,failed,waiting,drained,window,changed>>
AckNext == Ack(TRUE,FALSE) \/ Ack(TRUE,TRUE) \/ Ack(FALSE,FALSE) \/ Ack(FALSE,TRUE)
Next == Send \/ AckNext \/ (\E w\in {0,4}: Change(w)) \/ CancelCredit \/ Wait \/ Drop
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(AckNext)
DrainProgress == waiting=1 ~> drained=1
TypeOK == /\ sent\in 0..2 /\ a1\in 0..2 /\ a2\in 0..2 /\ p1\in 0..4 /\ p2\in 0..4
          /\ failed\in 0..1 /\ dropped\in 0..1 /\ waiting\in 0..1 /\ drained\in 0..1
          /\ lost\in 0..1 /\ changed\in 0..1 /\ window\in {0,1,4} /\ event\in 0..9
VariableReadiness == event=1 /\ sent=2 /\ failed=0 =>
                      (p2=1 <=> Ready(InFlight,MaxSize,window))
AckOwnership == lost=0
DrainsWhenEmpty == waiting=1 /\ AllAcked(a1,a2) => drained=1
DrainProvenance == drained=1 => waiting=1 /\ AllAcked(a1,a2)
FailureReleasesBlocked == failed=1 => p1#2 /\ p2#2
=============================================================================
