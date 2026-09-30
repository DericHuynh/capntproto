--------------------------- MODULE RouteGeneration ---------------------------
EXTENDS Naturals
CONSTANT Fault
\* The final three nonzero identifiers, projected to 1..3. next=0 is exhaustion.
\* Two observers may retain/release identifiers. Release never recycles one.
VARIABLES next, a, b, used, duplicate, last, ok, available, event
vars == <<next,a,b,used,duplicate,last,ok,available,event>>
Init == /\ next=1 /\ a=0 /\ b=0 /\ used=0 /\ duplicate=0
        /\ last=0 /\ ok=0 /\ available=0 /\ event=0
Seen(id) == (used \div (2^(id-1))) % 2 = 1
Allocate(slot) ==
    /\ IF slot=1 THEN a=0 ELSE b=0
    /\ LET success == (next#0 /\ ~(Fault="early" /\ next=3)) \/ Fault="zero"
           value == IF success THEN next ELSE 0
       IN /\ last'=value /\ ok'=(IF success THEN 1 ELSE 0)
          /\ a'=(IF slot=1 THEN value ELSE a)
          /\ b'=(IF slot=2 THEN value ELSE b)
          /\ next'=(IF next=0 THEN 0 ELSE IF next=3 THEN
                         (IF Fault="wrap" THEN 1 ELSE 0) ELSE next+1)
          /\ used'=(IF value=0 THEN used ELSE
                         IF Seen(value) THEN used ELSE used+2^(value-1))
          /\ duplicate'=(IF value=0 THEN duplicate ELSE
                              IF Seen(value) THEN 1 ELSE duplicate)
    /\ available'=(IF next#0 THEN 1 ELSE 0) /\ event'=slot
Release(slot) ==
    /\ IF slot=1 THEN a#0 ELSE b#0
    /\ next'=(IF Fault="recycle" THEN (IF slot=1 THEN a ELSE b) ELSE next)
    /\ a'=(IF slot=1 THEN 0 ELSE a) /\ b'=(IF slot=2 THEN 0 ELSE b)
    /\ event'=slot+2
    /\ UNCHANGED <<used,duplicate,last,ok,available>>
Next == Allocate(1) \/ Allocate(2) \/ Release(1) \/ Release(2)
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(Next)
TypeOK == /\ next\in 0..3 /\ a\in 0..3 /\ b\in 0..3 /\ used\in 0..7
          /\ duplicate\in 0..1 /\ last\in 0..3 /\ ok\in 0..1
          /\ available\in 0..1 /\ event\in 0..4
Nonzero == ok=1 => last#0
AllocationContract == event\in {1,2} => ok=available
NoReuse == duplicate=0
Retired == used=7 => next=0
DistinctLive == a=0 \/ b=0 \/ a#b
Progress == <>(next=0)
=============================================================================
