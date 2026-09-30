--------------------------- MODULE RpcShortenLookup ---------------------------
EXTENDS Naturals, TLC
CONSTANTS Reject, WaitResolution, FollowLaterRoute
VARIABLES stream,shortened,lookup,old
vars == <<stream,shortened,lookup,old>>
Init == /\ stream=TRUE /\ shortened=0 /\ lookup=0 /\ old=FALSE
Result(s,atOld) == IF (FollowLaterRoute /\ s=1) \/ ~atOld THEN 3 ELSE 2
Lookup == /\ lookup=0 /\ old'=(shortened#1)
          /\ lookup'=IF stream \/ (WaitResolution /\ shortened=0) THEN 1
                       ELSE Result(shortened,old')
          /\ UNCHANGED <<stream,shortened>>
Shorten == /\ shortened=0 /\ shortened'=IF Reject THEN 2 ELSE 1
           /\ lookup'=IF lookup=1 /\ ~stream THEN Result(shortened',old) ELSE lookup
           /\ UNCHANGED <<stream,old>>
Complete == /\ stream /\ stream'=FALSE
            /\ lookup'=IF lookup=1 /\ ~(WaitResolution /\ shortened=0)
                         THEN Result(shortened,old) ELSE lookup
            /\ UNCHANGED <<shortened,old>>
DropLookup == /\ lookup\in {1,2,3} /\ lookup'=4
              /\ UNCHANGED <<stream,shortened,old>>
Next == Lookup \/ Shorten \/ Complete \/ DropLookup
Spec == Init /\ [][Next]_vars
TypeOK == /\ stream\in BOOLEAN /\ shortened\in 0..2 /\ lookup\in 0..4 /\ old\in BOOLEAN
LocalAvailable == (lookup=1 /\ old) => stream
StableLookup == lookup=3 => ~old
StreamBarrier == lookup\in {2,3} => ~stream
=============================================================================
