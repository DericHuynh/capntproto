-------------------------- MODULE GrantRevocation --------------------------
EXTENDS Naturals
CONSTANTS Watched, Fault
\* Root 0 has branch 1 and sibling 3; branch 1 has leaf 2. Rights and
\* authenticated peer binding are outside this notification-lifecycle model.
VARIABLES revoked, waiter, snapshot, event, target, result, expected
vars == <<revoked,waiter,snapshot,event,target,result,expected>>
Ancestors == CASE Watched=0 -> {0} [] Watched=1 -> {0,1}
                 [] Watched=2 -> {0,1,2} [] OTHER -> {0,3}
Set(mask, bit) == (mask \div (2^bit)) % 2 = 1
Live == \A i \in Ancestors: ~Set(revoked,i)
Observed == CASE Fault="ownOnly" -> Set(revoked,Watched)
              [] Fault="crossBranch" -> revoked#0
              [] Fault="lostEarly" -> \E i \in Ancestors: Set(revoked,i) /\ ~Set(snapshot,i)
              [] Fault="alwaysReady" -> TRUE
              [] OTHER -> ~Live
Init == /\ revoked=0 /\ waiter=0 /\ snapshot=0
        /\ event=0 /\ target=0 /\ result=0 /\ expected=0
\* 0 absent, 1 constructed but unpolled, 2 registered/pending, 3 completed.
Start == /\ waiter=0 /\ waiter'=1 /\ snapshot'=revoked /\ event'=1
         /\ result'=0 /\ expected'=0 /\ UNCHANGED <<revoked,target>>
Poll == /\ waiter \in {1,2}
        /\ waiter'=(IF Observed THEN 3 ELSE 2) /\ event'=2
        /\ result'=(IF Observed THEN 1 ELSE 0)
        /\ expected'=(IF Live THEN 0 ELSE 1)
        /\ UNCHANGED <<revoked,snapshot,target>>
Cancel == /\ waiter#0 /\ waiter'=0 /\ event'=3
          /\ result'=0 /\ expected'=0 /\ UNCHANGED <<revoked,snapshot,target>>
Revoke(i) == /\ revoked'=(IF Set(revoked,i) THEN revoked ELSE revoked+2^i)
             /\ event'=4 /\ target'=i /\ result'=0 /\ expected'=0
             /\ UNCHANGED <<waiter,snapshot>>
Next == Start \/ Poll \/ Cancel \/ (\E i \in 0..3: Revoke(i))
Spec == Init /\ [][Next]_vars
FairSpec == Spec /\ WF_vars(Poll)
TypeOK == /\ revoked \in 0..15 /\ waiter \in 0..3 /\ snapshot \in 0..15
          /\ event \in 0..4 /\ target \in 0..3 /\ result \in 0..1 /\ expected \in 0..1
Notification == result=expected
NoPrematureCompletion == waiter=3 => ~Live
EventuallyObserved == (waiter=2 /\ ~Live) ~> (waiter=0 \/ waiter=3)
=============================================================================
