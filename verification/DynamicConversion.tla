------------------------- MODULE DynamicConversion -------------------------
EXTENDS Integers
CONSTANT Fault
\* Checked conversion followed by assignment on success. Destinations are
\* Int8, UInt8, or a two-name enum with open UInt16 ordinals. Values are encoded
\* as n+1 for the scalar trace format; -1 is the smallest representative.
\* Cases 0..15 correspond to Rust model_input(): integer boundaries, integral/
\* fractional/non-finite floats, names, bool, and same/foreign enum values.
VARIABLES input,target,success,active,stored,oldActive,oldStored,event
vars == <<input,target,success,active,stored,oldActive,oldStored,event>>
Init == /\ input=0 /\ target=0 /\ success=0 /\ active=0 /\ stored=0
        /\ oldActive=0 /\ oldStored=0 /\ event=0
Number(i) == CASE i=0 -> -1 [] i=1 -> 0 [] i=2 -> 127 [] i=3 -> 128
               [] i=4 -> 255 [] i=5 -> 256 [] i=12 -> 65535 [] i=13 -> 65536
               [] OTHER -> 1
Integer(i) == i\in 0..5 \/ i\in {12,13}
Numeric(i) == Integer(i) \/ i=6
Allowed(i,t) == CASE t=0 -> Numeric(i) /\ Number(i)\in -128..127
                  [] t=1 -> Numeric(i) /\ Number(i)\in 0..255
                  [] OTHER -> (Integer(i) /\ Number(i)\in 0..65535) \/ i\in {9,14}
Accepted(i,t) == IF Fault="wrapUnsigned" /\ i=0 /\ t=1 THEN TRUE
                ELSE IF Fault="truncateFraction" /\ i=7 /\ t\in 0..1 THEN TRUE
                ELSE IF Fault="floatEnum" /\ i=6 /\ t=2 THEN TRUE
                ELSE IF Fault="foreignEnum" /\ i=15 /\ t=2 THEN TRUE
                ELSE Allowed(i,t)
Convert(i,t) == /\ input'=i /\ target'=t /\ event'=1
                /\ success'=IF Accepted(i,t) THEN 1 ELSE 0
                /\ oldActive'=IF Accepted(i,t) THEN 0 ELSE active
                /\ oldStored'=IF Accepted(i,t) THEN 0 ELSE stored
                /\ active'=IF Accepted(i,t) \/ Fault="selectOnError" THEN t+1 ELSE active
                /\ stored'=IF Accepted(i,t) THEN Number(i)+1 ELSE stored
Next == \E i\in 0..15,t\in 0..2: Convert(i,t)
Spec == Init /\ [][Next]_vars
TypeOK == /\ input\in 0..15 /\ target\in 0..2 /\ success\in 0..1
          /\ active\in 0..3 /\ stored\in 0..65537
          /\ oldActive\in 0..3 /\ oldStored\in 0..65537 /\ event\in 0..1
CorrectConversion == event=1 => /\ (success=1) = Allowed(input,target)
                               /\ (success=1 => stored=Number(input)+1 /\ active=target+1)
FailurePreservesDestination == event=1 /\ success=0 => active=oldActive /\ stored=oldStored
=============================================================================
