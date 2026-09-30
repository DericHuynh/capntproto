------------------------- MODULE RpcTailAdoption -------------------------
EXTENDS Naturals, TLC
CONSTANTS BrokenTransfer, BrokenLateAck
VARIABLES adopted, finished, canceled, completed, running, returns, used
vars == <<adopted, finished, canceled, completed, running, returns, used>>
\* A caller has an outgoing question and is executing the peer's redirected
\* call. Its Results contains a capability. The peer can transfer that result
\* with TakeFromOtherQuestion, then Finish its original answer.
Init == /\ adopted = FALSE /\ finished = FALSE /\ canceled = FALSE
        /\ completed = FALSE /\ running = TRUE /\ returns = 0 /\ used = FALSE
Adopt == /\ ~adopted /\ adopted' = TRUE
         /\ returns' = IF BrokenLateAck /\ canceled THEN returns ELSE 1
         /\ UNCHANGED <<finished, canceled, completed, running, used>>
Finish == /\ adopted /\ ~finished /\ finished' = TRUE
          /\ running' = (running /\ ~canceled /\ ~BrokenTransfer)
          /\ UNCHANGED <<adopted, canceled, completed, returns, used>>
CancelCaller == /\ ~canceled /\ ~used /\ canceled' = TRUE
                /\ running' = (running /\ ~finished)
                /\ UNCHANGED <<adopted, finished, completed, returns, used>>
\* A canceled adopter no longer polls the producer; the old pipeline retains
\* its future until Finish but this scenario issues no more pipeline calls.
Complete == /\ running /\ (~adopted \/ ~canceled) /\ running' = FALSE /\ completed' = TRUE /\ returns' = 1
            /\ UNCHANGED <<adopted, finished, canceled, used>>
UseCapability == /\ adopted /\ completed /\ ~canceled /\ ~used /\ used' = TRUE
                 /\ UNCHANGED <<adopted, finished, canceled, completed, running, returns>>
Next == Adopt \/ Finish \/ CancelCaller \/ Complete \/ UseCapability
Spec == Init /\ [][Next]_vars
TypeOK == /\ adopted \in BOOLEAN /\ finished \in BOOLEAN /\ canceled \in BOOLEAN
          /\ completed \in BOOLEAN /\ running \in BOOLEAN /\ used \in BOOLEAN
          /\ returns \in 0..1
AdoptedTaskLives == (adopted /\ ~canceled /\ ~completed) => running
RedirectAcknowledged == adopted => returns = 1
CancellationReleasesTask == (finished /\ canceled) => ~running
CapabilityProvenance == used => (adopted /\ completed /\ ~canceled)
=============================================================================
