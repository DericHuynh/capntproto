------------------------ MODULE RpcCallExecutor ------------------------
EXTENDS Naturals, TLC
CONSTANTS Fail, LoseOwner, KillLiveCaller
VARIABLES caller, executor, completed, running, alive, probed
vars == <<caller,executor,completed,running,alive,probed>>
\* One non-cancellable method after dispatch and result capability allocation.
Init == /\ caller = TRUE /\ executor = TRUE /\ completed = FALSE
        /\ running = TRUE /\ alive = TRUE /\ probed = FALSE
DropCaller == /\ caller /\ caller' = FALSE
              /\ running' = (running /\ executor /\ ~LoseOwner)
              /\ alive' = running'
              /\ UNCHANGED <<executor,completed,probed>>
StopExecutor == /\ executor /\ executor' = FALSE
                /\ running' = (running /\ caller /\ ~KillLiveCaller)
                /\ alive' = (running' \/ (caller /\ completed /\ ~Fail))
                /\ UNCHANGED <<caller,completed,probed>>
Complete == /\ running /\ completed' = TRUE /\ running' = FALSE
            /\ alive' = (caller /\ ~Fail)
            /\ UNCHANGED <<caller,executor,probed>>
ProbeStopped == /\ ~executor /\ ~probed /\ probed' = TRUE
                /\ UNCHANGED <<caller,executor,completed,running,alive>>
Next == DropCaller \/ StopExecutor \/ Complete \/ ProbeStopped
Spec == Init /\ [][Next]_vars
TypeOK == /\ caller \in BOOLEAN /\ executor \in BOOLEAN /\ completed \in BOOLEAN
          /\ running \in BOOLEAN /\ alive \in BOOLEAN /\ probed \in BOOLEAN
TaskOwnership == running = (~completed /\ (caller \/ executor))
CapabilityOwnership == alive = (running \/ (caller /\ completed /\ ~Fail))
NoRevival == completed => ~running
=============================================================================
