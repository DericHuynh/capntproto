--------------------------- MODULE NativeListCast ---------------------------
EXTENDS Naturals
CONSTANT Fault
\* Eight list casts: UInt32, enum, branded struct, nested branded struct,
\* wrong primitive kind, interface, wrong interface ID, and wrong list depth.
\* Registration is loader-local (enum/struct/interface bits). Selecting a case
\* constructs fresh one-element storage; casts borrow it only during the action.
\* Three actions suffice to select/register/cast. Native writes change that same
\* storage. Capability invocation/ownership and arbitrary schemas are tested
\* separately, not represented by this finite compatibility/storage model.
VARIABLES registered, selected, result, expectedResult, value, expectedValue, event, steps
vars == <<registered,selected,result,expectedResult,value,expectedValue,event,steps>>
Has(k) == (registered \div 2^k) % 2 = 1
Compatible == selected \in {0,1,2,3,5}
Registered == CASE selected=1 -> Has(0) [] selected\in {2,3}->Has(1)
                  [] selected\in {5,6}->Has(2) [] OTHER->TRUE
Expected == Compatible /\ Registered
Allowed == CASE Fault="skipRegistration" -> Compatible
             [] Fault="checkBrand" /\ selected\in {2,3} -> FALSE
             [] Fault="wrongKind" /\ selected=4 -> TRUE
             [] Fault="wrongId" /\ selected=6 -> Has(2)
             [] Fault="wrongDepth" /\ selected=7 -> TRUE
             [] OTHER -> Expected
Init == /\ registered=0 /\ selected=0 /\ result=0 /\ expectedResult=0
        /\ value=0 /\ expectedValue=0 /\ event=0 /\ steps=0
Select(i) == /\ steps<3 /\ i#selected /\ selected'=i /\ event'=i+1 /\ steps'=steps+1
             /\ value'=0 /\ expectedValue'=0 /\ result'=0 /\ expectedResult'=0
             /\ UNCHANGED registered
Register(k) == /\ steps<3 /\ ~Has(k) /\ registered'=registered+2^k
               /\ event'=9+k /\ steps'=steps+1 /\ result'=0 /\ expectedResult'=0
               /\ UNCHANGED <<selected,value,expectedValue>>
Read == /\ steps<3 /\ event'=12 /\ steps'=steps+1
        /\ result'=(IF Allowed THEN 1 ELSE 0)
        /\ expectedResult'=(IF Expected THEN 1 ELSE 0)
        /\ UNCHANGED <<registered,selected,value,expectedValue>>
Write == /\ steps<3 /\ selected\notin {5,6} /\ event'=13 /\ steps'=steps+1
         /\ result'=(IF Allowed THEN 1 ELSE 0)
         /\ expectedResult'=(IF Expected THEN 1 ELSE 0)
         /\ value'=(IF Fault="copyWrite" THEN value
                    ELSE IF Allowed \/ Fault="mutateRejected" THEN 1 ELSE value)
         /\ expectedValue'=(IF Expected THEN 1 ELSE expectedValue)
         /\ UNCHANGED <<registered,selected>>
Next == (\E i\in 0..7:Select(i)) \/ (\E k\in 0..2:Register(k)) \/ Read \/ Write
Spec == Init /\ [][Next]_vars
TypeOK == /\ registered\in 0..7 /\ selected\in 0..7 /\ result\in 0..1 /\ expectedResult\in 0..1
          /\ value\in 0..1 /\ expectedValue\in 0..1 /\ event\in 0..13 /\ steps\in 0..3
CastResult == result=expectedResult
SameStorage == value=expectedValue
=============================================================================
