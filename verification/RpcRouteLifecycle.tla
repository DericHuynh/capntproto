------------------------- MODULE RpcRouteLifecycle -------------------------
EXTENDS Naturals
CONSTANT Fault
VARIABLES phase, cause, first, drained, closed, receipt, late, event
vars == <<phase,cause,first,drained,closed,receipt,late,event>>
Init == /\ phase=0 /\ cause=0 /\ first=0 /\ drained=0 /\ closed=0
        /\ receipt=0 /\ late=0 /\ event=0
Install == /\ phase=0 /\ phase'=1 /\ event'=1
           /\ UNCHANGED <<cause,first,drained,closed,receipt,late>>
Begin == /\ phase=1 /\ phase'=2 /\ event'=2
         /\ UNCHANGED <<cause,first,drained,closed,receipt,late>>
Drain == /\ phase=2 /\ drained=0 /\ drained'=1 /\ event'=3
         /\ UNCHANGED <<phase,cause,first,closed,receipt,late>>
Close == /\ phase=2 /\ closed=0 /\ (drained=1 \/ Fault="earlyClose")
         /\ closed'=1 /\ event'=4
         /\ UNCHANGED <<phase,cause,first,drained,receipt,late>>
Receive == /\ phase=2 /\ closed=1 /\ receipt=0 /\ receipt'=1 /\ event'=5
           /\ UNCHANGED <<phase,cause,first,drained,closed,late>>
Finish == /\ phase=2 /\ (receipt=1 \/ Fault="earlySuccess")
          /\ phase'=4 /\ cause'=1 /\ first'=1 /\ event'=6
          /\ UNCHANGED <<drained,closed,receipt,late>>
Fail == /\ phase<3 /\ phase'=3 /\ cause'=3 /\ first'=3 /\ event'=7
        /\ UNCHANGED <<drained,closed,receipt,late>>
Cancel == /\ phase<3 /\ phase'=4 /\ cause'=2 /\ first'=2 /\ event'=8
          /\ UNCHANGED <<drained,closed,receipt,late>>
LateInstall == /\ phase>=3 /\ late=0 /\ late'=1 /\ event'=9
               /\ phase'=IF Fault="resurrect" THEN 1 ELSE phase
               /\ UNCHANGED <<cause,first,drained,closed,receipt>>
LateCause == /\ phase>=3 /\ late=0 /\ late'=1 /\ event'=10
             /\ cause'=IF Fault="replaceCause" THEN 0 ELSE cause
             /\ UNCHANGED <<phase,first,drained,closed,receipt>>
Next == Install \/ Begin \/ Drain \/ Close \/ Receive \/ Finish \/ Fail \/ Cancel \/ LateInstall \/ LateCause
Spec == Init /\ [][Next]_vars
TypeOK == /\ phase\in 0..4 /\ cause\in 0..3 /\ first\in 0..3
          /\ drained\in 0..1 /\ closed\in 0..1 /\ receipt\in 0..1
          /\ late\in 0..1 /\ event\in 0..10
Terminal == first#0 => phase>=3
FirstCause == cause=first
OutputFence == closed=1 => drained=1
Success == cause=1 => phase=4 /\ drained=1 /\ closed=1 /\ receipt=1
=============================================================================
