----------------------- MODULE DynamicFieldPresence -----------------------
EXTENDS Naturals
CONSTANT Fault
\* One union: Void, UInt32(default 42), Text(default "default"), group.
\* Scalar bits and the pointer slot can remain populated when the tag changes.
\* Tag 4 represents an unknown future arm; field 4 is the enclosing group.
\* Both modes only inspect storage. Pointer payloads and allocation are outside
\* this abstraction, as are floating point bit patterns (covered natively).
VARIABLES bits, pointer, tag, field, mode, result, event
vars == <<bits,pointer,tag,field,mode,result,event>>
Init == /\ bits=0 /\ pointer=0 /\ tag=0 /\ field=0 /\ mode=0 /\ result=0 /\ event=0
ResetObservation == /\ field'=0 /\ mode'=0 /\ result'=0
WriteBits == /\ bits'=1-bits /\ event'=1 /\ ResetObservation
             /\ UNCHANGED <<pointer,tag>>
WritePointer == /\ pointer'=1-pointer /\ event'=2 /\ ResetObservation
                /\ UNCHANGED <<bits,tag>>
Select(t) == /\ tag'=t /\ event'=3 /\ ResetObservation
             /\ UNCHANGED <<bits,pointer>>
Active(f) == f=4 \/ tag=f
Expected(f,m) == IF ~Active(f) THEN 0
                 ELSE CASE f\in {3,4} -> 1
                        [] f=2 -> pointer
                        [] m=0 -> 1
                        [] f=0 -> 0
                        [] OTHER -> bits
Observed(f,m) ==
    IF Fault="ignoreUnion" /\ f=2 THEN pointer
    ELSE IF Fault="decodedZero" /\ Active(f) /\ f=1 /\ m=1 THEN 1
    ELSE IF Fault="pointerContents" /\ Active(f) /\ f=2 /\ m=1 THEN 0
    ELSE IF Fault="emptyGroup" /\ Active(f) /\ f\in {3,4} /\ m=1 THEN 0
    ELSE Expected(f,m)
Inspect(f,m) == /\ field'=f /\ mode'=m /\ result'=Observed(f,m) /\ event'=4
                /\ UNCHANGED <<bits,pointer,tag>>
Next == WriteBits \/ WritePointer \/ (\E t\in 0..4: Select(t))
        \/ (\E f\in 0..4, m\in 0..1: Inspect(f,m))
Spec == Init /\ [][Next]_vars
TypeOK == /\ bits\in 0..1 /\ pointer\in 0..1 /\ tag\in 0..4
          /\ field\in 0..4 /\ mode\in 0..1 /\ result\in 0..1 /\ event\in 0..4
AccuratePresence == event=4 => result=Expected(field,mode)
=============================================================================
