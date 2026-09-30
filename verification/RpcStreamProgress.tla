------------------------- MODULE RpcStreamProgress -------------------------
EXTENDS Naturals
CONSTANT Fault
VARIABLES read, sent, accepted, txEof, fin, graceful, received, delivered,
          rxFin, closed, event, amount
vars == <<read,sent,accepted,txEof,fin,graceful,received,delivered,
          rxFin,closed,event,amount>>
Init == /\ read=0 /\ sent=0 /\ accepted=0 /\ txEof=0 /\ fin=0 /\ graceful=0
        /\ received=0 /\ delivered=0 /\ rxFin=0 /\ closed=0 /\ event=0 /\ amount=0
Record(e,n) == /\ event'=e /\ amount'=n
Read(n) == /\ txEof=0 /\ read=sent /\ read+n<=2 /\ read'=read+n /\ Record(1,n)
           /\ UNCHANGED <<sent,accepted,txEof,fin,graceful,received,delivered,rxFin,closed>>
Send(n) == /\ sent<read /\ n<=read-sent /\ accepted'=accepted+n
           /\ sent'=(IF Fault="loseWrite" THEN read ELSE sent+n) /\ Record(2,n)
           /\ UNCHANGED <<read,txEof,fin,graceful,received,delivered,rxFin,closed>>
Eof == /\ txEof=0 /\ read=sent /\ txEof'=1 /\ Record(3,0)
       /\ UNCHANGED <<read,sent,accepted,fin,graceful,received,delivered,rxFin,closed>>
Finish == /\ fin=0 /\ graceful=0 /\ (txEof=1 \/ Fault="earlyFin")
          /\ fin'=1 /\ Record(4,0)
          /\ UNCHANGED <<read,sent,accepted,txEof,graceful,received,delivered,rxFin,closed>>
Graceful == /\ graceful=0 /\ fin=0 /\ graceful'=1 /\ Record(5,0)
            /\ UNCHANGED <<read,sent,accepted,txEof,fin,received,delivered,rxFin,closed>>
Receive(n,end) == /\ rxFin=0 /\ received=delivered /\ received+n<=2
                 /\ (n>0 \/ end=1)
                 /\ received'=received+n /\ rxFin'=end /\ Record(6,n)
                 /\ UNCHANGED <<read,sent,accepted,txEof,fin,graceful,delivered,closed>>
Deliver(n) == /\ n<=received-delivered /\ delivered'=delivered+n /\ Record(7,n)
              /\ UNCHANGED <<read,sent,accepted,txEof,fin,graceful,received,rxFin,closed>>
Close == /\ closed=0 /\ ((rxFin=1 /\ received=delivered) \/ Fault="earlyClose")
         /\ closed'=1 /\ Record(8,0)
         /\ UNCHANGED <<read,sent,accepted,txEof,fin,graceful,received,delivered,rxFin>>
CancelWait == /\ Record(9,0)
              /\ UNCHANGED <<read,sent,accepted,txEof,fin,graceful,received,delivered,rxFin,closed>>
Next == (\E n\in 1..2: Read(n)) \/ (\E n\in 0..2: Send(n)) \/ Eof \/ Finish \/ Graceful
        \/ (\E n\in 0..2,end\in 0..1: Receive(n,end))
        \/ (\E n\in 1..2: Deliver(n)) \/ Close \/ CancelWait
Spec == Init /\ [][Next]_vars
TypeOK == /\ read\in 0..2 /\ sent\in 0..2 /\ accepted\in 0..2
          /\ received\in 0..2 /\ delivered\in 0..2
          /\ txEof\in 0..1 /\ fin\in 0..1 /\ graceful\in 0..1
          /\ rxFin\in 0..1 /\ closed\in 0..1 /\ event\in 0..9 /\ amount\in 0..2
ByteConservation == sent=accepted /\ sent<=read /\ delivered<=received
FinSound == fin=1 => txEof=1 /\ sent=read /\ graceful=0
CloseSound == closed=1 => rxFin=1 /\ received=delivered
=============================================================================
