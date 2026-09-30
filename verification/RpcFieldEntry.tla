-------------------- MODULE RpcFieldEntry --------------------
EXTENDS Naturals, TLC
CONSTANTS Layout, Fault
\* Layout: 0 absent, 1 sufficient struct, 2 old struct, 3 wrong pointer kind.
\* Consuming a compatible entry uses its retained editor without reacquisition.
VARIABLES phase, value, layout, allocated, changed, repeats, error, strictAllocated, event
vars == <<phase,value,layout,allocated,changed,repeats,error,strictAllocated,event>>
Init == /\ phase=0 /\ value=(IF Layout\in {1,2} THEN 7 ELSE 0)
        /\ layout=Layout /\ allocated=FALSE /\ changed=FALSE /\ repeats=0
        /\ error=0 /\ strictAllocated=FALSE /\ event=0
Inspect == /\ phase=0
           /\ phase'=(IF layout=0 THEN 1 ELSE IF layout=3 THEN 5 ELSE 2)
           /\ error'=(IF layout=3 THEN 1 ELSE 0)
           /\ allocated'=(Fault="eagerUpgrade" /\ layout=2)
           /\ layout'=(IF Fault="eagerUpgrade" /\ layout=2 THEN 1 ELSE layout)
           /\ event'=1 /\ UNCHANGED <<value,changed,repeats,strictAllocated>>
Edit == /\ phase=2
        /\ phase'=(IF layout=2 /\ Fault#"editUpgrade" THEN 5 ELSE 3)
        /\ error'=(IF layout=2 /\ Fault#"editUpgrade" THEN 2 ELSE 0)
        /\ strictAllocated'=(layout=2 /\ Fault="editUpgrade")
        /\ allocated'=(allocated \/ (layout=2 /\ Fault="editUpgrade"))
        /\ layout'=(IF Fault="editUpgrade" THEN 1 ELSE layout)
        /\ repeats'=(IF Fault="repeatAcquisition" THEN repeats+1 ELSE repeats)
        /\ event'=2 /\ UNCHANGED <<value,changed>>
Ensure == /\ phase=2 /\ phase'=3 /\ layout'=1 /\ error'=0
          /\ allocated'=(layout=2)
          /\ value'=(IF Fault="loseValue" /\ layout=2 THEN 0 ELSE value)
          /\ repeats'=(IF Fault="repeatAcquisition" /\ layout=1 THEN repeats+1 ELSE repeats)
          /\ event'=3 /\ UNCHANGED <<changed,strictAllocated>>
Initialize == /\ phase=1 /\ phase'=3 /\ layout'=1 /\ allocated'=TRUE
              /\ event'=4 /\ UNCHANGED <<value,changed,repeats,error,strictAllocated>>
Write == /\ phase=3 /\ ~changed /\ value'=9 /\ changed'=TRUE /\ event'=5
         /\ UNCHANGED <<phase,layout,allocated,repeats,error,strictAllocated>>
Drop == /\ phase\in {1,2,3} /\ phase'=4 /\ event'=6
        /\ UNCHANGED <<value,layout,allocated,changed,repeats,error,strictAllocated>>
Replace == /\ phase\in {4,5} /\ phase'=6 /\ value'=11 /\ layout'=1 /\ event'=7
           /\ UNCHANGED <<allocated,changed,repeats,error,strictAllocated>>
Next == Inspect \/ Edit \/ Ensure \/ Initialize \/ Write \/ Drop \/ Replace
Spec == Init /\ [][Next]_vars
TypeOK == /\ phase\in 0..6 /\ value\in {0,7,9,11} /\ layout\in 0..3
          /\ repeats\in 0..1 /\ error\in 0..2 /\ event\in 0..7
          /\ allocated\in BOOLEAN /\ changed\in BOOLEAN /\ strictAllocated\in BOOLEAN
LazyInspection == phase\in {1,2} => ~allocated
StrictEdit == ~strictAllocated
CachedAcquisition == repeats=0
PreserveValue == ~changed /\ phase#6 => value=(IF Layout\in {1,2} THEN 7 ELSE 0)
ReadyLayout == phase=3 => layout=1
=============================================================================
