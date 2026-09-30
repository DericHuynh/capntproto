--------------------- MODULE NoiseHandshakeConfirmation ---------------------
EXTENDS Naturals
CONSTANT Fault
VARIABLES phase, client, server, clientKeys, serverKeys, verified, otherVerified,
          forged, delayed, replayed, initialReplay, origin, event,
          confirmed, data, received
vars == <<phase, client, server, clientKeys, serverKeys, verified, otherVerified,
          forged, delayed, replayed, initialReplay, origin, event,
          confirmed, data, received>>

\* One IK handshake, two address tuples, and at most one corruption, delay,
\* confirmation replay and Initial replay. HANDSHAKE_DONE may arrive separately
\* from the IK reply; a single stream message can confirm the server's original
\* address before the client receives HANDSHAKE_DONE. Cryptography is abstracted as an
\* authentication predicate. Individual protected packets are scheduled;
\* coalescing, timers, congestion windows and cryptographic secrecy are outside
\* this model. Rust replay uses real Snow handshakes and protected packets.
Init == /\ phase = 0 /\ client = 0 /\ server = 0
        /\ clientKeys = 1 /\ serverKeys = 0
        /\ verified = 0 /\ otherVerified = 0
        /\ forged = 0 /\ delayed = 0 /\ replayed = 0
        /\ initialReplay = 0 /\ origin = 0 /\ event = 0
        /\ confirmed = 0 /\ data = 0 /\ received = 0

Initiator == /\ phase = 0 /\ phase' = 1 /\ event' = 1 /\ server' = 1
             /\ serverKeys' = IF Fault = "earlyRetire" THEN 0 ELSE 1
             /\ UNCHANGED <<client, clientKeys, verified, otherVerified,
                            forged, delayed, replayed, initialReplay, origin,
                            confirmed, data, received>>
Reply(done) ==
    /\ phase = 1
    /\ phase' = 2
    /\ client' = 1
    /\ confirmed' = done
    /\ event' = IF done = 1 THEN 2 ELSE 10
    /\ UNCHANGED <<server, clientKeys, serverKeys, verified, otherVerified,
                   forged, delayed, replayed, initialReplay, origin, data, received>>
HandshakeDone == /\ phase >= 2 /\ confirmed = 0 /\ confirmed' = 1 /\ event' = 11
                 /\ UNCHANGED <<phase, client, server, clientKeys, serverKeys,
                                verified, otherVerified, forged, delayed, replayed,
                                initialReplay, origin, data, received>>
Confirm(payload) ==
    /\ phase = 2
    /\ phase' = 3
    /\ (payload = 1 \/ confirmed = 1)
    /\ data' = payload
    /\ event' = IF payload = 1 THEN 12 ELSE 3
    /\ clientKeys' = IF Fault = "clientRetain" THEN 1 ELSE 0
    /\ UNCHANGED <<client, server, serverKeys, verified, otherVerified,
                   forged, delayed, replayed, initialReplay, origin, confirmed, received>>
Corrupt == /\ phase = 3 /\ forged = 0 /\ forged' = 1 /\ event' = 4
           /\ verified' = IF Fault = "forgedVerify" THEN 1 ELSE verified
           /\ received' = IF Fault = "forgedData" THEN data ELSE received
           /\ UNCHANGED <<phase, client, server, clientKeys, serverKeys,
                          otherVerified, delayed, replayed, initialReplay, origin, confirmed, data>>
Accept(where) == /\ phase = 3 /\ phase' = 4 /\ origin' = where
                 /\ event' = IF where = 1 THEN 5 ELSE 6
                 /\ serverKeys' = IF Fault = "noRetire" THEN 1 ELSE 0
                 /\ verified' = IF where = 1 \/ Fault = "migratedVerify" THEN 1 ELSE 0
                 /\ received' = data
                 /\ UNCHANGED <<client, server, clientKeys, otherVerified,
                                forged, delayed, replayed, initialReplay, confirmed, data>>
Replay == /\ phase = 4 /\ replayed = 0 /\ replayed' = 1 /\ event' = 7
          /\ received' = IF Fault = "duplicateData" THEN received + data ELSE received
          /\ UNCHANGED <<phase, client, server, clientKeys, serverKeys,
                         verified, otherVerified, forged, delayed, initialReplay, origin, confirmed, data>>
Delay == /\ phase = 1 /\ delayed = 0 /\ delayed' = 1 /\ event' = 8
         /\ UNCHANGED <<phase, client, server, clientKeys, serverKeys,
                        verified, otherVerified, forged, replayed, initialReplay, origin, confirmed, data, received>>
ReplayInitial == /\ phase > 0 /\ initialReplay = 0 /\ initialReplay' = 1 /\ event' = 9
                 /\ UNCHANGED <<phase, client, server, clientKeys, serverKeys,
                                verified, otherVerified, forged, delayed, replayed, origin, confirmed, data, received>>
Next == Initiator \/ Reply(0) \/ Reply(1) \/ Confirm(0) \/ Confirm(1)
        \/ Corrupt \/ Accept(1) \/ Accept(2) \/ Replay \/ Delay \/ ReplayInitial \/ HandshakeDone
Spec == Init /\ [][Next]_vars
TypeOK == /\ phase \in 0..4 /\ event \in 0..12 /\ origin \in 0..2
          /\ <<client, server, clientKeys, serverKeys, verified, otherVerified,
                forged, delayed, replayed, initialReplay, confirmed, data>> \in [1..12 -> 0..1]
          /\ received \in 0..2
RetainUntilProof == phase \in 1..3 => serverKeys = 1
RetireAfterProof == phase = 4 => serverKeys = 0
ClientRetires == phase >= 3 => clientKeys = 0
OriginalAddressOnly == verified = 1 => phase = 4 /\ origin = 1
MigrationNeedsValidation == otherVerified = 0
Established == client = (IF phase >= 2 THEN 1 ELSE 0)
               /\ server = (IF phase >= 1 THEN 1 ELSE 0)
ConfirmationAfterReply == confirmed = 1 => client = 1
AuthenticatedOnce == received = (IF phase = 4 THEN data ELSE 0)
=============================================================================
