--------------------------- MODULE CapnpRpc ---------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC

\* A first, deliberately partial model of ordinary bilateral RPC.
\* Source: rpc.capnp at 3a82de9b39736a2625f03c93b2b7c50642dd5b25.
\* See README.md for assumptions, wire correspondence, and exclusions.
\* Call.onlyPromisePipeline and Return.noFinishNeeded are fixed to FALSE.

CONSTANTS NumIds, MaxCalls, Callers, AllowDisconnect, Bug

Vats == {1, 2}
Other(v) == 3 - v
Ids == 1..NumIds
Tokens == Vats \X (1..MaxCalls)
Slots == Vats \X Ids
None == <<0, 0>>
Results == {"none", "ok", "exception", "canceled"}

ASSUME /\ NumIds \in Nat \ {0}
       /\ MaxCalls \in Nat \ {0}
       /\ Callers \subseteq Vats
       /\ AllowDisconnect \in BOOLEAN
       /\ Bug \in {"none", "reuseAfterFinish", "cancelNeededParent",
                    "reorderPipeline"}

\* Slot <<v, q>> always denotes caller v's question and Other(v)'s answer.
\* Thus the two vats may use the same numeric question ID independently.
\* Tokens identify invocation incarnations in local state and observations.
\* Message.token is GHOST metadata, never used to look up Return/Finish or
\* a promised-answer target. Such lookups use numeric IDs only.
VARIABLES connected, wire, questions, answers, issued, calls, deliveries, faults

vars == <<connected, wire, questions, answers, issued, calls, deliveries, faults>>

EmptyCall == [id |-> 0, parent |-> None, dep |-> None, phase |-> "unused",
              result |-> "none", observed |-> "none",
              finishSent |-> FALSE, finishRecv |-> FALSE,
              returnSent |-> FALSE, returnRecv |-> FALSE]

Message(kind, id, parentId, token, result) ==
    [kind |-> kind, id |-> id, parentId |-> parentId,
     token |-> token, result |-> result]

Init ==
    /\ connected = TRUE
    /\ wire = [v \in Vats |-> <<>>]
    /\ questions = [s \in Slots |-> None]
    /\ answers = [s \in Slots |-> None]
    /\ issued = [v \in Vats |-> 0]
    /\ calls = [t \in Tokens |-> EmptyCall]
    /\ deliveries = <<>>
    /\ faults = {}

Issued(t) == calls[t].phase # "unused"
Delivered(t) == t \in {deliveries[i] : i \in 1..Len(deliveries)}
Slot(t) == <<t[1], calls[t].id>>

ParentAllowed(v, p) ==
    IF p = 0 THEN TRUE
    ELSE LET t == questions[<<v, p>>]
         IN IF t = None THEN FALSE ELSE ~calls[t].finishSent

SendCall(v, id, p) ==
    /\ connected
    /\ v \in Callers
    /\ issued[v] < MaxCalls
    /\ questions[<<v, id>>] = None
    /\ ParentAllowed(v, p)
    /\ LET t == <<v, issued[v] + 1>>
           parent == IF p = 0 THEN None ELSE questions[<<v, p>>]
       IN /\ issued' = [issued EXCEPT ![v] = @ + 1]
          /\ questions' = [questions EXCEPT ![<<v, id>>] = t]
          /\ calls' = [calls EXCEPT ![t] =
               [EmptyCall EXCEPT !.id = id, !.parent = parent, !.phase = "sent"]]
          /\ wire' = [wire EXCEPT ![v] =
               Append(@, Message("Call", id, p, t, "none"))]
    /\ UNCHANGED <<connected, answers, deliveries, faults>>

SendFinish(t) ==
    /\ connected
    /\ Issued(t)
    /\ ~calls[t].finishSent
    /\ calls' = [calls EXCEPT ![t].finishSent = TRUE]
    /\ questions' = IF calls[t].returnRecv \/ Bug = "reuseAfterFinish"
                     THEN [questions EXCEPT ![Slot(t)] = None]
                     ELSE questions
    /\ wire' = [wire EXCEPT ![t[1]] =
         Append(@, Message("Finish", calls[t].id, 0, t, "none"))]
    /\ UNCHANGED <<connected, answers, issued, deliveries, faults>>

ReceiveCall(v, m) ==
    LET t == m.token
        dep == IF m.parentId = 0 THEN None ELSE answers[<<v, m.parentId>>]
        valid == /\ answers[<<v, m.id>>] = None
                 /\ (m.parentId = 0 \/ dep # None)
                 /\ dep = calls[t].parent
    IN /\ answers' = [answers EXCEPT ![<<v, m.id>>] = t]
       /\ calls' = [calls EXCEPT ![t].phase = "waiting", ![t].dep = dep]
       /\ faults' = IF valid THEN faults ELSE faults \cup {"callTargetOrId"}
       /\ UNCHANGED <<questions, deliveries>>

ReceiveFinish(v, m) ==
    LET t == answers[<<v, m.id>>]
    IN IF t = None
       THEN /\ faults' = faults \cup {"finishWithoutAnswer"}
            /\ UNCHANGED <<answers, questions, calls, deliveries>>
       ELSE /\ calls' = [calls EXCEPT ![t].finishRecv = TRUE]
            /\ answers' = IF calls[t].returnSent
                          THEN [answers EXCEPT ![<<v, m.id>>] = None]
                          ELSE answers
            /\ faults' = IF t = m.token /\ ~calls[t].finishRecv
                          THEN faults ELSE faults \cup {"wrongFinish"}
            /\ UNCHANGED <<questions, deliveries>>

ReceiveReturn(v, m) ==
    LET caller == Other(v)
        t == questions[<<caller, m.id>>]
    IN IF t = None
       THEN /\ faults' = faults \cup {"returnWithoutQuestion"}
            /\ UNCHANGED <<answers, questions, calls, deliveries>>
       ELSE /\ calls' = [calls EXCEPT ![t].returnRecv = TRUE,
                                     ![t].observed = m.result]
            /\ questions' = IF calls[t].finishSent
                            THEN [questions EXCEPT ![<<caller, m.id>>] = None]
                            ELSE questions
            /\ faults' = IF t = m.token /\ ~calls[t].returnRecv
                          THEN faults ELSE faults \cup {"wrongReturn"}
            /\ UNCHANGED <<answers, deliveries>>

\* One FIFO per sending vat carries ALL message kinds, not separate queues
\* for Call and Finish. Each receiver always consumes its queue head.
Receive(v) ==
    /\ connected
    /\ Len(wire[v]) > 0
    /\ LET m == Head(wire[v])
       IN CASE m.kind = "Call" -> ReceiveCall(v, m)
            [] m.kind = "Finish" -> ReceiveFinish(v, m)
            [] m.kind = "Return" -> ReceiveReturn(v, m)
    /\ wire' = [wire EXCEPT ![v] = Tail(@)]
    /\ UNCHANGED <<connected, issued>>

\* Every successful result denotes a fresh local object at a fixed valid
\* pointer path. Direct calls all use one pre-established root capability.
\* Comparing parent tokens here represents a local per-reference queue.
EarlierWaiting(t) ==
    \E u \in Tokens : /\ u[1] = t[1]
                      /\ u[2] < t[2]
                      /\ calls[u].phase = "waiting"
                      /\ calls[u].dep = calls[t].dep

TargetReady(t) ==
    IF calls[t].dep = None THEN TRUE ELSE calls[calls[t].dep].result = "ok"

Dispatch(t) ==
    /\ connected
    /\ calls[t].phase = "waiting"
    /\ TargetReady(t)
    /\ (~EarlierWaiting(t) \/ (Bug = "reorderPipeline" /\ calls[t].dep # None))
    /\ calls' = [calls EXCEPT ![t].phase = "running"]
    /\ deliveries' = Append(deliveries, t)
    /\ UNCHANGED <<connected, wire, questions, answers, issued, faults>>

Complete(t) ==
    /\ connected
    /\ calls[t].phase = "running"
    /\ \E r \in {"ok", "exception"} :
          calls' = [calls EXCEPT ![t].phase = "done", ![t].result = r]
    /\ UNCHANGED <<connected, wire, questions, answers, issued, deliveries, faults>>

FailPipeline(t) ==
    /\ connected
    /\ calls[t].phase = "waiting"
    /\ calls[t].dep # None
    /\ calls[calls[t].dep].result \in {"exception", "canceled"}
    /\ calls' = [calls EXCEPT ![t].phase = "done", ![t].result = "exception"]
    /\ UNCHANGED <<connected, wire, questions, answers, issued, deliveries, faults>>

HasWaitingChild(t) ==
    \E u \in Tokens : calls[u].phase = "waiting" /\ calls[u].dep = t

\* Finish does not revoke previously accepted pipeline dependencies.
\* This policy conservatively retains parents until all waiting children
\* dispatch, fail, or cancel. It may retain more work than strictly necessary.
Cancel(t) ==
    /\ connected
    /\ calls[t].phase \in {"waiting", "running"}
    /\ calls[t].finishRecv
    /\ (~HasWaitingChild(t) \/ Bug = "cancelNeededParent")
    /\ calls' = [calls EXCEPT ![t].phase = "done", ![t].result = "canceled"]
    /\ UNCHANGED <<connected, wire, questions, answers, issued, deliveries, faults>>

SendReturn(t) ==
    /\ connected
    /\ calls[t].phase = "done"
    /\ ~calls[t].returnSent
    /\ wire' = [wire EXCEPT ![Other(t[1])] =
         Append(@, Message("Return", calls[t].id, 0, t, calls[t].result))]
    /\ calls' = [calls EXCEPT ![t].returnSent = TRUE]
    /\ answers' = IF calls[t].finishRecv
                  THEN [answers EXCEPT ![Slot(t)] = None] ELSE answers
    /\ UNCHANGED <<connected, questions, issued, deliveries, faults>>

\* Abstract simultaneous detection of connection loss. Historical calls
\* and delivery observations are retained solely for checking properties.
Disconnect ==
    /\ connected /\ AllowDisconnect
    /\ connected' = FALSE
    /\ wire' = [v \in Vats |-> <<>>]
    /\ questions' = [s \in Slots |-> None]
    /\ answers' = [s \in Slots |-> None]
    /\ calls' = [t \in Tokens |->
         IF Issued(t) /\ ~calls[t].returnRecv
         THEN [calls[t] EXCEPT !.observed = "disconnected"] ELSE calls[t]]
    /\ UNCHANGED <<issued, deliveries, faults>>

Progress(t) == Dispatch(t) \/ Complete(t) \/ FailPipeline(t) \/ Cancel(t) \/ SendReturn(t)

Next == \/ \E v \in Vats, id \in Ids, p \in {0} \cup Ids : SendCall(v, id, p)
        \/ \E t \in Tokens : SendFinish(t) \/ Progress(t)
        \/ \E v \in Vats : Receive(v)
        \/ Disconnect

Spec == Init /\ [][Next]_vars

\* Progress assumptions: eventual transport service and eventual application
\* completion/cancellation. Neither sending new calls nor disconnecting is fair.
LiveSpec == Spec /\ (\A v \in Vats : WF_vars(Receive(v)))
                 /\ (\A t \in Tokens : WF_vars(Progress(t)))
CleanupSpec == LiveSpec /\ (\A t \in Tokens : WF_vars(SendFinish(t)))

TypeOK ==
    /\ connected \in BOOLEAN
    /\ wire \in [Vats -> Seq([kind : {"Call", "Return", "Finish"},
                              id : Ids, parentId : {0} \cup Ids,
                              token : Tokens, result : Results])]
    /\ questions \in [Slots -> Tokens \cup {None}]
    /\ answers \in [Slots -> Tokens \cup {None}]
    /\ issued \in [Vats -> 0..MaxCalls]
    /\ calls \in [Tokens -> [id : {0} \cup Ids,
                             parent : Tokens \cup {None}, dep : Tokens \cup {None},
                             phase : {"unused", "sent", "waiting", "running", "done"},
                             result : Results, observed : Results \cup {"disconnected"},
                             finishSent : BOOLEAN, finishRecv : BOOLEAN,
                             returnSent : BOOLEAN, returnRecv : BOOLEAN]]
    /\ deliveries \in Seq(Tokens)
    /\ faults \subseteq {"callTargetOrId", "finishWithoutAnswer", "wrongFinish",
                          "returnWithoutQuestion", "wrongReturn"}

ProtocolSafety == faults = {}

NoPrematureReuse ==
    \A t, u \in Tokens :
       (Issued(t) /\ Issued(u) /\ t[1] = u[1] /\ t[2] < u[2]
        /\ calls[t].id = calls[u].id)
       => (calls[t].finishSent /\ calls[t].returnRecv)

QuestionLifetime == connected =>
    \A t \in Tokens : Issued(t) =>
       ((questions[Slot(t)] = t) <=> ~(calls[t].finishSent /\ calls[t].returnRecv))

AnswerLifetime == connected =>
    \A t \in Tokens : calls[t].phase \in {"waiting", "running", "done"} =>
       ((answers[Slot(t)] = t) <=> ~(calls[t].finishRecv /\ calls[t].returnSent))

ReturnAgreement ==
    \A t \in Tokens : calls[t].returnRecv =>
       (calls[t].returnSent /\ calls[t].observed = calls[t].result)

CancellationJustified ==
    \A t \in Tokens : calls[t].result = "canceled" => calls[t].finishRecv

AtMostOnceDelivery == Cardinality({deliveries[i] : i \in 1..Len(deliveries)})
                      = Len(deliveries)

PipelineSoundness ==
    \A t \in Tokens : Delivered(t) /\ calls[t].parent # None =>
       (calls[t].dep = calls[t].parent /\ calls[calls[t].parent].result = "ok")

InterestedChildPreserved ==
    \A t \in Tokens :
       (calls[t].phase = "waiting" /\ calls[t].dep # None /\ ~calls[t].finishRecv)
       => calls[calls[t].dep].result # "canceled"

\* This checks delivery order, never completion/Return order. Canceled calls
\* may be skipped. "parent" is the logical reference identity in this subset.
EOrder ==
    /\ \A t, u \in Tokens :
          (Delivered(t) /\ Issued(u) /\ t[1] = u[1] /\ u[2] < t[2]
           /\ calls[u].parent = calls[t].parent)
          => (Delivered(u) \/ calls[u].result = "canceled")
    /\ \A i, j \in 1..Len(deliveries) :
          LET t == deliveries[i] u == deliveries[j]
          IN (i < j /\ t[1] = u[1] /\ calls[t].parent = calls[u].parent)
             => t[2] < u[2]

TablesEmpty == /\ \A s \in Slots : questions[s] = None /\ answers[s] = None
               /\ \A v \in Vats : wire[v] = <<>>

DisconnectClean == ~connected =>
    /\ TablesEmpty
    /\ \A t \in Tokens : Issued(t) /\ ~calls[t].returnRecv
                         => calls[t].observed = "disconnected"

EveryCallSettles == \A t \in Tokens : Issued(t) ~> (calls[t].returnRecv \/ ~connected)
EventuallyQuiescent == <>[]TablesEmpty

\* Witness predicates: negating one as an invariant asks TLC for a trace.
SawReuse == \E t, u \in Tokens : Issued(t) /\ Issued(u) /\ t[1] = u[1]
                 /\ t[2] < u[2] /\ calls[t].id = calls[u].id
SawEarlyPipeline == \E t \in Tokens : Delivered(t) /\ calls[t].parent # None
                       /\ ~calls[calls[t].parent].returnRecv
SawCrossedFinish == \E t \in Tokens : calls[t].finishSent /\ ~calls[t].finishRecv
                       /\ calls[t].returnSent /\ ~calls[t].returnRecv
NoEarlyPipelineWitness == ~SawEarlyPipeline
NoReuseWitness == ~SawReuse
NoCrossedFinishWitness == ~SawCrossedFinish
======================================================================
