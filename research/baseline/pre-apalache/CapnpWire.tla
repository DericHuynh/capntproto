--------------------------- MODULE CapnpWire ---------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC
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
Ref(kind, id) == [kind |-> kind, id |-> id]
Op(kind, index) == [kind |-> kind, index |-> index]
Descriptor(kind, id, transform) == [kind |-> kind, id |-> id, transform |-> transform]
Lookup(table, id) == IF id \in DOMAIN table THEN table[id] ELSE Broken
\* Nodes are records with tag, cap, and pointer sequence. Null/missing/nonstruct
\* traversal produces a broken capability. noop changes neither type nor value.
RECURSIVE Walk(_, _, _)
Walk(nodes, node, ops) ==
    IF node \notin DOMAIN nodes THEN Broken
    ELSE IF Len(ops) = 0
         THEN IF nodes[node].kind = "cap" THEN nodes[node].cap ELSE Broken
         ELSE LET op == Head(ops)
              IN IF op.kind = "noop" THEN Walk(nodes, node, Tail(ops))
                 ELSE IF op.kind = "getPointerField" /\ nodes[node].kind = "struct"
                 THEN IF op.index < Len(nodes[node].pointers)
                      THEN Walk(nodes, nodes[node].pointers[op.index + 1], Tail(ops)) ELSE Broken
                 ELSE Broken
Decode(d, exports, imports, answerNodes, answerRoots) ==
    CASE d.kind = "none" -> NoCap
      [] d.kind \in {"senderHosted", "senderPromise"} -> Lookup(imports, d.id)
      [] d.kind = "receiverHosted" -> Lookup(exports, d.id)
      [] d.kind = "receiverAnswer" ->
           IF d.id \in DOMAIN answerRoots THEN Walk(answerNodes, answerRoots[d.id], d.transform) ELSE Broken
      [] d.kind = "thirdPartyHosted" -> Ref("introduction", d.id)
      [] OTHER -> Broken
Target(t, exports, nodes, roots) ==
    IF t.kind = "importedCap" THEN Lookup(exports, t.id)
    ELSE IF t.kind = "promisedAnswer" /\ t.id \in DOMAIN roots
         THEN Walk(nodes, roots[t.id], t.transform) ELSE Broken
\* Data pointers may not refer to none tombstones in the cap table.
PayloadCap(table, index) == IF index < Len(table)
                          THEN IF table[index + 1] = NoCap THEN Broken ELSE table[index + 1]
                          ELSE Broken
AttachedFd(index, fds) == IF index < Len(fds) THEN fds[index + 1] ELSE "noFd"
\* Unknown question-creating requests can fail with an exception. Resolve
\* rollback returns the replacement credit (CapnpCapabilities.ReceiveExport).
UnimplementedEffect(kind) ==
    IF kind \in {"bootstrap", "call", "provide", "accept", "join"} THEN "exception"
    ELSE IF kind = "resolve" THEN "releaseReplacement"
    ELSE "abort"
=============================================================================
