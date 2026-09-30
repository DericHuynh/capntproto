--------------------- MODULE RpcSchemaIntrospection ---------------------
EXTENDS Naturals
CONSTANT Fault
\* One generic scope: default, explicit AnyPointer, Text, wholly symbolic,
\* inherited symbolic, or inherited empty. Independent union tags exercise
\* known and unknown arms. At most three operations, including rebind/erase.
VARIABLES mode, scopes, count, argument, branded, tag, selected, readable, steps, event
vars == <<mode,scopes,count,argument,branded,tag,selected,readable,steps,event>>
Scopes(m) == IF m \in {1,2,4,5} THEN 1 ELSE 0
Count(m) == IF m \in {1,2} THEN 1 ELSE 0
Argument(m) == IF m=2 THEN 1 ELSE IF m\in {3,4} THEN 2 ELSE 0
Branded(m) == IF m=0 THEN 0 ELSE 1
Init == /\ mode=0 /\ scopes=0 /\ count=0 /\ argument=0 /\ branded=0
        /\ tag=0 /\ selected=1 /\ readable=0 /\ steps=0 /\ event=0
SetBrand(m,e) ==
    /\ steps<3 /\ steps'=steps+1 /\ event'=e /\ mode'=m
    /\ scopes'=IF Fault="dropExplicitDefault" /\ m=1 THEN 0 ELSE Scopes(m)
    /\ count'=Count(m) /\ branded'=Branded(m)
    /\ argument'=IF Fault="keepSymbolicOnErase" /\ e=1 /\ mode\in {3,4}
                  THEN 2 ELSE Argument(m)
    /\ UNCHANGED <<tag,selected,readable>>
Inherit == SetBrand(IF mode=0 THEN 5 ELSE IF mode=3 THEN 4 ELSE mode, 5)
Tag(t) ==
    /\ steps<3 /\ steps'=steps+1 /\ event'=6 /\ tag'=t
    /\ selected'=IF t<2 THEN t+1 ELSE IF Fault="selectNonUnion" THEN 3 ELSE 0
    /\ readable'=IF t=1 \/ Fault="readInactive" THEN 1 ELSE 0
    /\ UNCHANGED <<mode,scopes,count,argument,branded>>
Next == SetBrand(0,1) \/ SetBrand(1,2) \/ SetBrand(2,3) \/ SetBrand(3,4)
        \/ Inherit \/ (\E t\in 0..2: Tag(t))
Spec == Init /\ [][Next]_vars
TypeOK == /\ mode\in 0..5 /\ scopes\in 0..1 /\ count\in 0..1
          /\ argument\in 0..2 /\ branded\in 0..1 /\ tag\in 0..2
          /\ selected\in 0..3 /\ readable\in 0..1 /\ steps\in 0..3 /\ event\in 0..6
ExplicitScopes == scopes=Scopes(mode) /\ count=Count(mode) /\ branded=Branded(mode)
ResolvedArguments == argument=Argument(mode)
UnknownUnion == tag=2 => selected=0
ActiveRead == readable=1 <=> tag=1
=============================================================================
