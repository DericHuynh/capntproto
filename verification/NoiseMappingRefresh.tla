------------------------- MODULE NoiseMappingRefresh -------------------------
EXTENDS Naturals
CONSTANT Fault
VARIABLES request, pending, observed, closed, event, saved, late, foreign
vars == <<request,pending,observed,closed,event,saved,late,foreign>>
Init == /\ request=0 /\ pending=0 /\ observed=0 /\ closed=0 /\ event=0
        /\ saved=0 /\ late=0 /\ foreign=0
Start == /\ request<2 /\ pending=0 /\ closed=0 /\ request'=request+1
         /\ pending'=1 /\ event'=1
         /\ UNCHANGED <<observed,closed,saved,late,foreign>>
Response == /\ pending=1 /\ closed=0 /\ pending'=0 /\ observed'=request /\ event'=2
            /\ UNCHANGED <<request,closed,saved,late,foreign>>
Timeout == /\ pending=1 /\ pending'=0 /\ observed'=IF Fault="staleHint" THEN observed ELSE 0
           /\ event'=3 /\ UNCHANGED <<request,closed,saved,late,foreign>>
Late == /\ request=2 /\ late=0 /\ late'=1 /\ event'=4 /\ saved'=observed
        /\ observed'=IF Fault="oldResponse" /\ pending=1 THEN 1 ELSE observed
        /\ UNCHANGED <<request,pending,closed,foreign>>
Foreign == /\ request>0 /\ foreign=0 /\ foreign'=1 /\ event'=5 /\ saved'=observed
           /\ observed'=IF Fault="foreignResponse" /\ pending=1 THEN request ELSE observed
           /\ UNCHANGED <<request,pending,closed,late>>
Close == /\ closed=0 /\ closed'=1 /\ pending'=0 /\ observed'=0 /\ event'=6
         /\ UNCHANGED <<request,saved,late,foreign>>
Next == Start \/ Response \/ Timeout \/ Late \/ Foreign \/ Close
Spec == Init /\ [][Next]_vars
TypeOK == /\ request\in 0..2 /\ pending\in 0..1 /\ observed\in 0..2
          /\ closed\in 0..1 /\ saved\in 0..2 /\ late\in 0..1 /\ foreign\in 0..1 /\ event\in 0..6
Transaction == event=4 => observed=saved
Source == event=5 => observed=saved
Withdraw == event=3 => observed=0
Stopped == closed=1 => pending=0 /\ observed=0
LiveSpec == Spec /\ WF_vars(Timeout)
ProbeSettles == pending=1 ~> pending=0
=============================================================================
