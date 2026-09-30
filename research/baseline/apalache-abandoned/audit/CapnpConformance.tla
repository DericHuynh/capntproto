------------------------ MODULE CapnpConformance ------------------------
EXTENDS CapnpNetworkChecks

\* Independent requirements, not deliberate bug variants. The pre-repair
\* version is archived under baseline/pre-handoff. Reachable checks and
\* the two Wire* predicates below are handler/abstraction diagnostics.
AutomaticHandoffPlan == <<
    Boot(2,3),
    [Boot(1,2) EXCEPT !.wait={1}],
    Call(1,2,2,"promise",1),
    [Command("resolve",2,0) EXCEPT !.ref=1, !.wait={1,3}, !.data=1]
>>

\* rpc.capnp level 3 (93-95), CapDescriptor (1102-1109): receiving an
\* introduction must lead to a direct pickup without application-level RPC
\* bookkeeping. This workload retains the capability, never disconnects,
\* and has enough IDs/operations; LiveSpec supplies the model's fairness.
AutomaticHandoff == (net.offers # {}) ~>
    (\E n \in 1..(net.next-1) : net.ops[n].kind="accept")
AutomaticHandoffSettles == (net.offers # {}) ~>
    (\E n \in 1..(net.next-1) : net.ops[n].automatic /\
        net.ops[n].handoffDone /\ net.ops[n].observed="results")
ProviderCapabilitySurvives == <>(net.labels[7] # 0 /\
    net.ops[net.labels[7]].observed="results")

\* Positive control: the existing scripted handoff can initiate Accept.
ScriptedHandoffPlan == AutomaticHandoffPlan \o <<
    [Command("accept",1,3) EXCEPT !.after={4}]
>>
ScriptedHandoff == AutomaticHandoff
EqualJoinOutcome == <>(net.joins[1]="authenticated")
UnequalJoinOutcome == <>(net.joins[1]="unequal")

\* Release has only id and referenceCount (rpc.capnp 684-695).
\* A receiver's counter transition must be definable without ticket IDs.
\* Fixture: two valid export credits, imported and subsequently released.
\* Compare delivery with its ghost ticket annotation erased. This probes
\* whether tickets really are observers; it is NOT a reachable Next trace.
WireReleaseEffect ==
    LET encoded == EncodePayload(net,<<1,2>>,<<Object(1,1),Object(1,1)>>)
        held == ImportCredits(encoded.state,encoded.tickets)
        releasing == ReleaseImports(held,encoded.tickets)
        message == [Message EXCEPT !.kind="release", !.id=1, !.count=2]
        after == ReceiveRelease(releasing,<<2,1>>,message)
    IN after.exports[<<<<1,2>>,1>>].count = 0

\* Call.methodId determines the invoked method (rpc.capnp 414-415).
\* Vary the incoming method while retaining the paired caller metadata.
\* A receiver driven by the wire should read m.method, not the paired op.
\* Deliberately breaks the pairing relation: locality diagnostic only.
WireRequestMethod ==
    LET asked == Ask(net,<<1,2>>,"call",NoTarget,<<>>,Flags,
                    "echo",0,"caller",0,0,0,0,0,0,0)
        message == [Head(asked.wire[<<1,2>>]) EXCEPT !.method="factory"]
        received == ReceiveRequest(asked,<<1,2>>,message)
    IN received.ops[message.op].method = message.method

\* Unlike the erasure diagnostic, this checks real generated releases on
\* every reachable state, with the model's annotation relation intact.
GeneratedReleaseEffect == \A c \in Channels :
    IF net.wire[c]= <<>> THEN TRUE
    ELSE LET m==Head(net.wire[c])
         IN IF m.kind # "release" THEN TRUE
            ELSE ReceiveRelease(net,c,m).exports[<<Rev(c),m.id>>].count =
                 net.exports[<<Rev(c),m.id>>].count - m.count

UnitSpec == Init /\ [][UNCHANGED net]_vars
=============================================================================
