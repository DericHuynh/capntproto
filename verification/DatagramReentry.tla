------------------------- MODULE DatagramReentry -------------------------
EXTENDS Naturals
CONSTANT Fault
\* One local sender and its clone; one/two sequence IDs and three free queue
\* slots. A synchronous receiver waker may offer two fragments, close the
\* sender, or retry an earlier receipt during the outer batch's first send.
\* Packet queue digits (base four): sequence 1/2 or unrelated pressure 3.
VARIABLES limit,fragments,callback,next,closed,connected,queue,queued,receipt,width,
          event,result,expected,outerSequence,nestedSequence,nestedResult,woke,
          seenNext,seenCapacity,beforeNext,beforeQueue,beforeQueued,expectedQueue
vars == <<limit,fragments,callback,next,closed,connected,queue,queued,receipt,width,
          event,result,expected,outerSequence,nestedSequence,nestedResult,woke,
          seenNext,seenCapacity,beforeNext,beforeQueue,beforeQueued,expectedQueue>>
Power(n) == CASE n=0->1 [] n=1->4 [] n=2->16 [] OTHER->64
Append(q,n,s,k) == q + s*Power(n)*(IF k=0 THEN 0 ELSE IF k=1 THEN 1 ELSE 5)
Pressure(p) == CASE p=0->0 [] p=1->3 [] p=2->15 [] OTHER->63
OfferResult(n,c,t,space,k) == IF c=1 \/ n>limit THEN 3
                            ELSE IF t=0 THEN 4 ELSE IF space<k THEN 2 ELSE 1
RetryResult(c,t,space,k) == IF c=1 \/ t=0 THEN 4 ELSE IF space<k THEN 2 ELSE 1
Init == /\ limit=0 /\ fragments=0 /\ callback=0 /\ next=1 /\ closed=0
        /\ connected=0 /\ queue=0 /\ queued=0 /\ receipt=0 /\ width=0
        /\ event=0 /\ result=0 /\ expected=0 /\ outerSequence=0
        /\ nestedSequence=0 /\ nestedResult=0 /\ woke=0 /\ seenNext=0
        /\ seenCapacity=0 /\ beforeNext=1 /\ beforeQueue=0 /\ beforeQueued=0
        /\ expectedQueue=0
Configure(l,pre,p,f,a,c,t) ==
    /\ event=0 /\ pre<=l
    /\ limit'=l /\ next'=pre+1 /\ queue'=Pressure(p) /\ queued'=p
    /\ fragments'=f /\ callback'=a /\ closed'=c /\ connected'=t
    /\ receipt'=pre /\ width'=(IF pre=0 THEN 0 ELSE 1) /\ event'=1
    /\ UNCHANGED <<result,expected,outerSequence,nestedSequence,nestedResult,woke,
                    seenNext,seenCapacity,beforeNext,beforeQueue,beforeQueued,expectedQueue>>
Offer ==
    /\ event=1 /\ event'=2
    /\ beforeNext'=next /\ beforeQueue'=queue /\ beforeQueued'=queued
    /\ expected'=OfferResult(next,closed,connected,3-queued,fragments)
    /\ result'=OfferResult(next,IF Fault="close" THEN 0 ELSE closed,connected,3-queued,fragments)
    /\ outerSequence'=(IF result'=1 THEN next ELSE 0)
    /\ woke'=(IF result'=1 /\ callback>0 /\ queued=0 THEN 1 ELSE 0)
    /\ seenNext'=(IF woke'=0 THEN 0 ELSE IF Fault="late" THEN next ELSE next+1)
    /\ seenCapacity'=(IF woke'=0 THEN 0 ELSE 3-queued-(IF Fault="reserve" THEN 1 ELSE fragments))
    /\ nestedResult'=(IF woke'=0 THEN 0
                         ELSE IF callback=1 THEN OfferResult(seenNext',closed,connected,seenCapacity',2)
                         ELSE IF callback=3 /\ receipt>0 THEN RetryResult(closed,connected,seenCapacity',width)
                         ELSE 0)
    /\ nestedSequence'=(IF callback=1 /\ nestedResult'=1 THEN seenNext' ELSE 0)
    /\ next'=(IF result'#1 THEN next+(IF Fault="burn" THEN 1 ELSE 0)
                ELSE IF Fault\in {"late","rewind"} THEN next+1
                ELSE next+1+(IF nestedSequence'>0 THEN 1 ELSE 0))
    /\ closed'=(IF woke'=1 /\ callback=2 THEN 1 ELSE closed)
    /\ LET nwidth == IF nestedResult'=1 THEN (IF callback=1 THEN 2 ELSE width) ELSE 0
           nseq == IF callback=1 THEN nestedSequence' ELSE receipt
           first == Append(queue,queued,next,1)
           middle == Append(first,queued+1,nseq,nwidth)
       IN /\ queue'=(IF result'=1 THEN Append(middle,queued+1+nwidth,next,fragments-1)
                     ELSE IF Fault="partial" /\ result'=2 /\ queued<3 THEN Append(queue,queued,next,1)
                     ELSE queue)
          /\ queued'=(IF result'=1 THEN queued+fragments+nwidth
                      ELSE IF Fault="partial" /\ result'=2 /\ queued<3 THEN queued+1 ELSE queued)
    /\ receipt'=(IF result'=1 THEN next ELSE receipt)
    /\ width'=(IF result'=1 THEN fragments ELSE width)
    /\ UNCHANGED <<limit,fragments,callback,connected,expectedQueue>>
\* Follow-up: retry, close-and-retry, drain-and-retry, or drain-and-offer.
Follow(e) ==
    /\ event=2 /\ e\in 3..6 /\ (e=6 \/ receipt>0) /\ event'=e
    /\ beforeNext'=next /\ beforeQueue'=queue /\ beforeQueued'=queued
    /\ closed'=(IF e=4 THEN 1 ELSE closed)
    /\ LET q==IF e\in {5,6} THEN 0 ELSE queue
           n==IF e\in {5,6} THEN 0 ELSE queued
           size==IF e=6 THEN 1 ELSE width
           seq==IF e=6 THEN next ELSE receipt
           normal==IF e=6 THEN OfferResult(next,closed',connected,3-n,size)
                   ELSE RetryResult(closed',connected,3-n,size)
           encoded==IF Fault="payload" /\ e#6 THEN 3 ELSE seq
       IN /\ result'=normal /\ expected'=normal
          /\ queued'=(IF normal=1 THEN n+size ELSE n)
          /\ queue'=(IF normal=1 THEN Append(q,n,encoded,size) ELSE q)
          /\ expectedQueue'=(IF normal=1 THEN Append(q,n,seq,size) ELSE q)
          /\ next'=next+(IF normal=1 /\ (e=6 \/ Fault="retry") THEN 1 ELSE 0)
          /\ receipt'=(IF normal=1 /\ e=6 THEN next ELSE receipt)
          /\ width'=(IF normal=1 /\ e=6 THEN 1 ELSE width)
    /\ UNCHANGED <<limit,fragments,callback,connected,outerSequence,nestedSequence,nestedResult,
                    woke,seenNext,seenCapacity>>
Next == (\E l\in 1..2,pre\in 0..2,p\in 0..3,f\in 1..2,a\in 0..3,c,t\in 0..1: Configure(l,pre,p,f,a,c,t))
        \/ Offer \/ (\E e\in 3..6: Follow(e))
Spec == Init /\ [][Next]_vars
TypeOK == /\ limit\in 0..2 /\ fragments\in 0..2 /\ callback\in 0..3 /\ next\in 1..4
          /\ closed\in 0..1 /\ connected\in 0..1 /\ queue\in 0..255 /\ queued\in 0..4
          /\ receipt\in 0..2 /\ width\in 0..2 /\ event\in 0..6
          /\ result\in 0..4 /\ expected\in 0..4 /\ outerSequence\in 0..2
          /\ nestedSequence\in 0..2 /\ nestedResult\in 0..4 /\ woke\in 0..1
          /\ seenNext\in 0..3 /\ seenCapacity\in 0..3 /\ beforeNext\in 1..3
          /\ beforeQueue\in 0..63 /\ beforeQueued\in 0..3 /\ expectedQueue\in 0..63
ExpectedOperation == result=expected
CommittedBeforeWake == event=2 /\ woke=1 => seenNext=beforeNext+1
ReservationBeforeWake == event=2 /\ woke=1 => seenCapacity=3-beforeQueued-fragments
UniqueIssuance == nestedSequence>0 => nestedSequence#outerSequence
AllocationAccounting == event=2 /\ result=1 => next=beforeNext+1+(IF nestedSequence>0 THEN 1 ELSE 0)
FailureAtomic == event=2 /\ result#1 => queue=beforeQueue /\ queued=beforeQueued
FailureSequence == event=2 /\ result#1 => next=beforeNext
RetrySequence == event\in 3..5 => next=beforeNext
RetryPayload == event\in 3..5 => queue=expectedQueue
Bounds == queued<=3 /\ next<=limit+1
=============================================================================
