------------------------ MODULE RpcArenaResize ------------------------
EXTENDS Naturals, TLC
CONSTANT Fault
VARIABLES length, tail, shrunk, grown, spacer, allocated, added, reclaimed,
          prefix, fresh, moved, address, kept
vars == <<length,tail,shrunk,grown,spacer,allocated,added,reclaimed,prefix,fresh,moved,address,kept>>
Bit(b) == IF b THEN 1 ELSE 0
Words(n) == CASE n=8 -> 1 [] n=24 -> 3 [] OTHER -> 5
Init == /\ length=24 /\ tail=TRUE /\ shrunk=FALSE /\ grown=FALSE /\ spacer=FALSE
        /\ allocated=4 /\ added=0 /\ reclaimed=0 /\ prefix=85 /\ fresh=TRUE
        /\ moved=FALSE /\ address=1 /\ kept=24
Shrink == /\ ~shrunk /\ shrunk'=TRUE /\ length'=8 /\ kept'=8
          /\ reclaimed'=reclaimed+(IF tail THEN Words(length)-1 ELSE 0)
          /\ allocated'=allocated-(IF tail /\ Fault#"skipReclaim" THEN Words(length)-1 ELSE 0)
          /\ UNCHANGED <<tail,grown,spacer,added,prefix,fresh,moved,address>>
Grow == /\ ~grown /\ grown'=TRUE /\ length'=40 /\ tail'=TRUE
        /\ added'=added+(IF tail THEN 5-Words(length) ELSE 6)
        /\ allocated'=allocated+(IF tail /\ Fault#"skipInPlace" THEN 5-Words(length) ELSE 6)
        /\ moved'=~tail /\ address'=(IF tail THEN address ELSE 2)
        /\ fresh'=(Fault#"unclearedGrowth")
        /\ UNCHANGED <<shrunk,spacer,reclaimed,prefix,kept>>
Spacer == /\ ~spacer /\ spacer'=TRUE /\ tail'=FALSE /\ allocated'=allocated+2
          /\ prefix'=(IF shrunk /\ Fault="overlapReuse" THEN 170 ELSE prefix)
          /\ fresh'=(Fault#"unclearedReuse")
          /\ UNCHANGED <<length,shrunk,grown,added,reclaimed,moved,address,kept>>
Next == Shrink \/ Grow \/ Spacer
Spec == Init /\ [][Next]_vars
TypeOK == /\ length\in {8,24,40} /\ kept\in {8,24} /\ allocated\in 0..12
          /\ added\in 0..6 /\ reclaimed\in 0..4 /\ prefix\in {85,170} /\ address\in 1..2
          /\ \A b\in {tail,shrunk,grown,spacer,fresh,moved}:b\in BOOLEAN
Accounting == allocated=4+2*Bit(spacer)+added-reclaimed
Preserved == prefix=85 /\ address=1+Bit(moved)
Initialized == fresh
=============================================================================
