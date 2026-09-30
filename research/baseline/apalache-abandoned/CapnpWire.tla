--------------------------- MODULE CapnpWire ---------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC, Apalache, CapnpComponentTypes
\* Value-level wire interpretation shared by implementations of the state
\* machines. Pointer indices are NOT schema ordinals. No serialization assumed.
MessageKinds == {"unimplemented", "abort", "bootstrap", "call", "return", "finish",
                 "resolve", "release", "disembargo", "obsoleteSave", "obsoleteDelete",
                 "provide", "accept", "thirdPartyAnswer", "join"}
ReturnKinds == {"results", "exception", "canceled", "resultsSentElsewhere",
                "takeFromOtherQuestion", "awaitFromThirdParty"}
DescriptorKinds == {"none", "senderHosted", "senderPromise", "receiverHosted",
                    "receiverAnswer", "thirdPartyHosted"}
TargetKinds == {"importedCap", "promisedAnswer"}
DisembargoKinds == {"senderLoopback", "receiverLoopback", "accept"}
ResultDestinations == {"caller", "yourself", "thirdParty"}
ExceptionKinds == {"failed", "overloaded", "disconnected", "unimplemented"}
TransformKinds == {"noop", "getPointerField"}
NoCap == [kind |-> "none", id |-> 0]
Broken == [kind |-> "broken", id |-> 0]
\* @type: (Str, Int) => $wireRef;
Ref(kind, id) == [kind |-> kind, id |-> id]
\* @type: (Str, Int) => $wireOp;
Op(kind, index) == [kind |-> kind, index |-> index]
\* @type: (Str, Int, Seq($wireOp)) => $wireDesc;
Descriptor(kind, id, transform) == [kind |-> kind, id |-> id, transform |-> transform]
\* @type: (Int -> $wireRef, Int) => $wireRef;
Lookup(table, id) == IF id \in DOMAIN table THEN table[id] ELSE Broken
\* Nodes are records with tag, cap, and pointer sequence. Null/missing/nonstruct
\* traversal produces a broken capability. noop changes neither type nor value.
\* @type: (Int -> $wireNode, Int, Seq($wireOp)) => $wireRef;
Walk(nodes,node,ops) ==
    LET \* @type: ({node: Int, broken: Bool}, $wireOp) => {node: Int, broken: Bool};
        Step(acc,op) ==
            IF acc.broken \/ acc.node \notin DOMAIN nodes THEN [acc EXCEPT !.broken=TRUE]
            ELSE IF op.kind="noop" THEN acc
            ELSE IF op.kind="getPointerField" /\ nodes[acc.node].kind="struct" /\
                    op.index \in 0..(Len(nodes[acc.node].pointers)-1)
                 THEN [acc EXCEPT !.node=nodes[acc.node].pointers[op.index+1]]
                 ELSE [acc EXCEPT !.broken=TRUE]
        last==ApaFoldSeqLeft(Step,[node |-> node, broken |-> FALSE],ops)
    IN IF last.broken \/ last.node \notin DOMAIN nodes THEN Broken
       ELSE IF nodes[last.node].kind="cap" THEN nodes[last.node].cap ELSE Broken
\* @type: ($wireDesc, Int -> $wireRef, Int -> $wireRef, Int -> $wireNode, Int -> Int) => $wireRef;
Decode(d, exports, imports, answerNodes, answerRoots) ==
    CASE d.kind = "none" -> NoCap
      [] d.kind \in {"senderHosted", "senderPromise"} -> Lookup(imports, d.id)
      [] d.kind = "receiverHosted" -> Lookup(exports, d.id)
      [] d.kind = "receiverAnswer" ->
           IF d.id \in DOMAIN answerRoots THEN Walk(answerNodes, answerRoots[d.id], d.transform) ELSE Broken
      [] d.kind = "thirdPartyHosted" -> Ref("introduction", d.id)
      [] OTHER -> Broken
\* @type: ($wireDesc, Int -> $wireRef, Int -> $wireNode, Int -> Int) => $wireRef;
Target(t, exports, nodes, roots) ==
    IF t.kind = "importedCap" THEN Lookup(exports, t.id)
    ELSE IF t.kind = "promisedAnswer" /\ t.id \in DOMAIN roots
         THEN Walk(nodes, roots[t.id], t.transform) ELSE Broken
\* Data pointers may not refer to none tombstones in the cap table.
\* @type: (Seq($wireRef), Int) => $wireRef;
PayloadCap(table, index) == IF index < Len(table)
                          THEN IF table[index + 1] = NoCap THEN Broken ELSE table[index + 1]
                          ELSE Broken
\* @type: (Int, Seq(Str)) => Str;
AttachedFd(index, fds) == IF index < Len(fds) THEN fds[index + 1] ELSE "noFd"
\* Unknown question-creating requests can fail with an exception. Resolve
\* rollback returns the replacement credit (CapnpCapabilities.ReceiveExport).
UnimplementedEffect(kind) ==
    IF kind \in {"bootstrap", "call", "provide", "accept", "join"} THEN "exception"
    ELSE IF kind = "resolve" THEN "releaseReplacement"
    ELSE "abort"
=============================================================================
