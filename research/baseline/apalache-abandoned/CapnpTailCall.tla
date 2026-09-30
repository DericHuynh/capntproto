------------------------- MODULE CapnpTailCall -------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC, CapnpComponentTypes
\* Tail-call redirection: bilateral yourself/takeFromOtherQuestion or level-3
\* thirdParty/awaitFromThirdParty/ThirdPartyAnswer. All channels are FIFO.
CONSTANTS
    \* @type: Bool;
    ThirdParty,
    \* @type: Bool;
    AllowThirdPartyTailCall,
    \* @type: Str;
    Bug,
    \* @type: Bool;
    AllowFailure
Vats == {1, 2, 3}
Callee == IF ThirdParty THEN 3 ELSE 1
Channels == {c \in Vats \X Vats : c[1] # c[2]}
\* IDs are <<high two bits, low 30 bits>>, avoiding TLC signed-int overflow.
\* Classes 0, 1, and 2/3 denote [0,2^30), [2^30,2^31), [2^31,2^32).
\* @type: $pair;
OrdinaryId == <<0, 0>>
\* @type: $pair;
AdoptedId == <<1, 0>>
\* @type: $pair;
PipelineId == <<2, 0>>
Direct == ThirdParty /\ AllowThirdPartyTailCall
Proxy == ThirdParty /\ ~AllowThirdPartyTailCall
\* @type: (Str, $pair) => $tailMessage;
Msg(kind, id) == [kind |-> kind, id |-> id]
VARIABLES
    \* @type: $pair -> Seq($tailMessage);
    wire,
    \* @type: Set(Str);
    events,
    \* @type: Set(Str);
    faults
\* @type: <<$pair -> Seq($tailMessage), Set(Str), Set(Str)>>;
vars == <<wire, events, faults>>
Has(e) == e \in events
Init == /\ wire = [c \in Channels |-> <<>>]
        /\ events = {} /\ faults = {}
\* @type: (Str, $pair, Str, $pair) => Bool;
Send(event, c, kind, id) ==
    /\ ~Has("failed") /\ ~Has(event)
    /\ events' = events \cup {event}
    /\ wire' = [wire EXCEPT ![c] = Append(@, Msg(kind, id))]
    /\ UNCHANGED faults
Forward == Send("forwardSent", <<2, Callee>>, "Call", OrdinaryId)
Redirect == /\ Has("forwardSent") /\ ~Proxy
            /\ Send("redirectSent", <<2, 1>>,
                    IF ThirdParty THEN "awaitFromThirdParty" ELSE "takeFromOtherQuestion", OrdinaryId)
Adopt == /\ Direct /\ Has("callSeen")
         /\ Send("adoptSent", <<3, 1>>, "ThirdPartyAnswer",
                  IF Bug = "idCollision" THEN OrdinaryId ELSE AdoptedId)
Complete == /\ ~Has("failed") /\ Has("callSeen") /\ ~Has("complete")
            /\ events' = events \cup {"complete"}
            /\ UNCHANGED <<wire, faults>>
ReturnElsewhere == /\ Has("complete")
                   /\ Send("elsewhereSent", <<Callee, 2>>,
                           IF Proxy THEN "proxyResults" ELSE "resultsSentElsewhere", OrdinaryId)
DirectReturn == /\ Direct /\ Has("complete") /\ Has("adoptSent")
                /\ Send("directSent", <<3, 1>>, "results", AdoptedId)
ProxyReturn == /\ Proxy /\ Has("proxyResultsSeen")
               /\ Send("proxyReturnSent", <<2, 1>>, "proxyReturn", OrdinaryId)
\* @type: $pair => Bool;
Receive(c) ==
    /\ ~Has("failed") /\ Len(wire[c]) > 0
    /\ LET m == Head(wire[c])
           e == CASE m.kind = "Call" -> "callSeen"
                  [] m.kind \in {"awaitFromThirdParty", "takeFromOtherQuestion"} -> "redirectSeen"
                  [] m.kind = "ThirdPartyAnswer" -> "adoptSeen"
                  [] m.kind = "results" -> "directSeen"
                  [] m.kind = "resultsSentElsewhere" -> "elsewhereSeen"
                  [] m.kind = "proxyResults" -> "proxyResultsSeen"
                  [] m.kind = "proxyReturn" -> "proxyReturnSeen"
                  [] m.kind = "FinishOriginal" -> "originalFinished"
                  [] m.kind = "FinishForward" -> "forwardFinished"
                  [] m.kind = "FinishAdopted" -> "adoptedFinished"
       IN /\ events' = events \cup {e}
          /\ faults' = IF (m.kind = "ThirdPartyAnswer" /\ m.id # AdoptedId) \/
                          (m.kind = "results" /\ ~Has("adoptSeen"))
                       THEN faults \cup {"answerNamespace"} ELSE faults
    /\ wire' = [wire EXCEPT ![c] = Tail(@)]
Observe ==
    /\ ~Has("failed") /\ ~Has("observed")
    /\ IF Proxy THEN Has("proxyReturnSeen")
       ELSE /\ (Has("redirectSeen") \/ Bug = "eagerAdopt")
            /\ IF ThirdParty THEN Has("adoptSeen") /\ Has("directSeen")
                             ELSE Has("callSeen") /\ Has("complete")
    /\ events' = events \cup {"observed"}
    /\ UNCHANGED <<wire, faults>>
FinishOriginal == /\ Has("observed")
                  /\ Send("originalFinishSent", <<1, 2>>, "FinishOriginal", OrdinaryId)
FinishForward == /\ Has("originalFinished")
                 /\ Send("forwardFinishSent", <<2, Callee>>, "FinishForward", OrdinaryId)
FinishAdopted == /\ Direct /\ Has("observed")
                 /\ Send("adoptedFinishSent", <<1, 3>>, "FinishAdopted", AdoptedId)
Fail == /\ AllowFailure /\ ~Has("failed")
        /\ events' = events \cup {"failed"}
        /\ wire' = [c \in Channels |-> <<>>] /\ UNCHANGED faults
Next == Forward \/ Redirect \/ Adopt \/ Complete \/ ReturnElsewhere \/ DirectReturn \/
        Observe \/ ProxyReturn \/ FinishOriginal \/ FinishForward \/ FinishAdopted \/ Fail \/
        (\E c \in Channels : Receive(c))
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(Forward) /\ WF_vars(Redirect) /\ WF_vars(Adopt)
                 /\ WF_vars(Complete) /\ WF_vars(ReturnElsewhere) /\ WF_vars(DirectReturn)
                 /\ WF_vars(ProxyReturn) /\ WF_vars(Observe)
                 /\ WF_vars(FinishOriginal) /\ WF_vars(FinishForward) /\ WF_vars(FinishAdopted)
                 /\ (\A c \in Channels : WF_vars(Receive(c)))
TailSettles == <>(Has("observed") \/ Has("failed"))
TailReclaims == <>(Has("failed") \/ (Has("forwardFinished") /\
                (~Direct \/ Has("adoptedFinished"))))
ProtocolSafety == faults = {}
RedirectAuthority == Has("observed") => (Has("redirectSeen") \/ (Proxy /\ Has("proxyReturnSeen")))
TailCallPermission == ThirdParty /\ Has("redirectSent") => AllowThirdPartyTailCall
ReturnAgreement == Has("observed") => Has("complete") /\
                    (~Direct \/ (Has("adoptSeen") /\ Has("directSeen")))
FinishChain == Has("forwardFinishSent") => Has("originalFinished")
Namespaces == OrdinaryId[1] = 0 /\ AdoptedId[1] = 1 /\ PipelineId[1] \in {2, 3}
NoTailCallWitness == ~Has("observed")
NoEarlyThirdPartyAnswerWitness == ~(Has("adoptSeen") /\ ~Has("redirectSeen"))
NoTailCleanupWitness == ~(Has("forwardFinished") /\ Has("elsewhereSeen") /\
                          (~Direct \/ Has("adoptedFinished")))
=============================================================================
