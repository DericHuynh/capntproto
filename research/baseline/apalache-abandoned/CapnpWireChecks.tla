------------------------- MODULE CapnpWireChecks -------------------------
EXTENDS CapnpWire
VARIABLES
    \* @type: Int;
    step
\* @type: <<Int>>;
vars == <<step>>
Init == step = 0
Next == step' = step
Spec == Init /\ [][Next]_vars
\* @type: (Str, $wireRef, Seq(Int)) => $wireNode;
Node(kind, cap, pointers) == [kind |-> kind, cap |-> cap, pointers |-> pointers]
Nodes == (0 :> Node("struct", NoCap, <<1, 2>>)) @@
         (1 :> Node("struct", NoCap, <<2>>)) @@
         (2 :> Node("cap", Ref("local", 7), <<>>))
\* @type: Seq($wireOp);
Fields == <<Op("getPointerField", 0), Op("getPointerField", 0)>>
Exports == 5 :> Ref("local", 7)
Imports == 5 :> Ref("import", 5)
Roots == (4 :> 0) @@ (6 :> 2)
TransformSemantics ==
    /\ Walk(Nodes, 0, Fields) = Ref("local", 7)
    /\ Walk(Nodes, 0, <<Op("noop", 0)>> \o Fields) = Ref("local", 7)
    /\ Walk(Nodes, 0, <<Op("getPointerField", 1)>>) = Ref("local", 7)
    /\ Walk(Nodes, 0, <<Op("getPointerField", 2)>>) = Broken
    /\ Walk(Nodes, 2, <<Op("getPointerField", 0)>>) = Broken
    /\ Walk(Nodes, 2, <<>>) = Ref("local", 7)
    /\ Walk(Nodes, 0, <<>>) = Broken
    /\ Walk(Nodes, 99, <<>>) = Broken
DescriptorSemantics ==
    /\ Decode(Descriptor("none", 0, <<>>), Exports, Imports, Nodes, Roots) = NoCap
    /\ \A kind \in {"senderHosted", "senderPromise"} :
           Decode(Descriptor(kind, 5, <<>>), Exports, Imports, Nodes, Roots) = Ref("import", 5)
    /\ Decode(Descriptor("receiverHosted", 5, <<>>), Exports, Imports, Nodes, Roots) = Ref("local", 7)
    /\ Decode(Descriptor("receiverAnswer", 4, Fields), Exports, Imports, Nodes, Roots) = Ref("local", 7)
    /\ Decode(Descriptor("thirdPartyHosted", 5, <<>>), Exports, Imports, Nodes, Roots) = Ref("introduction", 5)
    /\ Decode(Descriptor("receiverHosted", 99, <<>>), Exports, Imports, Nodes, Roots) = Broken
    /\ Target(Descriptor("promisedAnswer", 6, <<>>), Exports, Nodes, Roots) = Ref("local", 7)
    /\ Target(Descriptor("importedCap", 5, <<>>), Exports, Nodes, Roots) = Ref("local", 7)
PayloadSemantics ==
    /\ PayloadCap(<<NoCap, Ref("local", 7)>>, 0) = Broken
    /\ PayloadCap(<<NoCap, Ref("local", 7)>>, 1) = Ref("local", 7)
    /\ PayloadCap(<<NoCap>>, 1) = Broken
    /\ AttachedFd(0, <<"fd0">>) = "fd0"
    /\ AttachedFd(255, <<"fd0">>) = "noFd"
Compatibility ==
    /\ UnimplementedEffect("call") = "exception"
    /\ UnimplementedEffect("resolve") = "releaseReplacement"
    /\ UnimplementedEffect("obsoleteSave") = "abort"
=============================================================================
