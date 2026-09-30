---------------------- MODULE AuthenticatedHandoff -------------------------
EXTENDS Naturals
CONSTANT Fault
\* One provided introduction, one outstanding proxy call, one dedicated Noise
\* session, two Accept attempts and at most four operations. Proof abstracts
\* a completed, correctly bound handshake; Rust replay uses real UDP/Noise.
VARIABLES status, proof, phase, pending, accepted, live, swapped, attempts,
          result, expected, event, steps
vars == <<status,proof,phase,pending,accepted,live,swapped,attempts,
          result,expected,event,steps>>
Init == /\ status=0 /\ proof=0 /\ phase=1 /\ pending=1 /\ accepted=0
        /\ live=1 /\ swapped=0 /\ attempts=0 /\ result=0 /\ expected=0
        /\ event=0 /\ steps=0
Open(valid) ==
    /\ steps<4 /\ status=0
    /\ LET allowed == live=1 /\ swapped=0 /\ (valid \/ Fault="falseProof")
       IN /\ status'=(IF allowed THEN 1 ELSE 2)
          /\ result'=(IF allowed THEN 1 ELSE 0)
    /\ proof'=(IF valid THEN 1 ELSE 0)
    /\ expected'=(IF live=1 /\ swapped=0 /\ valid THEN 1 ELSE 0)
    /\ event'=(IF valid THEN 1 ELSE 2) /\ steps'=steps+1
    /\ UNCHANGED <<phase,pending,accepted,live,swapped,attempts>>
Drain ==
    /\ steps<4 /\ pending=1 /\ pending'=0
    /\ result'=1 /\ expected'=1 /\ event'=3 /\ steps'=steps+1
    /\ UNCHANGED <<status,proof,phase,accepted,live,swapped,attempts>>
Request(correct) ==
    /\ steps<4 /\ status\in {1,3} /\ attempts<2
    /\ LET allowed == status=1 /\ (correct \/ Fault="wrongId")
                        /\ (live=1 \/ Fault="revoked")
                        /\ (swapped=0 \/ Fault="rebound")
       IN /\ phase'=(IF allowed THEN IF pending=0 THEN 3 ELSE 2 ELSE phase)
          /\ accepted'=(IF allowed THEN IF Fault="duplicateGrant"
                          THEN accepted+1 ELSE 1 ELSE accepted)
          /\ result'=(IF allowed /\ (pending=0 \/ Fault="earlyGrant") THEN 1 ELSE 0)
    /\ expected'=(IF status=1 /\ correct /\ live=1 /\ swapped=0 /\ pending=0
                  THEN 1 ELSE 0)
    /\ attempts'=attempts+1 /\ event'=(IF correct THEN 4 ELSE 5) /\ steps'=steps+1
    /\ UNCHANGED <<status,proof,pending,live,swapped>>
Revoke ==
    /\ steps<4 /\ live=1 /\ live'=0 /\ result'=1 /\ expected'=1
    /\ event'=6 /\ steps'=steps+1
    /\ UNCHANGED <<status,proof,phase,pending,accepted,swapped,attempts>>
Replace ==
    /\ steps<4 /\ swapped=0 /\ live=1
    /\ swapped'=1 /\ phase'=1 /\ pending'=1 /\ accepted'=0
    /\ result'=1 /\ expected'=1 /\ event'=7 /\ steps'=steps+1
    /\ UNCHANGED <<status,proof,live,attempts>>
Close ==
    /\ steps<4 /\ status=1 /\ status'=3
    /\ result'=1 /\ expected'=1 /\ event'=8 /\ steps'=steps+1
    /\ UNCHANGED <<proof,phase,pending,accepted,live,swapped,attempts>>
Next == Open(TRUE) \/ Open(FALSE) \/ Drain \/ Request(TRUE) \/ Request(FALSE)
        \/ Revoke \/ Replace \/ Close
Spec == Init /\ [][Next]_vars
TypeOK == /\ status\in 0..3 /\ proof\in 0..1 /\ phase\in 1..3
          /\ pending\in 0..1 /\ accepted\in 0..2 /\ live\in 0..1
          /\ swapped\in 0..1 /\ attempts\in 0..2 /\ result\in 0..1
          /\ expected\in 0..1 /\ event\in 0..8 /\ steps\in 0..4
AuthenticatedPublication == status=1 => proof=1
CapabilitySafety == event\in {4,5} /\ result=1 =>
                    status=1 /\ proof=1 /\ live=1 /\ swapped=0 /\ pending=0 /\ event=4
SingleAcceptance == accepted<=1
ExpectedOutcome == result=expected
=============================================================================
