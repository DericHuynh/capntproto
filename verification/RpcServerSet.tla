--------------------------- MODULE RpcServerSet ---------------------------
EXTENDS Naturals, TLC
CONSTANTS Fail, EarlyLookup, WaitLaterCalls
VARIABLES a, b, lookup, ahead, broken, bstarted
vars == <<a,b,lookup,ahead,broken,bstarted>>
\* Calls: 0 absent, 1 queued, 2 running, 3 success, 4 caller dropped, 5 error.
\* Lookup: 0 absent, 1 waiting, 2 ready, 3 dropped.
Init == /\ a = 2 /\ b = 0 /\ lookup = 0 /\ ahead = FALSE
        /\ broken = FALSE /\ bstarted = FALSE
SendB == /\ b = 0
         /\ b' = IF broken THEN 5 ELSE IF a=2 THEN 1 ELSE 2
         /\ bstarted' = (b'=2)
         /\ UNCHANGED <<a,lookup,ahead,broken>>
Lookup == /\ lookup = 0 /\ ahead' = (b=0)
          /\ lookup' = IF EarlyLookup \/ (a#2 /\ b#2) THEN 2 ELSE 1
          /\ UNCHANGED <<a,b,broken,bstarted>>
FinishA(cancel) == /\ a=2 /\ a' = IF cancel THEN 4 ELSE IF Fail THEN 5 ELSE 3
                   /\ broken' = (broken \/ (~cancel /\ Fail))
                   /\ b' = IF b=1 THEN IF broken' THEN 5 ELSE 2 ELSE b
                   /\ bstarted' = (bstarted \/ b'=2)
                   /\ lookup' = IF lookup=1 /\ (b'#2 \/ (ahead /\ ~WaitLaterCalls)) THEN 2 ELSE lookup
                   /\ UNCHANGED ahead
FinishB(cancel) == /\ b=2 \/ (cancel /\ b=1)
                   /\ b' = IF cancel THEN 4 ELSE IF Fail THEN 5 ELSE 3
                   /\ broken' = (broken \/ (~cancel /\ Fail))
                   /\ lookup' = IF lookup=1 /\ a#2 THEN 2 ELSE lookup
                   /\ UNCHANGED <<a,ahead,bstarted>>
DropLookup == /\ lookup \in {1,2} /\ lookup'=3
              /\ UNCHANGED <<a,b,ahead,broken,bstarted>>
Next == SendB \/ Lookup \/ FinishA(FALSE) \/ FinishA(TRUE) \/ FinishB(FALSE) \/ FinishB(TRUE) \/ DropLookup
Spec == Init /\ [][Next]_vars
TypeOK == /\ a \in 2..5 /\ b \in 0..5 /\ lookup \in 0..3
          /\ ahead \in BOOLEAN /\ broken \in BOOLEAN /\ bstarted \in BOOLEAN
NoBypass == lookup=2 => (a#2 /\ (ahead \/ b#2))
SnapshotBarrier == (lookup=1 /\ a#2) => (~ahead /\ b=2)
SerialCalls == ~(a=2 /\ b=2)
=============================================================================
