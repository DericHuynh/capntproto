------------------------ MODULE RpcPipelineFence ------------------------
EXTENDS Naturals
CONSTANT Fault
VARIABLES route, proof, queued, canceled, delivered, lost, checked, alive, event
vars == <<route,proof,queued,canceled,delivered,lost,checked,alive,event>>
Init == /\ route=0 /\ proof=0 /\ queued=0 /\ canceled=0 /\ delivered=0
        /\ lost=0 /\ checked=0 /\ alive=0 /\ event=0
Queue == /\ route=0 /\ queued=0 /\ canceled=0 /\ queued'=1 /\ event'=1
         /\ delivered'=IF Fault="early" THEN 1 ELSE delivered
         /\ UNCHANGED <<route,proof,canceled,lost,checked,alive>>
Cancel == /\ route=0 /\ queued=1 /\ queued'=0 /\ canceled'=1 /\ event'=2
          /\ delivered'=IF Fault="cancellation" THEN 1 ELSE delivered
          /\ UNCHANGED <<route,proof,lost,checked,alive>>
Prove == /\ route=0 /\ route'=1 /\ proof'=1 /\ event'=3
         /\ UNCHANGED <<queued,canceled,delivered,lost,checked,alive>>
Expire == /\ route=0 /\ route'=(IF Fault="fallback" THEN 1 ELSE 2) /\ event'=4
          /\ UNCHANGED <<proof,queued,canceled,delivered,lost,checked,alive>>
Observe == /\ route#0 /\ queued=1 /\ queued'=0 /\ delivered'=1 /\ event'=5
           /\ UNCHANGED <<route,proof,canceled,lost,checked,alive>>
Lose == /\ route#0 /\ queued=0 /\ lost=0 /\ lost'=1 /\ event'=6
        /\ UNCHANGED <<route,proof,queued,canceled,delivered,checked,alive>>
Check == /\ lost=1 /\ checked=0 /\ checked'=1 /\ alive'=(IF route=1 THEN 1 ELSE 0) /\ event'=7
         /\ UNCHANGED <<route,proof,queued,canceled,delivered,lost>>
Next == Queue \/ Cancel \/ Prove \/ Expire \/ Observe \/ Lose \/ Check
Spec == Init /\ [][Next]_vars
TypeOK == /\ route\in 0..2 /\ proof\in 0..1 /\ queued\in 0..1 /\ canceled\in 0..1
          /\ delivered\in 0..1 /\ lost\in 0..1 /\ checked\in 0..1 /\ alive\in 0..1 /\ event\in 0..7
QueueUntilProof == route=0 /\ canceled=0 => delivered=0
Authenticated == route=1 => proof=1
Cancellation == canceled=1 => delivered=0
Survival == checked=1 => (alive=1 <=> route=1)
LiveSpec == Spec /\ WF_vars(Expire) /\ WF_vars(Observe)
Settles == route=0 ~> route#0
CallsSettle == queued=1 ~> queued=0
=============================================================================
