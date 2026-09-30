---------------------- MODULE NoiseDiscoveryFailover ----------------------
EXTENDS Naturals
CONSTANT Fault
VARIABLES phase, reader, elapsed, denied, retired, host, late, event
vars == <<phase,reader,elapsed,denied,retired,host,late,event>>
\* Three explicitly trusted readers, one tick per attempt, three ticks overall.
\* phase: 0 waiting, 1 resolved, 2 failed, 3 canceled. host: 1 pinned, 2 invalid.
Init == /\ phase=0 /\ reader=1 /\ elapsed=0 /\ denied=0
        /\ retired=0 /\ host=0 /\ late=0 /\ event=0
Retry(e) == /\ phase=0 /\ event'=e
            /\ LET time == IF e=3 THEN elapsed+1 ELSE elapsed
                   more == reader<3 /\ time<3
               IN /\ elapsed'=time
                  /\ reader'=IF more THEN reader+1 ELSE reader
                  /\ phase'=IF more THEN 0 ELSE 2
                  /\ retired'=IF time=3 THEN 1 ELSE 0
            /\ UNCHANGED <<denied,host,late>>
Success == /\ phase=0 /\ phase'=1 /\ host'=1 /\ event'=4
           /\ UNCHANGED <<reader,elapsed,denied,retired,late>>
Reject == /\ phase=0 /\ denied'=1 /\ event'=5
          /\ LET retry == Fault="retryDenied" /\ reader<3
             IN /\ phase'=IF retry THEN 0 ELSE 2
                /\ reader'=IF retry THEN reader+1 ELSE reader
          /\ UNCHANGED <<elapsed,retired,host,late>>
Invalid == /\ phase=0 /\ event'=6
           /\ phase'=IF Fault="acceptInvalid" THEN 1 ELSE 2
           /\ host'=IF Fault="acceptInvalid" THEN 2 ELSE 0
           /\ UNCHANGED <<reader,elapsed,denied,retired,late>>
Expire == /\ phase=0 /\ phase'=2 /\ elapsed'=3 /\ retired'=1 /\ event'=7
          /\ UNCHANGED <<reader,denied,host,late>>
Cancel == /\ phase=0 /\ phase'=3 /\ retired'=1 /\ event'=8
          /\ UNCHANGED <<reader,elapsed,denied,host,late>>
Late == /\ phase\in {2,3} /\ late=0 /\ late'=1 /\ event'=9
        /\ phase'=IF Fault="lateCommit" /\ retired=1 THEN 1 ELSE phase
        /\ host'=IF Fault="lateCommit" /\ retired=1 THEN 1 ELSE host
        /\ UNCHANGED <<reader,elapsed,denied,retired>>
Next == Retry(1) \/ Retry(2) \/ Retry(3) \/ Success \/ Reject \/ Invalid \/ Expire \/ Cancel \/ Late
Spec == Init /\ [][Next]_vars
TypeOK == /\ phase\in 0..3 /\ reader\in 1..3 /\ elapsed\in 0..3
          /\ denied\in 0..1 /\ retired\in 0..1 /\ host\in 0..2
          /\ late\in 0..1 /\ event\in 0..9
AuthoritativeStop == denied=1 => phase=2
Pinned == phase=1 => host=1
NoLateCommit == retired=1 => phase#1
WithinBudget == phase=1 => elapsed<3
LiveSpec == Spec /\ WF_vars(Expire)
Terminates == phase=0 ~> phase#0
=============================================================================
