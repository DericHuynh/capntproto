-------------------------- MODULE RouteTaskOwner --------------------------
EXTENDS Naturals
CONSTANT Fault
\* One old owner, two workers and a retained observer. A replacement owner may
\* start after stop, before canceled old workers have actually been polled.
\* Worker: 0 unpolled, 1 pending, 2 completed, 3 canceled/released.
\* Cause: 0 none, 1 connect failure, 2 replacement timeout, 3 cancellation.
\* Cancellation is synchronous; releasing the future requires an executor poll.
VARIABLES owner, stopped, dial, sibling, abortDial, abortSibling, supplied,
          outcome, first, fresh, newOutcome, event
vars == <<owner,stopped,dial,sibling,abortDial,abortSibling,supplied,
          outcome,first,fresh,newOutcome,event>>
Init == /\ owner=1 /\ stopped=0 /\ dial=0 /\ sibling=0
        /\ abortDial=0 /\ abortSibling=0 /\ supplied=0
        /\ outcome=0 /\ first=0 /\ fresh=0 /\ newOutcome=0 /\ event=0
PollDial ==
    /\ dial\in {0,1}
    /\ dial'=(IF abortDial=1 /\ Fault#"runCanceled" THEN 3
               ELSE IF supplied=1 THEN 2 ELSE 1)
    /\ outcome'=(IF dial'=2 /\ outcome=0 THEN 1 ELSE outcome)
    /\ first'=(IF dial'=2 /\ first=0 THEN 1 ELSE first)
    /\ event'=1
    /\ UNCHANGED <<owner,stopped,sibling,abortDial,abortSibling,supplied,fresh,newOutcome>>
PollSibling == /\ sibling\in {0,1}
               /\ sibling'=(IF abortSibling=1 THEN 3 ELSE 1) /\ event'=2
               /\ UNCHANGED <<owner,stopped,dial,abortDial,abortSibling,supplied,
                               outcome,first,fresh,newOutcome>>
Supply == /\ supplied=0 /\ supplied'=1 /\ event'=3
          /\ UNCHANGED <<owner,stopped,dial,sibling,abortDial,abortSibling,
                          outcome,first,fresh,newOutcome>>
Cancel(dropOwner) ==
    /\ owner=1
    /\ owner'=(IF dropOwner THEN 0 ELSE 1) /\ stopped'=1
    /\ abortDial'=(IF dropOwner /\ Fault="retainOwner" THEN abortDial ELSE 1)
    /\ abortSibling'=(IF Fault="missWorker" THEN abortSibling ELSE 1)
    /\ outcome'=(IF outcome=0 THEN 3 ELSE outcome)
    /\ first'=(IF first=0 THEN 3 ELSE first)
    /\ newOutcome'=(IF fresh#0 /\ Fault="crossGeneration" THEN 3 ELSE newOutcome)
    /\ event'=(IF dropOwner THEN 5 ELSE 4)
    /\ UNCHANGED <<dial,sibling,supplied,fresh>>
Replace == /\ stopped=1 /\ fresh=0 /\ fresh'=1 /\ event'=6
           /\ UNCHANGED <<owner,stopped,dial,sibling,abortDial,abortSibling,
                           supplied,outcome,first,newOutcome>>
PollNew == /\ fresh=1 /\ fresh'=2 /\ newOutcome'=2 /\ event'=7
           /\ UNCHANGED <<owner,stopped,dial,sibling,abortDial,abortSibling,
                           supplied,outcome,first>>
LateCause == /\ stopped=1
             /\ outcome'=(IF Fault="overwrite" THEN 1 ELSE outcome) /\ event'=8
             /\ UNCHANGED <<owner,stopped,dial,sibling,abortDial,abortSibling,
                             supplied,first,fresh,newOutcome>>
Next == PollDial \/ PollSibling \/ Supply \/ Cancel(FALSE) \/ Cancel(TRUE)
        \/ Replace \/ PollNew \/ LateCause
Spec == Init /\ [][Next]_vars
FairSpec == Spec /\ WF_vars(PollDial) /\ WF_vars(PollSibling) /\ WF_vars(PollNew)
TypeOK == /\ owner\in 0..1 /\ stopped\in 0..1 /\ dial\in 0..3
          /\ sibling\in {0,1,3} /\ abortDial\in 0..1 /\ abortSibling\in 0..1
          /\ supplied\in 0..1 /\ outcome\in {0,1,3} /\ first\in {0,1,3}
          /\ fresh\in 0..2 /\ newOutcome\in 0..3 /\ event\in 0..8
FirstCause == outcome=first
AllWorkersCanceled == stopped=1 => abortDial=1 /\ abortSibling=1
OwnerReleased == owner=0 => stopped=1 /\ abortDial=1
GenerationIsolation == newOutcome=(IF fresh=2 THEN 2 ELSE 0)
NoPostCancelCompletion == first=3 => dial#2
Progress == /\ (stopped=1) ~> (dial\in {2,3} /\ sibling=3)
            /\ (fresh=1) ~> (fresh=2)
=============================================================================
