------------------------ MODULE RpcPersistentSave ------------------------
EXTENDS Naturals, TLC
CONSTANTS Fault, DenyAfterStream
VARIABLES blocked, permitted, phase, caller, closed, issued, valid
vars == <<blocked,permitted,phase,caller,closed,issued,valid>>
Init == /\ blocked=TRUE /\ permitted=TRUE /\ phase=0 /\ caller=TRUE
        /\ closed=FALSE /\ issued=FALSE /\ valid=TRUE
Start == /\ phase=0 /\ phase'=(IF blocked THEN 1 ELSE 2)
         /\ issued'=(~blocked /\ ~closed /\ permitted)
         /\ valid'=(~issued' \/ ~DenyAfterStream)
         /\ UNCHANGED <<blocked,permitted,caller,closed>>
Stream == /\ blocked /\ blocked'=FALSE /\ permitted'=~DenyAfterStream
          /\ UNCHANGED <<phase,caller,closed,issued,valid>>
DropCaller == /\ phase=1 /\ caller /\ caller'=FALSE
              /\ phase'=(IF Fault="cancelProtected" THEN 3 ELSE phase)
              /\ UNCHANGED <<blocked,permitted,closed,issued,valid>>
Close == /\ ~closed /\ closed'=TRUE
         /\ UNCHANGED <<blocked,permitted,phase,caller,issued,valid>>
Finish == /\ phase=1 /\ (~blocked \/ Fault="skipBarrier") /\ phase'=2
          /\ issued'=(~closed /\ permitted)
          /\ valid'=(~issued' \/ (~blocked /\ ~DenyAfterStream))
          /\ UNCHANGED <<blocked,permitted,caller,closed>>
Next == Start \/ Stream \/ DropCaller \/ Close \/ Finish
Spec == Init /\ [][Next]_vars
TypeOK == /\ phase\in 0..3 /\ \A b\in {blocked,permitted,caller,closed,issued,valid}: b\in BOOLEAN
OrderedSave == valid
ProtectedSave == phase#3
=============================================================================
