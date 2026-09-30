---------------------- MODULE DiscoveryGenerationBoundary ----------------------
EXTENDS Naturals
CONSTANT Fault
\* Generations 1..3 map to the last three u64 values. One existing publication,
\* its renewal owner and an already captured lookup result; three operations.
VARIABLES issued,generation,host,owned,active,stopped,closed,steps,event,arg,target,
          result,expected,priorIssued,expectedActive,savedGeneration,savedHost
vars == <<issued,generation,host,owned,active,stopped,closed,steps,event,arg,target,
          result,expected,priorIssued,expectedActive,savedGeneration,savedHost>>
Init == /\ issued=1 /\ generation=1 /\ host=1 /\ owned=1 /\ active=1
        /\ stopped=0 /\ closed=0 /\ steps=0 /\ event=0 /\ arg=0 /\ target=0
        /\ result=0 /\ expected=0 /\ priorIssued=1 /\ expectedActive=1
        /\ savedGeneration=1 /\ savedHost=1
Publish(e,h) ==
    /\ steps<3 /\ event'=1 /\ arg'=e /\ target'=h /\ steps'=steps+1
    /\ LET matches == e=(IF active=1 THEN generation ELSE 0)
           normal == closed=0 /\ matches /\ issued<3
           allow == closed=0 /\ (matches \/ Fault="stalePublish")
                    /\ (issued<3 \/ Fault="wrap")
       IN /\ result'=(IF allow THEN 1 ELSE 0) /\ expected'=(IF normal THEN 1 ELSE 0)
          /\ issued'=(IF allow THEN IF issued=3 THEN 1 ELSE issued+1 ELSE issued)
          /\ generation'=(IF allow THEN issued' ELSE generation)
          /\ host'=(IF allow THEN h ELSE host)
          /\ active'=(IF allow THEN 1 ELSE active)
          /\ expectedActive'=(IF normal THEN 1 ELSE active)
          /\ savedGeneration'=(IF Fault="relabel" /\ allow THEN generation' ELSE savedGeneration)
          /\ savedHost'=(IF Fault="relabel" /\ allow THEN h ELSE savedHost)
    /\ priorIssued'=issued /\ UNCHANGED <<owned,stopped,closed>>
Renew ==
    /\ steps<3 /\ event'=2 /\ arg'=0 /\ target'=0 /\ steps'=steps+1
    /\ LET normal == stopped=0 /\ closed=0 /\ active=1 /\ generation=owned /\ issued<3
           allow == stopped=0 /\ closed=0 /\ active=1
                    /\ (generation=owned \/ Fault="staleRenew") /\ issued<3
       IN /\ result'=(IF allow THEN 1 ELSE 0) /\ expected'=(IF normal THEN 1 ELSE 0)
          /\ issued'=(IF allow THEN issued+1 ELSE issued)
          /\ generation'=(IF allow THEN issued' ELSE generation)
          /\ owned'=(IF allow THEN issued' ELSE owned)
    /\ expectedActive'=active /\ priorIssued'=issued
    /\ UNCHANGED <<host,active,stopped,closed,savedGeneration,savedHost>>
Revoke(e) ==
    /\ steps<3 /\ event'=3 /\ arg'=e /\ target'=0 /\ steps'=steps+1
    /\ result'=(IF active=1 /\ e=generation THEN 1 ELSE 0) /\ expected'=result'
    /\ active'=(IF result'=1 THEN 0 ELSE active) /\ expectedActive'=active'
    /\ issued'=(IF Fault="reuse" /\ result'=1 THEN 1 ELSE issued)
    /\ priorIssued'=issued
    /\ UNCHANGED <<generation,host,owned,stopped,closed,savedGeneration,savedHost>>
Stop ==
    /\ steps<3 /\ event'=4 /\ arg'=0 /\ target'=0 /\ steps'=steps+1
    /\ expectedActive'=(IF stopped=0 /\ generation=owned THEN 0 ELSE active)
    /\ active'=(IF stopped=0 /\ (generation=owned \/ Fault="staleStop") THEN 0 ELSE active)
    /\ stopped'=1 /\ result'=1 /\ expected'=1 /\ priorIssued'=issued
    /\ UNCHANGED <<issued,generation,host,owned,closed,savedGeneration,savedHost>>
Close ==
    /\ steps<3 /\ event'=5 /\ arg'=0 /\ target'=0 /\ steps'=steps+1
    /\ active'=0 /\ closed'=1 /\ expectedActive'=0
    /\ result'=1 /\ expected'=1 /\ priorIssued'=issued
    /\ UNCHANGED <<issued,generation,host,owned,stopped,savedGeneration,savedHost>>
Next == (\E e\in 0..3,h\in 1..2: Publish(e,h)) \/ Renew
        \/ (\E e\in 1..3: Revoke(e)) \/ Stop \/ Close
Spec == Init /\ [][Next]_vars
TypeOK == /\ issued\in 1..3 /\ generation\in 1..3 /\ host\in 1..2 /\ owned\in 1..3
          /\ active\in 0..1 /\ stopped\in 0..1 /\ closed\in 0..1 /\ steps\in 0..3
          /\ event\in 0..5 /\ arg\in 0..3 /\ target\in 0..2 /\ result\in 0..1
          /\ expected\in 0..1 /\ priorIssued\in 1..3 /\ expectedActive\in 0..1
          /\ savedGeneration\in 1..3 /\ savedHost\in 1..2
NoReuse == issued>=priorIssued
ExpectedOutcome == result=expected
SuccessorSurvives == active=expectedActive
CapturedBinding == savedGeneration=1 /\ savedHost=1
CurrentGeneration == generation<=issued /\ owned<=issued
Closed == closed=1 => active=0
=============================================================================
