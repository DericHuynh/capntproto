------------------------- MODULE RpcNoiseRendezvous -------------------------
EXTENDS Naturals, TLC
CONSTANTS Steal, Replay
VARIABLES phase, good, bad, event
vars == <<phase,good,bad,event>>
Init == /\ phase=0 /\ good=0 /\ bad=0 /\ event=0
Provide == /\ phase=0 /\ phase'=1 /\ event'=1
           /\ good'=IF good=1 THEN 2 ELSE good
           /\ bad'=IF Steal /\ bad=1 THEN 2 ELSE bad
AcceptGood == /\ good=0 /\ event'=2
              /\ good'=IF phase=0 THEN 1 ELSE IF phase=1 \/ Replay THEN 2 ELSE 4
              /\ UNCHANGED <<phase,bad>>
AcceptBad == /\ bad=0 /\ event'=3
             /\ bad'=IF Steal /\ phase=1 THEN 2 ELSE 1
             /\ UNCHANGED <<phase,good>>
CancelGood == /\ good=1 /\ good'=3 /\ event'=4 /\ UNCHANGED <<phase,bad>>
CancelBad == /\ bad=1 /\ bad'=3 /\ event'=5 /\ UNCHANGED <<phase,good>>
Finish == /\ phase=1 /\ phase'=2 /\ event'=6 /\ UNCHANGED <<good,bad>>
Next == Provide \/ AcceptGood \/ AcceptBad \/ CancelGood \/ CancelBad \/ Finish
Spec == Init /\ [][Next]_vars
TypeOK == /\ phase\in 0..2 /\ good\in 0..4 /\ bad\in 0..3 /\ event\in 0..6
RecipientBinding == bad#2
NoRetiredAccept == (event=2 /\ phase=2) => good=4
NoEarlyResult == phase=0 => good#2
=============================================================================
