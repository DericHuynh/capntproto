--------------------------- MODULE CapnpRpc ---------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC

\* Connection-local call lifecycle, parameterized over a network of vats.
\* Companion modules model capability accounting, routing, tail calls and joins.
\* Source: rpc.capnp at 0de72d8d8cec6b69edaa29de51d3bd490341f9c2.
\* See PROTOCOL.md for the correspondence and abstraction boundaries.

CONSTANTS NumIds, MaxCalls, Callers, AllowDisconnect, Bug, NumVats, Modes

Vats == 1..NumVats
Channels == {c \in Vats \X Vats : c[1] # c[2]}
Reverse(c) == <<c[2], c[1]>>
Ids == 1..NumIds
Tokens == Channels \X (1..MaxCalls)
Slots == Channels \X Ids
None == <<<<0, 0>>, 0>>
Results == {"none", "ok", "exception", "canceled"}

ASSUME /\ NumVats \in Nat \ {0, 1}
       /\ Modes \subseteq {"ordinary", "bootstrap", "noPipeline", "noFinish",
                             "pipelineOnly", "pipelineCompat", "workaround"}
       /\ Modes # {}
       /\ NumIds \in Nat \ {0}
       /\ MaxCalls \in Nat \ {0}
       /\ Callers \subseteq Vats
       /\ AllowDisconnect \in BOOLEAN
       /\ Bug \in {"none", "reuseAfterFinish", "cancelNeededParent",
                    "reorderPipeline"}

\* Slot <<channel, q>> denotes channel[1]'s question and channel[2]'s answer.
\* Thus the two vats may use the same numeric question ID independently.
\* Tokens identify invocation incarnations in local state and observations.
\* Message.token is GHOST metadata, never used to look up Return/Finish or
\* a promised-answer target. Such lookups use numeric IDs only.
VARIABLES connected, wire, questions, answers, issued, calls, deliveries, faults

vars == <<connected, wire, questions, answers, issued, calls, deliveries, faults>>

EmptyCall == [id |-> 0, parent |-> None, dep |-> None, phase |-> "unused",
              result |-> "none", observed |-> "none",
              finishSent |-> FALSE, finishRecv |-> FALSE,
              returnSent |-> FALSE, returnRecv |-> FALSE, mode |-> "ordinary"]

Message(kind, id, parentId, token, result, mode) ==
    [kind |-> kind, id |-> id, parentId |-> parentId,
     token |-> token, result |-> result, mode |-> mode]

Init ==
    /\ connected = Channels
    /\ wire = [v \in Channels |-> <<>>]
    /\ questions = [s \in Slots |-> None]
    /\ answers = [s \in Slots |-> None]
    /\ issued = [v \in Channels |-> 0]
    /\ calls = [t \in Tokens |-> EmptyCall]
    /\ deliveries = <<>>
    /\ faults = {}

Issued(t) == calls[t].phase # "unused"
Delivered(t) == t \in {deliveries[i] : i \in 1..Len(deliveries)}
Slot(t) == <<t[1], calls[t].id>>

PipelineOnly(t) == calls[t].mode \in {"pipelineOnly", "pipelineCompat"}
CallerDone(t) == calls[t].finishSent \/
                 (calls[t].mode = "noFinish" /\ calls[t].returnRecv)
QuestionDone(t) == (CallerDone(t) /\ calls[t].returnRecv) \/
                   (PipelineOnly(t) /\ calls[t].finishSent)
AnswerDone(t) == (calls[t].finishRecv /\ calls[t].returnSent) \/
                 (PipelineOnly(t) /\ calls[t].finishRecv) \/
                 (calls[t].mode = "noFinish" /\ calls[t].returnSent)

ParentAllowed(v, p) ==
    IF p = 0 THEN TRUE
    ELSE LET t == questions[<<v, p>>]
         IN IF t = None THEN FALSE ELSE ~CallerDone(t) /\ calls[t].mode \notin {"noPipeline", "noFinish"}

SendCall(v, id, p, mode) ==
    /\ v \in connected
    /\ v[1] \in Callers
    \* Pipeline-only IDs form a separate, never-wrapped finite allocation class.
    /\ \A t \in Tokens : (t[1] = v /\ calls[t].id = id /\ Issued(t)) =>
           (~PipelineOnly(t) /\ mode \notin {"pipelineOnly", "pipelineCompat"})
    /\ issued[v] < MaxCalls
    /\ questions[<<v, id>>] = None
    /\ ParentAllowed(v, p)
    /\ (mode = "bootstrap" => p = 0)
    /\ LET t == <<v, issued[v] + 1>>
           parent == IF p = 0 THEN None ELSE questions[<<v, p>>]
       IN /\ issued' = [issued EXCEPT ![v] = @ + 1]
          /\ questions' = [questions EXCEPT ![<<v, id>>] = t]
          /\ calls' = [calls EXCEPT ![t] =
               [EmptyCall EXCEPT !.id = id, !.parent = parent, !.phase = "sent", !.mode = mode]]
          /\ wire' = [wire EXCEPT ![v] =
               Append(@, Message(IF mode = "bootstrap" THEN "Bootstrap" ELSE "Call", id, p, t, "none", mode))]
    /\ UNCHANGED <<connected, answers, deliveries, faults>>

SendFinish(t) ==
    /\ t[1] \in connected
    /\ ~CallerDone(t)
    /\ Issued(t)
    /\ ~calls[t].finishSent
    /\ calls' = [calls EXCEPT ![t].finishSent = TRUE]
    /\ questions' = IF calls[t].returnRecv \/ PipelineOnly(t) \/ Bug = "reuseAfterFinish"
                     THEN [questions EXCEPT ![Slot(t)] = None]
                     ELSE questions
    /\ wire' = [wire EXCEPT ![t[1]] =
         Append(@, Message("Finish", calls[t].id, 0, t, "none", calls[t].mode))]
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
       THEN /\ faults' = IF m.mode = "noFinish" THEN faults
                            ELSE faults \cup {"finishWithoutAnswer"}
            /\ UNCHANGED <<answers, questions, calls, deliveries>>
       ELSE /\ calls' = [calls EXCEPT ![t].finishRecv = TRUE]
            /\ answers' = IF calls[t].returnSent \/ PipelineOnly(t)
                          THEN [answers EXCEPT ![<<v, m.id>>] = None]
                          ELSE answers
            /\ faults' = IF t = m.token /\ ~calls[t].finishRecv
                          THEN faults ELSE faults \cup {"wrongFinish"}
            /\ UNCHANGED <<questions, deliveries>>

ReceiveReturn(v, m) ==
    LET caller == Reverse(v)
        t == questions[<<caller, m.id>>]
    IN IF t = None
       THEN /\ faults' = IF m.mode \in {"pipelineOnly", "pipelineCompat"}
                            THEN faults ELSE faults \cup {"returnWithoutQuestion"}
            /\ UNCHANGED <<answers, questions, calls, deliveries>>
       ELSE /\ calls' = [calls EXCEPT ![t].returnRecv = TRUE,
                                     ![t].observed = m.result]
            /\ questions' = IF calls[t].finishSent \/ m.mode = "noFinish"
                            THEN [questions EXCEPT ![<<caller, m.id>>] = None]
                            ELSE questions
            /\ faults' = IF t = m.token /\ ~calls[t].returnRecv
                          THEN faults ELSE faults \cup {"wrongReturn"}
            /\ UNCHANGED <<answers, deliveries>>

\* One FIFO per directed connection carries ALL message kinds, not separate queues
\* for Call and Finish. Each receiver always consumes its queue head.
Receive(v) ==
    /\ v \in connected
    /\ Len(wire[v]) > 0
    /\ LET m == Head(wire[v])
       IN CASE m.kind \in {"Call", "Bootstrap"} -> ReceiveCall(v, m)
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
    IF calls[t].dep = None THEN TRUE ELSE calls[calls[t].dep].result = "ok" /\
         calls[calls[t].dep].mode # "noFinish"

Dispatch(t) ==
    /\ t[1] \in connected
    /\ calls[t].phase = "waiting"
    /\ TargetReady(t)
    /\ (~EarlierWaiting(t) \/ (Bug = "reorderPipeline" /\ calls[t].dep # None))
    /\ calls' = [calls EXCEPT ![t].phase = "running"]
    /\ deliveries' = Append(deliveries, t)
    /\ UNCHANGED <<connected, wire, questions, answers, issued, faults>>

Complete(t) ==
    /\ t[1] \in connected
    /\ calls[t].phase = "running"
    /\ \E r \in {"ok", "exception"} :
          calls' = [calls EXCEPT ![t].phase = "done", ![t].result = r]
    /\ UNCHANGED <<connected, wire, questions, answers, issued, deliveries, faults>>

FailPipeline(t) ==
    /\ t[1] \in connected
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
    /\ t[1] \in connected
    /\ calls[t].phase \in {"waiting", "running"}
    /\ calls[t].finishRecv
    /\ (calls[t].mode = "workaround" => Delivered(t))
    /\ (~HasWaitingChild(t) \/ Bug = "cancelNeededParent")
    /\ calls' = [calls EXCEPT ![t].phase = "done", ![t].result = "canceled"]
    /\ UNCHANGED <<connected, wire, questions, answers, issued, deliveries, faults>>

SendReturn(t) ==
    /\ t[1] \in connected
    /\ calls[t].phase = "done"
    /\ ~calls[t].returnSent
    /\ calls[t].mode # "pipelineOnly"
    /\ wire' = [wire EXCEPT ![Reverse(t[1])] =
         Append(@, Message("Return", calls[t].id, 0, t, calls[t].result, calls[t].mode))]
    /\ calls' = [calls EXCEPT ![t].returnSent = TRUE]
    /\ answers' = IF calls[t].finishRecv \/ calls[t].mode = "noFinish"
                  THEN [answers EXCEPT ![Slot(t)] = None] ELSE answers
    /\ UNCHANGED <<connected, questions, issued, deliveries, faults>>

\* Abstract simultaneous detection of connection loss. Historical calls
\* and delivery observations are retained solely for checking properties.
Disconnect(c) ==
    /\ c \in connected /\ AllowDisconnect
    /\ LET lost == {c, Reverse(c)}
       IN /\ connected' = connected \ lost
          /\ wire' = [v \in Channels |-> IF v \in lost THEN <<>> ELSE wire[v]]
          /\ questions' = [s \in Slots |-> IF s[1] \in lost THEN None ELSE questions[s]]
          /\ answers' = [s \in Slots |-> IF s[1] \in lost THEN None ELSE answers[s]]
          /\ calls' = [t \in Tokens |->
               IF t[1] \in lost /\ Issued(t) /\ ~calls[t].returnRecv
               THEN [calls[t] EXCEPT !.observed = "disconnected"] ELSE calls[t]]
    /\ UNCHANGED <<issued, deliveries, faults>>

Progress(t) == Dispatch(t) \/ Complete(t) \/ FailPipeline(t) \/ Cancel(t) \/ SendReturn(t)

Next == \/ \E v \in Channels, id \in Ids, p \in {0} \cup Ids, mode \in Modes :
              SendCall(v, id, p, mode)
        \/ \E t \in Tokens : SendFinish(t) \/ Progress(t)
        \/ \E v \in Channels : Receive(v)
        \/ \E c \in Channels : Disconnect(c)

Spec == Init /\ [][Next]_vars

\* Progress assumptions: eventual transport service and eventual application
\* completion/cancellation. Neither sending new calls nor disconnecting is fair.
LiveSpec == Spec /\ (\A v \in Channels : WF_vars(Receive(v)))
                 /\ (\A t \in Tokens : WF_vars(Progress(t)))
CleanupSpec == LiveSpec /\ (\A t \in Tokens : WF_vars(SendFinish(t)))

TypeOK ==
    /\ connected \subseteq Channels
    /\ wire \in [Channels -> Seq([kind : {"Call", "Bootstrap", "Return", "Finish"},
                              id : Ids, parentId : {0} \cup Ids,
                              token : Tokens, result : Results, mode : Modes])]
    /\ questions \in [Slots -> Tokens \cup {None}]
    /\ answers \in [Slots -> Tokens \cup {None}]
    /\ issued \in [Channels -> 0..MaxCalls]
    /\ calls \in [Tokens -> [id : {0} \cup Ids,
                             parent : Tokens \cup {None}, dep : Tokens \cup {None},
                             phase : {"unused", "sent", "waiting", "running", "done"},
                             result : Results, observed : Results \cup {"disconnected"},
                             finishSent : BOOLEAN, finishRecv : BOOLEAN,
                             returnSent : BOOLEAN, returnRecv : BOOLEAN,
                             mode : Modes \cup {"ordinary"}]]
    /\ deliveries \in Seq(Tokens)
    /\ faults \subseteq {"callTargetOrId", "finishWithoutAnswer", "wrongFinish",
                          "returnWithoutQuestion", "wrongReturn"}

ProtocolSafety == faults = {}

NoPrematureReuse ==
    \A t, u \in Tokens :
       (Issued(t) /\ Issued(u) /\ t[1] = u[1] /\ t[2] < u[2]
        /\ calls[t].id = calls[u].id)
       => (CallerDone(t) /\ calls[t].returnRecv /\ ~PipelineOnly(t))

QuestionLifetime ==
    \A t \in Tokens : (Issued(t) /\ t[1] \in connected) =>
       ((questions[Slot(t)] = t) <=> ~QuestionDone(t))

AnswerLifetime ==
    \A t \in Tokens : (t[1] \in connected /\ calls[t].phase \in {"waiting", "running", "done"}) =>
       ((answers[Slot(t)] = t) <=> ~AnswerDone(t))

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
               /\ \A v \in Channels : wire[v] = <<>>

DisconnectClean == \A c \in Channels \ connected :
    /\ wire[c] = <<>>
    /\ \A id \in Ids : questions[<<c, id>>] = None /\ answers[<<c, id>>] = None
    /\ \A t \in Tokens : (t[1] = c /\ Issued(t) /\ ~calls[t].returnRecv)
                          => calls[t].observed = "disconnected"

EveryCallSettles == \A t \in Tokens : Issued(t) ~>
    (calls[t].returnRecv \/ t[1] \notin connected \/
     (PipelineOnly(t) /\ calls[t].finishRecv))
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
