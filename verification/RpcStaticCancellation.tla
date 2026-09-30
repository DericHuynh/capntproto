---------------------- MODULE RpcStaticCancellation ----------------------
EXTENDS Naturals, TLC
CONSTANTS Allow, Early, Fail, CancelProtected, LoseEarlyResults
VARIABLES finished, connected, completed, running, alive, returns, kind, used, reused
vars == <<finished,connected,completed,running,alive,returns,kind,used,reused>>
\* Initial state is after dispatch and result capability allocation.
\* Return kind: 0 none, 1 results, 2 canceled, 3 exception.
Init == /\ finished = FALSE /\ connected = TRUE /\ completed = FALSE
        /\ running = TRUE /\ alive = TRUE /\ returns = 0 /\ kind = 0
        /\ used = FALSE /\ reused = FALSE
CanCancel == Allow \/ CancelProtected
Finish == /\ connected /\ ~finished /\ finished' = TRUE
          /\ running' = (running /\ ~CanCancel)
          /\ alive' = (running' /\ ~(Early /\ LoseEarlyResults))
          /\ returns' = IF running /\ CanCancel THEN returns + 1 ELSE returns
          /\ kind' = IF running /\ CanCancel THEN 2 ELSE kind
          /\ UNCHANGED <<connected,completed,used,reused>>
Disconnect == /\ connected /\ connected' = FALSE
              /\ running' = (running /\ ~CanCancel)
              /\ alive' = (running' /\ ~(Early /\ LoseEarlyResults))
              /\ UNCHANGED <<finished,completed,returns,kind,used,reused>>
Complete == /\ running /\ ~completed /\ completed' = TRUE /\ running' = FALSE
            /\ alive' = (connected /\ ~finished /\ ~Fail)
            /\ returns' = IF connected THEN returns + 1 ELSE returns
            /\ kind' = IF connected THEN IF finished THEN 2 ELSE IF Fail THEN 3 ELSE 1 ELSE kind
            /\ UNCHANGED <<finished,connected,used,reused>>
Use == /\ connected /\ completed /\ ~finished /\ ~Fail /\ ~used
       /\ used' = TRUE
       /\ UNCHANGED <<finished,connected,completed,running,alive,returns,kind,reused>>
Reuse == /\ connected /\ finished /\ ~running /\ ~reused /\ reused' = TRUE
         /\ UNCHANGED <<finished,connected,completed,running,alive,returns,kind,used>>
Next == Finish \/ Disconnect \/ Complete \/ Use \/ Reuse
Spec == Init /\ [][Next]_vars
TypeOK == /\ finished \in BOOLEAN /\ connected \in BOOLEAN /\ completed \in BOOLEAN
          /\ running \in BOOLEAN /\ alive \in BOOLEAN /\ returns \in 0..1
          /\ kind \in 0..3 /\ used \in BOOLEAN /\ reused \in BOOLEAN
ProtectedLifetime == (~Allow /\ ~completed) => running
ContextOwnership == alive = (running \/ (connected /\ completed /\ ~finished /\ ~Fail))
SingleReturn == returns <= 1
NoPrematureReturn == (~Allow /\ ~completed) => returns = 0
=============================================================================
