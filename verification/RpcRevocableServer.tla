----------------------- MODULE RpcRevocableServer -----------------------
EXTENDS Naturals, TLC
CONSTANTS Allow, Fail, IgnoreProtected, RevokeReturned
VARIABLES completed, running, revoked, caller, alive, server, outcome, owner, probed, used, retried
vars == <<completed,running,revoked,caller,alive,server,outcome,owner,probed,used,retried>>
\* A method has been dispatched and allocated a result capability.
\* For FD-backed local gates, fresh FD availability projects to server.
\* A descriptor obtained before the trace remains owned by the test client.
\* outcome: 0 no observation, 1 results, 2 revocation, 3 application failure.
Init == /\ completed = FALSE /\ running = TRUE /\ revoked = FALSE /\ caller = TRUE
        /\ alive = TRUE /\ server = TRUE /\ outcome = 0 /\ owner = TRUE
        /\ probed = FALSE /\ used = FALSE /\ retried = FALSE
DropCaller == /\ caller /\ caller' = FALSE
              /\ running' = (running /\ ~Allow)
              /\ alive' = running'
              /\ UNCHANGED <<completed,revoked,server,outcome,owner,probed,used,retried>>
Complete == /\ running /\ completed' = TRUE /\ running' = FALSE
            /\ alive' = (caller /\ ~Fail)
            /\ outcome' = IF caller THEN IF Fail THEN 3 ELSE 1 ELSE outcome
            /\ UNCHANGED <<revoked,caller,server,owner,probed,used,retried>>
Cancel == /\ revoked' = TRUE /\ server' = FALSE
          /\ running' = (running /\ IgnoreProtected /\ ~Allow)
          /\ alive' = (running' \/ (completed /\ caller /\ ~Fail /\ ~RevokeReturned))
          /\ outcome' = IF caller /\ ~completed /\ ~running' THEN 2 ELSE outcome
Revoke == /\ owner /\ ~revoked /\ Cancel
          /\ UNCHANGED <<completed,caller,owner,probed,used,retried>>
DropOwner == /\ owner /\ owner' = FALSE /\ Cancel
             /\ UNCHANGED <<completed,caller,probed,used,retried>>
Probe == /\ revoked /\ ~probed /\ probed' = TRUE
         /\ UNCHANGED <<completed,running,revoked,caller,alive,server,outcome,owner,used,retried>>
Retry == /\ revoked /\ owner /\ ~retried /\ retried' = TRUE
         /\ UNCHANGED <<completed,running,revoked,caller,alive,server,outcome,owner,probed,used>>
Use == /\ completed /\ caller /\ ~Fail /\ ~used /\ used' = TRUE
       /\ UNCHANGED <<completed,running,revoked,caller,alive,server,outcome,owner,probed,retried>>
Next == DropCaller \/ Complete \/ Revoke \/ DropOwner \/ Probe \/ Retry \/ Use
Spec == Init /\ [][Next]_vars
TypeOK == /\ completed \in BOOLEAN /\ running \in BOOLEAN /\ revoked \in BOOLEAN
          /\ caller \in BOOLEAN /\ alive \in BOOLEAN /\ server \in BOOLEAN
          /\ outcome \in 0..3 /\ owner \in BOOLEAN /\ probed \in BOOLEAN
          /\ used \in BOOLEAN /\ retried \in BOOLEAN
RevokedTask == revoked => ~running
ServerOwnership == server = ~revoked
CapabilityOwnership == alive = (running \/ (completed /\ caller /\ ~Fail))
NoFalseCompletion == (outcome = 1) => completed
=============================================================================
