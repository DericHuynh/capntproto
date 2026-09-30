----------------------------- MODULE RpcNoiseRoutes -----------------------------
EXTENDS Naturals, TLC
CONSTANTS WrongIdentity, EarlyData, OrphanDial, MalformedKey
VARIABLES phase, refs, attempts, closed, valid, delivered, event
vars == <<phase,refs,attempts,closed,valid,delivered,event>>
\* phase: absent, connecting, authenticated, failed. One reserved generation,
\* two handles. Network close, explicit disconnect and last-handle release cancel.
Init == /\ phase=0 /\ refs=0 /\ attempts=0 /\ closed=FALSE
        /\ valid=FALSE /\ delivered=FALSE /\ event=0
Request == /\ ~closed /\ attempts=0 /\ phase=0 /\ refs'=1
           /\ attempts'=1 /\ phase'=1 /\ event'=1
           /\ UNCHANGED <<closed,valid,delivered>>
Share == /\ ~closed /\ refs=1 /\ refs'=2 /\ event'=2
         /\ UNCHANGED <<phase,attempts,closed,valid,delivered>>
Authenticate == /\ phase=1 /\ phase'=2 /\ valid'=TRUE /\ event'=3
                /\ UNCHANGED <<refs,attempts,closed,delivered>>
Reject == /\ phase=1 /\ phase'=(IF WrongIdentity THEN 2 ELSE 3) /\ event'=4
          /\ UNCHANGED <<refs,attempts,closed,valid,delivered>>
BadKey == /\ phase=1 /\ phase'=(IF MalformedKey THEN 2 ELSE 3) /\ event'=10
          /\ UNCHANGED <<refs,attempts,closed,valid,delivered>>
Fail == /\ phase=1 /\ phase'=3 /\ event'=5
        /\ UNCHANGED <<refs,attempts,closed,valid,delivered>>
Release == /\ refs>0 /\ refs'=refs-1 /\ event'=6
           /\ phase'=(IF refs=1 /\ ~OrphanDial THEN 0 ELSE phase)
           /\ UNCHANGED <<attempts,closed,valid,delivered>>
Disconnect == /\ refs>0 /\ refs'=0 /\ phase'=0 /\ event'=7
              /\ UNCHANGED <<attempts,closed,valid,delivered>>
Close == /\ ~closed /\ closed'=TRUE /\ refs'=0 /\ phase'=0 /\ event'=8
         /\ UNCHANGED <<attempts,valid,delivered>>
Deliver == /\ ~delivered /\ (phase=2 \/ (EarlyData /\ phase=1))
           /\ delivered'=TRUE /\ event'=9 /\ UNCHANGED <<phase,refs,attempts,closed,valid>>
Next == Request \/ Share \/ Authenticate \/ Reject \/ Fail \/ Release \/ Disconnect \/ Close \/ Deliver \/ BadKey
Spec == Init /\ [][Next]_vars
TypeOK == /\ phase\in 0..3 /\ refs\in 0..2 /\ attempts\in 0..1
          /\ closed\in BOOLEAN /\ valid\in BOOLEAN /\ delivered\in BOOLEAN /\ event\in 0..10
IdentityBinding == phase=2 => valid
AuthenticatedData == delivered => valid
Lifetime == /\ (phase#0 => refs>0) /\ (closed => phase=0 /\ refs=0)
================================================================================
