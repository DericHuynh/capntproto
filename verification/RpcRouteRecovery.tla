------------------------- MODULE RpcRouteRecovery -------------------------
EXTENDS Naturals
CONSTANT Fault
VARIABLES generation, phase, cause, oldCause, savedCause, oldDropped, delivered, shared, event
vars == <<generation,phase,cause,oldCause,savedCause,oldDropped,delivered,shared,event>>
Init == /\ generation=0 /\ phase=0 /\ cause=0 /\ oldCause=0 /\ savedCause=0
        /\ oldDropped=0 /\ delivered=0 /\ shared=0 /\ event=0
Connect == /\ generation=0 /\ generation'=1 /\ phase'=1 /\ event'=1
           /\ UNCHANGED <<cause,oldCause,savedCause,oldDropped,delivered,shared>>
Fail(c,e) == /\ phase=1 /\ phase'=2 /\ cause'=c /\ event'=e
             /\ savedCause'=IF generation=1 THEN c ELSE savedCause
             /\ UNCHANGED <<generation,oldCause,oldDropped,delivered,shared>>
Recover == /\ generation=1 /\ phase=2 /\ event'=4
           /\ generation'=IF Fault="reuseGeneration" THEN 1 ELSE 2
           /\ phase'=IF Fault="earlyPublish" THEN 3 ELSE 1
           /\ oldCause'=cause /\ cause'=0 /\ shared'=0
           /\ UNCHANGED <<savedCause,oldDropped,delivered>>
Attach == /\ generation=1 /\ phase=2 /\ event'=5
          /\ generation'=2 /\ phase'=3 /\ delivered'=2
          /\ oldCause'=cause /\ cause'=0 /\ shared'=0
          /\ UNCHANGED <<savedCause,oldDropped>>
Authenticate == /\ generation=2 /\ phase=1 /\ phase'=3 /\ delivered'=2 /\ event'=6
                /\ UNCHANGED <<generation,cause,oldCause,savedCause,oldDropped,shared>>
DropOld == /\ generation=2 /\ oldDropped=0 /\ oldDropped'=1 /\ event'=7
           /\ phase'=IF Fault="dropCurrent" /\ phase#4 THEN 0 ELSE phase
           /\ oldCause'=IF Fault="replaceCause" THEN 0 ELSE oldCause
           /\ UNCHANGED <<generation,cause,savedCause,delivered,shared>>
Share == /\ phase\in {1,3} /\ shared=0 /\ shared'=1 /\ event'=8
         /\ UNCHANGED <<generation,phase,cause,oldCause,savedCause,oldDropped,delivered>>
Close == /\ phase\in {1,2,3} /\ phase'=4 /\ event'=9
         /\ UNCHANGED <<generation,cause,oldCause,savedCause,oldDropped,delivered,shared>>
Next == Connect \/ Fail(1,2) \/ Fail(2,3) \/ Recover \/ Attach \/ Authenticate \/ DropOld \/ Share \/ Close
Spec == Init /\ [][Next]_vars
TypeOK == /\ generation\in 0..2 /\ phase\in 0..4
          /\ cause\in 0..2 /\ oldCause\in 0..2 /\ savedCause\in 0..2
          /\ oldDropped\in 0..1 /\ delivered\in 0..2 /\ shared\in 0..1 /\ event\in 0..9
DistinctGeneration == oldCause#0 => generation=2
FirstCause == generation=2 => oldCause=savedCause /\ oldCause#0
Authenticated == phase=3 => delivered=generation
FreshSurvives == generation=2 /\ phase#4 => phase#0
LiveSpec == Spec /\ WF_vars(Fail(2,3))
SetupSettles == phase=1 ~> phase\in {2,3,4}
=============================================================================
