-------------------------- MODULE NoiseCandidateIo --------------------------
EXTENDS Naturals
CONSTANT Fault
\* Macro-steps of a real established session with a blocked candidate socket.
\* Phase: 0 blocked, 1 response held, 2 committed, 3 retired.
\* Result: 0 pending, 1 success, 2 timeout, 3 canceled, 4 candidate IO failure.
VARIABLES phase, result, expected, proof, accepted, progress, delivered,
          tampered, late, event
vars == <<phase,result,expected,proof,accepted,progress,delivered,tampered,late,event>>
Init == /\ phase=0 /\ result=0 /\ expected=0 /\ proof=0 /\ accepted=0
        /\ progress=0 /\ delivered=0 /\ tampered=0 /\ late=0 /\ event=0
Progress == /\ phase=0 /\ progress=0 /\ progress'=1 /\ event'=1
            /\ delivered'=(IF Fault="stall" THEN 0 ELSE 1)
            /\ UNCHANGED <<phase,result,expected,proof,accepted,tampered,late>>
Probe == /\ phase=0 /\ phase'=1 /\ proof'=1 /\ event'=2
         /\ UNCHANGED <<result,expected,accepted,progress,delivered,tampered,late>>
Corrupt == /\ phase=1 /\ tampered=0 /\ tampered'=1 /\ event'=3
           /\ phase'=(IF Fault="early" THEN 2 ELSE phase)
           /\ UNCHANGED <<result,expected,proof,accepted,progress,delivered,late>>
Validate == /\ phase=1 /\ phase'=2 /\ result'=1 /\ expected'=1
            /\ accepted'=1 /\ event'=4
            /\ UNCHANGED <<proof,progress,delivered,tampered,late>>
Retire(cause, action) ==
    /\ phase\in {0,1} /\ event'=action /\ expected'=cause
    /\ phase'=(IF Fault="blockedDeadline" /\ phase=0 /\ cause=2 THEN phase ELSE 3)
    /\ result'=(IF Fault="blockedDeadline" /\ phase=0 /\ cause=2 THEN result ELSE cause)
    /\ UNCHANGED <<proof,accepted,progress,delivered,tampered,late>>
LateProof == /\ phase=3 /\ proof=1 /\ late=0 /\ late'=1 /\ event'=8
             /\ phase'=(IF Fault="resurrect" THEN 2 ELSE phase)
             /\ UNCHANGED <<result,expected,proof,accepted,progress,delivered,tampered>>
LateCancel == /\ phase=2 /\ late=0 /\ late'=1 /\ event'=9
              /\ phase'=(IF Fault="retireActive" THEN 3 ELSE phase)
              /\ UNCHANGED <<result,expected,proof,accepted,progress,delivered,tampered>>
Next == Progress \/ Probe \/ Corrupt \/ Validate \/ Retire(2,5)
        \/ Retire(3,6) \/ Retire(4,7) \/ LateProof \/ LateCancel
Spec == Init /\ [][Next]_vars
TypeOK == /\ phase\in 0..3 /\ result\in 0..4 /\ expected\in 0..4
          /\ proof\in 0..1 /\ accepted\in 0..1 /\ progress\in 0..1
          /\ delivered\in 0..1 /\ tampered\in 0..1 /\ late\in 0..1 /\ event\in 0..9
ActiveProgress == delivered=progress
Authenticated == phase=2 => accepted=1
Completion == result=expected
Retirement == result\in 2..4 => phase=3
StableCommit == result=1 => phase=2
=============================================================================
