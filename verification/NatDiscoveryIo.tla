-------------------------- MODULE NatDiscoveryIo --------------------------
EXTENDS Naturals
CONSTANT Fault
\* A real one-shot discovery starts blocked on its first request send.
\* Phase 0=blocked, 1=sent, 2=terminal. Results: 1=address, 2=timeout,
\* 3=send error, 4=canceled. Replays obtain transaction IDs from emitted bytes.
VARIABLES phase, result, expected, emitted, allowed, invalid, retried, late, event
vars == <<phase,result,expected,emitted,allowed,invalid,retried,late,event>>
Init == /\ phase=0 /\ result=0 /\ expected=0 /\ emitted=0 /\ allowed=0
        /\ invalid=0 /\ retried=0 /\ late=0 /\ event=0
Send == /\ phase=0 /\ phase'=1 /\ emitted'=1 /\ allowed'=1 /\ event'=1
        /\ UNCHANGED <<result,expected,invalid,retried,late>>
Reply == /\ phase=1 /\ phase'=2 /\ result'=1 /\ expected'=1 /\ event'=2
         /\ UNCHANGED <<emitted,allowed,invalid,retried,late>>
Invalid == /\ phase=1 /\ invalid=0 /\ invalid'=1 /\ event'=3
           /\ result'=(IF Fault="foreign" THEN 1 ELSE result)
           /\ UNCHANGED <<phase,expected,emitted,allowed,retried,late>>
Expire == /\ phase\in {0,1} /\ phase'=2 /\ expected'=2 /\ event'=4
          /\ result'=(IF Fault="blockedDeadline" /\ phase=0 THEN 0 ELSE 2)
          /\ UNCHANGED <<emitted,allowed,invalid,retried,late>>
Fail == /\ phase=0 /\ phase'=2 /\ result'=3 /\ expected'=3 /\ event'=5
        /\ UNCHANGED <<emitted,allowed,invalid,retried,late>>
Cancel == /\ phase\in {0,1} /\ phase'=2 /\ result'=4 /\ expected'=4 /\ event'=6
          /\ UNCHANGED <<emitted,allowed,invalid,retried,late>>
Late == /\ phase=2 /\ late=0 /\ late'=1 /\ event'=7
        /\ emitted'=(IF Fault="lateSend" THEN emitted+1 ELSE emitted)
        /\ UNCHANGED <<phase,result,expected,allowed,invalid,retried>>
Boundary == /\ phase=1 /\ phase'=2 /\ expected'=2 /\ event'=8
            /\ result'=(IF Fault="lateDeadline" THEN 1 ELSE 2)
            /\ UNCHANGED <<emitted,allowed,invalid,retried,late>>
Retry == /\ phase=1 /\ retried=0 /\ retried'=1 /\ event'=9
         /\ emitted'=2 /\ allowed'=2
         /\ UNCHANGED <<phase,result,expected,invalid,late>>
Next == Send \/ Reply \/ Invalid \/ Expire \/ Fail \/ Cancel \/ Late \/ Boundary \/ Retry
Spec == Init /\ [][Next]_vars
TypeOK == /\ phase\in 0..2 /\ result\in 0..4 /\ expected\in 0..4
          /\ emitted\in 0..3 /\ allowed\in 0..2 /\ invalid\in 0..1
          /\ retried\in 0..1 /\ late\in 0..1 /\ event\in 0..9
Completion == result=expected
NoLateSend == emitted=allowed
=============================================================================
