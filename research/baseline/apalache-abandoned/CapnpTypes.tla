---------------- MODULE CapnpTypes ----------------
(*
 @typeAlias: channel = <<Int, Int>>;
 @typeAlias: slot = <<$channel, Int>>;
 @typeAlias: localSlot = <<Int, Int>>;
 @typeAlias: tableSlot = <<$channel, Str, Int>>;
 @typeAlias: ref = {tag: Str, host: Int, peer: Int, id: Int, generation: Int, path: Seq(Int)};
 @typeAlias: node = <<Str, Int, Seq(Int)>>;
 @typeAlias: target = {kind: Str, id: Int, path: Seq(Int)};
 @typeAlias: desc = {kind: Str, id: Int, path: Seq(Int), completion: Int, host: Int};
 @typeAlias: flags = {only: Bool, noPipeline: Bool, noFinish: Bool, workaround: Bool, allowThird: Bool, honorOnly: Bool, releaseParams: Bool, releaseResults: Bool};
 @typeAlias: export = {ref: $ref, count: Int, generation: Int, resolved: Bool, pinned: $ref, provision: Int};
 @typeAlias: import = {count: Int, generation: Int, resolution: $ref, offer: Seq(Int), handoff: Int};
 @typeAlias: op = {src: Int, dst: Int, id: Int, class: Str, kind: Str, phase: Str, arrival: Int, target: $ref, route: $ref, path: Seq(Int), flags: $flags, params: Seq($ref), paramTickets: Set(Int), result: Seq($ref), qResult: Seq($ref), content: Seq($node), qContent: Seq($node), resultTickets: Set(Int), outcome: Str, observed: Str, finishWanted: Bool, finishSent: Bool, finishRecv: Bool, returnSent: Bool, returnRecv: Bool, forward: Int, qRedirect: Int, adoption: Int, taken: Bool, destination: Str, completion: Int, embargo: Int, join: Int, part: Int, parts: Int, joinHost: Int, ordinal: Int, method: Str, data: Int, logical: Int, label: Int, dispatched: Bool, automatic: Bool, vine: $ref, handoffDone: Bool};
 @typeAlias: message = {kind: Str, id: Int, class: Str, op: Int, target: $target, caps: Seq($desc), content: Seq($node), tickets: Set(Int), flags: $flags, result: Str, other: Int, completion: Int, embargo: Int, join: Int, part: Int, parts: Int, host: Int, ordinal: Int, method: Str, data: Int, destination: Str, count: Int, context: Str, echoed: Str};
 @typeAlias: command = {kind: Str, src: Int, dst: Int, ref: Int, pick: Int, arg: Int, argPromise: Bool, local: $ref, target: $ref, method: Str, data: Int, path: Seq(Int), flags: $flags, after: Set(Int), wait: Set(Int), order: Int, other: Int, part: Int, parts: Int, embargo: Int, waitDirect: Bool};
 @typeAlias: credit = {channel: $channel, id: Int, generation: Int, phase: Str};
 @typeAlias: barrier = {host: Int, sender: Int, arrival: Int, ref: $ref, context: Str, completion: Int, embargo: Int, done: Bool};
 @typeAlias: state = {connected: Set($channel), closing: Set($channel), wire: $channel -> Seq($message), questions: $tableSlot -> Int, answers: $tableSlot -> Int, exports: $slot -> $export, imports: $slot -> $import, ops: Int -> $op, next: Int, arrivals: Int -> Int, promises: $localSlot -> $ref, proxies: $localSlot -> $ref, credits: Seq($credit), labels: Int -> Int, steps: Set(Int), barriers: Set(<<Int, Int>>), embargoQueue: Seq($barrier), offers: Set(<<Int, Int, Int, Int, Int>>), accepted: Set(<<Int, Int, Int>>), shortcuts: Set(<<$ref, $ref>>), joinCaps: Set(<<Int, Int, Int, Int>>), heldJoins: Set(Int), shares: Set(Int), awaiting: Int -> Int, adopted: Int -> Int, grants: Set(<<Int, Int, Int>>), joinGroups: Set(<<Int, Int, Int, Int, Int>>), joins: Int -> Str, sturdy: Set(<<Int, Int, $ref>>), deliveries: Seq(Int), events: Set(Str), faults: Set(Str)};
*)
CapnpTypes_aliases == TRUE
===================================================
