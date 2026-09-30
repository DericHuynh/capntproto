-------------------------- MODULE RpcIgnoreResult --------------------------
EXTENDS Naturals
CONSTANT Fault
\* One ordinary call, one result capability, both static cancellation policies.
\* Each action includes draining the executor and polling the retained completion
\* promise. Return success/failure is application completion, not early Results
\* release or pipeline publication. Transport ordering and arbitrary schedules
\* are outside this finite lifecycle abstraction.
VARIABLES started, cancellable, held, running, completed, outcome, alive,
          results, published, event
vars == <<started,cancellable,held,running,completed,outcome,alive,results,published,event>>
Init == /\ started=0 /\ cancellable=0 /\ held=0 /\ running=0
        /\ completed=0 /\ outcome=0 /\ alive=0 /\ results=0 /\ published=0 /\ event=0
Start(c) == /\ started=0 /\ started'=1 /\ cancellable'=c
            /\ held'=1 /\ running'=1 /\ alive'=1 /\ results'=1 /\ event'=1+c
            /\ UNCHANGED <<completed,outcome,published>>
Publish == /\ running=1 /\ results=1 /\ published=0
           /\ published'=1 /\ event'=3
           /\ outcome'=IF Fault="earlySuccess" THEN 1 ELSE outcome
           /\ UNCHANGED <<started,cancellable,held,running,completed,alive,results>>
DropResults == /\ running=1 /\ results=1 /\ results'=0 /\ event'=4
               /\ alive'=IF Fault="earlyRelease" THEN 0 ELSE alive
               /\ UNCHANGED <<started,cancellable,held,running,completed,outcome,published>>
Complete(value) == /\ running=1 /\ completed'=value /\ running'=0 /\ results'=0
                   /\ alive'=IF Fault="leakResult" THEN 1 ELSE 0
                   /\ outcome'=IF held=0 THEN 0 ELSE
                         IF Fault="swallowFailure" THEN 1 ELSE value
                   /\ held'=0 /\ event'=4+value
                   /\ UNCHANGED <<started,cancellable,published>>
Cancel == /\ held=1 /\ held'=0 /\ event'=7
          /\ running'=IF cancellable=1 THEN
                         IF Fault="ignoreCancellation" THEN 1 ELSE 0
                       ELSE IF Fault="cancelProtected" THEN 0 ELSE 1
          /\ alive'=running' /\ results'=IF running'=0 THEN 0 ELSE results
          /\ UNCHANGED <<started,cancellable,completed,outcome,published>>
Next == Start(0) \/ Start(1) \/ Publish \/ DropResults \/ Complete(1) \/ Complete(2) \/ Cancel
Spec == Init /\ [][Next]_vars
TypeOK == /\ started\in 0..1 /\ cancellable\in 0..1 /\ held\in 0..1
          /\ running\in 0..1 /\ completed\in 0..2 /\ outcome\in 0..2
          /\ alive\in 0..1 /\ results\in 0..1 /\ published\in 0..1 /\ event\in 0..7
CompletionObserved == outcome#0 => completed=outcome
CapabilityLifetime == alive=running
CancellationAllowed == event=7 /\ cancellable=1 => running=0
ProtectedCompletion == event=7 /\ cancellable=0 => running=1
=============================================================================
