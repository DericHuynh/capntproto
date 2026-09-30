------------------------- MODULE RpcBufferedFds -------------------------
EXTENDS Naturals
CONSTANTS Fault, Partial
\* Ordered wire: bare frame, FD-bearing frame, another FD-bearing frame.
\* The middle frame's descriptor send may stop inside its header/body.
\* a/b/x: 0 unsent, 1 kernel queue, 2 prefetched, 3 caller-owned, 4 closed.
\* x is an excess descriptor sent with a; the per-message limit is one.
VARIABLES s1,s2,s3,r1,r2,r3,a,b,x,delivered,reading,closed,ended,result,event
vars == <<s1,s2,s3,r1,r2,r3,a,b,x,delivered,reading,closed,ended,result,event>>
Init == /\ s1=0 /\ s2=0 /\ s3=0 /\ r1=0 /\ r2=0 /\ r3=0
        /\ a=0 /\ b=0 /\ x=0 /\ delivered=0 /\ reading=0 /\ closed=0
        /\ ended=0 /\ result=0 /\ event=0
First == /\ closed=0 /\ ended=0 /\ s1=0 /\ s1'=1 /\ event'=1
         /\ UNCHANGED <<s2,s3,r1,r2,r3,a,b,x,delivered,reading,closed,ended,result>>
Second == /\ closed=0 /\ ended=0 /\ s1=1 /\ s2=0
          /\ s2'=IF Partial=1 THEN 1 ELSE 2
          /\ a'=1 /\ x'=1 /\ event'=2
          /\ UNCHANGED <<s1,s3,r1,r2,r3,b,delivered,reading,closed,ended,result>>
Tail == /\ closed=0 /\ ended=0 /\ s2=1 /\ s2'=2 /\ event'=3
        /\ UNCHANGED <<s1,s3,r1,r2,r3,a,b,x,delivered,reading,closed,ended,result>>
Third == /\ closed=0 /\ ended=0 /\ s2=2 /\ s3=0
         /\ s3'=1 /\ b'=1 /\ event'=4
         /\ UNCHANGED <<s1,s2,r1,r2,r3,a,x,delivered,reading,closed,ended,result>>
Start == /\ closed=0 /\ reading=0 /\ reading'=1 /\ result'=0 /\ event'=5
         /\ UNCHANGED <<s1,s2,s3,r1,r2,r3,a,b,x,delivered,closed,ended>>
Pump ==
    /\ reading=1 /\ closed=0
    /\ r1'=IF delivered=0 THEN s1 ELSE r1
    /\ r2'=IF delivered=0 /\ s2>0 THEN (IF Partial=1 THEN 1 ELSE 2)
            ELSE IF delivered=1 THEN s2 ELSE r2
    /\ r3'=IF delivered=2 THEN s3 ELSE r3
    /\ LET ready == (delivered=0 /\ r1'=1) \/ (delivered=1 /\ r2'=2)
                      \/ (delivered=2 /\ r3'=1)
           closing == ~ready /\ ended=1
           gotA == a=1 /\ r2'>0
           nextA == IF gotA THEN 2 ELSE a
       IN /\ delivered'=IF ready THEN delivered+1 ELSE delivered
          /\ reading'=IF ready \/ closing THEN 0 ELSE 1
          /\ closed'=IF closing THEN 1 ELSE 0
          /\ result'=IF ready THEN 1 ELSE IF ~closing THEN 2
                     ELSE IF delivered=1 /\ r2'=1 THEN 4 ELSE 3
          /\ a'=IF ready /\ delivered=1 /\ nextA=2 THEN 3
                  ELSE IF Fault="earlyAttach" /\ ready /\ delivered=0 /\ nextA=2 THEN 3
                  ELSE IF closing /\ nextA=2 THEN 4 ELSE nextA
          /\ b'=IF ready /\ delivered=2 THEN 3
                  ELSE IF Fault="reuseBudget" /\ Partial=1 /\ ready /\ delivered=1 /\ s3=1 THEN 4
                  ELSE b
          /\ x'=IF gotA THEN (IF Fault="truncateLeak" THEN 2 ELSE 4) ELSE x
    /\ event'=6 /\ UNCHANGED <<s1,s2,s3,ended>>
Cancel == /\ reading=1 /\ closed'=1 /\ reading'=0 /\ result'=0 /\ event'=7
          /\ a'=IF a=2 /\ Fault#"leakCancel" THEN 4 ELSE a
          /\ UNCHANGED <<s1,s2,s3,r1,r2,r3,b,x,delivered,ended>>
DropA == /\ a=3 /\ a'=4 /\ event'=8
         /\ UNCHANGED <<s1,s2,s3,r1,r2,r3,b,x,delivered,reading,closed,ended,result>>
DropB == /\ b=3 /\ b'=4 /\ event'=9
         /\ UNCHANGED <<s1,s2,s3,r1,r2,r3,a,x,delivered,reading,closed,ended,result>>
End == /\ ended=0 /\ closed=0 /\ ended'=1 /\ event'=10
       /\ UNCHANGED <<s1,s2,s3,r1,r2,r3,a,b,x,delivered,reading,closed,result>>
Next == First \/ Second \/ Tail \/ Third \/ Start \/ Pump \/ Cancel \/ DropA \/ DropB \/ End
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(Start) /\ WF_vars(Pump)
EndProgress == ended=1 ~> closed=1
TypeOK == /\ s1\in 0..1 /\ s2\in 0..2 /\ s3\in 0..1
          /\ r1\in 0..1 /\ r2\in 0..2 /\ r3\in 0..1
          /\ a\in 0..4 /\ b\in 0..4 /\ x\in 0..4
          /\ delivered\in 0..3 /\ reading\in 0..1 /\ closed\in 0..1
          /\ ended\in 0..1 /\ result\in 0..4 /\ event\in 0..10
CorrectOwner == (a=3 => delivered>=2) /\ (b=3 => delivered=3)
SeparateBudgets == b=4 => delivered=3
NoRetainedAfterClose == closed=1 => a#2 /\ b#2 /\ x#2
BoundedDescriptors == x#2
NoEarlyFrame == (delivered>=1 => r1=1) /\ (delivered>=2 => r2=2) /\ (delivered=3 => r3=1)
=============================================================================
