-------------------------- MODULE RevisionBoundary --------------------------
EXTENDS Naturals
CONSTANT Fault
\* h and p are offsets from u64::MAX-2 in the primary Store object.
\* The second object starts at revision zero. Four operations, one compaction
\* and one reopen; batch writes place the second object first to test atomicity.
VARIABLES h,p,other,steps,reopened,compacted,event,arg,compare,badSecond,
          result,expected,expectedH,expectedP,expectedOther
vars == <<h,p,other,steps,reopened,compacted,event,arg,compare,badSecond,
          result,expected,expectedH,expectedP,expectedOther>>
Init == /\ h=0 /\ p=0 /\ other=0 /\ steps=0 /\ reopened=0 /\ compacted=0
        /\ event=0 /\ arg=0 /\ compare=0 /\ badSecond=0 /\ result=0 /\ expected=0
        /\ expectedH=0 /\ expectedP=0 /\ expectedOther=0
Stage(e) ==
    /\ steps<4 /\ event'=1 /\ arg'=e /\ compare'=0 /\ badSecond'=0
    /\ LET normal == e=h /\ h<2
           allow == (e=h \/ Fault="staleStage") /\ (h<2 \/ Fault="wrap")
       IN /\ h'=(IF allow THEN (h+1)%3 ELSE h)
          /\ result'=(IF allow THEN 1 ELSE 0) /\ expected'=(IF normal THEN 1 ELSE 0)
          /\ expectedH'=(IF normal THEN h+1 ELSE h)
    /\ expectedP'=p /\ expectedOther'=other /\ steps'=steps+1
    /\ UNCHANGED <<p,other,reopened,compacted>>
Publish(r,e) ==
    /\ steps<4 /\ event'=2 /\ arg'=r /\ compare'=e /\ badSecond'=0
    /\ LET normal == e=p /\ r>p /\ r<=h
           allow == (e=p \/ Fault="stalePublish") /\ (r>p \/ Fault="rewind") /\ (r<=h \/ Fault="future")
       IN /\ p'=(IF allow THEN r ELSE p)
          /\ result'=(IF allow THEN 1 ELSE 0) /\ expected'=(IF normal THEN 1 ELSE 0)
          /\ expectedP'=(IF normal THEN r ELSE p)
    /\ expectedH'=h /\ expectedOther'=other /\ steps'=steps+1
    /\ UNCHANGED <<h,other,reopened,compacted>>
Batch(e,bad) ==
    /\ steps<4 /\ event'=3 /\ arg'=e /\ compare'=0 /\ badSecond'=bad
    /\ LET allow == e=h /\ h<2 /\ bad=0
       IN /\ h'=(IF allow THEN h+1 ELSE h)
          /\ p'=(IF allow THEN h+1 ELSE p)
          /\ other'=(IF allow \/ Fault="partial" THEN other+1 ELSE other)
          /\ result'=(IF allow THEN 1 ELSE 0) /\ expected'=result'
          /\ expectedH'=h' /\ expectedP'=p'
          /\ expectedOther'=(IF allow THEN other+1 ELSE other)
    /\ steps'=steps+1 /\ UNCHANGED <<reopened,compacted>>
Maintain(reopen) ==
    /\ steps<4 /\ (IF reopen THEN reopened=0 ELSE compacted=0)
    /\ event'=(IF reopen THEN 4 ELSE 5) /\ arg'=0 /\ compare'=0 /\ badSecond'=0
    /\ reopened'=(IF reopen THEN 1 ELSE reopened)
    /\ compacted'=(IF reopen THEN compacted ELSE 1)
    /\ h'=(IF Fault="reuse" THEN 0 ELSE h)
    /\ expectedH'=h /\ expectedP'=p /\ expectedOther'=other
    /\ result'=1 /\ expected'=1 /\ steps'=steps+1
    /\ UNCHANGED <<p,other>>
Next == (\E e\in 0..2: Stage(e)) \/ (\E r,e\in 0..2: Publish(r,e))
        \/ (\E e\in 0..2,b\in 0..1: Batch(e,b)) \/ Maintain(TRUE) \/ Maintain(FALSE)
Spec == Init /\ [][Next]_vars
TypeOK == /\ h\in 0..2 /\ p\in 0..2 /\ other\in 0..4 /\ steps\in 0..4
          /\ reopened\in 0..1 /\ compacted\in 0..1 /\ event\in 0..5
          /\ arg\in 0..2 /\ compare\in 0..2 /\ badSecond\in 0..1
          /\ result\in 0..1 /\ expected\in 0..1
          /\ expectedH\in 0..2 /\ expectedP\in 0..2 /\ expectedOther\in 0..4
ExpectedOutcome == result=expected
ExactCounters == h=expectedH /\ p=expectedP /\ other=expectedOther
PublicationBounds == p<=h
=============================================================================
