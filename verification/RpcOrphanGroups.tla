------------------------ MODULE RpcOrphanGroups ------------------------
EXTENDS Naturals, TLC
\* Edge-prefix histories run against both compiled and runtime-loaded schemas.
\* Schema registry/brand mismatches refine type rejection; concrete metadata
\* and lifetimes are checked by Rust regressions and compiler acceptance cases.
CONSTANT Fault
\* A detached group owns its known fields; nested groups own their fields.
\* Edit completes before a user callback returns an error or unwinds. Both
\* outcomes preserve completed edits and are replayed separately in Rust.
VARIABLES materialized, location, cap, changed, selected, selection, read, held, taken, moved, returned, badType, badArena, sibling, address, refsA, refsB, refsC
vars == <<materialized,location,cap,changed,selected,selection,read,held,taken,moved,returned,badType,badArena,sibling,address,refsA,refsB,refsC>>
Bit(b) == IF b THEN 1 ELSE 0
Owner == location\in {0,1,2}
Init == /\ materialized=FALSE /\ location=0 /\ cap=1 /\ changed=FALSE /\ selected=FALSE /\ selection=0 /\ read=FALSE /\ held=FALSE /\ taken=FALSE /\ moved=0 /\ returned=FALSE /\ badType=FALSE /\ badArena=FALSE /\ sibling=91 /\ address=1 /\ refsA=1 /\ refsB=1 /\ refsC=0

Detach == /\ location=0
    /\ location'=1
    /\ sibling'=(IF Fault="touchSibling" THEN 92 ELSE sibling)
    /\ address'=(IF Fault="copyAddress" THEN 2 ELSE address)
    /\ UNCHANGED <<materialized,cap,changed,selected,selection,read,held,taken,moved,returned,badType,badArena,refsA,refsB,refsC>>

Edit == /\ location=1 /\ cap=1 /\ ~changed
    /\ changed'=TRUE
    /\ cap'=2
    /\ refsA'=refsA-(IF Fault="leakReplaced" THEN 0 ELSE 1)
    /\ refsC'=refsC+(IF Fault="loseOnError" THEN 0 ELSE 1)
    /\ UNCHANGED <<materialized,location,selected,selection,read,held,taken,moved,returned,badType,badArena,sibling,address,refsB>>

Select == /\ location=1 /\ ~selected
    /\ selected'=TRUE
    /\ selection'=1
    /\ refsB'=refsB-(IF Fault="leakNested" THEN 0 ELSE 1)
    /\ UNCHANGED <<materialized,location,cap,changed,read,held,taken,moved,returned,badType,badArena,sibling,address,refsA,refsC>>

Read == /\ location=1 /\ cap=1 /\ ~read
    /\ read'=TRUE
    /\ held'=TRUE
    /\ refsA'=refsA+(IF Fault="loseHeld" THEN 0 ELSE 1)
    /\ UNCHANGED <<materialized,location,cap,changed,selected,selection,taken,moved,returned,badType,badArena,sibling,address,refsB,refsC>>

Release == /\ held
    /\ held'=FALSE
    /\ refsA'=refsA-1
    /\ UNCHANGED <<materialized,location,cap,changed,selected,selection,read,taken,moved,returned,badType,badArena,sibling,address,refsB,refsC>>

Take == /\ location=1 /\ cap#0 /\ ~taken
    /\ taken'=TRUE
    /\ moved'=cap
    /\ cap'=0
    /\ refsA'=refsA-Bit(Fault="loseMoved" /\ cap=1)
    /\ refsC'=refsC-Bit(Fault="loseMoved" /\ cap=2)
    /\ UNCHANGED <<materialized,location,changed,selected,selection,read,held,returned,badType,badArena,sibling,address,refsB>>

Put == /\ location=1 /\ cap=0 /\ moved#0
    /\ returned'=TRUE
    /\ cap'=moved
    /\ moved'=0
    /\ UNCHANGED <<materialized,location,changed,selected,selection,read,held,taken,badType,badArena,sibling,address,refsA,refsB,refsC>>

DropMoved == /\ moved#0
    /\ moved'=0
    /\ refsA'=refsA-Bit(moved=1)
    /\ refsC'=refsC-Bit(moved=2)
    /\ UNCHANGED <<materialized,location,cap,changed,selected,selection,read,held,taken,returned,badType,badArena,sibling,address,refsB>>

BadType == /\ location=1 /\ ~badType
    /\ badType'=TRUE
    /\ selection'=(IF Fault="selectOnReject" THEN 1 ELSE selection)
    /\ UNCHANGED <<materialized,location,cap,changed,selected,read,held,taken,moved,returned,badArena,sibling,address,refsA,refsB,refsC>>

BadArena == /\ location=1 /\ ~badArena
    /\ badArena'=TRUE
    /\ selection'=(IF Fault="selectOnReject" THEN 1 ELSE selection)
    /\ UNCHANGED <<materialized,location,cap,changed,selected,read,held,taken,moved,returned,badType,sibling,address,refsA,refsB,refsC>>

Adopt == /\ location=1
    /\ location'=2
    /\ UNCHANGED <<materialized,cap,changed,selected,selection,read,held,taken,moved,returned,badType,badArena,sibling,address,refsA,refsB,refsC>>

Drop == /\ location\in {1,2}
    /\ location'=3
    /\ refsA'=refsA-Bit(cap=1)
    /\ refsB'=refsB-Bit(~selected)
    /\ refsC'=refsC-Bit(cap=2)
    /\ UNCHANGED <<materialized,cap,changed,selected,selection,read,held,taken,moved,returned,badType,badArena,sibling,address>>

Materialize == /\ location=1 /\ ~materialized /\ materialized'=TRUE
    /\ address'=(IF Fault="copyGroupStorage" THEN 2 ELSE address)
    /\ refsA'=refsA+Bit(Fault="leakMaterialized" /\ cap=1)
    /\ UNCHANGED <<location,cap,changed,selected,selection,read,held,taken,moved,returned,badType,badArena,sibling,refsB,refsC>>

Next == Materialize \/ Detach \/ Edit \/ Select \/ Read \/ Release \/ Take \/ Put \/ DropMoved \/ BadType \/ BadArena \/ Adopt \/ Drop
Spec == Init /\ [][Next]_vars
TypeOK == /\ location\in 0..3 /\ cap\in 0..2 /\ moved\in 0..2 /\ selection\in 0..1
          /\ sibling\in 91..92 /\ address\in 1..2 /\ refsA\in 0..3 /\ refsB\in 0..1 /\ refsC\in 0..1
          /\ \A b\in {materialized,changed,selected,read,held,taken,returned,badType,badArena}:b\in BOOLEAN
Ownership == /\ refsA=Bit(Owner /\ cap=1)+Bit(held)+Bit(moved=1)
             /\ refsB=Bit(Owner /\ ~selected)
             /\ refsC=Bit(Owner /\ cap=2)+Bit(moved=2)
Selection == selection=Bit(selected)
Isolation == sibling=91 /\ address=1
=============================================================================
