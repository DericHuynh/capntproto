------------------------- MODULE CapnpPersistence -------------------------
EXTENDS Naturals, FiniteSets, TLC
\* persistent.capnp: save is an ordinary interface call; Bootstrap is NOT
\* Restore. Realm-specific sturdy references survive connection teardown.
\* Ideal sealed-reference/restorer contract, not a prescribed realm format.
CONSTANT AllowUnsealed
Owners == {1, 2}
VARIABLES connection, epoch, live, saved, sealedFor, restored, failure
vars == <<connection, epoch, live, saved, sealedFor, restored, failure>>
Init == /\ connection = TRUE /\ epoch = 1 /\ live = {<<1, 1>>}
        /\ saved = FALSE /\ sealedFor = 0 /\ restored = {} /\ failure = {}
Save(owner) ==
    /\ connection /\ <<1, epoch>> \in live /\ ~saved
    /\ owner \in Owners \/ (owner = 0 /\ AllowUnsealed)
    /\ saved' = TRUE /\ sealedFor' = owner
    /\ UNCHANGED <<connection, epoch, live, restored, failure>>
Disconnect ==
    /\ connection /\ connection' = FALSE /\ live' = {}
    /\ UNCHANGED <<epoch, saved, sealedFor, restored, failure>>
Reconnect ==
    /\ ~connection /\ epoch = 1 /\ connection' = TRUE /\ epoch' = 2
    /\ UNCHANGED <<live, saved, sealedFor, restored, failure>>
Restore(owner) ==
    /\ connection /\ saved /\ epoch = 2 /\ owner \notin restored \cup failure
    /\ IF sealedFor \in {0, owner}
       THEN /\ live' = live \cup {<<owner, epoch>>}
            /\ restored' = restored \cup {owner} /\ UNCHANGED failure
       ELSE /\ failure' = failure \cup {owner} /\ UNCHANGED <<live, restored>>
    /\ UNCHANGED <<connection, epoch, saved, sealedFor>>
Next == (\E owner \in {0} \cup Owners : Save(owner)) \/ Disconnect \/ Reconnect \/
        (\E owner \in Owners : Restore(owner))
Spec == Init /\ [][Next]_vars
Sealing == \A owner \in restored : sealedFor \in {0, owner}
NoTransientResurrection == \A ref \in live : ref[2] = epoch
DisconnectClean == ~connection => live = {}
NoRestoreWitness == restored = {}
=============================================================================
