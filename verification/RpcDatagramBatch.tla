------------------------- MODULE RpcDatagramBatch -------------------------
EXTENDS Naturals, TLC
CONSTANT Bug
VARIABLES queued, next, receipt, closed, beforeQueue, beforeNext, result, event
vars == <<queued,next,receipt,closed,beforeQueue,beforeNext,result,event>>
\* Two free queue slots, two fragments per offer, and two sequence numbers.
\* Queue pressure and draining can interleave with a whole-batch reservation.
Init == /\ queued=0 /\ next=1 /\ receipt=0 /\ closed=FALSE
        /\ beforeQueue=0 /\ beforeNext=1 /\ result=0 /\ event=0
Send(retry,e) ==
    LET valid == retry \/ next<=2
        accepted == valid /\ queued=0 /\ (~closed \/ Bug="closed")
    IN /\ beforeQueue'=queued /\ beforeNext'=next /\ event'=e
       /\ result'=IF accepted THEN 1 ELSE IF ~closed /\ valid THEN 2 ELSE 3
       /\ queued'=IF accepted THEN 2 ELSE
                     IF Bug="partial" /\ ~closed /\ valid /\ queued=1 THEN 2 ELSE queued
       /\ next'=IF accepted /\ ~retry THEN next+1 ELSE
                   IF Bug="burn" /\ ~retry /\ ~accepted /\ next<=2 THEN next+1 ELSE
                   IF Bug="retry" /\ retry /\ accepted /\ next<=2 THEN next+1 ELSE next
       /\ receipt'=IF accepted /\ ~retry THEN next ELSE receipt
       /\ UNCHANGED closed
Offer == Send(FALSE,2)
Retry == /\ receipt>0 /\ Send(TRUE,3)
Pressure == /\ ~closed /\ queued<2 /\ queued'=queued+1 /\ event'=1 /\ result'=0
            /\ UNCHANGED <<next,receipt,closed,beforeQueue,beforeNext>>
Drain == /\ queued>0 /\ queued'=queued-1 /\ event'=4 /\ result'=0
         /\ UNCHANGED <<next,receipt,closed,beforeQueue,beforeNext>>
Close == /\ ~closed /\ closed'=TRUE /\ event'=5 /\ result'=0
         /\ UNCHANGED <<queued,next,receipt,beforeQueue,beforeNext>>
Next == Pressure \/ Offer \/ Retry \/ Drain \/ Close
Spec == Init /\ [][Next]_vars
TypeOK == /\ queued\in 0..2 /\ next\in 1..3 /\ receipt\in 0..2 /\ closed\in BOOLEAN
          /\ beforeQueue\in 0..2 /\ beforeNext\in 1..3 /\ result\in 0..3 /\ event\in 0..5
FailureAtomic == result\in {2,3} => queued=beforeQueue
FailureSequence == result\in {2,3} => next=beforeNext
RetrySequence == event=3 => next=beforeNext
ClosedAdmission == closed => result#1
=============================================================================
