---------------------- MODULE RpcIndependentPipeline ----------------------
EXTENDS Naturals
CONSTANT Fault
\* Local pipeline publication: one retained parent, one promised capability,
\* at most two children, and one optional whenResolved observer. Progress
\* abstracts draining the executor between actions; this is not a scheduler
\* or liveness proof. RPC, cancellation, membranes and nested paths have native tests.
\* offered/returned/observed: 0=pending, 1=success, 2=error.
\* Events: 1=publish, 2/3=resolve/reject target, 4/5=return success/error,
\*         6=child call, 7=duplicate publication, 8=observe capability resolution.
VARIABLES published,offered,returned,sent,done,failed,duplicate,watching,observed,event
vars == <<published,offered,returned,sent,done,failed,duplicate,watching,observed,event>>
Init == /\ published=0 /\ offered=0 /\ returned=0 /\ sent=0
        /\ done=0 /\ failed=0 /\ duplicate=0 /\ watching=0 /\ observed=0 /\ event=0
Route(p,o,r) == IF p=1 \/ r=1 THEN o ELSE IF r=2 THEN 2 ELSE 0
ActualRoute(p,o,r) == IF Fault="waitForReturn" /\ r=0 THEN 0
                     ELSE IF Fault="revokeOnError" /\ r=2 THEN 2
                     ELSE Route(p,o,r)
Resolution(p,o,r,w) == IF w=1 THEN Route(p,o,r) ELSE 0
Progress(p,o,r,s,w) == /\ published'=p /\ offered'=o /\ returned'=r /\ sent'=s
                    /\ done'=(IF ActualRoute(p,o,r)=1 THEN s ELSE 0)
                    /\ failed'=(IF ActualRoute(p,o,r)=2 THEN s ELSE 0)
                    /\ duplicate'=0
                    /\ watching'=w
                    /\ observed'=(IF Fault="delayObserver" /\ r=0 THEN 0
                                    ELSE Resolution(p,o,r,w))
Publish == /\ published=0 /\ returned=0 /\ event'=1
           /\ Progress(1,offered,IF Fault="completeOnPublish" THEN 1 ELSE 0,sent,watching)
Offer(o) == /\ offered=0 /\ event'=o+1 /\ Progress(published,o,returned,sent,watching)
Return(r) == /\ returned=0 /\ event'=r+3 /\ Progress(published,offered,r,sent,watching)
Call == /\ sent<2 /\ event'=6 /\ Progress(published,offered,returned,sent+1,watching)
Duplicate == /\ published=1 /\ returned=0 /\ event'=7
             /\ duplicate'=IF Fault="replacePublished" THEN 1 ELSE 0
             /\ UNCHANGED <<published,offered,returned,sent,done,failed,watching,observed>>
Watch == /\ watching=0 /\ event'=8 /\ Progress(published,offered,returned,sent,1)
Next == Publish \/ Offer(1) \/ Offer(2) \/ Return(1) \/ Return(2) \/ Call \/ Duplicate \/ Watch
Spec == Init /\ [][Next]_vars
TypeOK == /\ published\in 0..1 /\ offered\in 0..2 /\ returned\in 0..2
          /\ sent\in 0..2 /\ done\in 0..2 /\ failed\in 0..2
          /\ duplicate\in 0..1 /\ watching\in 0..1 /\ observed\in 0..2 /\ event\in 0..8
PublicationIsIndependent == event=1 => returned=0
PublicationIsSingleUse == duplicate=0
CorrectRouting == /\ done=(IF Route(published,offered,returned)=1 THEN sent ELSE 0)
                  /\ failed=(IF Route(published,offered,returned)=2 THEN sent ELSE 0)
CorrectResolution == observed=Resolution(published,offered,returned,watching)
=============================================================================
