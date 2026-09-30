-------------------------- MODULE StorageCrash --------------------------
EXTENDS Naturals
CONSTANT Bound
VARIABLES head, published, diskHead, diskPublished, phase, kind
vars == <<head, published, diskHead, diskPublished, phase, kind>>
Init == /\ head=0 /\ published=0 /\ diskHead=0 /\ diskPublished=0 /\ phase=0 /\ kind=0
BeginPut == /\ phase=0 /\ head<Bound /\ phase'=1 /\ kind'=1
            /\ UNCHANGED <<head,published,diskHead,diskPublished>>
BeginPublish == /\ phase=0 /\ published<head /\ phase'=1 /\ kind'=2
                /\ UNCHANGED <<head,published,diskHead,diskPublished>>
WriteComplete == /\ phase=1 /\ phase'=2
                 /\ UNCHANGED <<head,published,diskHead,diskPublished,kind>>
Persist == /\ phase=2 /\ phase'=3
           /\ diskHead'=IF kind=1 THEN head+1 ELSE diskHead
           /\ diskPublished'=IF kind=2 THEN head ELSE diskPublished
           /\ UNCHANGED <<head,published,kind>>
Acknowledge == /\ phase=3 /\ phase'=0 /\ kind'=0
               /\ head'=diskHead /\ published'=diskPublished
               /\ UNCHANGED <<diskHead,diskPublished>>
Crash == /\ phase'=0 /\ kind'=0 /\ head'=diskHead /\ published'=diskPublished
         /\ UNCHANGED <<diskHead,diskPublished>>
Next == BeginPut \/ BeginPublish \/ WriteComplete \/ Persist \/ Acknowledge \/ Crash
Spec == Init /\ [][Next]_vars
TypeOK == /\ head \in 0..Bound /\ published \in 0..head
          /\ diskHead \in 0..Bound /\ diskPublished \in 0..diskHead
          /\ phase \in 0..3 /\ kind \in 0..2
AcknowledgedDurable == /\ head<=diskHead /\ published<=diskPublished
IdleConsistent == phase=0 => head=diskHead /\ published=diskPublished /\ kind=0
PublishedNeverRollsBack == [][published' >= published]_vars
=============================================================================
