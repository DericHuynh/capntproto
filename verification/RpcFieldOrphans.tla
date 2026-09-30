------------------------- MODULE RpcFieldOrphans -------------------------
EXTENDS Naturals, TLC
CONSTANTS IgnoreArena, LoseOnError, LoseHeld
VARIABLES src, dst, other, orphan, held, taken, wrong, moved, released, read, alive1, alive2, alive3
vars == <<src,dst,other,orphan,held,taken,wrong,moved,released,read,alive1,alive2,alive3>>
Alive1 == src=1 \/ dst=1 \/ orphan=1 \/ held=1
Init == /\ src=1 /\ dst=2 /\ other=3 /\ orphan=0 /\ held=0
        /\ taken=FALSE /\ wrong=FALSE /\ moved=FALSE /\ released=FALSE /\ read=FALSE
        /\ alive1=TRUE /\ alive2=TRUE /\ alive3=TRUE
Take == /\ src=1 /\ ~taken /\ src'=0 /\ orphan'=1 /\ taken'=TRUE
        /\ UNCHANGED <<dst,other,held,wrong,moved,released,read>>
Wrong == /\ orphan=1 /\ ~wrong /\ wrong'=TRUE
         /\ orphan'=IF LoseOnError \/ IgnoreArena THEN 0 ELSE orphan
         /\ other'=IF IgnoreArena THEN 1 ELSE other
         /\ UNCHANGED <<src,dst,held,taken,moved,released,read>>
Adopt == /\ orphan=1 /\ orphan'=0 /\ dst'=1 /\ moved'=TRUE
         /\ UNCHANGED <<src,other,held,taken,wrong,released,read>>
DropOrphan == /\ orphan=1 /\ orphan'=0 /\ released'=TRUE
              /\ UNCHANGED <<src,dst,other,held,taken,wrong,moved,read>>
Read == /\ ~read /\ (src=1 \/ dst=1) /\ held'=1 /\ read'=TRUE
        /\ UNCHANGED <<src,dst,other,orphan,taken,wrong,moved,released>>
DropHeld == /\ held=1 /\ held'=0
            /\ UNCHANGED <<src,dst,other,orphan,taken,wrong,moved,released,read>>
ClearSource == /\ src=1 /\ src'=0
               /\ UNCHANGED <<dst,other,orphan,held,taken,wrong,moved,released,read>>
ClearDest == /\ dst#0 /\ dst'=0
             /\ UNCHANGED <<src,other,orphan,held,taken,wrong,moved,released,read>>
Next == /\ (Take \/ Wrong \/ Adopt \/ DropOrphan \/ Read \/ DropHeld \/ ClearSource \/ ClearDest)
        /\ alive1'=IF LoseHeld /\ orphan=1 /\ released' THEN FALSE ELSE Alive1'
        /\ alive2'=(dst'=2) /\ alive3'=(other'=3)
Spec == Init /\ [][Next]_vars
TypeOK == /\ src\in 0..1 /\ dst\in 0..2 /\ other\in {1,3} /\ orphan\in 0..1 /\ held\in 0..1
          /\ \A x\in {taken,wrong,moved,released,read,alive1,alive2,alive3}:x\in BOOLEAN
ArenaIsolation == other=3
FailedAdoption == (wrong /\ ~moved /\ ~released) => orphan=1
Ownership == /\ alive1=Alive1 /\ alive2=(dst=2) /\ alive3=(other=3)
UniquePointerOwner == (IF src=1 THEN 1 ELSE 0)+(IF dst=1 THEN 1 ELSE 0)+orphan <= 1
=============================================================================
