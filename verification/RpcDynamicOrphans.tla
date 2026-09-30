----------------------- MODULE RpcDynamicOrphans -----------------------
EXTENDS Naturals, TLC
\* Edge-prefix histories run against both compiled and runtime-loaded schemas.
\* Schema registry/brand mismatches refine type rejection; concrete metadata
\* and lifetimes are checked by Rust regressions and compiler acceptance cases.
CONSTANT Fault
VARIABLES src, dst, foreign, orphan, held, taken, arenaError, typeError,
          contextError, moved, released, read, tag, alive1, alive2, alive3
vars == <<src,dst,foreign,orphan,held,taken,arenaError,typeError,contextError,
          moved,released,read,tag,alive1,alive2,alive3>>
Owned == src=1 \/ dst=1 \/ orphan=1 \/ held=1
Init == /\ src=1 /\ dst=2 /\ foreign=3 /\ orphan=0 /\ held=0
        /\ taken=FALSE /\ arenaError=FALSE /\ typeError=FALSE /\ contextError=FALSE
        /\ moved=FALSE /\ released=FALSE /\ read=FALSE /\ tag=1
        /\ alive1=TRUE /\ alive2=TRUE /\ alive3=TRUE
Take == /\ src=1 /\ ~taken /\ taken'=TRUE /\ orphan'=1
        /\ src'=(IF Fault="duplicateOwner" THEN 1 ELSE 0)
        /\ UNCHANGED <<dst,foreign,held,arenaError,typeError,contextError,moved,released,read,tag>>
WrongArena == /\ orphan=1 /\ ~arenaError /\ arenaError'=TRUE
              /\ foreign'=(IF Fault="ignoreArena" THEN 1 ELSE foreign)
              /\ orphan'=(IF Fault\in {"ignoreArena","loseOnError"} THEN 0 ELSE orphan)
              /\ UNCHANGED <<src,dst,held,taken,typeError,contextError,moved,released,read,tag>>
WrongType == /\ orphan=1 /\ ~typeError /\ typeError'=TRUE
             /\ tag'=(IF Fault="ignoreType" THEN 0 ELSE tag)
             /\ UNCHANGED <<src,dst,foreign,orphan,held,taken,arenaError,contextError,moved,released,read>>
WrongContext == /\ orphan=1 /\ ~contextError /\ contextError'=TRUE
                /\ orphan'=(IF Fault="ignoreContext" THEN 0 ELSE orphan)
                /\ dst'=(IF Fault="ignoreContext" THEN 1 ELSE dst)
                /\ UNCHANGED <<src,foreign,held,taken,arenaError,typeError,moved,released,read,tag>>
Adopt == /\ orphan=1 /\ orphan'=0 /\ dst'=1 /\ moved'=TRUE
         /\ UNCHANGED <<src,foreign,held,taken,arenaError,typeError,contextError,released,read,tag>>
DropOrphan == /\ orphan=1 /\ orphan'=0 /\ released'=TRUE
              /\ UNCHANGED <<src,dst,foreign,held,taken,arenaError,typeError,contextError,moved,read,tag>>
Read == /\ ~read /\ (src=1 \/ dst=1) /\ held'=1 /\ read'=TRUE
        /\ UNCHANGED <<src,dst,foreign,orphan,taken,arenaError,typeError,contextError,moved,released,tag>>
DropHeld == /\ held=1 /\ held'=0
            /\ UNCHANGED <<src,dst,foreign,orphan,taken,arenaError,typeError,contextError,moved,released,read,tag>>
ClearSource == /\ src=1 /\ src'=0
               /\ UNCHANGED <<dst,foreign,orphan,held,taken,arenaError,typeError,contextError,moved,released,read,tag>>
ClearDest == /\ dst#0 /\ dst'=0
             /\ UNCHANGED <<src,foreign,orphan,held,taken,arenaError,typeError,contextError,moved,released,read,tag>>
Next == /\ (Take \/ WrongArena \/ WrongType \/ WrongContext \/ Adopt \/ DropOrphan \/ Read \/ DropHeld \/ ClearSource \/ ClearDest)
        /\ alive1'=(IF Fault="loseHeld" /\ orphan=1 /\ released' THEN FALSE ELSE Owned')
        /\ alive2'=(IF Fault="leakReplacement" THEN alive2 ELSE dst'=2)
        /\ alive3'=(foreign'=3)
Spec == Init /\ [][Next]_vars
TypeOK == /\ src\in 0..1 /\ dst\in 0..2 /\ foreign\in {1,3} /\ orphan\in 0..1 /\ held\in 0..1 /\ tag\in 0..1
          /\ \A x\in {taken,arenaError,typeError,contextError,moved,released,read,alive1,alive2,alive3}:x\in BOOLEAN
ArenaIsolation == foreign=3
ErrorPreservesOwnership == ((arenaError \/ contextError) /\ ~moved /\ ~released) => orphan=1
UnionUnchanged == tag=1
Ownership == /\ alive1=Owned /\ alive2=(dst=2) /\ alive3=(foreign=3)
UniquePointerOwner == (IF src=1 THEN 1 ELSE 0)+(IF dst=1 THEN 1 ELSE 0)+orphan <= 1
=============================================================================
