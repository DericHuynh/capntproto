------------------------ MODULE CapnpNetworkChecks ------------------------
EXTENDS CapnpNetwork
Boot(a,b) == Command("bootstrap",a,b)
Call(a,b,ref,method,data) == [Command("call",a,b) EXCEPT
    !.ref=ref, !.wait={ref}, !.method=method, !.data=data]
Done(a,b,ref) == [Command("finish",a,b) EXCEPT !.ref=ref, !.wait={ref}]
\* @type: Seq($command);
BasicPlan == <<
    Boot(1,2),
    [Call(1,2,1,"factory",2) EXCEPT !.local=Object(1,2)],
    Done(1,2,2),
    [Done(1,2,1) EXCEPT !.after={3}]
>>
\* @type: Seq($command);
CancelPlan == <<
    Boot(1,2),
    [Call(1,2,1,"echo",0) EXCEPT !.local=Object(1,2)],
    [Command("finish",1,2) EXCEPT !.ref=2, !.after={2}],
    [Done(1,2,1) EXCEPT !.after={3}]
>>
\* @type: Seq($command);
PipelinePlan == <<
    Boot(1,2),
    [Call(1,2,1,"factory",2) EXCEPT !.local=Object(1,2)],
    [Command("pipeline",1,2) EXCEPT !.ref=2, !.after={2}, !.method="echo", !.local=Object(1,1)],
    [Command("finish",1,2) EXCEPT !.ref=2, !.after={3}],
    Done(1,2,3),
    [Done(1,2,1) EXCEPT !.after={4,5}]
>>
\* @type: Seq($command);
SameNumericIdsPlan == <<Boot(1,2), Boot(2,1), Boot(1,3),
    Done(1,2,1), Done(2,1,2), Done(1,3,3)>>
\* @type: Seq($command);
TailLocalPlan == <<Boot(1,2),
    [Call(1,2,1,"tail",0) EXCEPT !.local=Object(1,2)],
    Done(1,2,2), [Done(1,2,1) EXCEPT !.after={3}]>>
\* @type: Seq($command);
ThirdTailPlan == <<Boot(2,3), [Boot(1,2) EXCEPT !.wait={1}],
    [Command("proxy",2,0) EXCEPT !.ref=1, !.wait={1}, !.data=1],
    [Call(1,2,2,"factory",2) EXCEPT !.after={3}, !.local=Object(1,2)],
    Done(1,2,4), [Done(1,2,2) EXCEPT !.after={5}]>>
\* @type: Seq($command);
ScriptedLegacyHandoffPlan == <<
    Boot(2,3), [Boot(1,2) EXCEPT !.wait={1}],
    Call(1,2,2,"promise",1),
    [Command("call",1,2) EXCEPT !.ref=3, !.wait={3}, !.method="echo", !.data=0, !.order=1, !.flags=[Flags EXCEPT !.allowThird=FALSE]],
    [Command("resolve",2,0) EXCEPT !.ref=1, !.wait={1}, !.after={4}, !.data=1],
    [Command("accept",1,3) EXCEPT !.after={5}, !.embargo=1],
    [Command("barrier",1,2) EXCEPT !.after={6}, !.embargo=1],
    [Command("pipeline",1,3) EXCEPT !.ref=6, !.after={6}, !.method="echo", !.data=0, !.order=1, !.path= <<>>],
    [Done(1,3,8) EXCEPT !.wait={4,8}], [Done(1,3,6) EXCEPT !.after={9}],
    [Done(1,2,4) EXCEPT !.after={10}], [Done(1,2,3) EXCEPT !.after={11}],
    [Done(1,2,2) EXCEPT !.after={12}]
>>
\* The runtime chooses Accept, its embargo, and the new route. The application
\* makes both calls on exactly the same returned promise capability.
\* @type: Seq($command);
HandoffPlan == <<
    Boot(2,3), [Boot(1,2) EXCEPT !.wait={1}],
    Call(1,2,2,"promise",1),
    [Call(1,2,3,"echo",0) EXCEPT !.order=1, !.flags.allowThird=FALSE],
    [Command("resolve",2,0) EXCEPT !.ref=1, !.wait={1}, !.after={4}, !.data=1],
    [Call(1,2,3,"factory",2) EXCEPT !.after={5}, !.waitDirect=TRUE, !.order=1],
    [Call(1,3,6,"echo",0) EXCEPT !.wait={4,6}],
    Done(1,3,7), [Done(1,3,6) EXCEPT !.after={8}],
    [Done(1,2,4) EXCEPT !.after={9}], [Done(1,2,3) EXCEPT !.after={10}],
    [Done(1,2,2) EXCEPT !.after={11}], [Done(2,3,1) EXCEPT !.after={12}]
>>
\* A third-party capability appears directly in a Return's capability table.
\* @type: Seq($command);
ReturnHandoffPlan == <<
    Boot(2,3), [Boot(1,2) EXCEPT !.wait={1}],
    [Command("resolve",2,0) EXCEPT !.ref=1, !.wait={1}, !.data=1],
    [Call(1,2,2,"get",1) EXCEPT !.after={3}],
    [Call(1,2,4,"factory",2) EXCEPT !.waitDirect=TRUE],
    Call(1,3,5,"echo",0), Done(1,3,6),
    [Done(1,3,5) EXCEPT !.after={7}], [Done(1,2,4) EXCEPT !.after={8}],
    [Done(1,2,2) EXCEPT !.after={9}], [Done(2,3,1) EXCEPT !.after={10}]
>>
\* A Call parameter introduces a third-party capability; the callee invokes
\* that capability and returns through ordinary/tail-call lifecycle machinery.
\* @type: Seq($command);
ParameterHandoffPlan == <<
    Boot(1,3), [Boot(1,2) EXCEPT !.wait={1}],
    [Call(1,2,2,"tail",0) EXCEPT !.arg=1, !.flags.allowThird=FALSE],
    Done(1,2,3), [Done(1,2,2) EXCEPT !.after={4}],
    [Done(1,3,1) EXCEPT !.after={5}]
>>
\* @type: Seq($command);
CanceledReturnHandoffPlan == SubSeq(ReturnHandoffPlan,1,4) \o <<
    [Command("finish",1,2) EXCEPT !.ref=4, !.after={4}],
    [Done(1,2,2) EXCEPT !.after={5}], [Done(2,3,1) EXCEPT !.after={6}]
>>
\* @type: Seq($command);
ProviderLossHandoffPlan == <<
    Boot(2,3), [Boot(1,2) EXCEPT !.wait={1}],
    Call(1,2,2,"promise",1),
    [Command("resolve",2,0) EXCEPT !.ref=1, !.wait={3}, !.data=1],
    [Call(1,2,3,"echo",0) EXCEPT !.after={4}, !.waitDirect=TRUE],
    [Command("close",1,2) EXCEPT !.wait={5}],
    [Call(1,2,3,"echo",0) EXCEPT !.after={6}],
    Done(1,3,7), [Done(1,3,5) EXCEPT !.after={8}]
>>
ProvidedResourcesReleased == <>[](\A n \in 1..(net.next-1) :
    net.ops[n].kind="provide" => (net.ops[n].finishRecv \/ net.ops[n].phase="disconnected"))
AutomaticHandoffsComplete == <>[](\A n \in 1..(net.next-1) :
    net.ops[n].automatic => net.ops[n].handoffDone)
HandoffResourcesReleased == <>[](
    /\ \A n \in 1..(net.next-1) : net.ops[n].automatic =>
        (net.ops[n].handoffDone /\ net.questions[OSlot(net.ops[n])] # n /\ net.answers[OSlot(net.ops[n])] # n)
    /\ \A k \in Slots : net.exports[k].provision # 0 => net.exports[k].count=0)
VinesHeldUntilAcceptance == \A n \in 1..(net.next-1) :
    LET o==net.ops[n] r==o.vine
    IN (o.automatic /\ ~o.handoffDone /\ <<r.host,r.peer>> \in net.connected) =>
       (net.imports[<<<<r.host,r.peer>>,r.id>>].count>0 /\
        net.imports[<<<<r.host,r.peer>>,r.id>>].generation=r.generation)
NoRuntimeHandoffCallWitness == ~\E n \in 1..(net.next-1) :
    net.ops[n].kind="call" /\ net.ops[n].src=1 /\ net.ops[n].dst=3 /\
    net.ops[n].label=6 /\ net.ops[n].returnRecv
\* @type: Seq($command);
JoinPlan == <<
    Boot(2,4), [Boot(3,4) EXCEPT !.wait={1}], [Boot(1,2) EXCEPT !.wait={2}], [Boot(1,3) EXCEPT !.wait={3}],
    [Command("proxy",2,0) EXCEPT !.ref=1, !.wait={1}, !.data=1],
    [Command("proxy",3,0) EXCEPT !.ref=2, !.wait={2}, !.data=1],
    [Command("join",1,2) EXCEPT !.ref=3, !.wait={3,4}, !.after={5,6}, !.data=1, !.part=1, !.parts=2],
    [Command("join",1,3) EXCEPT !.ref=4, !.wait={4}, !.after={7}, !.data=1, !.part=2, !.parts=2],
    [Command("connectJoin",1,0) EXCEPT !.after={7,8}, !.data=1, !.parts=2],
    [Command("accept",1,4) EXCEPT !.after={9}, !.other=MaxOps+1],
    Call(1,4,10,"factory",2),
    [Done(1,2,7) EXCEPT !.after={10}], [Done(1,3,8) EXCEPT !.after={10}],
    Done(1,4,11), [Done(1,4,10) EXCEPT !.after={11}]
>>
\* @type: Seq($command);
NoFinishPlan == <<Boot(1,2),
    [Call(1,2,1,"echo",0) EXCEPT !.flags=[Flags EXCEPT !.noFinish=TRUE]],
    [Call(1,2,1,"echo",0) EXCEPT !.wait={2}, !.flags=[Flags EXCEPT !.noFinish=TRUE]],
    [Done(1,2,1) EXCEPT !.wait={3}]>>
\* @type: Seq($command);
OnlyPlan == <<Boot(1,2),
    [Call(1,2,1,"factory",2) EXCEPT !.flags=[Flags EXCEPT !.only=TRUE]],
    [Command("pipeline",1,2) EXCEPT !.ref=2, !.after={2}, !.method="echo"],
    [Command("finish",1,2) EXCEPT !.ref=2, !.after={3}],
    Done(1,2,3), [Done(1,2,1) EXCEPT !.after={5}]>>
\* @type: Seq($command);
OnlyCompatPlan == [OnlyPlan EXCEPT ![2].flags.honorOnly=FALSE]
\* @type: Seq($command);
NoPipelinePlan == <<Boot(1,2),
    [Call(1,2,1,"echo",0) EXCEPT !.flags=[Flags EXCEPT !.noPipeline=TRUE]],
    Done(1,2,2), [Done(1,2,1) EXCEPT !.after={3}]>>
\* @type: Seq($command);
ExplicitReleasePlan == [BasicPlan EXCEPT
    ![2].flags.releaseParams=FALSE, ![3].flags.releaseResults=FALSE]
\* @type: Seq($command);
WorkaroundPlan == <<Boot(1,2), Call(1,2,1,"promise",1),
    [Call(1,2,2,"echo",0) EXCEPT !.flags=[Flags EXCEPT !.workaround=TRUE]],
    [Command("finish",1,2) EXCEPT !.ref=3, !.after={3}],
    [Command("resolve",2,0) EXCEPT !.data=1, !.target=Object(2,1), !.after={4}],
    [Done(1,2,2) EXCEPT !.after={5}], [Done(1,2,1) EXCEPT !.after={6}]>>
\* @type: Seq($command);
BrokenPromisePlan == [WorkaroundPlan EXCEPT ![3].flags.workaround=FALSE, ![5].target=Broken]
\* @type: Seq($command);
PersistencePlan == <<Boot(1,2), Call(1,2,1,"save",1),
    [Command("close",1,2) EXCEPT !.wait={2}],
    [Command("reconnect",1,2) EXCEPT !.after={3}],
    [Boot(1,2) EXCEPT !.after={4}], Call(1,2,5,"restore",0),
    Call(1,2,6,"echo",0), Done(1,2,7),
    [Done(1,2,6) EXCEPT !.after={8}], [Done(1,2,5) EXCEPT !.after={9}]>>
\* @type: Seq($command);
LossPlan == <<Boot(1,2),
    [Command("close",1,2) EXCEPT !.after={1}]>>
\* @type: Seq($command);
UnsupportedPlan == <<Command("unsupported",1,2)>>
\* @type: Seq($command);
UnauthorizedAcceptPlan == <<[Command("accept",1,2) EXCEPT !.other=1], Done(1,2,1)>>
\* @type: Seq($command);
DirectProvidePlan == <<Boot(2,3),
    [Command("provide",2,3) EXCEPT !.ref=1, !.wait={1}, !.other=1],
    [Command("accept",1,3) EXCEPT !.ref=2, !.after={2}],
    Call(1,3,3,"echo",0), Done(1,3,4),
    [Done(1,3,3) EXCEPT !.after={5}],
    [Command("finish",2,3) EXCEPT !.ref=2, !.after={6}],
    [Done(2,3,1) EXCEPT !.after={7}]>>
\* @type: Seq($command);
ThirdTailFallbackPlan == [ThirdTailPlan EXCEPT ![4].flags.allowThird=FALSE]
\* @type: Seq($command);
ThirdTailCancelPlan == [ThirdTailPlan EXCEPT ![5].wait={}, ![5].after={4}]
\* @type: Seq($command);
ThirdTailPipelinePlan == SubSeq(ThirdTailPlan,1,4) \o <<
    [Command("pipeline",1,2) EXCEPT !.ref=4, !.after={4}, !.method="echo", !.flags.allowThird=FALSE],
    Done(1,2,5), [Done(1,2,4) EXCEPT !.after={6}],
    [Done(1,2,2) EXCEPT !.after={7}]>>
\* @type: Seq($command);
LocalJoinPlan == <<Boot(1,2), Call(1,2,1,"factory",2),
    [Command("join",1,2) EXCEPT !.ref=1, !.wait={2}, !.data=1, !.part=1, !.parts=2],
    [Command("join",1,2) EXCEPT !.ref=1, !.after={3}, !.data=1, !.part=2, !.parts=2],
    [Command("connectJoin",1,0) EXCEPT !.after={3,4}, !.data=1, !.parts=2],
    [Done(1,2,3) EXCEPT !.after={5}], [Done(1,2,4) EXCEPT !.after={6}]>>
\* @type: Seq($command);
UnequalJoinPlan == [LocalJoinPlan EXCEPT ![4].ref=2]
\* @type: Seq($command);
CanceledJoinPlan == [LocalJoinPlan EXCEPT ![5].kind="cancelJoin"]

\* @type: Seq($command);
NestedPipelinePlan == [PipelinePlan EXCEPT ![2].method="nested", ![3].path= <<-1,0,-1,0,-1>>]
\* @type: Seq($command);
InvalidPipelinePlan == [PipelinePlan EXCEPT ![3].path= <<0,0>>]

\* @type: Seq($command);
LoopbackPlan == <<Boot(2,1), [Boot(1,2) EXCEPT !.wait={1}], Call(1,2,2,"promise",1),
    [Call(1,2,3,"echo",0) EXCEPT !.flags.allowThird=FALSE],
    [Command("resolve",2,0) EXCEPT !.ref=1, !.wait={1}, !.after={4}, !.data=1],
    [Command("loopback",1,2) EXCEPT !.ref=3, !.wait={3}, !.after={5}, !.data=1, !.embargo=1],
    [Done(1,2,4) EXCEPT !.after={6}], [Done(1,2,3) EXCEPT !.after={7}],
    [Done(1,2,2) EXCEPT !.after={8}]>>
\* @type: Seq($command);
ReleasedResolvePlan == <<Boot(1,2), Call(1,2,1,"promise",1),
    [Command("resolve",2,0) EXCEPT !.data=1, !.target=Object(2,1), !.wait={2}],
    Done(1,2,2), [Done(1,2,1) EXCEPT !.after={3,4}]>>
NoLoopbackNetworkWitness == "loopback" \notin net.events
NoReleasedResolveNetworkWitness == "releasedResolve" \notin net.events
NoAcceptBeforeProvideNetworkWitness == ~\E n \in 1..(net.next-1) :
    net.ops[n].kind="accept" /\ net.ops[n].phase="waiting" /\
    ProvideOps(net,net.ops[n].completion,net.ops[n].dst)={}
NoLiveJoinRootsWitness == net.heldJoins # {} \/ net.steps # 1..Len(Plan)

\* @type: Seq($command);
PairPlan == [BasicPlan EXCEPT ![2].method="pair"]
\* @type: Seq($command);
DuplicatePlan == [BasicPlan EXCEPT ![2].method="duplicate"]
\* @type: Seq($command);
ReceiverAnswerPlan == <<Boot(1,2), Call(1,2,1,"factory",2),
    [Call(1,2,1,"tail",0) EXCEPT !.arg=2, !.argPromise=TRUE, !.after={2}],
    [Done(1,2,2) EXCEPT !.after={3}], Done(1,2,3),
    [Done(1,2,1) EXCEPT !.after={4,5}]>>

\* @type: Seq($command);
ExportedQuestionPlan == <<Boot(1,2), [Boot(1,3) EXCEPT !.wait={1}],
    [Call(1,2,1,"factory",2) EXCEPT !.wait={2}],
    [Call(1,3,2,"echo",0) EXCEPT !.arg=3, !.argPromise=TRUE, !.after={3}, !.flags.allowThird=FALSE],
    [Done(1,2,3) EXCEPT !.after={4}], Done(1,3,4),
    [Done(1,3,2) EXCEPT !.after={6}], [Done(1,2,1) EXCEPT !.after={7}]>>

\* @type: Seq($command);
AdoptedPipelinePlan == SubSeq(ThirdTailPlan,1,4) \o <<
    [Command("adoptedPipeline",1,3) EXCEPT !.ref=4, !.after={4}, !.method="echo", !.path= <<0>>],
    Done(1,3,5), [Done(1,2,4) EXCEPT !.after={6}],
    [Done(1,2,2) EXCEPT !.after={7}]>>

\* @type: Seq($command);
ProvideLossPlan == <<Boot(2,3),
    [Command("provide",2,3) EXCEPT !.ref=1, !.wait={1}, !.other=1],
    [Command("accept",1,3) EXCEPT !.ref=2, !.after={2}],
    [Command("close",2,3) EXCEPT !.after={3}],
    [Done(1,3,3) EXCEPT !.after={4}]>>

\* @type: Seq($command);
UnresolvedTailCancelPlan == SubSeq(ThirdTailPlan,1,3) \o <<
    [Command("proxy",3,0) EXCEPT !.data=1, !.target=Promise(3,1), !.after={3}],
    [Call(1,2,2,"factory",2) EXCEPT !.after={4}],
    [Command("finish",1,2) EXCEPT !.ref=5, !.after={5}],
    [Done(1,2,2) EXCEPT !.after={6}]>>
NoEarlyAdoptedFinishWitness == ~\E n \in 1..(net.next-1) :
    net.ops[n].class="adopted" /\ net.ops[n].finishRecv /\ ~net.ops[n].returnSent

\* @type: Seq($command);
AdoptedLossPlan == SubSeq(ThirdTailPlan,1,4) \o <<
    [Command("closeAdopted",1,3) EXCEPT !.ref=4, !.after={4}],
    [Done(1,2,4) EXCEPT !.after={5}], [Done(1,2,2) EXCEPT !.after={6}]>>
\* @type: Seq($command);
AdoptionFailurePlan == SubSeq(ThirdTailPlan,1,3) \o <<
    [Command("close",1,3) EXCEPT !.after={3}],
    [Call(1,2,2,"factory",2) EXCEPT !.after={4}],
    Done(1,2,5), [Done(1,2,2) EXCEPT !.after={6}]>>
NoAdoptionFailureWitness == "adoptionFailure" \notin net.events
NoUnequalNetworkWitness == "unequalJoin" \notin net.events
NoRestoreNetworkWitness == "restore" \notin net.events
NoCleanupNetworkWitness == ~\E n \in 1..(net.next-1) :
    net.ops[n].class="adopted" /\ net.ops[n].finishSent /\ net.ops[n].finishRecv /\ net.ops[n].returnRecv
=============================================================================
