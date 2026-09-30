------------------------- MODULE PublicationCursor -------------------------
EXTENDS Naturals
CONSTANT Fault
\* Two objects contain revisions 1..3; revision 2 is always a draft.
\* Object 0 starts published at 1 and can publish 3. Object 1 is already at 3.
\* One current Store, another foreign Store, one compaction, one reopen, and one
\* retained cursor. Successful I/O only. Numeric imports are explicit operations.
VARIABLES latest, floor, compact, epoch, held, owner, object, after, checkpoint,
          event, key, requested, target, result, value, resultKey, nextAfter
vars == <<latest,floor,compact,epoch,held,owner,object,after,checkpoint,
          event,key,requested,target,result,value,resultKey,nextAfter>>
P(k) == IF k=0 THEN latest ELSE 3
F(k) == IF k=0 THEN floor ELSE IF compact=1 THEN 3 ELSE 0
Validate(k,a) == IF a<F(k) THEN 5
                 ELSE IF a>P(k) \/ a=2 \/ a=4 THEN 4 ELSE 1
First(k,a) == IF a=0 THEN 1 ELSE IF a<P(k) THEN 3 ELSE 0
Expected == IF target=1 \/ owner#epoch THEN 6
            ELSE IF Validate(object,checkpoint)#1 THEN Validate(object,checkpoint)
            ELSE IF First(object,checkpoint)=0 THEN 3 ELSE 2
Clear == /\ key'=0 /\ requested'=0 /\ target'=0 /\ result'=0
         /\ value'=0 /\ resultKey'=0 /\ nextAfter'=0
Init == /\ latest=1 /\ floor=0 /\ compact=0 /\ epoch=0 /\ held=0
        /\ owner=0 /\ object=0 /\ after=0 /\ checkpoint=0 /\ event=0
        /\ key=0 /\ requested=0 /\ target=0 /\ result=0
        /\ value=0 /\ resultKey=0 /\ nextAfter=0
Import(k,a) ==
    /\ held=0 /\ event'=1 /\ key'=k /\ requested'=a
    /\ LET valid == IF Fault="draft" /\ a=2 /\ a>=F(k) /\ a<=P(k)
                    THEN 1 ELSE Validate(k,a)
       IN /\ result'=valid
          /\ held'=(IF valid=1 THEN 1 ELSE 0)
          /\ owner'=(IF valid=1 THEN epoch ELSE 0)
          /\ object'=(IF valid=1 THEN k ELSE 0)
          /\ after'=(IF valid=1 THEN a ELSE 0)
          /\ checkpoint'=after'
    /\ target'=0 /\ value'=0 /\ resultKey'=0 /\ nextAfter'=0
    /\ UNCHANGED <<latest,floor,compact,epoch>>
Read(t) ==
    /\ held=1 /\ event'=2 /\ target'=t /\ key'=0 /\ requested'=0
    /\ LET foreign == t=1 \/ owner#epoch
           valid == IF Fault="floor" /\ after<F(object) THEN 1 ELSE Validate(object,after)
           first == First(object,after)
           outcome == IF foreign /\ Fault#"owner" THEN 6 ELSE
                      IF valid#1 THEN valid ELSE IF first=0 THEN 3 ELSE 2
       IN /\ result'=outcome
          /\ value'=(IF outcome=2 THEN IF Fault="skip" THEN 3 ELSE first ELSE 0)
          /\ resultKey'=(IF outcome=2 THEN IF Fault="object" THEN 1-object ELSE object ELSE 0)
          /\ nextAfter'=(IF outcome=2 /\ Fault#"next" THEN value' ELSE 0)
          /\ after'=(IF outcome=2 /\ Fault="advance" THEN first ELSE after)
    /\ UNCHANGED <<latest,floor,compact,epoch,held,owner,object,checkpoint>>
Advance == /\ event=2 /\ result=2 /\ event'=3 /\ after'=nextAfter
           /\ checkpoint'=nextAfter /\ Clear
           /\ UNCHANGED <<latest,floor,compact,epoch,held,owner,object>>
Drop == /\ held=1 /\ held'=0 /\ event'=4
        /\ owner'=0 /\ object'=0 /\ after'=0 /\ checkpoint'=0 /\ Clear
        /\ UNCHANGED <<latest,floor,compact,epoch>>
Publish == /\ latest=1 /\ latest'=3 /\ event'=5 /\ Clear
           /\ UNCHANGED <<floor,compact,epoch,held,owner,object,after,checkpoint>>
Compact == /\ compact=0 /\ compact'=1 /\ floor'=latest /\ event'=6 /\ Clear
           /\ UNCHANGED <<latest,epoch,held,owner,object,after,checkpoint>>
Reopen == /\ epoch=0 /\ epoch'=1 /\ event'=7 /\ Clear
          /\ UNCHANGED <<latest,floor,compact,held,owner,object,after,checkpoint>>
Idle == /\ event'=8 /\ Clear
        /\ UNCHANGED <<latest,floor,compact,epoch,held,owner,object,after,checkpoint>>
Next == (\E k\in 0..1, a\in 0..4: Import(k,a)) \/ (\E t\in 0..1: Read(t))
        \/ Advance \/ Drop \/ Publish \/ Compact \/ Reopen \/ Idle
Spec == Init /\ [][Next]_vars
TypeOK == /\ latest\in {1,3} /\ floor\in {0,1,3} /\ compact\in 0..1
          /\ epoch\in 0..1 /\ held\in 0..1 /\ owner\in 0..1 /\ object\in 0..1
          /\ after\in 0..4 /\ checkpoint\in 0..4 /\ event\in 0..8
          /\ key\in 0..1 /\ requested\in 0..4 /\ target\in 0..1
          /\ result\in 0..6 /\ value\in 0..3 /\ resultKey\in 0..1 /\ nextAfter\in 0..3
ImportValidation == event=1 => result=Validate(key,requested)
OwnerSeparation == event=2 /\ (target=1 \/ owner#epoch) => result=6
ReadValidation == event=2 => result=Expected
CursorImmutable == held=1 => after=checkpoint
EventBinding == event=2 /\ result=2 => resultKey=object /\ value=First(object,checkpoint)
NextPosition == event=2 /\ result=2 => nextAfter=value
=============================================================================
