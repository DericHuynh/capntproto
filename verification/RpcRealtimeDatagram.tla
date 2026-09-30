------------------------ MODULE RpcRealtimeDatagram ------------------------
EXTENDS Naturals, TLC
CONSTANT Bug
VARIABLES live, lane, status, queued, applied, canceled, closed, unauthorized,
          response, responseClosed, reply, replyClosed, event
vars == <<live,lane,status,queued,applied,canceled,closed,unauthorized,
          response,responseClosed,reply,replyClosed,event>>
\* One immutable snapshot, up to two copies in the lossy network. Status is
\* unknown/pending/applied/canceled/closed. Publication is the existing receiver
\* action; replacement, skew and expiry are checked by RpcRealtimeReceiver.
Init == /\ live=TRUE /\ lane=TRUE /\ status=0 /\ queued=0 /\ applied=0
        /\ canceled=FALSE /\ closed=FALSE /\ unauthorized=FALSE
        /\ response=0 /\ responseClosed=FALSE /\ reply=0 /\ replyClosed=FALSE /\ event=0
Enqueue == /\ lane /\ queued<2 /\ queued'=queued+1 /\ event'=1
           /\ UNCHANGED <<live,lane,status,applied,canceled,closed,unauthorized,response,responseClosed,reply,replyClosed>>
Deliver == /\ lane /\ queued>0 /\ queued'=queued-1 /\ event'=2
           /\ status'=IF (live \/ Bug="closeLeak") /\
                  (status=0 \/ (Bug="forgetCancel" /\ status=3) \/
                   (Bug="duplicate" /\ status=2) \/ (Bug="closeLeak" /\ status=4))
                      THEN 1 ELSE status
           /\ UNCHANGED <<live,lane,applied,canceled,closed,unauthorized,response,responseClosed,reply,replyClosed>>
Lose == /\ queued>0 /\ queued'=queued-1 /\ event'=3
        /\ UNCHANGED <<live,lane,status,applied,canceled,closed,unauthorized,response,responseClosed,reply,replyClosed>>
Cancel == /\ status\in {0,1} /\ event'=4
          /\ status'=IF closed THEN 4 ELSE 3
          /\ canceled'=~closed
          /\ UNCHANGED <<live,lane,queued,applied,closed,unauthorized,response,responseClosed,reply,replyClosed>>
Apply == /\ status=1 /\ status'=2 /\ applied'=applied+1 /\ event'=5
         /\ UNCHANGED <<live,lane,queued,canceled,closed,unauthorized,response,responseClosed,reply,replyClosed>>
Revoke == /\ live /\ live'=FALSE /\ closed'=TRUE /\ event'=6
          /\ status'=IF status=1 THEN 4 ELSE status
          /\ UNCHANGED <<lane,queued,applied,canceled,unauthorized,response,responseClosed,reply,replyClosed>>
Stop == /\ lane /\ lane'=FALSE /\ live'=FALSE /\ closed'=TRUE /\ queued'=0 /\ event'=7
        /\ status'=IF status=1 THEN 4 ELSE status
        /\ UNCHANGED <<applied,canceled,unauthorized,response,responseClosed,reply,replyClosed>>
BadToken == /\ lane /\ event'=8
            /\ unauthorized'=(unauthorized \/ (Bug="authority" /\ live /\ status=0))
            /\ status'=IF Bug="authority" /\ live /\ status=0 THEN 1 ELSE status
            /\ UNCHANGED <<live,lane,queued,applied,canceled,closed,response,responseClosed,reply,replyClosed>>
Malformed == /\ lane /\ event'=9
             /\ UNCHANGED <<live,lane,status,queued,applied,canceled,closed,unauthorized,response,responseClosed,reply,replyClosed>>
\* A reliable reply can be generated before data arrives but observed after it
\* has been applied. Unknown and pending replies therefore make no final claim.
Query == /\ response=0 /\ event'=10 /\ responseClosed'=closed
         /\ response'=IF Bug="falseReceipt" /\ status=0 THEN 3 ELSE status+1
         /\ UNCHANGED <<live,lane,status,queued,applied,canceled,closed,unauthorized,reply,replyClosed>>
Reply == /\ response>0 /\ reply'=response /\ replyClosed'=responseClosed
         /\ response'=0 /\ responseClosed'=FALSE /\ event'=11
         /\ UNCHANGED <<live,lane,status,queued,applied,canceled,closed,unauthorized>>
Next == Enqueue \/ Deliver \/ Lose \/ Cancel \/ Apply \/ Revoke \/ Stop \/ BadToken \/ Malformed \/ Query \/ Reply
Spec == Init /\ [][Next]_vars
TypeOK == /\ live\in BOOLEAN /\ lane\in BOOLEAN /\ status\in 0..4 /\ queued\in 0..2
          /\ applied\in 0..2 /\ canceled\in BOOLEAN /\ closed\in BOOLEAN
          /\ unauthorized\in BOOLEAN /\ response\in 0..5 /\ reply\in 0..5
          /\ responseClosed\in BOOLEAN /\ replyClosed\in BOOLEAN /\ event\in 0..11
Authority == ~unauthorized
AtMostOnce == applied<=1
NoCanceled == canceled => status=3
Revoked == closed => status#1
ReceiptTruth == (response=3 \/ reply=3) => applied>0
Lifetime == (~lane => ~live /\ closed /\ queued=0) /\ (~live => closed)
=============================================================================
