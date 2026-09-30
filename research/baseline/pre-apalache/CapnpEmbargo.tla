-------------------------- MODULE CapnpEmbargo --------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC
\* P at vat 2 resolves to Q at vat 3; Q resolves to an object at vat 4.
\* A disembargo addressed to P MUST rendezvous at Q, not Q's new target.
\* This isolates the immutable forwarding edge in the Tribble four-way race.
CONSTANT Bug, Loopback
Host == IF Loopback THEN 1 ELSE 4
Vats == 1..4
Channels == {c \in Vats \X Vats : c[1] # c[2]}
VARIABLES wire, resolvedP, resolvedQ, pinned, notified, barrierSent,
          barrierSeen, sent, deliveries, faults
vars == <<wire, resolvedP, resolvedQ, pinned, notified, barrierSent,
          barrierSeen, sent, deliveries, faults>>
Init == /\ wire = [c \in Channels |-> <<>>]
        /\ resolvedP = FALSE /\ resolvedQ = FALSE /\ pinned = 0
        /\ notified = FALSE /\ barrierSent = FALSE /\ barrierSeen = FALSE
        /\ sent = 0 /\ deliveries = <<>> /\ faults = {}
SendOld == /\ sent = 0
           /\ wire' = [wire EXCEPT ![<<1, 2>>] = Append(@, "old")]
           /\ sent' = 1
           /\ UNCHANGED <<resolvedP, resolvedQ, pinned, notified, barrierSent, barrierSeen, deliveries, faults>>
ResolveP == /\ ~resolvedP /\ sent = 1
            /\ resolvedP' = TRUE /\ pinned' = 3
            /\ wire' = [wire EXCEPT ![<<2, 1>>] = Append(@, "Resolve")]
            /\ UNCHANGED <<resolvedQ, notified, barrierSent, barrierSeen, sent, deliveries, faults>>
ResolveQ == /\ ~resolvedQ /\ resolvedP
            /\ resolvedQ' = TRUE
            /\ pinned' = IF Bug = "shortenTwice" THEN Host ELSE pinned
            /\ UNCHANGED <<wire, resolvedP, notified, barrierSent, barrierSeen, sent, deliveries, faults>>
SendBarrier == /\ notified /\ ~barrierSent
               /\ barrierSent' = TRUE
               /\ wire' = [wire EXCEPT ![<<1, 2>>] = Append(@, "Disembargo")]
               /\ UNCHANGED <<resolvedP, resolvedQ, pinned, notified, barrierSeen, sent, deliveries, faults>>
SendNew == /\ barrierSeen /\ sent = 1
           /\ sent' = 2
           /\ wire' = [wire EXCEPT ![<<1, 3>>] = Append(@, "new")]
           /\ UNCHANGED <<resolvedP, resolvedQ, pinned, notified, barrierSent, barrierSeen, deliveries, faults>>
Receive(c) ==
    /\ Len(wire[c]) > 0
    /\ (c[2] = 2 => resolvedP)
    /\ (c[2] = 3 /\ Head(wire[c]) \in {"old", "new"} => resolvedQ)
    /\ LET m == Head(wire[c])
           atBoundary == c = <<2, 3>> /\ m = "Disembargo"
           dst == IF c[2] = 2 THEN pinned ELSE IF atBoundary THEN 1 ELSE Host
           forward == c[2] = 2 \/ (c[2] = 3 /\ m \in {"old", "new", "Disembargo"})
       IN /\ wire' = IF forward
                     THEN [wire EXCEPT ![c] = Tail(@),
                            ![<<c[2], dst>>] = Append(@, IF atBoundary THEN "ack" ELSE m)]
                     ELSE [wire EXCEPT ![c] = Tail(@)]
          /\ notified' = (notified \/ m = "Resolve")
          /\ barrierSeen' = (barrierSeen \/ m = "ack")
          /\ deliveries' = IF c[2] = Host /\ m \in {"old", "new"}
                            THEN Append(deliveries, m) ELSE deliveries
          /\ faults' = IF m = "Disembargo" /\ c[2] = Host
                       THEN faults \cup {"wrongRendezvous"} ELSE faults
    /\ UNCHANGED <<resolvedP, resolvedQ, pinned, barrierSent, sent>>
Next == SendOld \/ ResolveP \/ ResolveQ \/ SendBarrier \/ SendNew \/ (\E c \in Channels : Receive(c))
Spec == Init /\ [][Next]_vars
PinnedForwarding == resolvedP => pinned = 3
EOrder == deliveries \in {<<>>, <<"old">>, <<"old", "new">>}
ProtocolSafety == faults = {}
NoEmbargoWitness == deliveries # <<"old", "new">>
=============================================================================
