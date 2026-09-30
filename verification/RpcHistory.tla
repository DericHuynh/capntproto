-------------------- MODULE RpcHistory --------------------
EXTENDS Naturals, TLC
CONSTANT Fault
\* Two publications, two explicit consumer requests. Actions are observed after
\* draining the executor. Delivered results remain valid after later revocation.
VARIABLES published, floor, live, closed, cursor, status, value, resultFloor,
          calls, canceled, revoked, trimmed, dropped, acknowledged
vars == <<published,floor,live,closed,cursor,status,value,resultFloor,calls,
          canceled,revoked,trimmed,dropped,acknowledged>>
\* status: 0=idle, 1=waiting, 2=event, 3=gap, 4=error.
Reply(pub, fl, alive, shut) ==
  IF (~alive /\ Fault#"ignoreRevoke") \/ shut THEN <<4,0,0>>
  ELSE IF cursor<fl THEN <<3,0,fl>>
  ELSE IF cursor<pub THEN <<2,cursor+1,fl>>
  ELSE <<1,0,0>>
SetReply(reply) == /\ status'=reply[1] /\ value'=reply[2] /\ resultFloor'=reply[3]
Init == /\ published=0 /\ floor=0 /\ live=TRUE /\ closed=FALSE /\ cursor=0
        /\ status=0 /\ value=0 /\ resultFloor=0 /\ calls=0
        /\ canceled=FALSE /\ revoked=FALSE /\ trimmed=FALSE
        /\ dropped=FALSE /\ acknowledged=FALSE
Start == /\ status#1 /\ calls<2 /\ calls'=calls+1
         /\ SetReply(Reply(published,floor,live,closed))
         /\ cursor'=(IF Fault="implicitAck" /\ Reply(published,floor,live,closed)[1]=2
                        THEN cursor+1 ELSE cursor)
         /\ UNCHANGED <<published,floor,live,closed,canceled,revoked,trimmed,dropped,acknowledged>>
Publish == /\ published<2 /\ published'=published+1
           /\ IF status=1 THEN SetReply(Reply(published+1,floor,live,closed))
                          ELSE UNCHANGED <<status,value,resultFloor>>
           /\ UNCHANGED <<floor,live,closed,cursor,calls,canceled,revoked,trimmed,dropped,acknowledged>>
Trim == /\ ~trimmed /\ trimmed'=TRUE /\ floor'=published
        /\ IF status=1 THEN SetReply(Reply(published,published,live,closed))
                       ELSE UNCHANGED <<status,value,resultFloor>>
        /\ UNCHANGED <<published,live,closed,cursor,calls,canceled,revoked,dropped,acknowledged>>
Cancel == /\ ~canceled /\ canceled'=TRUE /\ closed'=TRUE
          /\ IF status=1 /\ Fault#"stuckCancel" THEN SetReply(<<4,0,0>>)
                                                   ELSE UNCHANGED <<status,value,resultFloor>>
          /\ UNCHANGED <<published,floor,live,cursor,calls,revoked,trimmed,dropped,acknowledged>>
Revoke == /\ ~revoked /\ revoked'=TRUE /\ live'=FALSE
          /\ IF status=1 THEN SetReply(Reply(published,floor,FALSE,closed))
                         ELSE UNCHANGED <<status,value,resultFloor>>
          /\ UNCHANGED <<published,floor,closed,cursor,calls,canceled,trimmed,dropped,acknowledged>>
Drop == /\ status=1 /\ ~dropped /\ dropped'=TRUE
        /\ SetReply(<<0,0,0>>)
        /\ UNCHANGED <<published,floor,live,closed,cursor,calls,canceled,revoked,trimmed,acknowledged>>
Ack == /\ status=2 /\ ~acknowledged /\ acknowledged'=TRUE
       /\ cursor'=value /\ SetReply(<<0,0,0>>)
       /\ UNCHANGED <<published,floor,live,closed,calls,canceled,revoked,trimmed,dropped>>
Next == Start \/ Publish \/ Trim \/ Cancel \/ Revoke \/ Drop \/ Ack
Spec == Init /\ [][Next]_vars
TypeOK == /\ published\in 0..2 /\ floor\in 0..2 /\ cursor\in 0..2
          /\ status\in 0..4 /\ value\in 0..2 /\ resultFloor\in 0..2 /\ calls\in 0..2
          /\ \A b\in {live,closed,canceled,revoked,trimmed,dropped,acknowledged}: b\in BOOLEAN
ConsumerOwnsCursor == ~acknowledged => cursor=0
OrderedEvent == status=2 => value>cursor /\ value<=published /\ value>resultFloor
WaitIsAuthorized == status=1 => live /\ ~closed /\ cursor=published /\ cursor>=floor
GapIsExplicit == status=3 => cursor<resultFloor
\* Under fair publish/cleanup, no pending call waits indefinitely.
FairSpec == Spec /\ WF_vars(Publish) /\ WF_vars(Cancel) /\ WF_vars(Revoke)
WaitTerminates == (status=1) ~> (status#1)
=============================================================================
