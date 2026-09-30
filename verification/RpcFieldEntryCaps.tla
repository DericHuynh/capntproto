-------------------- MODULE RpcFieldEntryCaps --------------------
EXTENDS Naturals, TLC
CONSTANT Fault
VARIABLES slot, entry, client, inspected, consumed, reads, alive, leaked, event
vars == <<slot,entry,client,inspected,consumed,reads,alive,leaked,event>>
Init == /\ slot=TRUE /\ entry=FALSE /\ client=FALSE /\ inspected=FALSE
        /\ consumed=FALSE /\ reads=0 /\ alive=TRUE /\ leaked=FALSE /\ event=0
Inspect == /\ slot /\ ~inspected /\ inspected'=TRUE /\ entry'=TRUE /\ reads'=1 /\ event'=1
           /\ UNCHANGED <<slot,client,consumed,leaked>>
Consume == /\ entry /\ entry'=FALSE /\ client'=TRUE /\ consumed'=TRUE
           /\ reads'=(IF Fault="reacquire" THEN reads+1 ELSE reads) /\ event'=2
           /\ UNCHANGED <<slot,inspected,leaked>>
DropEntry == /\ entry /\ entry'=FALSE /\ leaked'=(Fault="leakEntry") /\ event'=3
             /\ UNCHANGED <<slot,client,inspected,consumed,reads>>
Clear == /\ slot /\ ~entry /\ slot'=FALSE /\ event'=4
         /\ UNCHANGED <<entry,client,inspected,consumed,reads,leaked>>
DropClient == /\ client /\ client'=FALSE /\ event'=5
              /\ UNCHANGED <<slot,entry,inspected,consumed,reads,leaked>>
Next == /\ (Inspect \/ Consume \/ DropEntry \/ Clear \/ DropClient)
        /\ alive'=(IF Fault="loseClient" THEN slot' \/ entry' ELSE slot' \/ entry' \/ client' \/ leaked')
Spec == Init /\ [][Next]_vars
TypeOK == /\ \A b\in {slot,entry,client,inspected,consumed,alive,leaked}: b\in BOOLEAN
          /\ reads\in 0..2 /\ event\in 0..5
CapabilityLifetime == alive=(slot \/ entry \/ client)
CachedHook == reads<=1
BorrowedSlot == entry => slot
=============================================================================
