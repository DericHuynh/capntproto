-------------------------- MODULE RpcDeferredHandoff --------------------------
EXTENDS Naturals, TLC
CONSTANTS Reflection, LocalFinal, RejectAccept, RejectForward, EagerAccept, LoseVine, ReuseEmbargo
VARIABLES b,c,d,r,ub,uc,ud,ur,f1,f2,accepts,calls,provision,localAccepts
vars == <<b,c,d,r,ub,uc,ud,ur,f1,f2,accepts,calls,provision,localAccepts>>
LiveVine == (b /\ ~ub) \/ (~RejectForward /\ ((c /\ ~uc) \/ (d /\ ~ud) \/ (r /\ ~ur)))
N(x) == IF x THEN 1 ELSE 0
Used == N(ub)+(IF RejectForward THEN 0 ELSE N(uc)+N(ud)+N(ur))
Init == /\ b=TRUE /\ c=FALSE /\ d=FALSE /\ r=FALSE
        /\ ub=FALSE /\ uc=FALSE /\ ud=FALSE /\ ur=FALSE
        /\ f1=FALSE /\ f2=FALSE /\ accepts=(IF EagerAccept THEN 1 ELSE 0)
        /\ calls=0 /\ provision=TRUE /\ localAccepts=0
ForwardB == /\ b /\ ~ub /\ ~f1 /\ f1'=TRUE /\ c'=TRUE /\ r'=Reflection
            /\ UNCHANGED <<b,d,ub,uc,ud,ur,f2,accepts,calls>>
ForwardNext == /\ ~RejectForward /\ f1 /\ ~f2 /\ (IF Reflection THEN r /\ ~ur ELSE c /\ ~uc)
               /\ f2'=TRUE /\ d'=TRUE
               /\ UNCHANGED <<b,c,r,ub,uc,ud,ur,f1,accepts,calls>>
UseB == /\ b /\ ~ub /\ ub'=TRUE /\ UNCHANGED <<b,c,d,r,uc,ud,ur,f1,f2>>
UseC == /\ c /\ ~uc /\ uc'=TRUE /\ UNCHANGED <<b,c,d,r,ub,ud,ur,f1,f2>>
UseD == /\ d /\ ~ud /\ ud'=TRUE /\ UNCHANGED <<b,c,d,r,ub,uc,ur,f1,f2>>
UseR == /\ r /\ ~ur /\ ur'=TRUE /\ UNCHANGED <<b,c,d,r,ub,uc,ud,f1,f2>>
Use == /\ (UseB \/ UseC \/ UseD \/ UseR)
       /\ accepts'=Used'
       /\ calls'=(IF RejectAccept THEN 0 ELSE IF ReuseEmbargo /\ accepts>0 THEN calls ELSE Used')
DropB == /\ b /\ b'=FALSE /\ UNCHANGED <<c,d,r>>
DropC == /\ c /\ c'=FALSE /\ UNCHANGED <<b,d,r>>
DropD == /\ d /\ d'=FALSE /\ UNCHANGED <<b,c,r>>
DropR == /\ r /\ r'=FALSE /\ UNCHANGED <<b,c,d>>
Drop == /\ (DropB \/ DropC \/ DropD \/ DropR)
        /\ UNCHANGED <<ub,uc,ud,ur,f1,f2,accepts,calls>>
Next == /\ (ForwardB \/ ForwardNext \/ Use \/ Drop)
        /\ localAccepts'=(IF LocalFinal THEN N(ud') ELSE 0)
        /\ provision'=(IF LoseVine THEN b' /\ ~ub' ELSE LiveVine')
Spec == Init /\ [][Next]_vars
TypeOK == /\ \A x\in {b,c,d,r,ub,uc,ud,ur,f1,f2,provision}: x\in BOOLEAN
          /\ accepts\in 0..4 /\ calls\in 0..4 /\ localAccepts\in 0..1
DeferredUntilUsed == accepts=Used
IndependentAccepts == calls=(IF RejectAccept THEN 0 ELSE Used)
LocalRendezvous == localAccepts=(IF LocalFinal THEN N(ud) ELSE 0)
VineLifetime == provision=LiveVine
=============================================================================
