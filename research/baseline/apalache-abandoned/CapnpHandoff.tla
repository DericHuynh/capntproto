------------------------- MODULE CapnpHandoff -------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC, CapnpComponentTypes
\* Provider 2, host 3, recipients 1 (and optionally 4 through forwarding).
\* Unique completion/embargo tokens stand for authenticated VatNetwork values.
CONSTANTS
    \* @type: Set(Int);
    Recipients,
    \* @type: Str;
    Bug,
    \* @type: Bool;
    AllowFailure,
    \* @type: Bool;
    UseFallback
Vats == {1, 2, 3, 4}
Channels == {c \in Vats \X Vats : c[1] # c[2]}
\* @type: (Str, Int) => $handoffMessage;
Msg(kind, r) == [kind |-> kind, recipient |-> r]
VARIABLES
    \* @type: $pair -> Seq($handoffMessage);
    wire,
    \* @type: Int -> Str;
    phase,
    \* @type: Bool;
    provided,
    \* @type: Bool;
    provideSent,
    \* @type: Bool;
    finishSent,
    \* @type: Set(Int);
    vine,
    \* @type: Set(Int);
    acceptSeen,
    \* @type: Set(Int);
    barriers,
    \* @type: Set(Int);
    returned,
    \* @type: Set(Int);
    oldSent,
    \* @type: Set(Int);
    newSent,
    \* @type: Set(Int);
    pending,
    \* @type: Int -> Seq(Int);
    deliveries,
    \* @type: Bool;
    failed,
    \* @type: Set(Int);
    authenticated
\* @type: <<$pair -> Seq($handoffMessage), Int -> Str, Bool, Bool, Bool, Set(Int), Set(Int), Set(Int), Set(Int), Set(Int), Set(Int), Set(Int), Int -> Seq(Int), Bool, Set(Int)>>;
vars == <<wire, phase, provided, provideSent, finishSent, vine,
          acceptSeen, barriers, returned, oldSent, newSent, pending,
          deliveries, failed, authenticated>>
Init == /\ wire = [c \in Channels |-> <<>>]
        /\ phase = [r \in Recipients |-> "proxy"]
        /\ provided = FALSE /\ provideSent = FALSE /\ finishSent = FALSE
        /\ vine = Recipients /\ acceptSeen = {} /\ barriers = {} /\ returned = {}
        /\ oldSent = {} /\ newSent = {} /\ pending = {}
        /\ deliveries = [r \in Recipients |-> <<>>]
        /\ failed = FALSE /\ authenticated = {}
SendOld(r) ==
    /\ ~failed /\ r \notin oldSent /\ phase[r] = "proxy"
    /\ oldSent' = oldSent \cup {r}
    /\ wire' = [wire EXCEPT ![<<r, 2>>] = Append(@, Msg("old", r))]
    /\ UNCHANGED <<phase, provided, provideSent, finishSent, vine, acceptSeen,
                   barriers, returned, newSent, pending, deliveries, failed, authenticated>>
Introduce ==
    /\ ~failed /\ ~provideSent /\ oldSent = Recipients
    /\ provideSent' = TRUE
    /\ wire' = [c \in Channels |->
           IF c = <<2, 3>> THEN Append(wire[c], Msg("Provide", 1))
           ELSE IF c[1] = 2 /\ c[2] \in Recipients
                THEN Append(wire[c], Msg("thirdPartyHosted", c[2])) ELSE wire[c]]
    /\ UNCHANGED <<phase, provided, finishSent, vine, acceptSeen, barriers,
                   returned, oldSent, newSent, pending, deliveries, failed, authenticated>>
Accept(r) ==
    /\ ~failed /\ ~UseFallback /\ phase[r] = "introduced"
    /\ phase' = [phase EXCEPT ![r] = "accepting"]
    /\ authenticated' = authenticated \cup {r}
    /\ wire' = [wire EXCEPT ![<<r, 3>>] = Append(@, Msg("Accept", r)),
                              ![<<r, 2>>] = Append(@, Msg("Disembargo", r))]
    /\ UNCHANGED <<provided, provideSent, finishSent, vine, acceptSeen, barriers,
                   returned, oldSent, newSent, pending, deliveries, failed>>
\* Level 1/2 receivers may call the descriptor's vine. Receiving that Call
\* closes Provide, while the vine continues to proxy to the target.
Fallback(r) ==
    /\ ~failed /\ UseFallback /\ Recipients = {r} /\ phase[r] = "introduced"
    /\ phase' = [phase EXCEPT ![r] = "fallback"]
    /\ newSent' = newSent \cup {r}
    /\ wire' = [wire EXCEPT ![<<r, 2>>] = Append(@, Msg("newVine", r))]
    /\ UNCHANGED <<provided, provideSent, finishSent, vine, acceptSeen, barriers,
                   returned, oldSent, pending, deliveries, failed, authenticated>>
SendNew(r) ==
    /\ ~failed /\ phase[r] \in {"accepting", "accepted"} /\ r \notin newSent
    /\ newSent' = newSent \cup {r}
    /\ wire' = [wire EXCEPT ![<<r, 3>>] = Append(@, Msg("pipeline", r))]
    /\ UNCHANGED <<phase, provided, provideSent, finishSent, vine, acceptSeen,
                   barriers, returned, oldSent, pending, deliveries, failed, authenticated>>

\* All messages on a directed connection share one FIFO. At host 3, completion
\* may arrive before Provide, and Disembargo may arrive before Accept.
\* @type: $pair => Bool;
Receive(c) ==
    /\ ~failed /\ Len(wire[c]) > 0
    /\ LET m == Head(wire[c]) r == m.recipient
       IN /\ wire' = IF c[2] = 2 /\ m.kind = "newVine"
                     THEN [wire EXCEPT ![c] = Tail(@),
                            ![<<2, 3>>] = Append(Append(@, m), Msg("FinishProvide", r))]
                     ELSE IF c[2] = 2 /\ m.kind \in {"old", "Disembargo"}
                     THEN [wire EXCEPT ![c] = Tail(@), ![<<2, 3>>] = Append(@, m)]
                     ELSE [wire EXCEPT ![c] = Tail(@)]
          /\ finishSent' = (finishSent \/ (c[2] = 2 /\ m.kind = "newVine"))
          /\ provided' = IF m.kind = "Provide" THEN TRUE
                          ELSE IF m.kind = "FinishProvide" THEN FALSE ELSE provided
          /\ phase' = IF m.kind = "thirdPartyHosted" THEN [phase EXCEPT ![r] = "introduced"]
                       ELSE IF m.kind = "ReturnAccept" THEN [phase EXCEPT ![r] = "accepted"] ELSE phase
          /\ acceptSeen' = IF m.kind = "Accept" THEN acceptSeen \cup {r} ELSE acceptSeen
          /\ barriers' = IF c[2] = 3 /\ m.kind = "Disembargo"
                         THEN barriers \cup {r} ELSE barriers
          /\ pending' = IF m.kind = "pipeline" THEN pending \cup {r} ELSE pending
          /\ deliveries' = IF c[2] = 3 /\ m.kind = "old"
                            THEN [deliveries EXCEPT ![r] = Append(@, 1)]
                            ELSE IF c[2] = 3 /\ m.kind = "newVine"
                            THEN [deliveries EXCEPT ![r] = Append(@, 2)] ELSE deliveries
          /\ vine' = IF m.kind = "ReleaseVine" THEN vine \ {r} ELSE vine
    /\ UNCHANGED <<provideSent, returned, oldSent, newSent, failed, authenticated>>
Ready(r) == (provided \/ r \in returned) /\ r \in acceptSeen /\
            (r \in barriers \/ Bug = "skipEmbargo") /\ r \in authenticated
ReturnAccept(r) ==
    /\ ~failed /\ Ready(r) /\ r \notin returned
    /\ returned' = returned \cup {r}
    /\ wire' = [wire EXCEPT ![<<3, r>>] = Append(@, Msg("ReturnAccept", r))]
    /\ UNCHANGED <<phase, provided, provideSent, finishSent, vine, acceptSeen,
                   barriers, oldSent, newSent, pending, deliveries, failed, authenticated>>
DeliverNew(r) ==
    /\ ~failed /\ Ready(r) /\ r \in pending
    /\ pending' = pending \ {r}
    /\ deliveries' = [deliveries EXCEPT ![r] = Append(@, 2)]
    /\ UNCHANGED <<wire, phase, provided, provideSent, finishSent, vine, acceptSeen,
                   barriers, returned, oldSent, newSent, failed, authenticated>>
ReleaseVine(r) ==
    /\ ~failed /\ phase[r] = "accepted"
    /\ phase' = [phase EXCEPT ![r] = "released"]
    /\ wire' = [wire EXCEPT ![<<r, 2>>] = Append(@, Msg("ReleaseVine", r))]
    /\ UNCHANGED <<provided, provideSent, finishSent, vine, acceptSeen, barriers,
                   returned, oldSent, newSent, pending, deliveries, failed, authenticated>>
FinishProvide ==
    /\ ~failed /\ provideSent /\ ~finishSent /\ vine = {}
    /\ finishSent' = TRUE
    /\ wire' = [wire EXCEPT ![<<2, 3>>] = Append(@, Msg("FinishProvide", 1))]
    /\ UNCHANGED <<phase, provided, provideSent, vine, acceptSeen, barriers,
                   returned, oldSent, newSent, pending, deliveries, failed, authenticated>>
Fail == /\ AllowFailure /\ ~failed /\ failed' = TRUE
        /\ wire' = [c \in Channels |-> <<>>] /\ provided' = FALSE /\ vine' = {}
        /\ pending' = {} /\ phase' = [r \in Recipients |-> "broken"]
        /\ UNCHANGED <<provideSent, finishSent, acceptSeen, barriers, returned,
                       oldSent, newSent, deliveries, authenticated>>
Next == \/ Introduce \/ FinishProvide \/ Fail
        \/ \E c \in Channels : Receive(c)
        \/ \E r \in Recipients : SendOld(r) \/ Accept(r) \/ Fallback(r) \/ SendNew(r) \/
                                  ReturnAccept(r) \/ DeliverNew(r) \/ ReleaseVine(r)
Spec == Init /\ [][Next]_vars
\* Fairness here includes a finite application workload and eventual vine release.
LiveSpec == Spec /\ WF_vars(Introduce) /\ WF_vars(FinishProvide)
                 /\ (\A c \in Channels : WF_vars(Receive(c)))
                 /\ (\A r \in Recipients :
                       /\ WF_vars(SendOld(r)) /\ WF_vars(Accept(r))
                       /\ WF_vars(ReturnAccept(r)) /\ WF_vars(DeliverNew(r))
                       /\ WF_vars(ReleaseVine(r)))
EveryAcceptSettles == provideSent ~>
    (failed \/ (\A r \in Recipients : phase[r] \in {"accepted", "released"}))
EveryNewDelivered == \A r \in Recipients : r \in newSent ~>
    (failed \/ (2 \in {deliveries[r][i] : i \in 1..Len(deliveries[r])}))
EventuallyReleased == <>[](failed \/
    (finishSent /\ ~provided /\ (\A c \in Channels : wire[c] = <<>>)))
TypeOK == /\ phase \in [Recipients -> {"proxy", "introduced", "accepting", "accepted", "released", "broken", "fallback"}]
          /\ vine \subseteq Recipients /\ acceptSeen \subseteq Recipients
          /\ barriers \subseteq Recipients /\ returned \subseteq Recipients
          /\ pending \subseteq Recipients
          /\ deliveries \in [Recipients -> Seq({1, 2})]
EOrder == \A r \in Recipients : deliveries[r] \in {<<>>, <<1>>, <<1, 2>>}
EmbargoSafety == returned \subseteq barriers
AuthenticatedAccept == returned \subseteq authenticated
VineLifetime == ~failed => \A r \in Recipients :
    phase[r] \in {"proxy", "introduced", "accepting"} => r \in vine
NoPrematureFinish == finishSent => (failed \/ returned = Recipients \/
                        (\A r \in Recipients : phase[r] = "fallback"))
NoVineFallbackWitness == ~\E r \in Recipients :
    phase[r] = "fallback" /\ finishSent /\ ~provided /\ deliveries[r] = <<1, 2>>
NoHandoffWitness == ~\E r \in Recipients : phase[r] = "released" /\ deliveries[r] = <<1, 2>>
NoAcceptBeforeProvideWitness == provided \/ acceptSeen = {}
=============================================================================
