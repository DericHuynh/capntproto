-------------------------- MODULE NoiseSocketDriver --------------------------
EXTENDS Naturals
CONSTANT Fault
\* A real driver is initially blocked emitting its first encrypted flight.
\* The adapter polls only after a wake. 1=closed, 2=deadline, 3=IO, 4=dropped.
VARIABLES closed, expired, failed, writable, dropped, pending,
          result, expected, emitted, allowed, event
vars == <<closed,expired,failed,writable,dropped,pending,
          result,expected,emitted,allowed,event>>
Init == /\ closed=0 /\ expired=0 /\ failed=0 /\ writable=0 /\ dropped=0
        /\ pending=0 /\ result=0 /\ expected=0 /\ emitted=0 /\ allowed=0 /\ event=0
Close == /\ closed=0 /\ closed'=1 /\ pending'=1 /\ event'=1
         /\ UNCHANGED <<expired,failed,writable,dropped,result,expected,emitted,allowed>>
Expire == /\ expired=0 /\ expired'=1 /\ pending'=1 /\ event'=2
          /\ UNCHANGED <<closed,failed,writable,dropped,result,expected,emitted,allowed>>
Fail == /\ failed=0 /\ failed'=1 /\ pending'=1 /\ event'=3
        /\ UNCHANGED <<closed,expired,writable,dropped,result,expected,emitted,allowed>>
Unblock == /\ writable=0 /\ writable'=1 /\ pending'=1 /\ event'=4
           /\ UNCHANGED <<closed,expired,failed,dropped,result,expected,emitted,allowed>>
Drop == /\ dropped=0 /\ dropped'=1 /\ event'=5
        /\ result'=(IF result=0 THEN 4 ELSE result)
        /\ expected'=(IF expected=0 THEN 4 ELSE expected)
        /\ UNCHANGED <<closed,expired,failed,writable,pending,emitted,allowed>>
\* Dedicated socket closure is an IO error, not shared-listener shutdown.
\* The outer shutdown deadline wins over IO when both are ready in one poll.
\* Send errors are observed by a send attempt, not by an idle receive wait.
Cause == IF expired=1 THEN 2 ELSE IF closed=1 THEN 1
         ELSE IF failed=1 /\ emitted=0 THEN 3 ELSE 0
BrokenCause == IF Fault="lateDeadline" /\ closed=1 THEN 1
               ELSE IF Fault="lateDeadline" /\ failed=1 /\ emitted=0 THEN 3
               ELSE IF expired=1 THEN 2
               ELSE IF closed=1 /\ Fault#"missClose" THEN 1
               ELSE IF failed=1 /\ emitted=0 THEN 3 ELSE 0
Poll == /\ pending=1 /\ pending'=0 /\ event'=6
        /\ expected'=(IF expected#0 THEN expected ELSE Cause)
        /\ result'=(IF result#0 /\ Fault#"overwrite" THEN result ELSE BrokenCause)
        /\ allowed'=(IF expected=0 /\ Cause=0 /\ writable=1 THEN 1 ELSE allowed)
        /\ emitted'=(IF writable=1 /\ (Fault="leakSend" \/ (result=0 /\ BrokenCause=0))
                     THEN 1 ELSE emitted)
        /\ UNCHANGED <<closed,expired,failed,writable,dropped>>
Next == Close \/ Expire \/ Fail \/ Unblock \/ Drop \/ Poll
Spec == Init /\ [][Next]_vars
TypeOK == /\ closed\in 0..1 /\ expired\in 0..1 /\ failed\in 0..1
          /\ writable\in 0..1 /\ dropped\in 0..1 /\ pending\in 0..1
          /\ result\in 0..4 /\ expected\in 0..4 /\ emitted\in 0..1
          /\ allowed\in 0..1 /\ event\in 0..6
TerminalCause == result=expected
NoLateEmission == emitted=allowed
=============================================================================
