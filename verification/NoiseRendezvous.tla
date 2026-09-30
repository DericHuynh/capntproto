------------------------- MODULE NoiseRendezvous -------------------------
EXTENDS Naturals
CONSTANT Fault
VARIABLES phase, proof, punched, retired, closed, late, event
vars == <<phase,proof,punched,retired,closed,late,event>>
Init == /\ phase=1 /\ proof=0 /\ punched=0 /\ retired=0 /\ closed=0 /\ late=0 /\ event=0
Punch == /\ phase=1 /\ punched=0 /\ punched'=1 /\ event'=1
         /\ phase'=IF Fault="punchAuthenticates" THEN 2 ELSE phase
         /\ UNCHANGED <<proof,retired,closed,late>>
Authenticate == /\ phase=1 /\ phase'=2 /\ proof'=1 /\ event'=2
                /\ UNCHANGED <<punched,retired,closed,late>>
Retire(e) == /\ phase=1 /\ phase'=3 /\ retired'=1 /\ event'=e
             /\ UNCHANGED <<proof,punched,closed,late>>
Close == /\ closed=0 /\ closed'=1 /\ event'=5
         /\ phase'=IF phase=1 \/ Fault="closeInstalled" THEN 3 ELSE phase
         /\ retired'=IF phase=1 THEN 1 ELSE retired
         /\ UNCHANGED <<proof,punched,late>>
Late == /\ phase=3 /\ late=0 /\ late'=1 /\ event'=6
        /\ phase'=IF Fault="resurrect" THEN 2 ELSE phase
        /\ UNCHANGED <<proof,punched,retired,closed>>
Next == Punch \/ Authenticate \/ Retire(3) \/ Retire(4) \/ Close \/ Late
Spec == Init /\ [][Next]_vars
TypeOK == /\ phase\in 1..3 /\ proof\in 0..1 /\ punched\in 0..1 /\ retired\in 0..1
          /\ closed\in 0..1 /\ late\in 0..1 /\ event\in 0..6
NoResurrection == retired=1 => phase=3
Authentication == phase=2 => proof=1
InstalledSurvives == proof=1 => phase=2
LiveSpec == Spec /\ WF_vars(Retire(4))
Settles == phase=1 ~> phase#1
=============================================================================
