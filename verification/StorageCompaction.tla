-------------------- MODULE StorageCompaction --------------------
EXTENDS Naturals, TLC
CONSTANTS Latest, Fault
\* One object, three revisions, one compaction/reopen/resize and one mapped
\* snapshot. Replacement is atomic here; filesystem-stage faults and truncated
\* checkpoints are exercised separately in Rust under the fsync/rename contract.
VARIABLES head, published, r1, r2, r3, bytes, quota, opened, snap, snapValue,
          captured, compactions, restarts, rejected, issued, ackPub, unsafeOpen
vars == <<head,published,r1,r2,r3,bytes,quota,opened,snap,snapValue,captured,
          compactions,restarts,rejected,issued,ackPub,unsafeOpen>>
Has(r) == CASE r=1 -> r1 [] r=2 -> r2 [] r=3 -> r3 [] OTHER -> FALSE
N(b) == IF b THEN 1 ELSE 0
Keep(r) == Has(r) /\ (r=head \/ r=published \/ (~Latest /\ r>published))
Retain(r) == Keep(r) /\ ~(Fault="losePublication" /\ r=published)
                       /\ ~(Fault="loseDraft" /\ r>published /\ r<head)
Init == /\ head=0 /\ published=0 /\ r1=FALSE /\ r2=FALSE /\ r3=FALSE
        /\ bytes=64 /\ quota=464 /\ opened=TRUE /\ snap=0 /\ snapValue=0
        /\ captured=FALSE /\ compactions=0 /\ restarts=0 /\ rejected=FALSE
        /\ issued=0 /\ ackPub=0 /\ unsafeOpen=FALSE
Put == /\ opened /\ head<3 /\ bytes+104<=quota
       /\ head'=head+1 /\ issued'=issued+1
       /\ r1'=(r1 \/ head=0) /\ r2'=(r2 \/ head=1) /\ r3'=(r3 \/ head=2)
       /\ bytes'=bytes+104
       /\ UNCHANGED <<published,quota,opened,snap,snapValue,captured,compactions,restarts,rejected,ackPub,unsafeOpen>>
Publish(r) == /\ opened /\ r>published /\ Has(r) /\ bytes+96<=quota
              /\ published'=r /\ ackPub'=r /\ bytes'=bytes+96
              /\ UNCHANGED <<head,r1,r2,r3,quota,opened,snap,snapValue,captured,compactions,restarts,rejected,issued,unsafeOpen>>
Compact == /\ opened /\ compactions=0
           /\ compactions'=1
           /\ r1'=Retain(1) /\ r2'=Retain(2) /\ r3'=Retain(3)
           /\ head'=(IF Fault="resetHead" THEN N(Retain(1))+N(Retain(2))+N(Retain(3)) ELSE head)
           /\ bytes'=64+104*(N(Retain(1))+N(Retain(2))+N(Retain(3)))+96*N(published>0)
           /\ snapValue'=(IF Fault="overwriteSnapshot" /\ snap>0 THEN head ELSE snapValue)
           /\ UNCHANGED <<published,quota,opened,snap,captured,restarts,rejected,issued,ackPub,unsafeOpen>>
Capture == /\ opened /\ published>0 /\ ~captured
           /\ snap'=published /\ snapValue'=published /\ captured'=TRUE
           /\ UNCHANGED <<head,published,r1,r2,r3,bytes,quota,opened,compactions,restarts,rejected,issued,ackPub,unsafeOpen>>
Drop == /\ snap>0 /\ snap'=0 /\ snapValue'=0
        /\ UNCHANGED <<head,published,r1,r2,r3,bytes,quota,opened,captured,compactions,restarts,rejected,issued,ackPub,unsafeOpen>>
Close == /\ opened /\ opened'=FALSE
         /\ UNCHANGED <<head,published,r1,r2,r3,bytes,quota,snap,snapValue,captured,compactions,restarts,rejected,issued,ackPub,unsafeOpen>>
Reopen == /\ ~opened /\ restarts=0 /\ (snap=0 \/ Fault="loseLock")
          /\ opened'=TRUE /\ restarts'=1 /\ unsafeOpen'=(snap>0)
          /\ published'=(IF Fault="loseCursor" THEN 0 ELSE published)
          /\ UNCHANGED <<head,r1,r2,r3,bytes,quota,snap,snapValue,captured,compactions,rejected,issued,ackPub>>
Grow == /\ opened /\ quota=464 /\ quota'=768
        /\ UNCHANGED <<head,published,r1,r2,r3,bytes,opened,snap,snapValue,captured,compactions,restarts,rejected,issued,ackPub,unsafeOpen>>
Reject == /\ opened /\ head<3 /\ bytes+104>quota /\ ~rejected
          /\ rejected'=TRUE
          /\ bytes'=(IF Fault="writePastQuota" THEN bytes+104 ELSE bytes)
          /\ UNCHANGED <<head,published,r1,r2,r3,quota,opened,snap,snapValue,captured,compactions,restarts,issued,ackPub,unsafeOpen>>
Next == Put \/ (\E r\in 1..3: Publish(r)) \/ Compact \/ Capture \/ Drop \/ Close \/ Reopen \/ Grow \/ Reject
Spec == Init /\ [][Next]_vars
TypeOK == /\ head\in 0..3 /\ published\in 0..3 /\ snap\in 0..3 /\ snapValue\in 0..3
          /\ quota\in {464,768} /\ bytes\in 64..900 /\ issued\in 0..3 /\ ackPub\in 0..3
          /\ compactions\in 0..1 /\ restarts\in 0..1
          /\ \A b\in {r1,r2,r3,opened,captured,rejected,unsafeOpen}: b\in BOOLEAN
HeadPreserved == head=issued /\ (head>0 => Has(head))
PublicationPreserved == published=ackPub /\ (published>0 => Has(published))
DraftsPreserved == ~Latest => (\A r\in 1..head: r>published => Has(r))
SnapshotStable == snap=snapValue
ExclusiveWriter == ~unsafeOpen
QuotaBound == bytes<=quota
=============================================================================
