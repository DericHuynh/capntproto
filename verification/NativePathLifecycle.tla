------------------------- MODULE NativePathLifecycle -------------------------
EXTENDS Naturals
CONSTANT Fault
VARIABLES phase, client, server, clientGen, serverGen, validated, active,
          clientData, serverData, blocked, oldBlocked, lost, forged, replayed, event
vars == <<phase, client, server, clientGen, serverGen, validated, active,
          clientData, serverData, blocked, oldBlocked, lost, forged, replayed, event>>

\* Initially an authenticated two-peer connection has spare connection IDs.
\* One alternate client address is probed. A bounded partition may discard the
\* first probe and a timer-driven retry; at most one forged response batch is
\* injected. Validation settles both directions before migration. Application
\* data then completes with the old path blocked. Both endpoints are replaced,
\* preserving identities and reusing the active CIDs but generating fresh Native
\* ephemerals. Old STREAM ciphertext is replayed before or after fresh data.
\* validated refers only to the old generation's alternate path. Data records
\* completed streams in each current receiver, not exactly-once RPC effects.
\* Flight/transfer actions abstract bounded packet/timer settlement in Rust.
\* Fairness assumes service and delivery after the finite partition. This does
\* not model arbitrary routing, listener restart, crash persistence or crypto.
Init == /\ phase = 0 /\ client = 1 /\ server = 1
        /\ clientGen = 0 /\ serverGen = 0 /\ validated = 0 /\ active = 0
        /\ clientData = 0 /\ serverData = 0 /\ blocked = 0 /\ oldBlocked = 0
        /\ lost = 0 /\ forged = 0 /\ replayed = 0 /\ event = 0

Probe ==
    /\ phase = 0 /\ phase' = 1 /\ event' = 1
    /\ UNCHANGED <<client, server, clientGen, serverGen, validated, active,
                   clientData, serverData, blocked, oldBlocked, lost, forged, replayed>>
PartitionProbe ==
    /\ phase = 1 /\ lost = 0 /\ phase' = 2 /\ lost' = 1 /\ blocked' = 1 /\ event' = 2
    /\ validated' = IF Fault = "timeoutValidates" THEN 1 ELSE validated
    /\ UNCHANGED <<client, server, clientGen, serverGen, active, clientData,
                   serverData, oldBlocked, forged, replayed>>
HealProbe ==
    /\ phase = 2 /\ phase' = 1 /\ blocked' = 0 /\ event' = 3
    /\ UNCHANGED <<client, server, clientGen, serverGen, validated, active,
                   clientData, serverData, oldBlocked, lost, forged, replayed>>
Challenge ==
    /\ phase = 1 /\ phase' = 3 /\ event' = 4
    /\ UNCHANGED <<client, server, clientGen, serverGen, validated, active,
                   clientData, serverData, blocked, oldBlocked, lost, forged, replayed>>
CorruptResponse ==
    /\ phase = 3 /\ forged = 0 /\ forged' = 1 /\ event' = 5
    /\ validated' = IF Fault = "forgedValidates" THEN 1 ELSE validated
    /\ UNCHANGED <<phase, client, server, clientGen, serverGen, active,
                   clientData, serverData, blocked, oldBlocked, lost, replayed>>
Validate ==
    /\ phase = 3 /\ phase' = 4 /\ validated' = 1 /\ event' = 6
    /\ UNCHANGED <<client, server, clientGen, serverGen, active,
                   clientData, serverData, blocked, oldBlocked, lost, forged, replayed>>
MigrateTransfer ==
    /\ phase = 4 /\ phase' = 5 /\ oldBlocked' = 1 /\ event' = 7
    /\ active' = IF Fault = "missingMigration" THEN 0 ELSE 1
    /\ clientData' = 1 /\ serverData' = 1
    /\ UNCHANGED <<client, server, clientGen, serverGen, validated, blocked, lost, forged, replayed>>
RestartServer ==
    /\ phase = 5 /\ phase' = 6 /\ serverGen' = 1 /\ validated' = 0 /\ event' = 8
    /\ server' = IF Fault = "restartKeepsAuth" THEN 1 ELSE 0
    /\ serverData' = IF Fault = "restartCarriesStream" THEN serverData ELSE 0
    /\ UNCHANGED <<client, clientGen, active, clientData, blocked, oldBlocked, lost, forged, replayed>>
ReconnectClient ==
    /\ phase = 6 /\ phase' = 7 /\ clientGen' = 1 /\ client' = 0 /\ clientData' = 0 /\ event' = 9
    /\ UNCHANGED <<server, serverGen, validated, active, serverData, blocked, oldBlocked, lost, forged, replayed>>
FreshHandshake ==
    /\ phase = 7 /\ phase' = 8 /\ client' = 1 /\ server' = 1 /\ event' = 10
    /\ UNCHANGED <<clientGen, serverGen, validated, active, clientData,
                   serverData, blocked, oldBlocked, lost, forged, replayed>>
ReplayOld ==
    /\ phase \in 8..9 /\ replayed = 0 /\ replayed' = 1 /\ event' = 11
    /\ serverData' = IF Fault = "staleDelivers" THEN 2 ELSE serverData
    /\ UNCHANGED <<phase, client, server, clientGen, serverGen, validated, active,
                   clientData, blocked, oldBlocked, lost, forged>>
FreshTransfer ==
    /\ phase = 8 /\ phase' = 9 /\ clientData' = 1 /\ serverData' = 1 /\ event' = 12
    /\ UNCHANGED <<client, server, clientGen, serverGen, validated, active,
                   blocked, oldBlocked, lost, forged, replayed>>

Next == Probe \/ PartitionProbe \/ HealProbe \/ Challenge \/ CorruptResponse \/ Validate
        \/ MigrateTransfer \/ RestartServer \/ ReconnectClient \/ FreshHandshake \/ ReplayOld \/ FreshTransfer
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(Next)
TypeOK == /\ phase \in 0..9 /\ event \in 0..12 /\ clientData \in 0..2 /\ serverData \in 0..2
          /\ <<client, server, clientGen, serverGen, validated, active, blocked,
                oldBlocked, lost, forged, replayed>> \in [1..11 -> 0..1]
ValidationProof == validated = (IF phase \in 4..5 THEN 1 ELSE 0)
AuthenticationState == /\ client = (IF phase = 7 THEN 0 ELSE 1)
                       /\ server = (IF phase \in 6..7 THEN 0 ELSE 1)
StreamGeneration == /\ clientData = (IF phase \in {5, 6, 9} THEN 1 ELSE 0)
                    /\ serverData = (IF phase \in {5, 9} THEN 1 ELSE 0)
ActiveRoute == /\ active = (IF phase >= 5 THEN 1 ELSE 0)
               /\ oldBlocked = active /\ blocked = (IF phase = 2 THEN 1 ELSE 0)
Generations == /\ clientGen = (IF phase >= 7 THEN 1 ELSE 0)
               /\ serverGen = (IF phase >= 6 THEN 1 ELSE 0)
EventuallyTransferred == <> (phase = 9 /\ replayed = 1)
=============================================================================
