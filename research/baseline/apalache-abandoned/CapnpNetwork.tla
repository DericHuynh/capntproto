--------------------------- MODULE CapnpNetwork ---------------------------
EXTENDS Integers, Sequences, FiniteSets, TLC, Apalache, CapnpTypes
\* Executable, composed RPC semantics. All message kinds use the SAME queues,
\* question/answer tables, capability tables, and ownership ledger.
\* Credits and paired operation records are auxiliary execution state, not
\* independent ghost observers. The trace observes application delivery.
CONSTANTS
    \* @type: Int;
    VatCount,
    \* @type: Int;
    IdCount,
    \* @type: Int;
    MaxOps,
    \* @type: Seq($command);
    Plan,
    \* @type: Str;
    Bug,
    \* @type: Bool;
    AllowLoss,
    \* @type: Bool;
    Introductions,
    \* @type: Bool;
    ExpectSuccess
Vats == 1..VatCount
Ids == 1..IdCount
Channels == {c \in Vats \X Vats : c[1] # c[2]}
Slots == Channels \X Ids
LocalSlots == Vats \X Ids
\* @type: $channel=>$channel;
Rev(c) == <<c[2], c[1]>>
\* @type: (Str, Int, Int, Int, Int, Seq(Int)) => $ref;
Ref(tag, host, peer, id, generation, path) ==
    [tag |-> tag, host |-> host, peer |-> peer, id |-> id,
     generation |-> generation, path |-> path]
Null == Ref("null", 0, 0, 0, 0, <<>>)
Broken == Ref("broken", 0, 0, 0, 0, <<>>)
\* @type: (Int, Int)=>$ref;
Object(v, id) == Ref("object", v, 0, id, 0, <<>>)
\* @type: (Int, Int)=>$ref;
Promise(v, id) == Ref("promise", v, 0, id, 0, <<>>)
\* @type: (Int, Int, Seq(Int))=>$ref;
Answer(v, op, path) == Ref("answer", v, 0, op, 0, path)
\* @type: (Int, Int, Int, Int)=>$ref;
Import(v, peer, id, gen) == Ref("import", v, peer, id, gen, <<>>)
\* @type: (Str, Int, Seq(Int)) => $target;
Target(kind, id, path) == [kind |-> kind, id |-> id, path |-> path]
NoTarget == Target("none", 0, <<>>)
\* @type: $flags;
Flags == [only |-> FALSE, noPipeline |-> FALSE, noFinish |-> FALSE,
          workaround |-> FALSE, allowThird |-> TRUE, honorOnly |-> TRUE,
          releaseParams |-> TRUE, releaseResults |-> TRUE]
\* @type: $export;
ExportEntry == [ref |-> Null, count |-> 0, generation |-> 0, resolved |-> FALSE,
                pinned |-> Null, provision |-> 0]
\* @type: $import;
ImportEntry == [count |-> 0, generation |-> 0, resolution |-> Null,
                offer |-> <<>>, handoff |-> 0]
\* @type: $op;
Operation == [src |-> 0, dst |-> 0, id |-> 0, class |-> "ordinary", kind |-> "call",
    phase |-> "unused", arrival |-> 0, target |-> Null, route |-> Null, path |-> <<>>, flags |-> Flags,
    params |-> <<>>, paramTickets |-> {}, result |-> <<>>, qResult |-> <<>>,
    content |-> <<>>, qContent |-> <<>>, resultTickets |-> {}, outcome |-> "none", observed |-> "none",
    finishWanted |-> FALSE, finishSent |-> FALSE, finishRecv |-> FALSE, returnSent |-> FALSE, returnRecv |-> FALSE,
    forward |-> 0, qRedirect |-> 0, adoption |-> 0, taken |-> FALSE, destination |-> "caller", completion |-> 0,
    embargo |-> 0, join |-> 0, part |-> 0, parts |-> 0, joinHost |-> 0, ordinal |-> 0,
    method |-> "echo", data |-> 0, logical |-> 0, label |-> 0, dispatched |-> FALSE,
    automatic |-> FALSE, vine |-> Null, handoffDone |-> FALSE]
\* @type: $message;
Message == [kind |-> "none", id |-> 0, class |-> "ordinary", op |-> 0,
    target |-> NoTarget, caps |-> <<>>, content |-> <<>>, tickets |-> {}, flags |-> Flags,
    result |-> "none", other |-> 0, completion |-> 0, embargo |-> 0,
    join |-> 0, part |-> 0, parts |-> 0, host |-> 0, ordinal |-> 0,
    method |-> "echo", data |-> 0, destination |-> "caller", count |-> 0,
    context |-> "none", echoed |-> "none"]
\* @type: (Str, Int, Seq(Int), Int, Int) => $desc;
Desc(kind, id, path, completion, host) ==
    [kind |-> kind, id |-> id, path |-> path, completion |-> completion, host |-> host]
VARIABLE
    \* @type: $state;
    net
\* @type: <<$state>>;
vars == <<net>>
TableSlots == Channels \X {"ordinary", "adopted", "pipeline"} \X Ids
\* @type: ($channel, Str, Int)=>$tableSlot;
QSlot(c, cls, id) == <<c, cls, id>>
\* @type: $op=>$tableSlot;
OSlot(o) == QSlot(<<o.src, o.dst>>, o.class, o.id)
Init == net = [
    connected |-> Channels, closing |-> {}, wire |-> [c \in Channels |-> <<>>],
    questions |-> [k \in TableSlots |-> 0], answers |-> [k \in TableSlots |-> 0],
    exports |-> [k \in Slots |-> ExportEntry], imports |-> [k \in Slots |-> ImportEntry],
    ops |-> [i \in 1..MaxOps |-> Operation], next |-> 1,
    arrivals |-> [v \in Vats |-> 0],
    promises |-> [k \in LocalSlots |-> Null], proxies |-> [k \in LocalSlots |-> Null],
    credits |-> <<>>, labels |-> [i \in DOMAIN Plan |-> 0], steps |-> {},
    barriers |-> {}, embargoQueue |-> <<>>, offers |-> {}, accepted |-> {}, shortcuts |-> {},
    joinCaps |-> {}, heldJoins |-> {}, shares |-> {}, awaiting |-> [i \in 1..MaxOps |-> 0], adopted |-> [i \in 1..MaxOps |-> 0],
    grants |-> {}, joinGroups |-> {}, joins |-> [i \in DOMAIN Plan |-> "unused"],
    sturdy |-> {}, deliveries |-> <<>>, events |-> {}, faults |-> {}]
\* @type: ($state, Str)=>$state;
PutEvent(state, e) ==
    (LET s == state IN  [s EXCEPT !.events = @ \cup {e}]
    )
\* @type: ($state, Str)=>$state;
Fault(state, e) ==
    (LET s == state IN  [s EXCEPT !.faults = @ \cup {e}]
    )
\* @type: ($state, $channel, $message)=>$state;
Enqueue(state, c, m) ==
    (LET s == state IN  IF c \in s.closing THEN s ELSE [s EXCEPT !.wire[c] = Append(@, m)]
    )
\* @type: ($state, Set(Int))=>Set(Int);
CreditSet(s, ids) ==
 {i \in 1..Len(s.credits) : i \in ids}
\* @type: Set(Int) => Int;
Min(xs) ==
    LET \* @type: ({empty: Bool, value: Int}, Int) => {empty: Bool, value: Int};
        Step(acc,x) == [empty |-> FALSE, value |-> IF acc.empty \/ x<acc.value THEN x ELSE acc.value]
    IN ApaFoldSet(Step,[empty |-> TRUE, value |-> 0],xs).value
\* @type: $flags=>Str;
Class(flags) == IF flags.only THEN "pipeline" ELSE "ordinary"
\* @type: ($state, $channel, Str)=>Set(Int);
FreeQuestion(s, c, cls) ==
 {id \in Ids : s.questions[QSlot(c, cls, id)] = 0 /\
    (cls # "adopted" \/ s.answers[QSlot(c,cls,id)]=0) /\
    (cls # "pipeline" \/ ~\E i \in 1..(s.next-1) :
        s.ops[i].src = c[1] /\ s.ops[i].dst = c[2] /\ s.ops[i].class = cls /\ s.ops[i].id = id)}
\* @type: ($state, $channel, $flags)=>Bool;
CanAsk(s, c, flags) ==
 c \in s.connected /\ c \notin s.closing /\ s.next <= MaxOps /\ FreeQuestion(s,c,Class(flags)) # {}

\* Payload content is a finite pointer graph separate from the capability table.
\* Nodes: <<kind, capTableIndex, pointerNodeIds>>. Root is the last node.
\* Missing, scalar, null, non-capability and out-of-bounds targets are broken.
\* @type: Int => $node;
CapNode(i) == <<"cap",i,<<>>>>
\* @type: Seq(Int) => $node;
StructNode(ptrs) == <<"struct",0,ptrs>>
\* @type: (Seq(a), (Int => b)) => Seq(b);
MapSeqIndices(xs,F(_)) == ApaFoldSeqLeft(LAMBDA acc,x:Append(acc,F(Len(acc)+1)),<<>>,xs)
\* @type: Seq($ref) => Seq($node);
FlatContent(caps) == MapSeqIndices(caps,LAMBDA i:CapNode(i)) \o
                     <<StructNode(MapSeqIndices(caps,LAMBDA i:i))>>
\* @type: Seq($node);
RawContent == <<CapNode(1)>>
\* @type: Seq($node);
NestedContent == <<CapNode(1),StructNode(<<1>>),StructNode(<<2>>) >>
\* @type: (Seq($ref), Seq($node), Int, Seq(Int))=>$ref;
WalkContent(caps,nodes,node,path) ==
    LET \* @type: (Int, Int) => Int;
        Step(current,index) ==
        IF current \notin 1..Len(nodes) THEN 0
        ELSE IF index=-1 THEN current
        ELSE LET x==nodes[current]
             IN IF x[1]="struct" /\ index \in 0..(Len(x[3])-1)
                THEN x[3][index+1] ELSE 0
        last==ApaFoldSeqLeft(Step,node,path)
    IN IF last \notin 1..Len(nodes) THEN Broken
       ELSE LET x==nodes[last]
            IN IF x[1]="cap" /\ x[2] \in 1..Len(caps) THEN caps[x[2]] ELSE Broken
\* @type: (Seq($ref), Seq($node), Seq(Int))=>$ref;
AtPath(caps,content,path) == WalkContent(caps,content,Len(content),path)


\* The finite universe is closed under the synthetic Answer/Question edges
\* made by Follow. Each traversal either terminates or repeats a vertex.
\* One additional sentinel iteration detects a cycle after visiting every
\* vertex. This is an exact fold, not a user-selected recursion cutoff.
\* @type: Seq($ref) => Set($ref);
RefSet(xs) == {xs[i] : i \in 1..Len(xs)}
\* @type: ($state, $ref) => Set($ref);
ReferenceUniverse(s,reference) ==
    LET base=={Null,Broken,reference} \cup
        {s.promises[k] : k \in LocalSlots} \cup {s.proxies[k] : k \in LocalSlots} \cup
        {s.exports[k].ref : k \in Slots} \cup {s.exports[k].pinned : k \in Slots} \cup
        {s.imports[k].resolution : k \in Slots} \cup
        {s.ops[n].target : n \in 1..MaxOps} \cup {s.ops[n].route : n \in 1..MaxOps} \cup
        UNION {RefSet(s.ops[n].result) \cup RefSet(s.ops[n].qResult) \cup RefSet(s.ops[n].params) : n \in 1..MaxOps} \cup
        {x[1] : x \in s.shortcuts} \cup {x[2] : x \in s.shortcuts} \cup
        {x[3] : x \in s.sturdy}
        paths=={r.path : r \in base}
    IN base \cup
       {Answer(v,n,path) : v \in Vats, n \in 1..MaxOps, path \in paths} \cup
       {Ref("question",v,s.ops[n].dst,n,0,path) : v \in Vats, n \in 1..MaxOps, path \in paths} \cup
       {Ref("traversalSentinel",0,0,0,0,<<>>)}
\* @type: $ref => {ref: $ref, done: Bool};
StopRef(r) == [ref |-> r, done |-> TRUE]
\* @type: $ref => {ref: $ref, done: Bool};
ContinueRef(r) == [ref |-> r, done |-> FALSE]
\* @type: ($state, $ref) => {ref: $ref, done: Bool};
FollowOne(s,r) ==
    CASE r.tag="promise" ->
         LET t==s.promises[<<r.host,r.id>>] IN IF t=Null THEN StopRef(r) ELSE ContinueRef(t)
    [] r.tag="answer" ->
         IF s.ops[r.id].class="adopted"
         THEN ContinueRef(Answer(r.host,s.ops[r.id].forward,r.path))
         ELSE IF s.ops[r.id].forward # 0
         THEN StopRef(Ref("question",r.host,s.ops[s.ops[r.id].forward].dst,s.ops[r.id].forward,0,r.path))
         ELSE IF s.ops[r.id].outcome="none" THEN StopRef(r)
         ELSE IF s.ops[r.id].outcome="results"
              THEN ContinueRef(AtPath(s.ops[r.id].result,s.ops[r.id].content,r.path)) ELSE StopRef(Broken)
    [] r.tag="question" ->
         LET o==s.ops[r.id]
         IN IF o.observed="results" THEN ContinueRef(AtPath(o.qResult,o.qContent,r.path))
            ELSE IF o.observed \in {"exception","canceled","disconnected"} THEN StopRef(Broken)
            ELSE StopRef(Ref("question",r.host,o.dst,r.id,0,r.path))
    [] r.tag="vine" -> ContinueRef(s.ops[r.id].route)
    [] r.tag="object" ->
         LET t==s.proxies[<<r.host,r.id>>] IN IF t=Null THEN StopRef(r) ELSE ContinueRef(t)
    [] r.tag="import" ->
         IF <<r.host,r.peer>> \notin s.connected \/
            s.imports[<<<<r.host,r.peer>>,r.id>>].generation # r.generation \/
            s.imports[<<<<r.host,r.peer>>,r.id>>].count=0 THEN StopRef(Broken) ELSE StopRef(r)
    [] OTHER -> StopRef(r)
\* @type: (($ref => {ref: $ref, done: Bool}), Set($ref), $ref, Set($ref)) => $ref;
WalkRefs(Hop(_),universe,reference,seen) ==
    LET \* @type: ({ref: $ref, done: Bool, seen: Set($ref)}, $ref) => {ref: $ref, done: Bool, seen: Set($ref)};
        Step(acc,unused) ==
        IF acc.done THEN acc
        ELSE IF acc.ref \in acc.seen THEN [acc EXCEPT !.ref=Broken, !.done=TRUE]
        ELSE LET hop==Hop(acc.ref)
             IN [ref |-> hop.ref, done |-> hop.done, seen |-> acc.seen \cup {acc.ref}]
    IN ApaFoldSet(Step,[ref |-> reference, done |-> FALSE, seen |-> seen],universe).ref
\* @type: ($state, $ref, Set($ref)) => $ref;
Follow(s,reference,seen) ==
    WalkRefs(LAMBDA r:FollowOne(s,r),ReferenceUniverse(s,reference),reference,seen)
\* @type: ($state, $ref)=>Bool;
Ready(s, r) ==
 Follow(s,r,{}).tag \notin {"promise", "answer"}
\* Local capability handles can outlive their old wire import. Shortcuts are
\* keyed by the whole reference, including its connection/import generation.
\* Follow deliberately does not consult them: old exported promise edges
\* must continue forwarding to the exact announced reference (Tribble rule).
\* @type: ($state, $ref) => {ref: $ref, done: Bool};
ClientOne(s,r) ==
    LET routes=={x \in s.shortcuts : x[1]=r}
    IN IF routes # {} THEN ContinueRef(Guess(routes)[2])
       ELSE IF r.tag="import"
            THEN LET 
        \* @type: $slot;
        k == <<<<r.host,r.peer>>,r.id>> e==s.imports[k]
                 IN IF FollowOne(s,r).ref=Broken THEN StopRef(Broken)
                    ELSE IF (\E x \in s.shortcuts : x[1]=e.resolution) \/
                            (e.resolution.tag="import" /\
                             s.imports[<<<<e.resolution.host,e.resolution.peer>>,e.resolution.id>>].offer # <<>>)
                         THEN ContinueRef(e.resolution) ELSE StopRef(r)
            ELSE LET followed==Follow(s,r,{})
                 IN IF followed=r THEN StopRef(r) ELSE ContinueRef(followed)
\* @type: ($state, $ref, Set($ref)) => $ref;
ClientRef(s,reference,seen) ==
    WalkRefs(LAMBDA r:ClientOne(s,r),ReferenceUniverse(s,reference),reference,seen)
\* @type: ($state, $ref, $ref)=>$state;
SetShortcut(state,old,replacement) == (LET s == state IN 
    [s EXCEPT !.shortcuts={x \in @ : x[1] # old} \cup {<<old,replacement>>}])
\* @type: ($state, $ref)=>Bool;
ClientReady(s,reference) == (LET r == ClientRef(s,reference,{}) IN 
    IF r.tag # "import" THEN TRUE
    ELSE LET e==s.imports[<<<<r.host,r.peer>>,r.id>>]
         IN e.offer= <<>> \/ e.handoff # 0 \/ ~Introductions)
\* @type: Str=>Int;
ClassCode(cls) == CASE cls="ordinary" -> 1 [] cls="adopted" -> 2 [] OTHER -> 3
\* @type: Int=>Str;
CodeClass(code) == CASE code=1 -> "ordinary" [] code=2 -> "adopted" [] OTHER -> "pipeline"
\* @type: ($state, $channel, $target)=>$ref;
DecodeTarget(s,c,t) ==
    IF t.kind = "importedCap"
    THEN IF t.id \in Ids /\ s.exports[<<Rev(c),t.id>>].count > 0
         THEN LET e==s.exports[<<Rev(c),t.id>>]
              IN IF e.resolved THEN e.pinned ELSE e.ref ELSE Broken
    ELSE IF t.kind = "promisedAnswer"
         THEN LET slot == QSlot(c,CodeClass(t.path[1]),t.id)
                  op == s.answers[slot]
              IN IF op = 0 THEN Broken
                 ELSE IF s.ops[op].flags.noPipeline THEN Broken
                 ELSE Answer(c[2],op,Tail(t.path))
    ELSE Broken
\* @type: ($state, $ref)=>$target;
WireTarget(s,r) ==
    IF r.tag = "import" THEN Target("importedCap",r.id,<<>>)
    ELSE IF r.tag = "question" THEN Target("promisedAnswer",s.ops[r.id].id,
                                      <<ClassCode(s.ops[r.id].class)>> \o r.path)
    ELSE NoTarget
\* @type: (Int, Int, Seq(Int))=>$ref;
RemoteQuestion(v, op, path) == Ref("question",v,0,op,0,path)

\* EncodePayload allocates/reuses actual export IDs. Each descriptor occurrence
\* creates one credit; receiverHosted/receiverAnswer/none create none.
\* @type: ($state, $channel, $ref)=>Set(Int);
ExportCandidates(s,c,r) ==
 {id \in Ids : s.exports[<<c,id>>].count > 0 /\
                              s.exports[<<c,id>>].ref = r /\ ~s.exports[<<c,id>>].resolved}
\* @type: ($state, $channel)=>Set(Int);
FreeExports(s,c) ==
 {id \in Ids : s.exports[<<c,id>>].count = 0}
\* @type: ($state, $channel, $ref)=>Bool;
CanEncode(s,c,r) ==
 r.tag="null" \/
    (r.tag = "import" /\ r.peer = c[2]) \/
    (r.tag = "question" /\ s.ops[r.id].dst=c[2]) \/
    ((ExportCandidates(s,c,r) # {} \/ FreeExports(s,c) # {}) /\
      ((Introductions /\ r.tag="import" /\ r.peer # c[2]) =>
         (CanAsk(s,<<c[1],r.peer>>,Flags) /\ FreeExports(s,c) # {})))
\* @type: ($state, $channel, Str, $target, {state: $state, descriptors: Seq($desc), tickets: Set(Int)}, $flags, Str, Int, Str, Int, Int, Int, Int, Int, Int, Int)=>$state;
AskEncoded(state,c,kind,target,encoded,flags,method,data,destination,completion,embargo,join,part,parts,label,logical) ==
    (LET s == state IN 
    LET n == encoded.state.next id == Min(FreeQuestion(encoded.state,c,Class(flags)))
        op == [Operation EXCEPT !.src=c[1], !.dst=c[2], !.id=id, !.class=Class(flags),
                !.kind=kind, !.phase="sent",
                !.route=IF target.kind="importedCap" THEN Import(c[1],c[2],target.id,s.imports[<<c,target.id>>].generation) ELSE Null, !.flags=flags, !.paramTickets=encoded.tickets,
                !.method=method, !.data=data, !.destination=destination, !.completion=completion,
                !.embargo=embargo, !.join=join, !.part=part, !.parts=parts,
                !.label=label, !.logical=IF logical=0 THEN n ELSE logical]
        m == [Message EXCEPT !.kind=kind, !.id=id, !.class=op.class, !.op=n,
                !.target=target, !.caps=encoded.descriptors, !.tickets=encoded.tickets,
                !.flags=flags, !.method=method, !.data=data, !.destination=destination,
                !.completion=completion, !.embargo=embargo, !.join=join, !.part=part, !.parts=parts]
        out == [encoded.state EXCEPT !.next=@+1, !.ops[n]=op,
                !.questions[OSlot(op)]=n]
    IN Enqueue(out,c,m)
    )

\* @type: ($state, $channel, $ref)=>{state: $state, descriptor: $desc, ticket: Int};
ExportOne(state,c,r) == (LET s == state IN 
    LET candidates == ExportCandidates(s,c,r)
             id == Min(IF candidates # {} THEN candidates ELSE FreeExports(s,c))
             
        \* @type: $slot;
        k == <<c,id>>
             gen == s.exports[k].generation + IF s.exports[k].count = 0 THEN 1 ELSE 0
             ticket == Len(s.credits)+1
             e == [IF s.exports[k].count=0 THEN ExportEntry ELSE s.exports[k] EXCEPT !.ref = r, !.count = s.exports[k].count+1,
                                      !.generation = gen]
             out == [s EXCEPT !.exports[k] = e, !.credits = Append(@,
                       [channel |-> c, id |-> id, generation |-> gen, phase |-> "outgoing"])]
         IN [state |-> out, ticket |-> ticket,
             descriptor |-> Desc(IF r.tag \in {"promise","answer","question","broken"}
                                  THEN "senderPromise" ELSE "senderHosted",id,<<>>,0,0)]
    )

\* @type: ($state, $channel, $ref)=>{state: $state, descriptor: $desc, ticket: Int};
EncodeOne(state,c,r) ==
    (LET s == state IN 
    IF r = Null THEN [state |-> s, descriptor |-> Desc("none",0,<<>>,0,0), ticket |-> 0]
    ELSE IF r.tag = "import" /\ r.peer = c[2]
         THEN [state |-> s, descriptor |-> Desc("receiverHosted",r.id,<<>>,0,0), ticket |-> 0]
    ELSE IF r.tag = "question" /\ s.ops[r.id].dst = c[2]
         THEN [state |-> s, descriptor |-> Desc("receiverAnswer",s.ops[r.id].id,
                  <<ClassCode(s.ops[r.id].class)>> \o r.path,0,0), ticket |-> 0]
    ELSE IF Introductions /\ r.tag="import" /\ r.peer # c[2]
         THEN LET p==s.next
                  provided==AskEncoded(s,<<c[1],r.peer>>,"provide",WireTarget(s,r),
                                [state |-> s, descriptors |-> <<>>, tickets |-> {}],Flags,
                                "echo",0,"caller",p,0,0,0,0,0,0)
              IN (LET enc == ExportOne(provided,c,Ref("vine",c[1],r.peer,p,0,<<>>)) IN 
                  [state |-> [enc.state EXCEPT !.exports[<<c,enc.descriptor.id>>].provision=p,
                                               !.grants=@ \cup {<<p,r.peer,c[2]>>}],
                   descriptor |-> [enc.descriptor EXCEPT !.kind="thirdPartyHosted",
                                                        !.completion=p, !.host=r.peer],
                   ticket |-> enc.ticket])
    ELSE ExportOne(s,c,r))

\* @type: ($state, $channel, Seq($ref))=>{state: $state, descriptors: Seq($desc), tickets: Set(Int)};
EncodePayload(state,c,refs) ==
    LET \* @type: ({state: $state, descriptors: Seq($desc), tickets: Set(Int)}, $ref) => {state: $state, descriptors: Seq($desc), tickets: Set(Int)};
        Step(acc,r) ==
        LET enc==EncodeOne(acc.state,c,r)
        IN [state |-> enc.state, descriptors |-> Append(acc.descriptors,enc.descriptor),
            tickets |-> acc.tickets \cup (IF enc.ticket=0 THEN {} ELSE {enc.ticket})]
    IN ApaFoldSeqLeft(Step,[state |-> state, descriptors |-> <<>>, tickets |-> {}],refs)

\* Counter transitions use descriptor-occurrence credits; conservation checks
\* their consistency. Wire/local-state refinement is a separate obligation.
\* @type: ($state, Set(Int), $channel, Int)=>Int;
CreditCounts(s,ts,c,id) ==
 Cardinality({t \in ts : s.credits[t].channel = c /\ s.credits[t].id = id})
\* @type: ($state, Set(Int))=>$state;
ImportCredits(state,ts) ==
    (LET s == state IN 
    [s EXCEPT !.imports = [k \in Slots |-> [s.imports[k] EXCEPT !.count = @ +
          CreditCounts(s,{t \in ts : s.credits[t].phase = "outgoing"},Rev(k[1]),k[2]),
          !.resolution = IF s.imports[k].count=0 THEN Null ELSE @,
          !.offer = IF s.imports[k].count=0 THEN <<>> ELSE @,
          !.handoff = IF s.imports[k].count=0 THEN 0 ELSE @,
          !.generation = @ + IF s.imports[k].count=0 /\
            CreditCounts(s,{t \in ts : s.credits[t].phase="outgoing"},Rev(k[1]),k[2])>0 THEN 1 ELSE 0]],
       !.credits = MapSeqIndices(s.credits,LAMBDA i : IF i \in ts
          THEN [s.credits[i] EXCEPT !.phase = IF @ = "outgoing" THEN "held" ELSE "released"]
          ELSE s.credits[i])]
    )
\* @type: ($state, Set(Int))=>$state;
ReleaseImports(state,ts) ==
    (LET s == state IN 
    [s EXCEPT !.imports = [k \in Slots |-> [s.imports[k] EXCEPT !.count = @ -
          CreditCounts(s,{t \in ts : s.credits[t].phase = "held"},Rev(k[1]),k[2])]],
       !.credits = MapSeqIndices(s.credits,LAMBDA i : IF i \in ts /\ s.credits[i].phase = "held"
          THEN [s.credits[i] EXCEPT !.phase = "releasing"] ELSE s.credits[i])]
    )
\* @type: ($state, Set(Int))=>$state;
ReleaseExports(state,ts) ==
    (LET s == state IN 
    [s EXCEPT !.exports = [k \in Slots |-> [s.exports[k] EXCEPT !.count = @ -
          CreditCounts(s,{t \in ts : s.credits[t].phase \in {"outgoing","releasing"}},k[1],k[2])]],
       !.credits = MapSeqIndices(s.credits,LAMBDA i : IF i \in ts
          THEN [s.credits[i] EXCEPT !.phase = IF @ = "outgoing" THEN "releasedTransit" ELSE "released"]
          ELSE s.credits[i])]
    )
\* @type: ($state, $channel, $desc)=>$ref;
DecodeOne(s,c,d) ==
    CASE d.kind = "none" -> Null
      [] d.kind \in {"senderHosted","senderPromise","thirdPartyHosted"} ->
           Import(c[2],c[1],d.id,s.imports[<<Rev(c),d.id>>].generation)
      [] d.kind = "receiverHosted" -> s.exports[<<Rev(c),d.id>>].ref
      [] d.kind = "receiverAnswer" -> DecodeTarget(s,c,Target("promisedAnswer",d.id,d.path))
      [] OTHER -> Broken
\* @type: ($state, $channel, Seq($desc))=>Seq($ref);
DecodePayload(s,c,ds) ==
 ApaFoldSeqLeft(LAMBDA acc,d : Append(acc,DecodeOne(s,c,d)),<<>>,ds)
\* Every payload-bearing message can carry introductions, including Call
\* parameters and Return results. Discarded payloads do not start handoffs.
\* @type: ($state, $channel, Seq($desc))=>$state;
RegisterOffers(state,c,ds) == (LET s == state IN 
    LET indices=={i \in 1..Len(ds) : ds[i].kind="thirdPartyHosted"}
    IN [s EXCEPT
        !.offers=@ \cup {<<c[2],c[1],ds[i].host,ds[i].completion,ds[i].id>> : i \in indices},
        !.imports=[k \in Slots |->
            LET matching=={i \in indices : k = <<Rev(c),ds[i].id>>}
            IN IF matching={} THEN s.imports[k]
               ELSE LET d==ds[Min(matching)]
                    IN [s.imports[k] EXCEPT !.offer= <<d.host,d.completion>>]]])

\* A single allocator/lifetime implementation serves Call, Bootstrap, Provide,
\* Accept, and Join. Forwarding uses it too, never a parallel side table.
\* @type: ($state, $channel, Str, $target, Seq($ref), $flags, Str, Int, Str, Int, Int, Int, Int, Int, Int, Int)=>$state;
Ask(state,c,kind,target,params,flags,method,data,destination,completion,embargo,join,part,parts,label,logical) ==
    AskEncoded(state,c,kind,target,EncodePayload(state,c,params),flags,method,data,destination,
               completion,embargo,join,part,parts,label,logical)
\* @type: $op=>Bool;
QuestionDone(o) == (o.finishSent /\ (o.returnRecv \/ o.kind="provide" \/ o.flags.only)) \/
                  (o.returnRecv /\ o.flags.noFinish)
\* @type: $op=>Bool;
AnswerDone(o) == (o.finishRecv /\ (o.returnSent \/ o.kind="provide" \/ o.flags.only)) \/
                (o.returnSent /\ o.flags.noFinish)
\* @type: ($state, Int)=>$state;
Reap(state,n) ==
    (LET s == state IN 
    LET o == s.ops[n]
    IN [s EXCEPT !.questions[OSlot(o)] = IF QuestionDone(o) /\ @=n THEN 0 ELSE @,
                 !.answers[OSlot(o)] = IF AnswerDone(o) /\ @=n THEN 0 ELSE @]
    )
\* @type: ($ref, $channel, Int)=>Bool;
UsesImport(reference,c,id) == (LET r == reference IN 
    r.tag="import" /\ r.host=c[1] /\ r.peer=c[2] /\ r.id=id)
\* @type: ($state, $channel, Int)=>Bool;
ImportInUse(s,c,id) ==
    (s.imports[<<c,id>>].offer # <<>> /\
        (s.imports[<<c,id>>].handoff=0 \/ ~s.ops[s.imports[<<c,id>>].handoff].handoffDone)) \/
    (\E k \in Slots : s.imports[k].count>0 /\
        (UsesImport(ClientRef(s,s.imports[k].resolution,{}),c,id) \/
         LET original==Import(k[1][1],k[1][2],k[2],s.imports[k].generation)
             routed==ClientRef(s,original,{})
         IN routed # original /\ UsesImport(routed,c,id))) \/
    (\E n \in 1..(s.next-1) : ~s.ops[n].finishSent /\
        (\E i \in 1..Len(s.ops[n].qResult) :
            LET original==s.ops[n].qResult[i] routed==ClientRef(s,original,{})
            IN routed # original /\ UsesImport(routed,c,id))) \/
    (\E k \in Slots : s.exports[k].count>0 /\
        (UsesImport(Follow(s,s.exports[k].ref,{}),c,id) \/ UsesImport(s.exports[k].pinned,c,id))) \/
    (\E k \in LocalSlots : UsesImport(s.promises[k],c,id) \/ UsesImport(s.proxies[k],c,id)) \/
    (\E n \in 1..(s.next-1) : ~s.ops[n].finishRecv /\
        (\E i \in 1..Len(s.ops[n].result) : UsesImport(s.ops[n].result[i],c,id))) \/
    (\E n \in 1..(s.next-1) : s.ops[n].phase \in {"waiting","running","forwarded","provided"} /\
        (UsesImport(s.ops[n].target,c,id) \/ UsesImport(Follow(s,s.ops[n].target,{}),c,id) \/
         UsesImport(s.ops[n].route,c,id) \/
         (\E p \in 1..Len(s.ops[n].params) : UsesImport(s.ops[n].params[p],c,id) \/
             UsesImport(ClientRef(s,s.ops[n].params[p],{}),c,id))))
\* @type: ($state, Set(Int))=>Bool;
CanDrop(s,ts) == \A t \in ts :
    ~ImportInUse(s,Rev(s.credits[t].channel),s.credits[t].id)
\* @type: ($state, Int)=>Bool;
CanFinish(s,n) == IF Bug="finishExportedQuestion" THEN TRUE ELSE
    /\ ~\E k \in Slots : s.exports[k].count>0 /\ ~s.exports[k].resolved /\
          s.exports[k].ref.tag="question" /\ s.exports[k].ref.id=n
    /\ ~\E i \in 1..(s.next-1) : s.ops[i].phase="waiting" /\
          LET r==Follow(s,s.ops[i].target,{})
          IN r.tag="question" /\ r.id=n
\* @type: ($state, Int, Bool)=>$state;
Finish(state,n,release) ==
    (LET s == state IN 
    IF ~CanFinish(s,n) THEN [s EXCEPT !.ops[n].finishWanted=TRUE, !.ops[n].flags.releaseResults=release]
    ELSE LET o == s.ops[n]
        drop == release /\ CanDrop(s,o.resultTickets)
        out == IF drop THEN ReleaseImports(s,o.resultTickets) ELSE s
        updated == [out EXCEPT !.ops[n].finishSent=TRUE, !.ops[n].finishWanted=FALSE, !.ops[n].flags.releaseResults=drop]
        m == [Message EXCEPT !.kind="finish", !.id=o.id, !.class=o.class,
                              !.flags=[o.flags EXCEPT !.releaseResults=drop]]
    IN IF o.returnRecv /\ o.flags.noFinish THEN updated
       ELSE Enqueue(Reap(updated,n),<<o.src,o.dst>>,m)
    )
\* @type: ($state, Int, Str, Seq($ref), Seq($node), Int, Int, Int)=>$state;
Return(state,n,kind,refs,content,other,host,ordinal) ==
    (LET s == state IN 
    LET o == s.ops[n] 
        \* @type: $channel;
        c == <<o.dst,o.src>>
        actual == IF o.finishRecv THEN <<>> ELSE refs
        IN (LET enc == EncodePayload(s,c,actual) IN 
    LET rp == o.flags.releaseParams /\ o.phase="done" /\ CanDrop(enc.state,o.paramTickets)
        released == IF rp THEN ReleaseImports(enc.state,o.paramTickets) ELSE enc.state
        updated == [released EXCEPT !.ops[n].returnSent=TRUE, !.ops[n].resultTickets=enc.tickets]
        m == [Message EXCEPT !.kind="return", !.id=o.id, !.class=o.class,
                !.caps=enc.descriptors, !.content=content, !.tickets=enc.tickets, !.result=kind,
                !.flags=[o.flags EXCEPT !.releaseParams=rp], !.other=other, !.completion=o.completion,
                !.host=host, !.ordinal=ordinal]
    IN Enqueue(Reap(updated,n),c,m)
    ))
\* @type: ($state, $channel, Set(Int))=>$state;
SendRelease(state,c,ts) ==
    (LET s == state IN 
    LET out == ReleaseImports(s,ts)
        id == Min({s.credits[t].id : t \in ts})
        m == [Message EXCEPT !.kind="release", !.id=id, !.count=Cardinality(ts), !.tickets=ts]
    IN Enqueue(out,c,m)
    )
\* @type: ($state, Set(Int))=>$state;
FinishProvisions(state,ns) ==
    LET \* @type: ($state, Int) => $state;
        Step(s,n) == IF n \notin ns \/ s.ops[n].finishSent \/ s.ops[n].phase="disconnected"
                    THEN s ELSE Finish(s,n,TRUE)
    IN ApaFoldSeqLeft(Step,state,MkSeq(MaxOps,LAMBDA n:n))

\* @type: ($state, $channel)=>$state;
Close(state,c) ==
    (LET before == state IN 
    LET provisions=={before.exports[k].provision : k \in
        {slot \in Slots : slot[1] \in {c,Rev(c)} /\ before.exports[slot].provision # 0}}
    IN (LET s == FinishProvisions(before,provisions) IN 
    LET lost == {c,Rev(c)}
        ts == {t \in 1..Len(s.credits) : s.credits[t].channel \in lost}
    IN PutEvent([s EXCEPT !.connected=@ \ lost, !.closing=@ \ lost,
         !.heldJoins={n \in @ : <<s.ops[n].src,s.ops[n].dst>> \notin lost},
         !.adopted=[j \in 1..MaxOps |->
             LET failed=={n \in 1..(s.next-1) : s.ops[n].class="adopted" /\
                 s.ops[n].completion=j /\ <<s.ops[n].src,s.ops[n].dst>> \in lost}
             IN IF failed={} THEN s.adopted[j] ELSE Min(failed)],
         !.wire=[k \in Channels |-> IF k \in lost THEN <<>> ELSE s.wire[k]],
         !.questions=[k \in TableSlots |-> IF k[1] \in lost THEN 0 ELSE s.questions[k]],
         !.answers=[k \in TableSlots |-> IF k[1] \in lost THEN 0 ELSE s.answers[k]],
         !.exports=[k \in Slots |-> IF k[1] \in lost THEN [ExportEntry EXCEPT !.generation=s.exports[k].generation] ELSE s.exports[k]],
         !.imports=[k \in Slots |-> IF k[1] \in lost THEN [ImportEntry EXCEPT !.generation=s.imports[k].generation+1] ELSE s.imports[k]],
         !.credits=MapSeqIndices(s.credits,LAMBDA t : IF t \in ts THEN [s.credits[t] EXCEPT !.phase="released"] ELSE s.credits[t]),
         !.ops=[n \in 1..MaxOps |-> IF n<s.next /\ <<s.ops[n].src,s.ops[n].dst>> \in lost
                THEN [s.ops[n] EXCEPT !.phase="disconnected", !.observed="disconnected",
                                      !.outcome="exception"] ELSE s.ops[n]]],"disconnect")
    ))
\* @type: ($state, $channel)=>$state;
Abort(state,c) ==
    (LET s == state IN  [Enqueue(s,c,[Message EXCEPT !.kind="abort"]) EXCEPT !.closing=@ \cup {c}]
    )
\* @type: ($state, $channel, $message)=>$state;
ReceiveRequest(state,c,m) ==
    (LET s == state IN 
    LET k == QSlot(c,m.class,m.id)
        imp == RegisterOffers(ImportCredits(s,m.tickets),c,m.caps)
        target == IF m.kind="bootstrap" THEN Object(c[2],1) ELSE DecodeTarget(s,c,m.target)
    IN IF s.answers[k] # 0 THEN Fault(Close(s,c),"duplicateQuestion")
       ELSE [imp EXCEPT !.answers[k]=m.op, !.ops[m.op].phase="waiting",
              !.ops[m.op].arrival=s.arrivals[c[2]]+1, !.arrivals[c[2]]=@+1,
              !.ops[m.op].target=target, !.ops[m.op].params=DecodePayload(imp,c,m.caps)]
    )
\* @type: ($state, $channel, $message)=>$state;
ReceiveFinish(state,c,m) ==
    (LET s == state IN 
    LET n == s.answers[QSlot(c,m.class,m.id)]
    IN IF n=0 THEN s \* noFinishNeeded and late pipeline-only Finish
       ELSE LET o == s.ops[n]
                released == IF m.flags.releaseResults THEN ReleaseExports(s,o.resultTickets) ELSE s
                out == Reap([released EXCEPT !.ops[n].finishRecv=TRUE,
                     !.ops[n].phase=IF o.kind="provide" THEN "done" ELSE @, !.heldJoins=@ \ {n}],n)
                child == o.forward
            IN IF child # 0 /\ o.class # "adopted" /\ ~s.ops[child].finishSent /\
                     <<s.ops[child].src,s.ops[child].dst>> \in s.connected
               THEN Finish(out,child,TRUE) ELSE out
    )
\* @type: ($state, $channel, $message)=>$state;
ReceiveReturn(state,c,m) ==
    (LET s == state IN 
    LET slot == QSlot(Rev(c),m.class,m.id)
        retired == {j \in 1..(s.next-1) : s.ops[j].class="pipeline" /\
                    OSlot(s.ops[j])=slot /\ s.ops[j].phase # "disconnected"}
        n == IF s.questions[slot] # 0 THEN s.questions[slot]
             ELSE IF m.class="pipeline" /\ retired # {} THEN Min(retired) ELSE 0
    IN IF n=0 THEN Close(s,c) \* retired pipeline-only ID; payload was already released
       ELSE LET o == s.ops[n]
                paramsReleased == IF m.flags.releaseParams THEN ReleaseExports(s,o.paramTickets) ELSE s
                imported == IF o.finishSent THEN ImportCredits(paramsReleased,m.tickets)
                            ELSE RegisterOffers(ImportCredits(paramsReleased,m.tickets),c,m.caps)
                discarded == IF o.finishSent /\ o.flags.releaseResults
                             THEN ReleaseImports(imported,m.tickets) ELSE imported
                out == [discarded EXCEPT !.ops[n].returnRecv=TRUE,
                         !.ops[n].qResult=IF o.finishSent THEN <<>> ELSE DecodePayload(imported,c,m.caps),
                         !.ops[n].qContent=m.content, !.ops[n].observed=m.result, !.ops[n].joinHost=m.host,
                         !.ops[n].ordinal=m.ordinal, !.ops[n].flags.noFinish=m.flags.noFinish]
                other == IF m.result="takeFromOtherQuestion"
                         THEN s.answers[QSlot(c,"ordinary",m.other)] ELSE 0
                redirect == IF m.result="takeFromOtherQuestion"
                            THEN IF other=0 THEN Fault(out,"missingTailQuestion")
                                 ELSE IF s.ops[other].taken THEN Fault(out,"duplicateTake")
                                 ELSE [out EXCEPT !.ops[n].qRedirect=other, !.ops[other].taken=TRUE]
                            ELSE IF m.result="awaitFromThirdParty"
                                 THEN IF ~o.flags.allowThird THEN Fault(out,"tailPermission")
                                      ELSE [out EXCEPT !.awaiting[m.other]=n,
                                              !.ops[n].completion=m.other]
                                 ELSE out
            IN Reap(redirect,n)
    )
\* @type: ($state, $channel, $message)=>$state;
ReceiveRelease(state,c,m) ==
    (LET s == state IN 
    IF s.exports[<<Rev(c),m.id>>].count < m.count THEN Fault(Close(s,c),"releaseUnderflow")
    ELSE ReleaseExports(s,m.tickets)
    )
\* @type: ($state, $channel, $message)=>$state;
ReceiveResolve(state,c,m) ==
    (LET s == state IN 
    LET 
        \* @type: $slot;
        k == <<Rev(c),m.id>>
        live == s.imports[k].count > 0
        imported == ImportCredits(s,m.tickets)
        r == IF m.result="exception" THEN Broken ELSE DecodeOne(imported,c,Head(m.caps))
        offered == IF live THEN RegisterOffers(imported,c,m.caps) ELSE imported
        resolved == IF live THEN [offered EXCEPT !.imports[k].resolution=r] ELSE offered
        out == IF live /\ m.caps # <<>> /\ Head(m.caps).kind="thirdPartyHosted"
               THEN SetShortcut(resolved,Import(c[2],c[1],m.id,s.imports[k].generation),r)
               ELSE resolved
    IN IF live \/ m.tickets={} THEN PutEvent(out,"resolve")
       ELSE LET id == Min({s.credits[t].id : t \in m.tickets})
            IN PutEvent(SendRelease(out,Rev(c),m.tickets),"releasedResolve")
    )
\* @type: ($state, $channel, $message)=>$state;
ReceiveAdoption(state,c,m) ==
    (LET s == state IN 
    LET n == m.op k == QSlot(Rev(c),m.class,m.id)
    IN IF m.class # "adopted" \/ s.questions[k] # 0 THEN Fault(Close(s,c),"adoptionNamespace")
       ELSE PutEvent([s EXCEPT !.questions[k]=n, !.adopted[m.completion]=n],
                     IF s.awaiting[m.completion]=0 THEN "earlyAdoption" ELSE "adoption")
    )

\* Embargo messages carry a local reference after numeric target resolution.
\* They enter the same dispatch FIFO as calls, including through proxies.
\* @type: ($state, $channel, $message)=>$state;
ReceiveDisembargo(state,c,m) ==
    (LET s == state IN 
    IF m.context="receiverLoopback"
    THEN PutEvent([s EXCEPT !.barriers=@ \cup {<<m.completion,m.embargo>>}],"loopback")
    ELSE LET vine == IF m.target.kind="importedCap"
                     THEN s.exports[<<Rev(c),m.target.id>>].provision ELSE 0
             r == IF vine # 0 THEN Ref("provision",c[2],s.ops[vine].dst,vine,0,<<>>)
                  ELSE DecodeTarget(s,c,m.target)
         IN [s EXCEPT !.arrivals[c[2]]=@+1, !.embargoQueue=Append(@,
               [host |-> c[2], sender |-> c[1], arrival |-> s.arrivals[c[2]]+1, ref |-> r, context |-> m.context,
                completion |-> m.completion, embargo |-> m.embargo, done |-> FALSE])]
    )
\* @type: ($state, $channel, $message)=>$state;
ReceiveUnimplemented(state,c,m) ==
    (LET s == state IN 
    IF m.echoed="resolve" THEN PutEvent(ReleaseExports(s,m.tickets),"unsupportedResolve")
    ELSE IF m.echoed \notin {"call","bootstrap","provide","accept","join"} THEN Abort(s,Rev(c))
    ELSE LET n == s.questions[QSlot(Rev(c),m.class,m.id)]
         IN IF n=0 THEN Close(s,c)
            ELSE PutEvent(Reap([ReleaseExports(s,s.ops[n].paramTickets) EXCEPT
                           !.ops[n].returnRecv=TRUE, !.ops[n].observed="exception"],n),"unsupportedRequest")
    )
\* @type: ($state, $channel, $message)=>$state;
Handle(state,c,m) ==
    (LET s == state IN 
    IF (Bug="rejectCall" /\ m.kind="call") \/ (Bug="rejectResolve" /\ m.kind="resolve")
    THEN Enqueue(s,Rev(c),[m EXCEPT !.kind="unimplemented", !.echoed=m.kind])
    ELSE CASE m.kind \in {"call","bootstrap","provide","accept","join"} -> ReceiveRequest(s,c,m)
      [] m.kind="return" -> ReceiveReturn(s,c,m)
      [] m.kind="finish" -> ReceiveFinish(s,c,m)
      [] m.kind="release" -> ReceiveRelease(s,c,m)
      [] m.kind="resolve" -> ReceiveResolve(s,c,m)
      [] m.kind="thirdPartyAnswer" -> ReceiveAdoption(s,c,m)
      [] m.kind="disembargo" -> ReceiveDisembargo(s,c,m)
      [] m.kind="unimplemented" -> ReceiveUnimplemented(s,c,m)
      [] m.kind="abort" -> Close(s,c)
      [] OTHER -> Enqueue(s,Rev(c),[m EXCEPT !.kind="unimplemented", !.echoed=m.kind])
    )
\* @type: $channel=>Bool;
Receive(c) ==
    /\ c \in net.connected /\ Len(net.wire[c])>0
    /\ net' = Handle([net EXCEPT !.wire[c]=Tail(@)],c,Head(net.wire[c]))

\* Earlier arrivals are respected per local reference, not by consulting the
\* desired global delivery order. Completed operation records are observers.
\* @type: ($state, $barrier)=>$ref;
BarrierTarget(s,b) ==
    IF b.ref.tag="provision" THEN Follow(s,s.ops[b.ref.id].route,{})
    ELSE IF b.ref.tag="answer" /\ s.ops[b.ref.id].kind="provide"
         THEN Follow(s,s.ops[b.ref.id].target,{}) ELSE Follow(s,b.ref,{})
\* @type: ($state, Int, $ref)=>Bool;
Earlier(s,n,r) ==
 \E i \in 1..(s.next-1) : s.ops[i].dst=s.ops[n].dst /\
    s.ops[i].arrival<s.ops[n].arrival /\
    s.ops[i].phase="waiting" /\ Follow(s,s.ops[i].target,{})=r /\ s.ops[i].kind \in {"call","join"}
\* @type: ($state, Int, $ref)=>Bool;
EarlierBarrier(s,n,r) == \E i \in 1..Len(s.embargoQueue) :
    LET b==s.embargoQueue[i]
    IN b.host=s.ops[n].dst /\ ~b.done /\ b.arrival<s.ops[n].arrival /\ BarrierTarget(s,b)=r
\* @type: ($state, Int)=>Bool;
HasDependents(s,n) ==
 \E i \in 1..(s.next-1) :
    s.ops[i].phase \in {"waiting","running","forwarded"} /\ ~s.ops[i].finishRecv /\
    ((s.ops[i].target.tag="answer" /\ s.ops[i].target.id=n) \/ s.ops[i].forward=n)
\* @type: ($state, $channel, Seq($ref))=>Bool;
CanEncodePayload(s,c,refs) ==
    LET \* @type: ({state: $state, ok: Bool}, $ref) => {state: $state, ok: Bool};
        Step(acc,r) ==
        IF acc.ok /\ CanEncode(acc.state,c,r)
        THEN [state |-> EncodeOne(acc.state,c,r).state, ok |-> TRUE]
        ELSE [acc EXCEPT !.ok=FALSE]
    IN ApaFoldSeqLeft(Step,[state |-> s, ok |-> TRUE],refs).ok

\* @type: ($state, $channel, $flags, Seq($ref))=>Bool;
CanAskPayload(s,c,flags,refs) ==
    /\ CanEncodePayload(s,c,refs)
    /\ CanAsk(EncodePayload(s,c,refs).state,c,flags)

\* Transparent forwarding creates a normal Call/Join and uses the same payload
\* ownership machinery. Self and third-party tail calls only change Return routing.
\* @type: ($state, Int, $ref)=>$state;
Forward(state,n,r) ==
    (LET s == state IN 
    LET o == s.ops[n] 
        \* @type: $channel;
        c == <<o.dst,r.peer>>
        kind == IF o.kind="join" THEN "join" ELSE "call"
        destination == IF kind="join" THEN "caller"
                       ELSE IF r.peer=o.src THEN "yourself"
                       ELSE IF o.flags.allowThird THEN "thirdParty" ELSE "caller"
        out == Ask(s,c,kind,WireTarget(s,r),o.params,o.flags,o.method,o.data,
                   destination,IF destination="thirdParty" THEN n ELSE 0,0,o.join,o.part,o.parts,0,o.logical)
        child == out.next-1
        linked == [out EXCEPT !.ops[n].forward=child, !.ops[n].phase="forwarded"]
        forwarded == IF o.finishRecv THEN Finish(linked,child,TRUE) ELSE linked
    IN IF kind="join" \/ destination="caller" THEN forwarded
       ELSE Return(forwarded,n,IF destination="yourself" THEN "takeFromOtherQuestion" ELSE "awaitFromThirdParty",
                   <<>>,<<>>,IF destination="yourself" THEN linked.ops[child].id ELSE n,0,0)
    )
\* @type: Int=>Bool;
Dispatch(n) ==
    /\ n<net.next /\ net.ops[n].phase="waiting"
    /\ <<net.ops[n].src,net.ops[n].dst>> \in net.connected
    /\ net.ops[n].kind \in {"call","bootstrap","join"}
    /\ LET o == net.ops[n] r == IF o.kind="bootstrap" THEN o.target ELSE Follow(net,o.target,{})
       IN /\ ~Earlier(net,n,r) /\ ~EarlierBarrier(net,n,r)
          /\ CASE r.tag \in {"import","question"} ->
                   /\ CanAskPayload(net,<<o.dst,r.peer>>,o.flags,o.params)
                   /\ net'=Forward(net,n,r)
               [] r.tag="object" ->
                   net'=[net EXCEPT !.ops[n].phase="running", !.ops[n].dispatched=TRUE,
                      !.deliveries=IF o.kind="call" /\ o.method # "tail" THEN Append(@,o.logical) ELSE @]
               [] r.tag \in {"broken","null"} ->
                   net'=[net EXCEPT !.ops[n].phase="done", !.ops[n].outcome="exception"]
               [] OTHER -> FALSE /\ UNCHANGED net

\* @type: Int=>Bool;
Cancel(n) ==
    /\ n<net.next /\ net.ops[n].phase \in {"waiting","running"}
    /\ net.ops[n].kind # "provide" /\ net.ops[n].finishRecv
    /\ (net.ops[n].adoption # 0 =>
        (net.ops[net.ops[n].adoption].finishRecv \/ net.ops[net.ops[n].adoption].phase="disconnected"))
    /\ ~HasDependents(net,n)
    /\ (~net.ops[n].flags.workaround \/ net.ops[n].dispatched)
    /\ net'=[net EXCEPT !.ops[n].phase="done", !.ops[n].outcome="canceled"]

\* Application semantics are deliberately small, but travel through real Call/
\* Return messages. Echo, factory, tail, save, and restore expose protocol edges.
\* @type: Int=>Bool;
Complete(n) ==
    /\ n<net.next /\ net.ops[n].phase="running" /\ net.ops[n].kind # "join"
    /\ LET o == net.ops[n]
           result == CASE o.kind="bootstrap" -> <<Object(o.dst,1)>>
                       [] o.method="echo" -> o.params
                       [] o.method \in {"factory","nested"} -> <<Object(o.dst,o.data)>>
                       [] o.method="pair" -> <<Object(o.dst,1),Object(o.dst,2)>>
                       [] o.method="duplicate" -> <<Object(o.dst,1),Object(o.dst,1)>>
                       [] o.method="promise" -> <<Promise(o.dst,o.data)>>
                       [] o.method="get" -> <<net.promises[<<o.dst,o.data>>]>>
                       [] OTHER -> <<>>
       IN /\ IF o.method="tail" /\ Len(o.params)>0
             THEN /\ ClientReady(net,Head(o.params))
                  /\ net'=[net EXCEPT !.ops[n].phase="waiting", !.ops[n].target=ClientRef(net,Head(o.params),{}),
                                   !.ops[n].arrival=net.arrivals[o.dst]+1, !.arrivals[o.dst]=@+1,
                                   !.ops[n].method="echo", !.ops[n].params=Tail(o.params)]
             ELSE IF o.method="save"
                  THEN net'=PutEvent([net EXCEPT !.ops[n].phase="done", !.ops[n].outcome="results",
                         !.sturdy=@ \cup {<<o.dst,o.data,Follow(net,o.target,{})>>}],"save")
             ELSE IF o.method="restore"
                  THEN LET matches == {x \in net.sturdy : x[1]=o.dst /\ x[2] \in {0,o.src}}
                       IN net'=PutEvent([net EXCEPT !.ops[n].phase="done",
                            !.ops[n].outcome=IF matches={} THEN "exception" ELSE "results",
                            !.ops[n].result=IF matches={} THEN <<>> ELSE <<(Guess(matches))[3]>>,
                            !.ops[n].content=FlatContent(<<Null>>)],"restore")
                  ELSE net'=[net EXCEPT !.ops[n].phase="done", !.ops[n].outcome="results", !.ops[n].result=result,
                      !.ops[n].content=IF o.kind="bootstrap" THEN RawContent
                                      ELSE IF o.method="nested" THEN NestedContent
                                      ELSE FlatContent(result)]

\* @type: Int=>Bool;
CompleteForward(n) ==
    /\ n<net.next /\ net.ops[n].phase="forwarded"
    /\ LET o==net.ops[n] child==net.ops[o.forward]
       IN /\ (child.returnRecv /\ child.observed \notin {"takeFromOtherQuestion","awaitFromThirdParty"}) \/ child.phase="disconnected"
          /\ net'=[net EXCEPT !.ops[n].phase="done", !.ops[n].outcome=child.observed,
                        !.ops[n].result=child.qResult, !.ops[n].content=child.qContent, !.ops[n].joinHost=child.joinHost,
                        !.ops[n].ordinal=child.ordinal]

\* A failed direct connection completes its authenticated VatNetwork rendezvous
\* with an error. The disconnected record is a transport notification, not a
\* fabricated RPC ThirdPartyAnswer; it allocates no question/answer table slot.
\* @type: Int=>Bool;
Adopt(n) ==
    /\ n<net.next /\ net.ops[n].destination="thirdParty" /\ net.ops[n].adoption=0
    /\ net.ops[n].phase # "sent" /\ net.ops[n].phase # "disconnected"
    /\ net.next<=MaxOps
    /\ LET o==net.ops[n] original==net.ops[o.completion]
           
        \* @type: $channel;
        c == <<original.src,o.dst>> free==FreeQuestion(net,c,"adopted")
       IN /\ free # {}
          /\ LET a==net.next id==Min(free)
                 cls==IF Bug="adoptionNamespace" THEN "ordinary" ELSE "adopted"
                 op==[Operation EXCEPT !.src=c[1], !.dst=c[2], !.id=id, !.class=cls,
                       !.phase="adopted", !.forward=n, !.completion=o.completion]
                 m==[Message EXCEPT !.kind="thirdPartyAnswer", !.op=a, !.id=id,
                                  !.class=cls, !.completion=o.completion]
             IN IF c \in net.connected
                THEN net'=Enqueue([net EXCEPT !.next=@+1, !.ops[a]=op,
                         !.answers[OSlot(op)]=a, !.ops[n].adoption=a],Rev(c),m)
                ELSE net'=PutEvent([net EXCEPT !.next=@+1,
                         !.ops[a]=[op EXCEPT !.phase="disconnected", !.observed="disconnected", !.outcome="exception"],
                         !.ops[n].adoption=a, !.adopted[o.completion]=a],"adoptionFailure")

\* @type: Int=>Bool;
SendReturn(n) ==
    /\ n<net.next /\ ~net.ops[n].returnSent
    /\ net.ops[n].kind # "provide"
    /\ ~(net.ops[n].flags.only /\ net.ops[n].flags.honorOnly)
    /\ <<net.ops[n].src,net.ops[n].dst>> \in net.connected
    /\ LET o==net.ops[n]
           isAdopted==o.phase="adopted"
           source==IF isAdopted THEN net.ops[o.forward] ELSE o
           kind==IF o.destination # "caller" THEN "resultsSentElsewhere" ELSE source.outcome
           refs==IF kind="results" THEN source.result ELSE <<>>
       IN /\ source.phase="done"
          /\ (o.destination="thirdParty" => o.adoption # 0)
          /\ CanEncodePayload(net,<<o.dst,o.src>>,IF o.finishRecv THEN <<>> ELSE refs)
          /\ ~(o.flags.noFinish /\ refs # <<>>)
          /\ net'=Return(net,n,kind,refs,source.content,0,source.joinHost,source.ordinal)

\* @type: Int=>Bool;
LinkAdoption(n) ==
    /\ n<net.next /\ net.ops[n].returnRecv
    /\ net.ops[n].observed="awaitFromThirdParty" /\ net.ops[n].qRedirect=0
    /\ LET child==net.adopted[net.ops[n].completion]
       IN /\ child # 0
          /\ net'=[net EXCEPT !.ops[n].qRedirect=child]

\* @type: Int=>Bool;
SettleRedirect(n) ==
    /\ n<net.next /\ net.ops[n].returnRecv
    /\ net.ops[n].observed \in {"takeFromOtherQuestion","awaitFromThirdParty"}
    /\ LET o==net.ops[n]
           child==IF o.observed="takeFromOtherQuestion" THEN o.qRedirect ELSE net.adopted[o.completion]
       IN /\ child # 0
          /\ IF o.observed="takeFromOtherQuestion"
             THEN /\ net.ops[child].phase="done"
                  /\ net'=[net EXCEPT !.ops[n].observed=net.ops[child].outcome, !.ops[n].qContent=net.ops[child].content,
                                        !.ops[n].qResult=IF o.finishSent THEN <<>> ELSE net.ops[child].result]
             ELSE /\ net.ops[child].returnRecv \/ net.ops[child].phase="disconnected"
                  /\ net'=PutEvent([net EXCEPT !.ops[n].observed=net.ops[child].observed, !.ops[n].qContent=net.ops[child].qContent,
                            !.ops[n].qResult=IF o.finishSent \/ net.ops[child].phase="disconnected" THEN <<>> ELSE net.ops[child].qResult, !.ops[n].qRedirect=child],"directTail")

\* @type: Int=>Bool;
FlushFinish(n) ==
    /\ n<net.next /\ net.ops[n].finishWanted /\ CanFinish(net,n)
    /\ <<net.ops[n].src,net.ops[n].dst>> \in net.connected
    /\ net'=Finish(net,n,net.ops[n].flags.releaseResults)

\* @type: Int=>Bool;
FinishRedirect(n) ==
    /\ n<net.next /\ net.ops[n].finishSent
    /\ LET child==net.ops[n].qRedirect
       IN /\ child # 0 /\ net.ops[child].class="adopted"
          /\ ~net.ops[child].finishSent
          /\ <<net.ops[child].src,net.ops[child].dst>> \in net.connected
          /\ net'=Finish(net,child,TRUE)

\* Resolve's old export is pinned to exactly the announced local reference.
\* Three-party resolutions allocate a normal Provide operation and a real vine
\* export, carrying the descriptor in the very same Resolve payload/credit path.
\* @type: $slot=>Bool;
ResolveExport(k) ==
    /\ k \in Slots /\ k[1] \in net.connected
    /\ net.exports[k].count>0 /\ ~net.exports[k].resolved
    /\ net.exports[k].ref.tag \in {"promise","answer","question","broken"}
    /\ LET e==net.exports[k] c==k[1] r==Follow(net,e.ref,{})
           third==Introductions /\ r.tag="import" /\ r.peer # c[2]
       IN /\ r.tag \notin {"promise","answer","question"}
          /\ CanEncode(net,c,r)
          /\ IF third
             THEN /\ CanAsk(net,<<c[1],r.peer>>,Flags)
                  /\ LET p==net.next
                         provided==Ask(net,<<c[1],r.peer>>,"provide",WireTarget(net,r),<<>>,Flags,
                                       "echo",0,"caller",p,0,0,0,0,0,0)
                         enc==EncodeOne(provided,c,Ref("vine",c[1],r.peer,p,0,<<>>))
                         d==[enc.descriptor EXCEPT !.kind="thirdPartyHosted", !.completion=p, !.host=r.peer]
                         out==[enc.state EXCEPT !.exports[k].resolved=TRUE, !.exports[k].pinned=r,
                              !.exports[<<c,d.id>>].provision=p, !.grants=@ \cup {<<p,r.peer,c[2]>>}]
                         m==[Message EXCEPT !.kind="resolve", !.id=k[2], !.caps= <<d>>,
                              !.tickets={enc.ticket}, !.result="cap"]
                     IN net'=Enqueue(out,c,m)
             ELSE LET enc==IF r=Broken THEN [state |-> net, descriptors |-> <<>>, tickets |-> {}]
                           ELSE EncodePayload(net,c,<<r>>)
                      out==[enc.state EXCEPT !.exports[k].resolved=TRUE, !.exports[k].pinned=r]
                      m==[Message EXCEPT !.kind="resolve", !.id=k[2], !.caps=enc.descriptors,
                             !.tickets=enc.tickets, !.result=IF r=Broken THEN "exception" ELSE "cap"]
                  IN net'=Enqueue(out,c,m)

\* Runtime-owned handoff. Accept and the barrier are emitted together; the
\* barrier follows earlier calls on the provider FIFO. New calls can pipeline
\* on Accept immediately, where the host holds them behind this embargo.
\* @type: $slot=>Bool;
StartHandoff(k) ==
    /\ k \in Slots /\ Introductions
    /\ net.imports[k].count>0 /\ net.imports[k].offer # <<>>
    /\ net.imports[k].handoff=0
    /\ LET e==net.imports[k] c==k[1] host==e.offer[1] completion==e.offer[2]
           
        \* @type: $channel;
        direct == <<c[1],host>> n==net.next
           vine==Import(c[1],c[2],k[2],e.generation)
       IN /\ c \in net.connected /\ c \notin net.closing
          /\ CanAsk(net,direct,Flags)
          /\ LET asked==Ask(net,direct,"accept",NoTarget,<<>>,Flags,
                            "echo",0,"caller",completion,n,0,0,0,0,0)
                 tracked==[asked EXCEPT !.imports[k].handoff=n,
                             !.ops[n].automatic=TRUE, !.ops[n].vine=vine]
                 routed==SetShortcut(tracked,vine,RemoteQuestion(c[1],n,<<>>))
                 barrier==[Message EXCEPT !.kind="disembargo", !.context="accept",
                     !.target=Target("importedCap",k[2],<<>>),
                     !.completion=completion, !.embargo=n]
             IN net'=Enqueue(routed,c,barrier)

\* @type: Int=>Bool;
FinishHandoff(n) ==
    /\ n<net.next /\ net.ops[n].automatic /\ ~net.ops[n].handoffDone
    /\ net.ops[n].returnRecv \/ net.ops[n].phase="disconnected"
    /\ LET o==net.ops[n]
           r==IF o.observed="results" THEN AtPath(o.qResult,o.qContent,<<>>) ELSE Broken
           routed==SetShortcut(net,o.vine,r)
           marked==[routed EXCEPT !.ops[n].handoffDone=TRUE]
       IN net'=IF o.phase="disconnected" \/ o.finishSent THEN marked
               ELSE Finish(marked,n,FALSE)

\* @type: Int=>Bool;
RegisterProvide(n) ==
    /\ n<net.next /\ net.ops[n].kind="provide" /\ net.ops[n].phase="waiting"
    /\ net'=[net EXCEPT !.ops[n].phase=IF net.ops[n].finishRecv THEN "done" ELSE "provided"]
\* @type: ($state, Int, Int)=>Set(Int);
ProvideOps(s,completion,host) ==
 {n \in 1..(s.next-1) :
    s.ops[n].kind="provide" /\ s.ops[n].completion=completion /\ s.ops[n].dst=host /\
    s.ops[n].phase="provided" /\ ~s.ops[n].finishRecv}
\* @type: ($state, $op)=>Bool;
AcceptReady(s,o) ==
 o.embargo=0 \/ <<o.completion,o.embargo>> \in s.barriers \/ Bug="skipEmbargo"
\* @type: Int=>Bool;
CompleteAccept(n) ==
    /\ n<net.next /\ net.ops[n].kind="accept" /\ net.ops[n].phase="waiting"
    /\ LET o==net.ops[n] ps==ProvideOps(net,o.completion,o.dst)
           joined=={x \in net.joinCaps : x[1]=o.completion /\ x[2]=o.dst /\ x[4]=o.src}
           valid== <<o.completion,o.dst,o.src>> \in net.grants
       IN /\ IF ~valid \/ (\E p \in 1..(net.next-1) : net.ops[p].kind="provide" /\
                     net.ops[p].completion=o.completion /\
                     (net.ops[p].finishRecv \/ net.ops[p].phase="disconnected"))
             THEN net'=[net EXCEPT !.ops[n].phase="done", !.ops[n].outcome="exception"]
             ELSE /\ ps # {} \/ joined # {}
                  /\ AcceptReady(net,o)
                  /\ LET r==IF joined # {} THEN Object(o.dst,(Guess(joined))[3])
                            ELSE net.ops[Min(ps)].target
                     IN net'=PutEvent([net EXCEPT !.ops[n].phase="done", !.ops[n].outcome="results",
                            !.ops[n].result= <<r>>, !.ops[n].content=RawContent, !.accepted=@ \cup {<<o.completion,o.src,o.embargo>>}],"accept")

\* @type: Int=>Bool;
DispatchBarrier(i) ==
    /\ i \in 1..Len(net.embargoQueue) /\ ~net.embargoQueue[i].done
    /\ LET b==net.embargoQueue[i] r==BarrierTarget(net,b)
           out==[net EXCEPT !.embargoQueue[i].done=TRUE]
           pending==\E n \in 1..(net.next-1) : net.ops[n].dst=b.host /\
                      net.ops[n].arrival<b.arrival /\
                      net.ops[n].phase="waiting" /\ net.ops[n].kind="call" /\
                      Follow(net,net.ops[n].target,{})=r
       IN /\ ~pending \/ Bug="barrierOvertakes"
          /\ CASE b.ref.tag="provision" ->
                  LET p==net.ops[b.ref.id]
                      m==[Message EXCEPT !.kind="disembargo", !.context="accept",
                          !.target=Target("promisedAnswer",p.id,<<ClassCode(p.class)>>),
                          !.completion=b.completion, !.embargo=b.embargo]
                  IN net'=Enqueue(out,<<b.host,p.dst>>,m)
             [] b.ref.tag="answer" /\ net.ops[b.ref.id].kind="provide" ->
                  net'=PutEvent([out EXCEPT !.barriers=@ \cup {<<b.completion,b.embargo>>}],"disembargo")
             [] r.tag="import" ->
                  IF b.context="senderLoopback" /\ r.peer # b.sender
                  THEN net'=Close(out,<<b.host,b.sender>>)
                  ELSE net'=Enqueue(out,<<b.host,r.peer>>,
                     [Message EXCEPT !.kind="disembargo", !.target=WireTarget(net,r),
                      !.context=IF b.context="senderLoopback" THEN "receiverLoopback" ELSE b.context,
                      !.completion=b.completion, !.embargo=b.embargo])
             [] r.tag \in {"object","broken"} ->
                  net'=IF b.context="senderLoopback" THEN Close(out,<<b.host,b.sender>>)
                       ELSE [out EXCEPT !.barriers=@ \cup {<<b.completion,b.embargo>>}]
             [] OTHER -> FALSE /\ UNCHANGED net

\* Join roots assign ordinals independently for each object. The joiner only
\* sees Return metadata; root identity is used locally and by transport proof.
\* @type: Int=>Bool;
CompleteJoin(n) ==
    /\ n<net.next /\ net.ops[n].kind="join" /\ net.ops[n].phase="running"
    /\ LET o==net.ops[n] r==Follow(net,o.target,{})
           group=={x \in net.joinGroups : x[1]=o.join /\ x[2]=r.host /\ x[3]=r.id}
       IN /\ r.tag="object"
          /\ net'=[net EXCEPT !.ops[n].phase="done", !.ops[n].outcome="results",
               !.ops[n].joinHost=r.host, !.ops[n].ordinal=Cardinality(group),
               !.joinGroups=@ \cup {<<o.join,r.host,r.id,o.part,n>>},
               !.heldJoins=@ \cup {n}]
\* @type: ($state, Int, Int)=>Set(Int);
JoinQuestions(s,j,client) ==
 {n \in 1..(s.next-1) : s.ops[n].kind="join" /\
                              s.ops[n].src=client /\ s.ops[n].label # 0 /\ s.ops[n].join=j}
\* @type: ($state, Int, Int, Int)=>Bool;
JoinResultsReady(s,j,client,count) ==
    LET ns==JoinQuestions(s,j,client)
    IN Cardinality(ns)=count /\ (\A n \in ns : s.ops[n].returnRecv)
\* @type: ($state, Set(Int), Int)=>Bool;
JoinCompatible(s,ns,count) ==
    /\ \A n \in ns : s.ops[n].observed="results"
    /\ Cardinality({s.ops[n].joinHost : n \in ns})=1
    /\ ({s.ops[n].ordinal : n \in ns}=0..(count-1) \/ Bug="hostOnlyJoin")
\* @type: ($state, Int, Int, Int)=>$state;
JoinConnect(state,j,client,count) ==
    (LET s == state IN 
    LET ns==JoinQuestions(s,j,client)
        \* @type: Set($localSlot);
        groups=={<<x[2],x[3]>> : x \in {g \in s.joinGroups : g[1]=j}}
        complete=={r \in groups : {x[4] : x \in {g \in s.joinGroups : g[1]=j /\
                                    g[2]=r[1] /\ g[3]=r[2] /\ g[5] \in s.heldJoins}}=1..count}
    IN IF ~JoinCompatible(s,ns,count) THEN PutEvent([s EXCEPT !.joins[j]="unequal"],"unequalJoin")
       ELSE IF complete={} /\ Bug # "hostOnlyJoin" THEN [s EXCEPT !.joins[j]="failed"]
       ELSE LET root==Guess(IF complete={} THEN groups ELSE complete)
                token==MaxOps+j
            IN PutEvent([s EXCEPT !.joins[j]="authenticated",
                    !.grants=@ \cup {<<token,root[1],client>>},
                    !.joinCaps=@ \cup {<<token,root[1],root[2],client>>}],"join")
    )

\* Local release is allowed only after all protocol users have relinquished
\* the imported reference. Exported proxies and pinned resolutions retain it.
\* @type: ($channel, Int)=>Bool;
ReleaseUnused(c,id) ==
    /\ c \in net.connected /\ net.imports[<<c,id>>].count>0
    /\ ~ImportInUse(net,c,id)
    /\ \A n \in 1..(net.next-1) :
         (net.ops[n].src=c[1] /\ ~net.ops[n].finishSent /\ ~net.ops[n].flags.noFinish) =>
         ~\E t \in net.ops[n].resultTickets : net.credits[t].channel=Rev(c) /\ net.credits[t].id=id
    /\ LET ts=={t \in 1..Len(net.credits) : net.credits[t].channel=Rev(c) /\
                                                net.credits[t].id=id /\ net.credits[t].phase="held"}
       IN /\ ts # {} /\ net'=SendRelease(net,c,ts)

\* @type: $slot=>Bool;
CloseVine(k) ==
    /\ k \in Slots /\ net.exports[k].provision # 0
    /\ LET n==net.exports[k].provision
           called==\E i \in 1..(net.next-1) : net.ops[i].dst=k[1][1] /\
                      net.ops[i].phase # "sent" /\ net.ops[i].target=net.exports[k].ref
       IN /\ ~net.ops[n].finishSent
          /\ net.ops[n].phase # "disconnected"
          /\ net.exports[k].count=0 \/ called
          /\ net'=Finish(net,n,TRUE)

ProtocolStep ==
    \/ \E c \in Channels : Receive(c)
    \/ \E n \in 1..MaxOps : Dispatch(n) \/ Complete(n) \/ Cancel(n) \/ SendReturn(n) \/
          CompleteForward(n) \/ Adopt(n) \/ LinkAdoption(n) \/ SettleRedirect(n) \/ FinishRedirect(n) \/ FlushFinish(n) \/ RegisterProvide(n) \/ CompleteAccept(n) \/ CompleteJoin(n)
    \/ \E k \in Slots : ResolveExport(k) \/ CloseVine(k) \/ StartHandoff(k)
    \/ \E n \in 1..MaxOps : FinishHandoff(n)
    \/ \E i \in 1..Len(net.embargoQueue) : DispatchBarrier(i)
    \/ \E c \in Channels, id \in Ids : ReleaseUnused(c,id)
    \/ \E c \in Channels : AllowLoss /\ c \in net.connected /\ net'=Close(net,c)

\* The environment is a finite, partially ordered application program. It does
\* NOT prescribe network delivery or protocol scheduling. Plans can overlap all
\* mechanisms in one state graph, on the same connections and numeric ID pools.
\* @type: (Str, Int, Int) => $command;
Command(kind,src,dst) == [kind |-> kind, src |-> src, dst |-> dst,
    ref |-> 0, pick |-> 1, arg |-> 0, argPromise |-> FALSE, local |-> Null, target |-> Null,
    method |-> "echo", data |-> 0, path |-> <<0>>, flags |-> Flags,
    after |-> {}, wait |-> {}, order |-> 0, other |-> 0, part |-> 0, parts |-> 0, embargo |-> 0,
    waitDirect |-> FALSE]
\* @type: ($state, Int)=>Bool;
FinishedResult(s,i) ==
 IF s.labels[i]=0 THEN FALSE ELSE
    s.ops[s.labels[i]].observed \in {"results","exception","canceled","disconnected"}
\* @type: ($state, $command)=>$ref;
PlanRef(s,p) ==
 IF p.target # Null THEN p.target
               ELSE IF p.ref=0 THEN Null
               ELSE IF s.labels[p.ref]=0 THEN Null
               ELSE LET o==s.ops[s.labels[p.ref]]
                    IN IF p.pick \in 1..Len(o.qResult) THEN o.qResult[p.pick] ELSE Null
\* @type: ($state, $command)=>Seq($ref);
PlanArgs(s,p) ==
 IF p.local # Null THEN <<p.local>>
                ELSE IF p.arg=0 THEN <<>>
                ELSE IF s.labels[p.arg]=0 THEN <<>>
                ELSE IF p.argPromise THEN <<RemoteQuestion(p.src,s.labels[p.arg],p.path)>>
                ELSE s.ops[s.labels[p.arg]].qResult
\* @type: ($state, Int)=>Bool;
PlanEnabled(s,i) ==
    /\ i \notin s.steps /\ Plan[i].after \subseteq s.steps
    /\ \A j \in Plan[i].wait : FinishedResult(s,j)
\* @type: ($state, Int, $channel, Str, $target, Seq($ref), Int, Int, Int, Int, Int)=>$state;
PlanAskOn(state,i,c,kind,target,params,completion,embargo,join,part,parts) ==
    (LET s == state IN 
    LET p==Plan[i]
        out==Ask(s,c,kind,target,params,p.flags,p.method,p.data,
                 "caller",completion,embargo,join,part,parts,i,0)
    IN [out EXCEPT !.steps=@ \cup {i}, !.labels[i]=out.next-1]
    )
\* @type: ($state, Int, Str, $target, Seq($ref), Int, Int, Int, Int, Int)=>$state;
PlanAsk(state,i,kind,target,params,completion,embargo,join,part,parts) ==
    PlanAskOn(state,i,<<Plan[i].src,Plan[i].dst>>,kind,target,params,completion,embargo,join,part,parts)
\* @type: Int=>Bool;
AppStep(i) ==
    /\ i \in 1..Len(Plan) /\ PlanEnabled(net,i)
    /\ LET p==Plan[i] 
        \* @type: $channel;
        c == <<p.src,p.dst>> r==PlanRef(net,p)
           out==[net EXCEPT !.steps=@ \cup {i}]
       IN CASE p.kind="bootstrap" ->
                /\ CanAsk(net,c,p.flags)
                /\ net'=PlanAsk(net,i,"bootstrap",NoTarget,<<>>,0,0,0,0,0)
          [] p.kind \in {"call","join","provide"} ->
                LET routed==IF p.kind="call" THEN ClientRef(net,r,{}) ELSE r
                    
        \* @type: $channel;
        destination == <<p.src,routed.peer>>
                IN /\ routed.tag \in {"import","question"} /\ routed.host=p.src
                   /\ (p.kind="call" => ClientReady(net,r))
                   /\ (~p.waitDirect \/ routed # r)
                   /\ CanAskPayload(net,destination,p.flags,PlanArgs(net,p))
                   /\ LET completion==IF p.kind="provide" THEN net.next ELSE 0
                          asked==PlanAskOn(net,i,destination,p.kind,WireTarget(net,routed),PlanArgs(net,p),completion,
                                          0,p.data,p.part,p.parts)
                      IN net'=IF p.kind="provide"
                              THEN [asked EXCEPT !.grants=@ \cup {<<completion,p.dst,p.other>>}] ELSE asked
          [] p.kind \in {"pipeline","adoptedPipeline"} ->
                /\ net.labels[p.ref] # 0
                /\ LET original==net.labels[p.ref]
                       n==IF p.kind="adoptedPipeline" THEN net.adopted[original] ELSE original
                   IN /\ n # 0
                      /\ LET o==net.ops[n]
                         IN /\ ~o.finishSent /\ ~o.flags.noPipeline
                            /\ CanAskPayload(net,c,p.flags,PlanArgs(net,p))
                            /\ net'=PlanAsk(net,i,"call",Target("promisedAnswer",o.id,<<ClassCode(o.class)>> \o p.path),
                                     PlanArgs(net,p),0,0,0,0,0)
          [] p.kind="finish" ->
                /\ net.labels[p.ref] # 0
                /\ LET n==net.labels[p.ref] o==net.ops[n]
                   IN /\ ~o.finishSent /\ c \in net.connected
                      /\ (o.kind="join" => net.joins[o.join] \in {"authenticated","unequal","failed","canceled"})
                      /\ net'=Finish(out,n,p.flags.releaseResults)
          [] p.kind="resolve" ->
                /\ net.promises[<<p.src,p.data>>]=Null
                /\ r # Null
                /\ net'=[out EXCEPT !.promises[<<p.src,p.data>>]=r]
          [] p.kind="proxy" ->
                /\ r # Null
                /\ net'=[out EXCEPT !.proxies[<<p.src,p.data>>]=r]
          [] p.kind="accept" ->
                /\ CanAsk(net,c,p.flags)
                /\ LET offers=={x \in net.offers : x[1]=p.src /\ x[3]=p.dst}
                       completion==IF p.other # 0 THEN p.other
                                   ELSE IF p.ref # 0 THEN net.ops[net.labels[p.ref]].completion
                                   ELSE IF offers={} THEN 0 ELSE (Guess(offers))[4]
                   IN /\ completion # 0
                      /\ net'=PlanAsk(net,i,"accept",NoTarget,<<>>,completion,p.embargo,0,0,0)
          [] p.kind="barrier" ->
                /\ LET offers=={x \in net.offers : x[1]=p.src /\ x[2]=p.dst}
                   IN /\ offers # {}
                      /\ LET offer==Guess(offers)
                             m==[Message EXCEPT !.kind="disembargo", !.context="accept",
                                    !.target=Target("importedCap",offer[5],<<>>),
                                    !.completion=offer[4], !.embargo=p.embargo]
                         IN net'=Enqueue(out,c,m)
          [] p.kind="loopback" ->
                /\ r.tag="import"
                /\ net'=Enqueue(out,c,[Message EXCEPT !.kind="disembargo", !.context="senderLoopback",
                    !.target=WireTarget(net,r), !.completion=p.data, !.embargo=p.embargo])
          [] p.kind="connectJoin" ->
                /\ JoinResultsReady(net,p.data,p.src,p.parts)
                /\ net'=JoinConnect(out,p.data,p.src,p.parts)
          [] p.kind="cancelJoin" -> net'=[out EXCEPT !.joins[p.data]="canceled"]
          [] p.kind \in {"close","closeAdopted"} ->
                /\ c \in net.connected
                /\ IF p.kind="closeAdopted" THEN net.labels[p.ref] # 0 /\ net.adopted[net.labels[p.ref]] # 0 ELSE TRUE
                /\ net'=Close(out,c)
          [] p.kind="reconnect" ->
                /\ c \notin net.connected
                /\ net'=[out EXCEPT !.connected=@ \cup {c,Rev(c)}]
          [] p.kind="unsupported" ->
                net'=Enqueue(out,c,[Message EXCEPT !.kind="obsoleteSave"])
          [] p.kind="release" ->
                /\ r.tag="import" /\ ~ImportInUse(net,c,r.id)
                /\ LET ts=={t \in 1..Len(net.credits) : net.credits[t].channel=Rev(c) /\
                                      net.credits[t].id=r.id /\ net.credits[t].phase="held"}
                   IN /\ ts # {} /\ net'=SendRelease(out,c,ts)
          [] OTHER -> FALSE /\ UNCHANGED net

Next == ProtocolStep \/ (\E i \in 1..Len(Plan) : AppStep(i))
Spec == Init /\ [][Next]_vars
\* @type: Int=>Bool;
Progress(n) == Dispatch(n) \/ Complete(n) \/ Cancel(n) \/ SendReturn(n) \/
    CompleteForward(n) \/ Adopt(n) \/ LinkAdoption(n) \/ SettleRedirect(n) \/ FinishRedirect(n) \/ FlushFinish(n) \/ RegisterProvide(n) \/ CompleteAccept(n) \/ CompleteJoin(n)
LiveSpec == Spec /\ (\A c \in Channels : WF_vars(Receive(c)))
                 /\ (\A n \in 1..MaxOps : WF_vars(Progress(n)))
                 /\ (\A i \in 1..Len(Plan) : WF_vars(AppStep(i)))
                 /\ (\A k \in Slots : WF_vars(ResolveExport(k)) /\ WF_vars(CloseVine(k)))
                 /\ (\A k \in Slots : WF_vars(StartHandoff(k)))
                 /\ (\A n \in 1..MaxOps : WF_vars(FinishHandoff(n)))
                 /\ (\A i \in 1..MaxOps : WF_vars(DispatchBarrier(i)))
                 /\ (\A c \in Channels, id \in Ids : WF_vars(ReleaseUnused(c,id)))

TypeOK == /\ net.next \in 1..(MaxOps+1)
          /\ net.connected \subseteq Channels
          /\ net.steps \subseteq 1..Len(Plan)
          /\ net.closing \subseteq net.connected
          /\ \A n \in 1..MaxOps :
               /\ net.ops[n].phase \in {"unused","sent","waiting","running","forwarded","done","provided","adopted","disconnected"}
               /\ net.ops[n].kind \in {"call","bootstrap","provide","accept","join"}
               /\ net.ops[n].class \in {"ordinary","pipeline","adopted"}
          /\ \A t \in 1..Len(net.credits) :
               net.credits[t].phase \in {"outgoing","held","releasing","releasedTransit","released"}
          /\ \A k \in Slots : net.exports[k].count \in Nat /\ net.imports[k].count \in Nat
          /\ \A k \in TableSlots : net.questions[k] \in 0..MaxOps /\ net.answers[k] \in 0..MaxOps
ProtocolSafety == net.faults={}
ExportedQuestionLifetime == \A k \in Slots :
    (net.exports[k].count>0 /\ ~net.exports[k].resolved /\ net.exports[k].ref.tag="question") =>
    ~net.ops[net.exports[k].ref.id].finishSent
ExpectedSuccess == ExpectSuccess => (\A n \in 1..(net.next-1) :
    (net.ops[n].label # 0 /\ net.ops[n].returnRecv /\ ~net.ops[n].finishSent /\ ~net.ops[n].finishWanted) =>
    net.ops[n].observed \notin {"exception","canceled","disconnected"})
ReferenceConservation == \A k \in Slots :
    /\ net.exports[k].count = Cardinality({t \in 1..Len(net.credits) :
          net.credits[t].channel=k[1] /\ net.credits[t].id=k[2] /\
          net.credits[t].phase \in {"outgoing","held","releasing"}})
    /\ net.imports[k].count = Cardinality({t \in 1..Len(net.credits) :
          net.credits[t].channel=Rev(k[1]) /\ net.credits[t].id=k[2] /\ net.credits[t].phase="held"})
NoStaleCredits == \A t \in 1..Len(net.credits) :
    net.credits[t].phase \in {"outgoing","held","releasing"} =>
    net.credits[t].generation=net.exports[<<net.credits[t].channel,net.credits[t].id>>].generation
QuestionLifetime == \A n \in 1..(net.next-1) :
    LET o==net.ops[n]
    IN (<<o.src,o.dst>> \in net.connected /\ o.class # "adopted" /\ o.phase # "disconnected") =>
        ((net.questions[OSlot(o)]=n) <=> ~QuestionDone(o))
AnswerLifetime == \A n \in 1..(net.next-1) :
    LET o==net.ops[n]
    IN (<<o.src,o.dst>> \in net.connected /\ o.phase \notin {"sent","disconnected"}) =>
        ((net.answers[OSlot(o)]=n) <=> ~AnswerDone(o))
\* @type: Int=><<Int, Str, Int>>;
OrderGroup(n) == LET o==net.ops[n] p==Plan[o.label]
                 IN <<o.src, IF p.order=0 THEN "ref" ELSE "group", IF p.order=0 THEN p.ref ELSE p.order>>
EOrder == \A i,j \in 1..Len(net.deliveries) :
    (i<j /\ OrderGroup(net.deliveries[i])=OrderGroup(net.deliveries[j])) => net.deliveries[i]<net.deliveries[j]
AtMostOnce == Cardinality({net.deliveries[i] : i \in 1..Len(net.deliveries)})=Len(net.deliveries)
CancellationSafety == \A n \in 1..(net.next-1) : net.ops[n].outcome="canceled" => net.ops[n].finishRecv
EmbargoSafety == \A n \in 1..(net.next-1) :
    LET o==net.ops[n]
    IN (o.kind="accept" /\ o.outcome="results" /\ o.embargo # 0)
       => <<o.completion,o.embargo>> \in net.barriers
AcceptAuthority == \A n \in 1..(net.next-1) :
    LET o==net.ops[n]
    IN (o.kind="accept" /\ o.outcome="results") => <<o.completion,o.dst,o.src>> \in net.grants
JoinAgreement == \A x \in net.joinCaps :
    LET j==x[1]-MaxOps
        \* @type: Set($localSlot);
        roots=={<<g[2],g[3]>> : g \in {a \in net.joinGroups : a[1]=j}}
    IN roots \subseteq {<<x[2],x[3]>>}
DisconnectClean == \A c \in Channels \ net.connected :
    /\ net.wire[c]= <<>>
    /\ \A id \in Ids : net.exports[<<c,id>>].count=0 /\ net.imports[<<c,id>>].count=0
PlanCompletes == <>(net.steps=1..Len(Plan))
EveryQuestionSettles == <>[](\A n \in 1..(net.next-1) :
    (net.ops[n].kind # "provide" /\ ~net.ops[n].flags.only) =>
    (net.ops[n].returnRecv \/ net.ops[n].phase="disconnected"))
EveryInterestedResultSettles == <>[](\A n \in 1..(net.next-1) :
    (net.ops[n].label # 0 /\ net.ops[n].kind # "provide" /\ ~net.ops[n].flags.only /\ ~net.ops[n].finishSent) =>
    net.ops[n].observed \in {"results","exception","canceled","disconnected"})
JoinResources == \A n \in net.heldJoins : ~net.ops[n].finishRecv
AdoptedCleanup == <>[](\A n \in 1..(net.next-1) :
    (net.ops[n].class="adopted" /\ net.ops[net.ops[n].completion].finishSent) =>
    ((net.ops[n].finishSent /\ net.ops[n].finishRecv) \/ net.ops[n].phase="disconnected"))
NoPlanWitness == net.steps # 1..Len(Plan)
NoHandoffWitness == "accept" \notin net.events
NoJoinWitness == "join" \notin net.events
NoDirectTailWitness == "directTail" \notin net.events
NoEarlyAdoptionWitness == "earlyAdoption" \notin net.events
=============================================================================
