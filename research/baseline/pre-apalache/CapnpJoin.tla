---------------------------- MODULE CapnpJoin ----------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC
\* One join with NParts independently routed key shares. A JoinResult's local
\* ordinal distinguishes objects on the SAME vat; vat identity alone is unsafe.
CONSTANTS NParts, Scenario, Bug, AllowCancel
Parts == 1..NParts
Vats == 1..6
Channels == {c \in Vats \X Vats : c[1] # c[2]}
Relay(p) == IF p = 1 THEN 2 ELSE 3
Root(p) == IF Scenario = "differentHost" /\ p = NParts THEN <<5, 1>>
           ELSE IF Scenario = "differentObject" /\ p = NParts THEN <<4, 2>>
           ELSE IF Scenario = "opaque" /\ p = NParts THEN <<3, 1>>
           ELSE <<4, 1>>
Roots == {Root(p) : p \in Parts}
Msg(kind, part, host, ordinal) ==
    [kind |-> kind, part |-> part, host |-> host, ordinal |-> ordinal]
EmptyResult == [host |-> 0, ordinal |-> 0]
VARIABLES wire, sent, groups, knowledge, results, status, joined,
          finishSent, relayReleased, rootReleased, acceptSent, acceptSeen
vars == <<wire, sent, groups, knowledge, results, status, joined,
          finishSent, relayReleased, rootReleased, acceptSent, acceptSeen>>
Init == /\ wire = [c \in Channels |-> <<>>] /\ sent = {}
        /\ groups = [root \in Roots |-> {}]
        /\ knowledge = [v \in Vats |-> IF v = 1 THEN Parts ELSE {}]
        /\ results = [p \in Parts |-> EmptyResult]
        /\ status = "collecting" /\ joined = <<0, 0>>
        /\ finishSent = {} /\ relayReleased = {} /\ rootReleased = {}
        /\ acceptSent = FALSE /\ acceptSeen = FALSE
SendJoin(p) ==
    /\ status = "collecting" /\ p \notin sent
    /\ sent' = sent \cup {p}
    /\ wire' = [wire EXCEPT ![<<1, Relay(p)>>] = Append(@, Msg("Join", p, 0, 0))]
    /\ UNCHANGED <<groups, knowledge, results, status, joined, finishSent,
                   relayReleased, rootReleased, acceptSent, acceptSeen>>

Receive(c) ==
    /\ Len(wire[c]) > 0
    /\ LET m == Head(wire[c]) p == m.part root == Root(p)
           terminal == c[2] = root[1]
           gotRoot == m.kind = "Join" /\ terminal
           response == Msg("Return", p, root[1], Cardinality(groups[root]))
           nextChannel == IF m.kind = "Join"
                          THEN IF terminal THEN <<c[2], c[1]>> ELSE <<c[2], root[1]>>
                          ELSE IF m.kind = "Return" THEN <<c[2], 1>>
                          ELSE <<c[2], root[1]>>
           forward == (m.kind = "Join") \/
                      (m.kind = "Return" /\ c[2] # 1) \/
                      (m.kind = "Finish" /\ ~terminal)
       IN /\ wire' = IF forward
                     THEN [wire EXCEPT ![c] = Tail(@),
                            ![nextChannel] = Append(@, IF gotRoot THEN response ELSE m)]
                     ELSE [wire EXCEPT ![c] = Tail(@)]
          /\ groups' = IF gotRoot THEN [groups EXCEPT ![root] = @ \cup {p}] ELSE groups
          /\ knowledge' = IF m.kind = "Join" THEN [knowledge EXCEPT ![c[2]] = @ \cup {p}]
                           ELSE knowledge
          /\ results' = IF m.kind = "Return" /\ c[2] = 1
                         THEN [results EXCEPT ![p] = [host |-> m.host, ordinal |-> m.ordinal]] ELSE results
          /\ rootReleased' = IF m.kind = "Finish" /\ terminal
                              THEN rootReleased \cup {p} ELSE rootReleased
          /\ relayReleased' = IF m.kind = "Finish"
                               THEN relayReleased \cup {p} ELSE relayReleased
    /\ UNCHANGED <<sent, status, joined, finishSent, acceptSent, acceptSeen>>
AllResults == \A p \in Parts : results[p].host # 0
Compatible == /\ Cardinality({results[p].host : p \in Parts}) = 1
              /\ (Bug = "hostEquality" \/
                   {results[p].ordinal : p \in Parts} = 0..(NParts - 1))
Decide ==
    /\ status = "collecting" /\ AllResults
    /\ status' = IF Compatible THEN "connecting" ELSE "unequal"
    /\ UNCHANGED <<wire, sent, groups, knowledge, results, joined, finishSent,
                   relayReleased, rootReleased, acceptSent, acceptSeen>>
\* Ideal VatNetwork contract: matching results are necessary, but the remote
\* endpoint must ALSO demonstrate possession of ALL key parts for ONE object.
Connect ==
    /\ status = "connecting"
    /\ \E root \in Roots :
         /\ root[1] = results[1].host
         /\ ((groups[root] = Parts /\ rootReleased = {}) \/ Bug = "hostEquality")
         /\ joined' = root
    /\ status' = "authenticated"
    /\ UNCHANGED <<wire, sent, groups, knowledge, results, finishSent,
                   relayReleased, rootReleased, acceptSent, acceptSeen>>
SendAccept ==
    /\ status = "authenticated" /\ ~acceptSent
    /\ acceptSent' = TRUE
    /\ UNCHANGED <<wire, sent, groups, knowledge, results, status, joined,
                   finishSent, relayReleased, rootReleased, acceptSeen>>
\* Abstract Accept / Return on the authenticated direct connection. Its ordinary
\* question/Finish lifetime is the same contract checked in CapnpRpc.
AcceptReturn ==
    /\ acceptSent /\ ~acceptSeen /\ status = "authenticated"
    /\ acceptSeen' = TRUE /\ status' = "equal"
    /\ UNCHANGED <<wire, sent, groups, knowledge, results, joined, finishSent,
                   relayReleased, rootReleased, acceptSent>>
Cancel ==
    /\ AllowCancel /\ status \in {"collecting", "connecting", "authenticated"}
    /\ status' = "canceled"
    /\ UNCHANGED <<wire, sent, groups, knowledge, results, joined, finishSent,
                   relayReleased, rootReleased, acceptSent, acceptSeen>>
Finish(p) ==
    /\ p \in sent \ finishSent
    /\ status \in {"equal", "unequal", "canceled"}
    /\ finishSent' = finishSent \cup {p}
    /\ wire' = [wire EXCEPT ![<<1, Relay(p)>>] = Append(@, Msg("Finish", p, 0, 0))]
    /\ UNCHANGED <<sent, groups, knowledge, results, status, joined,
                   relayReleased, rootReleased, acceptSent, acceptSeen>>
Next == \/ \E p \in Parts : SendJoin(p) \/ Finish(p)
        \/ \E c \in Channels : Receive(c)
        \/ Decide \/ Connect \/ SendAccept \/ AcceptReturn \/ Cancel
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(Decide) /\ WF_vars(Connect)
                 /\ WF_vars(SendAccept) /\ WF_vars(AcceptReturn)
                 /\ (\A c \in Channels : WF_vars(Receive(c)))
                 /\ (\A p \in Parts : WF_vars(SendJoin(p)) /\ WF_vars(Finish(p)))
JoinSettles == <>(status \in {"equal", "unequal", "canceled"})
JoinReclaims == status \in {"equal", "unequal", "canceled"} ~> (rootReleased = sent)
TypeOK == /\ sent \subseteq Parts /\ finishSent \subseteq Parts
          /\ relayReleased \subseteq Parts /\ rootReleased \subseteq Parts
          /\ groups \in [Roots -> SUBSET Parts]
          /\ knowledge \in [Vats -> SUBSET Parts]
          /\ status \in {"collecting", "connecting", "authenticated", "equal", "unequal", "canceled"}
DistributedEquality == status = "equal" => \A p \in Parts : Root(p) = joined
AuthenticatedJoin == status \in {"authenticated", "equal"} => groups[joined] = Parts
ResultDiscrimination == AllResults /\ Compatible => Cardinality(Roots) = 1
ShareSecrecy == \A v \in {2, 3} : knowledge[v] \subseteq {p \in Parts : Relay(p) = v}
Retention == /\ rootReleased \subseteq relayReleased /\ relayReleased \subseteq finishSent
             /\ (finishSent # {} => status \in {"equal", "unequal", "canceled"})
NoEqualWitness == status # "equal"
NoUnequalWitness == status # "unequal"
NoCancelCleanupWitness == ~(status = "canceled" /\ sent # {} /\ rootReleased = sent)
=============================================================================
