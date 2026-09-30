-------------------------- MODULE StorageObjectKeys --------------------------
EXTENDS Naturals
CONSTANT Fault
\* Two keys (zero and one), at most two writes total, one held snapshot,
\* one compaction and one reopen. Reads use published revisions. Snapshot
\* source coordinates are captured expectations, distinct from its observations.
VARIABLES h0,h1,p0,p1,w0,w1,snap,sourceKey,sourceRevision,object,revision,value,
          compact,reopened,event,key
vars == <<h0,h1,p0,p1,w0,w1,snap,sourceKey,sourceRevision,object,revision,value,
          compact,reopened,event,key>>
H(k) == IF k=0 THEN h0 ELSE h1
P(k) == IF k=0 THEN p0 ELSE p1
Init == /\ h0=0 /\ h1=0 /\ p0=0 /\ p1=0 /\ w0=0 /\ w1=0 /\ snap=0
        /\ sourceKey=0 /\ sourceRevision=0 /\ object=0 /\ revision=0 /\ value=0
        /\ compact=0 /\ reopened=0 /\ event=0 /\ key=0
Write(k) == /\ w0+w1<2 /\ event'=1 /\ key'=k
            /\ w0'=w0+(IF k=0 THEN 1 ELSE 0) /\ w1'=w1+(IF k=1 THEN 1 ELSE 0)
            /\ LET selected == IF Fault="crossKey" THEN 1-k ELSE k
               IN /\ h0'=h0+(IF selected=0 THEN 1 ELSE 0)
                  /\ h1'=h1+(IF selected=1 THEN 1 ELSE 0)
            /\ revision'=(IF Fault="relabel" /\ snap=1 /\ sourceKey=k THEN H(k)+1 ELSE revision)
            /\ UNCHANGED <<p0,p1,snap,sourceKey,sourceRevision,object,value,compact,reopened>>
Publish(k) == /\ P(k)<H(k) /\ event'=2 /\ key'=k
              /\ p0'=(IF k=0 THEN h0 ELSE p0) /\ p1'=(IF k=1 THEN h1 ELSE p1)
              /\ UNCHANGED <<h0,h1,w0,w1,snap,sourceKey,sourceRevision,object,revision,value,compact,reopened>>
Capture(k) == /\ snap=0 /\ P(k)>0 /\ snap'=1 /\ event'=3 /\ key'=k
              /\ sourceKey'=k /\ sourceRevision'=P(k)
              /\ object'=(IF Fault="label" THEN 1-k ELSE k) /\ revision'=P(k)
              /\ value'=10*(IF Fault="payload" THEN 1-k ELSE k)+P(k)
              /\ UNCHANGED <<h0,h1,p0,p1,w0,w1,compact,reopened>>
Drop == /\ snap=1 /\ snap'=0 /\ event'=4
        /\ sourceKey'=0 /\ sourceRevision'=0 /\ object'=0 /\ revision'=0 /\ value'=0
        /\ UNCHANGED <<h0,h1,p0,p1,w0,w1,compact,reopened,key>>
Compact == /\ compact=0 /\ compact'=1 /\ event'=5
           /\ value'=(IF Fault="mapping" /\ snap=1 THEN 0 ELSE value)
           /\ UNCHANGED <<h0,h1,p0,p1,w0,w1,snap,sourceKey,sourceRevision,object,revision,reopened,key>>
Reopen == /\ reopened=0 /\ snap=0 /\ reopened'=1 /\ event'=6
          /\ h0'=(IF Fault="recovery" THEN h1 ELSE h0) /\ h1'=(IF Fault="recovery" THEN h0 ELSE h1)
          /\ p0'=(IF Fault="recovery" THEN p1 ELSE p0) /\ p1'=(IF Fault="recovery" THEN p0 ELSE p1)
          /\ UNCHANGED <<w0,w1,snap,sourceKey,sourceRevision,object,revision,value,compact,key>>
Idle == /\ event'=7
        /\ UNCHANGED <<h0,h1,p0,p1,w0,w1,snap,sourceKey,sourceRevision,object,revision,value,compact,reopened,key>>
Next == (\E k\in 0..1: Write(k) \/ Publish(k) \/ Capture(k)) \/ Drop \/ Compact \/ Reopen \/ Idle
Spec == Init /\ [][Next]_vars
TypeOK == /\ h0\in 0..2 /\ h1\in 0..2 /\ p0\in 0..2 /\ p1\in 0..2 /\ w0\in 0..2 /\ w1\in 0..2
          /\ snap\in 0..1 /\ sourceKey\in 0..1 /\ sourceRevision\in 0..2
          /\ object\in 0..1 /\ revision\in 0..2 /\ value\in 0..12
          /\ compact\in 0..1 /\ reopened\in 0..1 /\ event\in 0..7 /\ key\in 0..1
ExactHeads == h0=w0 /\ h1=w1
PublicationBounds == p0<=h0 /\ p1<=h1
SnapshotIdentity == snap=1 => object=sourceKey /\ revision=sourceRevision
SnapshotData == snap=1 => value=10*sourceKey+sourceRevision
=============================================================================
