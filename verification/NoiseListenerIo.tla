-------------------------- MODULE NoiseListenerIo --------------------------
EXTENDS Naturals
CONSTANT Fault
\* Two genuinely authenticated routes start with blocked shared sends. A third
\* reservation has not accepted. Macro-steps include bounded delivery/polling.
VARIABLES blocked, dropped, draining, expired, transient, closed, pending,
          aliveA, aliveB, deliveredA, deliveredB, event
vars == <<blocked,dropped,draining,expired,transient,closed,pending,
          aliveA,aliveB,deliveredA,deliveredB,event>>
Init == /\ blocked=1 /\ dropped=0 /\ draining=0 /\ expired=0
        /\ transient=0 /\ closed=0 /\ pending=1 /\ aliveA=1 /\ aliveB=1
        /\ deliveredA=0 /\ deliveredB=0 /\ event=0
Unblock == /\ closed=0 /\ blocked=1 /\ blocked'=0 /\ event'=1
           /\ deliveredA'=aliveA
           /\ deliveredB'=(IF Fault="lostWake" THEN 0 ELSE 1)
           /\ UNCHANGED <<dropped,draining,expired,transient,closed,pending,aliveA,aliveB>>
DropRoute == /\ closed=0 /\ dropped=0 /\ dropped'=1 /\ aliveA'=0 /\ event'=2
             /\ aliveB'=(IF Fault="sibling" THEN 0 ELSE aliveB)
             /\ UNCHANGED <<blocked,draining,expired,transient,closed,pending,deliveredA,deliveredB>>
Drain == /\ closed=0 /\ draining=0 /\ draining'=1 /\ event'=3
         /\ pending'=(IF Fault="admission" THEN pending ELSE 0)
         /\ UNCHANGED <<blocked,dropped,expired,transient,closed,aliveA,aliveB,deliveredA,deliveredB>>
Expire == /\ closed=0 /\ expired=0 /\ expired'=1 /\ event'=4
          /\ pending'=(IF Fault="expiry" THEN pending ELSE 0)
          /\ UNCHANGED <<blocked,dropped,draining,transient,closed,aliveA,aliveB,deliveredA,deliveredB>>
Transient == /\ closed=0 /\ transient=0 /\ transient'=1 /\ event'=5
             /\ UNCHANGED <<blocked,dropped,draining,expired,closed,pending,aliveA,aliveB,deliveredA,deliveredB>>
Close(action) == /\ closed=0 /\ closed'=1 /\ pending'=0 /\ event'=action
                 /\ aliveA'=0 /\ aliveB'=(IF Fault="close" THEN aliveB ELSE 0)
                 /\ UNCHANGED <<blocked,dropped,draining,expired,transient,deliveredA,deliveredB>>
Next == Unblock \/ DropRoute \/ Drain \/ Expire \/ Transient
        \/ Close(6) \/ Close(7) \/ Close(8)
Spec == Init /\ [][Next]_vars
TypeOK == /\ blocked\in 0..1 /\ dropped\in 0..1 /\ draining\in 0..1
          /\ expired\in 0..1 /\ transient\in 0..1 /\ closed\in 0..1
          /\ pending\in 0..1 /\ aliveA\in 0..1 /\ aliveB\in 0..1
          /\ deliveredA\in 0..1 /\ deliveredB\in 0..1 /\ event\in 0..8
Progress == blocked=0 => deliveredB=1
Isolation == closed=0 => aliveB=1
Admission == draining=1 => pending=0
Expiry == expired=1 => pending=0
Cleanup == closed=1 => aliveA=0 /\ aliveB=0 /\ pending=0
NoEarlyDelivery == blocked=1 => deliveredA=0 /\ deliveredB=0
=============================================================================
