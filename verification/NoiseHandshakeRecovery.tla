----------------------- MODULE NoiseHandshakeRecovery -----------------------
EXTENDS Naturals
CONSTANT Fault
VARIABLES phase, client, server, verified, serverKeys, initialLost, replyLost,
          initialRetried, replyRetried, event
vars == <<phase, client, server, verified, serverKeys, initialLost, replyLost,
          initialRetried, replyRetried, event>>

\* One two-party IK handshake. At most one whole initiator flight and one
\* whole responder flight are lost. Retry abstracts bounded local PTO handling
\* until the relevant Initial is emitted; Rust drives actual recovery timers.
\* Delivery is an atomic flight action. Confirm settles HANDSHAKE_DONE recovery
\* and the client confirmation flight, with no further loss in this model.
\* Fairness assumes eventual timer service
\* and delivery after the finite loss budget. This is not arbitrary-network or
\* cryptographic verification, and does not model application retry semantics.
Init == /\ phase = 0 /\ client = 0 /\ server = 0 /\ verified = 0
        /\ serverKeys = 0 /\ initialLost = 0 /\ replyLost = 0
        /\ initialRetried = 0 /\ replyRetried = 0 /\ event = 0

DeliverInitial ==
    /\ phase = 0 /\ (initialLost = 0 \/ initialRetried = 1)
    /\ phase' = 1 /\ server' = 1 /\ serverKeys' = 1 /\ event' = 1
    /\ UNCHANGED <<client, verified, initialLost, replyLost, initialRetried, replyRetried>>
DropInitial ==
    /\ phase = 0 /\ initialLost = 0 /\ initialLost' = 1 /\ event' = 2
    /\ server' = IF Fault = "initialAuthenticates" THEN 1 ELSE server
    /\ UNCHANGED <<phase, client, verified, serverKeys, replyLost, initialRetried, replyRetried>>
RetryInitial ==
    /\ phase = 0 /\ initialLost = 1 /\ initialRetried = 0
    /\ initialRetried' = 1 /\ event' = 3
    /\ UNCHANGED <<phase, client, server, verified, serverKeys, initialLost, replyLost, replyRetried>>
DeliverReply ==
    /\ phase = 1 /\ (replyLost = 0 \/ replyRetried = 1)
    /\ phase' = 2 /\ client' = 1 /\ event' = 6
    /\ UNCHANGED <<server, verified, serverKeys, initialLost, replyLost, initialRetried, replyRetried>>
DropReply ==
    /\ phase = 1 /\ replyLost = 0 /\ replyLost' = 1 /\ event' = 4
    /\ client' = IF Fault = "replyAuthenticates" THEN 1 ELSE client
    /\ serverKeys' = IF Fault = "dropReplyKeys" THEN 0 ELSE serverKeys
    /\ UNCHANGED <<phase, server, verified, initialLost, initialRetried, replyRetried>>
RetryReply ==
    /\ phase = 1 /\ replyLost = 1 /\ replyRetried = 0
    /\ replyRetried' = 1 /\ event' = 5
    /\ verified' = IF Fault = "retryVerifies" THEN 1 ELSE verified
    /\ UNCHANGED <<phase, client, server, serverKeys, initialLost, replyLost, initialRetried>>
Confirm ==
    /\ phase = 2 /\ phase' = 3 /\ verified' = 1 /\ serverKeys' = 0 /\ event' = 7
    /\ UNCHANGED <<client, server, initialLost, replyLost, initialRetried, replyRetried>>
Next == DeliverInitial \/ DropInitial \/ RetryInitial \/ DeliverReply \/ DropReply \/ RetryReply \/ Confirm
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(Next)
TypeOK == /\ phase \in 0..3 /\ event \in 0..7
          /\ <<client, server, verified, serverKeys, initialLost, replyLost,
                initialRetried, replyRetried>> \in [1..8 -> 0..1]
DeliveredAuthentication == /\ client = (IF phase >= 2 THEN 1 ELSE 0)
                           /\ server = (IF phase >= 1 THEN 1 ELSE 0)
RetainForRecovery == serverKeys = (IF phase \in 1..2 THEN 1 ELSE 0)
ConfirmedAddress == verified = (IF phase = 3 THEN 1 ELSE 0)
EventuallyConfirmed == <> (phase = 3)
=============================================================================
