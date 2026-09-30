---------------- MODULE CapnpComponentTypes ----------------
(*
 @typeAlias: pair = <<Int, Int>>;
 @typeAlias: token = <<$pair, Int>>;
 @typeAlias: rpcCall = {id: Int, parent: $token, dep: $token, phase: Str, result: Str, observed: Str, finishSent: Bool, finishRecv: Bool, returnSent: Bool, returnRecv: Bool, mode: Str};
 @typeAlias: rpcMessage = {kind: Str, id: Int, parentId: Int, token: $token, result: Str, mode: Str};
 @typeAlias: capExport = {count: Int, generation: Int, promise: Bool, resolved: Bool, target: Int};
 @typeAlias: capImport = {count: Int, generation: Int, resolved: Bool};
 @typeAlias: capCredit = {id: Int, generation: Int, state: Str, source: Str};
 @typeAlias: capMessage = {kind: Str, id: Int, generation: Int, ticket: Int, replacement: Int};
 @typeAlias: handoffMessage = {kind: Str, recipient: Int};
 @typeAlias: joinMessage = {kind: Str, part: Int, host: Int, ordinal: Int};
 @typeAlias: joinResult = {host: Int, ordinal: Int};
 @typeAlias: tailMessage = {kind: Str, id: $pair};
 @typeAlias: twoPartyReply = {part: Int, succeeded: Bool, cap: Bool};
 @typeAlias: wireRef = {kind: Str, id: Int};
 @typeAlias: wireOp = {kind: Str, index: Int};
 @typeAlias: wireDesc = {kind: Str, id: Int, transform: Seq($wireOp)};
 @typeAlias: wireNode = {kind: Str, cap: $wireRef, pointers: Seq(Int)};
*)
CapnpComponentTypes_aliases == TRUE
============================================================
