---------------------- MODULE RpcRevocableStreaming ----------------------
EXTENDS Naturals, TLC
CONSTANTS Allow, Fail, SkipQueue, IgnoreProtected
VARIABLES running, completed, caller, revoked, sent, bcaller, bstarted, aout, bout, broken
vars == <<running,completed,caller,revoked,sent,bcaller,bstarted,aout,bout,broken>>
\* A is a streaming method sent as an ordinary request (waiting for its ack).
\* B is an ordinary method on the same capability. Outcomes: 0 pending/absent,
\* 1 success, 2 revoked, 3 sticky streaming failure.
Init == /\ running=TRUE /\ completed=FALSE /\ caller=TRUE /\ revoked=FALSE
        /\ sent=FALSE /\ bcaller=FALSE /\ bstarted=FALSE /\ aout=0 /\ bout=0 /\ broken=FALSE
Queued == sent /\ bcaller /\ bout=0
SendB == /\ ~sent /\ sent'=TRUE /\ bcaller'=TRUE
         /\ bout' = IF revoked THEN 2 ELSE IF broken THEN 3 ELSE IF running /\ caller /\ ~SkipQueue THEN 0 ELSE 1
         /\ bstarted' = (bout'=1)
         /\ UNCHANGED <<running,completed,caller,revoked,aout,broken>>
DropA == /\ caller /\ caller'=FALSE /\ running' = (running /\ ~Allow)
         /\ bout' = IF Queued /\ ~revoked /\ ~broken THEN 1 ELSE bout
         /\ bstarted' = (bstarted \/ bout'=1)
         /\ UNCHANGED <<completed,revoked,sent,bcaller,aout,broken>>
DropB == /\ bcaller /\ bcaller'=FALSE
         /\ UNCHANGED <<running,completed,caller,revoked,sent,bstarted,aout,bout,broken>>
Complete == /\ running /\ running'=FALSE /\ completed'=TRUE
            /\ aout' = IF caller THEN IF Fail THEN 3 ELSE 1 ELSE aout
            /\ broken' = (broken \/ (caller /\ Fail))
            /\ bout' = IF Queued THEN IF broken' THEN 3 ELSE 1 ELSE bout
            /\ bstarted' = (bstarted \/ bout'=1)
            /\ UNCHANGED <<caller,revoked,sent,bcaller>>
Revoke == /\ ~revoked /\ revoked'=TRUE
          /\ running' = (running /\ ~Allow /\ IgnoreProtected)
          /\ aout' = IF caller /\ ~completed /\ ~running' THEN 2 ELSE aout
          /\ bout' = IF Queued THEN 2 ELSE bout
          /\ UNCHANGED <<completed,caller,sent,bcaller,bstarted,broken>>
Next == SendB \/ DropA \/ DropB \/ Complete \/ Revoke
Spec == Init /\ [][Next]_vars
TypeOK == /\ running \in BOOLEAN /\ completed \in BOOLEAN /\ caller \in BOOLEAN
          /\ revoked \in BOOLEAN /\ sent \in BOOLEAN /\ bcaller \in BOOLEAN
          /\ bstarted \in BOOLEAN /\ aout \in 0..3 /\ bout \in 0..3 /\ broken \in BOOLEAN
NoOvertake == (bstarted /\ ~completed /\ caller) => revoked
RevokedTask == revoked => ~running
NoDispatchAfterFailure == (bout=2 \/ bout=3) => ~bstarted
=============================================================================
