------------------------ MODULE RpcExternalData ------------------------
EXTENDS Naturals, TLC
CONSTANT Fault
VARIABLES location, source, retained, refs, read, edited, resized, copied,
          rejected, moved, value, address, cap
vars == <<location,source,retained,refs,read,edited,resized,copied,rejected,moved,value,address,cap>>
Bit(b) == IF b THEN 1 ELSE 0
Live == location \in {1,2}
Init == /\ location=0 /\ source=TRUE /\ retained=FALSE /\ refs=1
        /\ read=FALSE /\ edited=FALSE /\ resized=FALSE /\ copied=FALSE
        /\ rejected=FALSE /\ moved=FALSE /\ value=85 /\ address=0 /\ cap=TRUE
Allocate == /\ location=0 /\ source /\ location'=1 /\ retained'=TRUE
            /\ refs'=refs+(IF Fault="earlyRelease" THEN 0 ELSE 1)
            /\ address'=(IF Fault="copyPayload" THEN 2 ELSE 1)
            /\ UNCHANGED <<source,read,edited,resized,copied,rejected,moved,value,cap>>
DropSource == /\ source /\ source'=FALSE /\ refs'=refs-1
              /\ UNCHANGED <<location,retained,read,edited,resized,copied,rejected,moved,value,address,cap>>
Read == /\ Live /\ ~read /\ read'=TRUE
        /\ UNCHANGED <<location,source,retained,refs,edited,resized,copied,rejected,moved,value,address,cap>>
Edit == /\ Live /\ ~edited /\ edited'=TRUE /\ value'=(IF Fault="allowWrite" THEN 170 ELSE value)
        /\ UNCHANGED <<location,source,retained,refs,read,resized,copied,rejected,moved,address,cap>>
Resize == /\ location=1 /\ ~resized /\ resized'=TRUE
          /\ address'=(IF Fault="allowResize" THEN 2 ELSE address)
          /\ UNCHANGED <<location,source,retained,refs,read,edited,copied,rejected,moved,value,cap>>
Copy == /\ Live /\ ~copied /\ copied'=TRUE /\ value'=(IF Fault="aliasCopy" THEN 170 ELSE value)
        /\ UNCHANGED <<location,source,retained,refs,read,edited,resized,rejected,moved,address,cap>>
Reject == /\ location=1 /\ ~rejected /\ rejected'=TRUE
          /\ cap'=(Fault#"releaseAmbient")
          /\ UNCHANGED <<location,source,retained,refs,read,edited,resized,copied,moved,value,address>>
Adopt == /\ location=1 /\ location'=2
         /\ UNCHANGED <<source,retained,refs,read,edited,resized,copied,rejected,moved,value,address,cap>>
Disown == /\ location=2 /\ ~moved /\ location'=1 /\ moved'=TRUE
          /\ UNCHANGED <<source,retained,refs,read,edited,resized,copied,rejected,value,address,cap>>
Drop == /\ Live /\ location'=3
        /\ value'=(IF Fault="zeroExternal" THEN 0 ELSE value)
        /\ UNCHANGED <<source,retained,refs,read,edited,resized,copied,rejected,moved,address,cap>>
Close == /\ location\in {0,3} /\ ~source /\ location'=4 /\ retained'=FALSE /\ cap'=FALSE
         /\ refs'=(IF Fault="leakArena" THEN refs ELSE 0)
         /\ UNCHANGED <<source,read,edited,resized,copied,rejected,moved,value,address>>
Next == Allocate \/ DropSource \/ Read \/ Edit \/ Resize \/ Copy \/ Reject \/ Adopt \/ Disown \/ Drop \/ Close
Spec == Init /\ [][Next]_vars
TypeOK == /\ location\in 0..4 /\ refs\in 0..2 /\ value\in {0,85,170} /\ address\in 0..2
          /\ \A b\in {source,retained,read,edited,resized,copied,rejected,moved,cap}: b\in BOOLEAN
Lifetime == refs=Bit(source)+Bit(retained)
Immutable == value=85
ZeroCopy == location\in {1,2,3} => address=1
Isolation == cap=(location#4)
=============================================================================
