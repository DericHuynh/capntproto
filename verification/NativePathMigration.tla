----------------------- MODULE NativePathMigration -----------------------
EXTENDS Naturals
CONSTANT Fault
VARIABLES phase, proof, retired, committed, lateProof, lateCancel, event
vars == <<phase,proof,retired,committed,lateProof,lateCancel,event>>
Init == /\ phase=0 /\ proof=0 /\ retired=0 /\ committed=0 /\ lateProof=0 /\ lateCancel=0 /\ event=0
Validate == /\ phase=0 /\ proof'=1 /\ phase'=1 /\ event'=1
            /\ UNCHANGED <<retired,committed,lateProof,lateCancel>>
Retire == /\ phase\in {0,1} /\ retired'=1 /\ phase'=3 /\ event'=2
          /\ UNCHANGED <<proof,committed,lateProof,lateCancel>>
Commit == /\ (phase=1 \/ (Fault="early" /\ phase=0)) /\ phase'=2 /\ committed'=1 /\ event'=3
          /\ UNCHANGED <<proof,retired,lateProof,lateCancel>>
LateProof == /\ phase=3 /\ lateProof=0 /\ lateProof'=1 /\ event'=4
             /\ phase'=IF Fault="resurrect" THEN 1 ELSE phase
             /\ UNCHANGED <<proof,retired,committed,lateCancel>>
LateCancel == /\ phase=2 /\ lateCancel=0 /\ lateCancel'=1 /\ event'=5
              /\ phase'=IF Fault="lateCancel" THEN 3 ELSE phase
              /\ UNCHANGED <<proof,retired,committed,lateProof>>
Next == Validate \/ Retire \/ Commit \/ LateProof \/ LateCancel
Spec == Init /\ [][Next]_vars
TypeOK == /\ phase\in 0..3 /\ proof\in 0..1 /\ retired\in 0..1 /\ committed\in 0..1 /\ lateProof\in 0..1 /\ lateCancel\in 0..1 /\ event\in 0..5
Authenticated == phase=2 => proof=1
NoResurrection == retired=1 => phase=3
StableCommit == committed=1 => phase=2
LiveSpec == Spec /\ WF_vars(Retire)
Settles == phase\in {0,1} ~> phase\in {2,3}
=============================================================================
