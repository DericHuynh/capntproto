---------------------- MODULE RouteShutdownCompletion ----------------------
EXTENDS Naturals
CONSTANTS Fault, Mode
\* Mode: healthy=0, flush failure=1, close failure=2, peer failure=3.
\* Result/cause: pending=0, receipt=1, flush error=2, close error=3,
\* transport error=4, deadline=5, cancellation=6, prior route failure=7.
\* Control outcome: pending=0, receipt=1, peer error=2, route stopped=3.
\* Readiness signals can arrive in any order. Only Poll commits a drain result.
VARIABLES flush, close, supplied, control, expired, cause, first, result, badTie, event
vars == <<flush,close,supplied,control,expired,cause,first,result,badTie,event>>
Init == /\ flush=0 /\ close=0 /\ supplied=0 /\ control=0 /\ expired=0
        /\ cause=0 /\ first=0 /\ result=0 /\ badTie=0 /\ event=0
Flush == /\ flush=0 /\ flush'=1 /\ event'=1
         /\ UNCHANGED <<close,supplied,control,expired,cause,first,result,badTie>>
Close == /\ close=0 /\ close'=1 /\ event'=2
         /\ UNCHANGED <<flush,supplied,control,expired,cause,first,result,badTie>>
Receipt == /\ supplied=0 /\ supplied'=1
           /\ control'=(IF control=0 THEN (IF Mode=3 THEN 2 ELSE 1) ELSE control) /\ event'=3
           /\ UNCHANGED <<flush,close,expired,cause,first,result,badTie>>
Expire == /\ expired=0 /\ expired'=1 /\ event'=4
          /\ UNCHANGED <<flush,close,supplied,control,cause,first,result,badTie>>
Terminal(kind) ==
    /\ cause'=(IF cause=0 \/ Fault="overwrite" THEN kind ELSE cause)
    /\ first'=(IF first=0 THEN kind ELSE first)
    /\ control'=(IF control=0 THEN 3 ELSE control) /\ event'=kind-1
    /\ UNCHANGED <<flush,close,supplied,expired,result,badTie>>
Chain(f,c,r) == IF ~f THEN 0 ELSE IF Mode=1 THEN 2
               ELSE IF ~c THEN 0 ELSE IF Mode=2 THEN 3
               ELSE IF r=0 THEN 0 ELSE IF r=1 THEN 1 ELSE 4
ReadyChain == Chain(flush=1,close=1,control)
Poll ==
    /\ result=0
    /\ LET chain == Chain(flush=1 \/ Fault="skipFlush",close=1 \/ Fault="skipClose",
                          IF Fault="skipReceipt" THEN 1 ELSE control)
           chosen == IF cause#0 /\ Fault#"ignoreTerminal" THEN cause
                     ELSE IF expired=1 /\ Fault="deadlineFirst" THEN 5
                     ELSE IF chain#0 THEN chain ELSE IF expired=1 THEN 5 ELSE 0
       IN /\ result'=chosen
          /\ cause'=(IF cause=0 THEN chosen ELSE cause)
          /\ first'=(IF first=0 THEN chosen ELSE first)
          /\ control'=(IF control=0 /\ chosen\in 2..7 THEN 3 ELSE control)
          /\ badTie'=(IF cause=0 /\ ReadyChain#0 /\ expired=1 /\ chosen=5 THEN 1 ELSE badTie)
    /\ event'=7
    /\ UNCHANGED <<flush,close,supplied,expired>>
Next == Flush \/ Close \/ Receipt \/ Expire \/ Terminal(6) \/ Terminal(7) \/ Poll
Spec == Init /\ [][Next]_vars
FairSpec == Spec /\ WF_vars(Poll)
TypeOK == /\ flush\in 0..1 /\ close\in 0..1 /\ supplied\in 0..1 /\ control\in 0..3
          /\ expired\in 0..1 /\ cause\in 0..7 /\ first\in 0..7 /\ result\in 0..7
          /\ badTie\in 0..1 /\ event\in 0..7
FirstCause == cause=first
OutcomeAgreement == result#0 => result=cause
AllFences == result=1 => flush=1 /\ close=1 /\ control=1 /\ Mode=0
CompletionPriority == badTie=0
Progress == (cause#0 \/ expired=1 \/ ReadyChain#0) ~> (result#0)
=============================================================================
