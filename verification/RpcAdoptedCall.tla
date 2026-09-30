------------------------- MODULE RpcAdoptedCall -------------------------
EXTENDS Naturals, TLC
CONSTANT Fault
VARIABLES oldFinished, directFinished, oldConnected, directConnected, ready,
          status, ack, returned, childRequested, childFinished, childReturned, event
vars == <<oldFinished,directFinished,oldConnected,directConnected,ready,status,ack,returned,childRequested,childFinished,childReturned,event>>
Init == /\ oldFinished=FALSE /\ directFinished=FALSE /\ oldConnected=TRUE /\ directConnected=TRUE
        /\ ready=FALSE /\ status=0 /\ ack=0 /\ returned=0 /\ childRequested=FALSE /\ childFinished=FALSE /\ childReturned=0 /\ event=0
\* One invocation shared by the forwarding and adopted answers. Status 0 is
\* running, 1 completed, 2 canceled; returned 0 absent, 1 results, 2 canceled.
Advance(o,d,oc,dc,r,ch,cf,e) ==
    /\ oldFinished'=o /\ directFinished'=d /\ oldConnected'=oc /\ directConnected'=dc /\ ready'=r /\ childRequested'=ch /\ childFinished'=cf /\ event'=e
    /\ status'=(IF status#0 THEN status ELSE IF (o /\ d /\ (~ch \/ cf \/ Fault="ignoreChild") /\ Fault#"leakBoth") \/ (Fault="cancelEarly" /\ (o \/ d))
                 THEN 2 ELSE IF r THEN 1 ELSE 0)
    /\ ack'=(IF ack#0 THEN ack ELSE IF status'#0 /\ oc /\ Fault#"loseAck" THEN IF Fault="duplicateAck" THEN 2 ELSE 1 ELSE 0)
    /\ returned'=(IF returned#0 THEN returned ELSE IF status'#0 /\ dc /\ Fault#"loseReturn"
                   THEN IF status'=1 /\ (~d \/ Fault="ignoreFinish") THEN 1 ELSE 2 ELSE 0)
    /\ childReturned'=(IF childReturned#0 THEN childReturned ELSE IF dc /\ ch
          THEN IF cf THEN 2 ELSE IF status'=1 THEN 1 ELSE 0 ELSE 0)
FinishOld == /\ oldConnected /\ ~oldFinished /\ Advance(TRUE,directFinished,oldConnected,directConnected,ready,childRequested,childFinished,1)
FinishDirect == /\ directConnected /\ ~directFinished /\ Advance(oldFinished,TRUE,oldConnected,directConnected,ready,childRequested,childFinished,2)
Ready == /\ ~ready /\ Advance(oldFinished,directFinished,oldConnected,directConnected,TRUE,childRequested,childFinished,3)
DisconnectOld == /\ oldConnected /\ Advance(TRUE,directFinished,FALSE,directConnected,ready,childRequested,childFinished,4)
DisconnectDirect == /\ directConnected /\ Advance(oldFinished,TRUE,oldConnected,FALSE,ready,childRequested,childRequested,5)
Pipeline == /\ directConnected /\ ~directFinished /\ ~childRequested
            /\ Advance(oldFinished,directFinished,oldConnected,directConnected,ready,TRUE,FALSE,6)
FinishChild == /\ directConnected /\ childRequested /\ ~childFinished
               /\ Advance(oldFinished,directFinished,oldConnected,directConnected,ready,childRequested,TRUE,7)
Next == Pipeline \/ FinishChild \/ FinishOld \/ FinishDirect \/ Ready \/ DisconnectOld \/ DisconnectDirect
Spec == Init /\ [][Next]_vars
TypeOK == /\ \A b\in {oldFinished,directFinished,oldConnected,directConnected,ready,childRequested,childFinished}: b\in BOOLEAN
          /\ status\in 0..2 /\ ack\in 0..2 /\ returned\in 0..2 /\ childReturned\in 0..2 /\ event\in 0..7
Ownership == /\ (status=0 => ~ready /\ (~(oldFinished /\ directFinished) \/ (childRequested /\ ~childFinished)))
             /\ (status=2 => oldFinished /\ directFinished /\ (~childRequested \/ childFinished))
             /\ (status=1 => ready)
Acknowledgement == oldConnected /\ status#0 => ack=1
SingleAck == ack<=1
Reply == directConnected /\ status#0 => returned#0
CancellationReturn == event=3 /\ directConnected /\ directFinished /\ status=1 => returned=2
ChildReply == childReturned=1 => status=1
ChildFinish == directConnected /\ childRequested /\ childFinished => childReturned#0
LiveSpec == Spec /\ WF_vars(FinishOld) /\ WF_vars(FinishDirect) /\ WF_vars(Ready)
Reclaims == <>(status#0)
=============================================================================
