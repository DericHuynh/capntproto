---------------------------- MODULE NativeRpcCast ----------------------------
EXTENDS Naturals
CONSTANT Fault
\* Loaded/compiled clients (cases 1..4) and result pipelines (5..7).
\* Same type, erased brand, unrelated ID, and an inherited interface.
\* Each setup owns one local server. Four actions may share/transfer/drop its
\* hook or invoke a native child through a two-pointer path and a group.
\* Calls finish between actions. Pending RPC scheduling is native-test scope.
VARIABLES mode, registered, selected, source, native, alive, result, expected,
          calls, returned, event, steps
vars == <<mode,registered,selected,source,native,alive,result,expected,calls,returned,event,steps>>
Compatible == selected \in {1,2,5,6}
Permitted == Compatible /\ (mode=1 \/ registered=1)
Allowed == CASE Fault="skipRegistration" -> Compatible
             [] Fault="checkBrand" /\ selected\in {2,6} -> FALSE
             [] Fault="wrongInterface" /\ selected\in {3,4} -> mode=1 \/ registered=1
             [] Fault="wrongStruct" /\ selected=7 -> mode=1 \/ registered=1
             [] OTHER -> Permitted
Init == /\ mode=0 /\ registered=0 /\ selected=0 /\ source=0 /\ native=0
        /\ alive=0 /\ result=0 /\ expected=0 /\ calls=0 /\ returned=0 /\ event=0 /\ steps=0
Setup(m,r,c) == /\ selected=0 /\ (m=0 \/ r=1)
                /\ mode'=m /\ registered'=r /\ selected'=c
                /\ source'=1 /\ alive'=1 /\ event'=c+7*r+14*m
                /\ UNCHANGED <<native,result,expected,calls,returned,steps>>
Share == /\ selected\in 1..4 /\ source=1 /\ native=0 /\ steps<4
         /\ native'=(IF Allowed THEN 1 ELSE 0)
         /\ result'=(IF Allowed THEN 1 ELSE 2) /\ expected'=(IF Permitted THEN 1 ELSE 2)
         /\ event'=31 /\ steps'=steps+1
         /\ UNCHANGED <<mode,registered,selected,source,alive,calls,returned>>
Release == /\ selected>0 /\ source=1 /\ native=0 /\ steps<4
           /\ native'=(IF Allowed THEN 1 ELSE 0)
           /\ source'=(IF Allowed THEN IF Fault="cloneOnRelease" THEN 1 ELSE 0
                        ELSE IF Fault="consumeRejected" THEN 0 ELSE 1)
           /\ alive'=(IF Fault="lostHook" /\ Allowed THEN 0 ELSE 1)
           /\ result'=(IF Allowed THEN 1 ELSE 2) /\ expected'=(IF Permitted THEN 1 ELSE 2)
           /\ event'=32 /\ steps'=steps+1
           /\ UNCHANGED <<mode,registered,selected,calls,returned>>
DropSource == /\ source=1 /\ steps<4 /\ source'=0 /\ alive'=native
              /\ event'=33 /\ steps'=steps+1
              /\ UNCHANGED <<mode,registered,selected,native,result,expected,calls,returned>>
DropNative == /\ native=1 /\ steps<4 /\ native'=0
              /\ alive'=(IF Fault="leakNative" THEN alive ELSE source)
              /\ event'=34 /\ steps'=steps+1
              /\ UNCHANGED <<mode,registered,selected,source,result,expected,calls,returned>>
Call == /\ native=1 /\ steps<4 /\ calls'=calls+1
        /\ returned'=(IF Fault="lostPath" THEN 0 ELSE 42)
        /\ event'=35 /\ steps'=steps+1
        /\ UNCHANGED <<mode,registered,selected,source,native,alive,result,expected>>
Next == (\E m\in 0..1,r\in 0..1,c\in 1..7:Setup(m,r,c)) \/ Share \/ Release \/ DropSource \/ DropNative \/ Call
Spec == Init /\ [][Next]_vars
TypeOK == /\ mode\in 0..1 /\ registered\in 0..1 /\ selected\in 0..7
          /\ source\in 0..1 /\ native\in 0..1 /\ alive\in 0..1 /\ result\in 0..2 /\ expected\in 0..2
          /\ calls\in 0..4 /\ returned\in {0,42} /\ event\in 0..35 /\ steps\in 0..4
Compatibility == result=expected
Retention == alive=(IF source+native>0 THEN 1 ELSE 0)
RejectedOwner == event=32 /\ result=2 => source=1
Transfer == event=32 /\ result=1 => source=0 /\ native=1
CallResult == calls>0 => returned=42
=============================================================================
