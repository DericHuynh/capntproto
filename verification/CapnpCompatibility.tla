------------------------ MODULE CapnpCompatibility ------------------------
EXTENDS CapnpWire
\* Recovery policy permitted by Message.unimplemented, and mandatory abort for
\* a senderLoopback whose target does not resolve back to its sender.
CONSTANT RequestKind
VARIABLES wire, sent, echoed, recovered, status, pending, replacement, outcome
vars == <<wire, sent, echoed, recovered, status, pending, replacement, outcome>>
Init == /\ wire = [v \in {1, 2} |-> <<>>]
        /\ sent = FALSE /\ echoed = FALSE /\ recovered = FALSE
        /\ status = [v \in {1, 2} |-> "open"]
        /\ pending = FALSE /\ replacement = 0 /\ outcome = "none"
SendRequest ==
    /\ ~sent /\ status[1] = "open"
    /\ sent' = TRUE /\ wire' = [wire EXCEPT ![1] = Append(@, RequestKind)]
    /\ pending' = (RequestKind \in {"bootstrap", "call", "provide", "accept", "join"})
    /\ replacement' = IF RequestKind = "resolve" THEN 1 ELSE 0
    /\ UNCHANGED <<echoed, recovered, status, outcome>>
ReceiveRequest ==
    /\ Len(wire[1]) > 0 /\ status[2] = "open" /\ ~echoed
    /\ echoed' = TRUE
    /\ wire' = [wire EXCEPT ![1] = Tail(@), ![2] = Append(@,
                     IF RequestKind = "invalidLoopback" THEN "abort" ELSE "unimplemented")]
    /\ status' = IF RequestKind = "invalidLoopback"
                  THEN [status EXCEPT ![2] = "halfClosed"] ELSE status
    /\ UNCHANGED <<sent, recovered, pending, replacement, outcome>>
ReceiveReply ==
    /\ Len(wire[2]) > 0 /\ ~recovered
    /\ recovered' = TRUE
    /\ LET abort == Head(wire[2]) = "abort" \/ UnimplementedEffect(RequestKind) = "abort"
       IN /\ wire' = [wire EXCEPT ![2] = Tail(@),
                              ![1] = IF abort THEN Append(@, "abort") ELSE @]
          /\ status' = IF abort THEN [status EXCEPT ![1] = "halfClosed"] ELSE status
          /\ outcome' = IF abort THEN "disconnected"
                         ELSE IF pending THEN "exception" ELSE "released"
    /\ pending' = FALSE /\ replacement' = 0
    /\ UNCHANGED <<sent, echoed>>
Close ==
    /\ recovered /\ status[1] = "halfClosed"
    /\ status' = [v \in {1, 2} |-> "closed"]
    /\ wire' = [v \in {1, 2} |-> <<>>]
    /\ UNCHANGED <<sent, echoed, recovered, pending, replacement, outcome>>
Next == SendRequest \/ ReceiveRequest \/ ReceiveReply \/ Close
Spec == Init /\ [][Next]_vars
RecoverQuestions == recovered /\ RequestKind \in {"bootstrap", "call", "provide", "accept", "join"}
                    => ~pending /\ outcome = "exception"
RecoverResolve == recovered /\ RequestKind = "resolve" => replacement = 0
VerifyLoopback == echoed /\ RequestKind = "invalidLoopback" => status[2] # "open"
AbortClosesTables == status[1] = "closed" => ~pending /\ replacement = 0 /\ wire[1] = <<>> /\ wire[2] = <<>>
NoRecoveryWitness == ~recovered
=============================================================================
