# Third-party tail-call answers

The runtime implements the standard level-3 `Call.sendResultsTo.thirdParty`,
`Return.awaitFromThirdParty` and `ThirdPartyAnswer` messages. A caller A can
invoke a relay B which tail-calls C; C then returns directly to A, including
result capabilities. The pinned C++ dispatcher leaves these messages
unimplemented, so this extends protocol support beyond that reference runtime.
The standard schema bytes are unchanged.

`Connection::supports_third_party_answers()` defaults to false. The native
Native network enables it using its existing authenticated rendezvous hooks.
It is independent of `supports_third_party()`, which enables capability
introductions. The caller advertises `allowThirdPartyTailCall` on ordinary
requests when its network supports answer adoption. Explicit tail calls and
transparent RPC proxy calls automatically use the third-party path when both
connections support it, belong to the same RPC system, and `introduce_to()`
can construct the contact. Other calls retain the existing forwarding path.
Opaque request wrappers do not expose the private optimization hook, preserving
their policy boundary. This is network configuration, not peer negotiation;
enable it only for a compatible network profile.

The relay sends a third-party destination on its outgoing Call and an
`awaitFromThirdParty` Return on the original question. The callee establishes
the introduced connection, sends `ThirdPartyAnswer`, executes the method once,
and sends the direct Return. Its forwarding connection receives
`resultsSentElsewhere`. A self-introduction adopts the local response through
the rendezvous without allocating a fictitious loopback wire question.

The caller installs a sparse question for the callee-selected ID in
`[2^30, 2^31)`. Ordinary caller IDs avoid this range; pipeline-only IDs retain
bit 31. Duplicate or invalid IDs, repeated answer claims, unpermitted redirects
and attempts to suppress the required Finish abort the offending connection.
Answer rendezvous and capability provisions are distinct opaque values:
`Accept` cannot consume an answer, and an answer cannot consume a provision.

Either the relay's redirect or the callee's adoption may arrive first. Even a
direct Return received early stays private until the authenticated rendezvous
matches. Native tokens bind the introducer, authenticated recipient and random
nonce. Dropping the registration retires it so a late adoption is canceled.
Before authorization arrives, the RPC system owns the completion waiter; a
callee disconnect is retained as a possible result for a delayed redirect.
Native now bounds unmatched setup with a ten-second deadline, configurable
through `native_rpc::Options::answer_setup_timeout`. Either missing side releases
its waiter/registration on expiry, including a direct Return received before
authorization. An expired caller cannot adopt a late answer. Timers stop at
authenticated matching and do not limit method execution. Other networks supply
their own `Connection::third_party_answer_timeout()` future. The Native registry
bounds waiters at 4,096, and the runtime bounds live or finished-but-unreturned
adopted questions at 4,096 per connection. See [Connection Recovery](Connection-Recovery.md)
for the deadline contract and fresh model/trace checks. Ordinary calls whose
relay never sends a redirect still require an application deadline.

The direct response retains its question and returned capabilities until the
application releases it. The original and direct answers independently retain
the callee's invocation. Finishing or disconnecting one route does not cancel
work still owned by the other route or by a dependent pipelined call. Dropping
all cancellable owners cancels the invocation; generated protected-method
policy continues to use the existing call executor. Callee-allocated answer
IDs remain reserved until Return and Finish, including value-only results.

## Early caller pipelines

After the authenticated rendezvous matches, public caller pipelines can address
the adopted answer before its Return. New pipeline references and previously
obtained references that have neither sent calls nor been encoded back onto the
relay connection move to the direct pipeline. Adoption alone does not resolve
the application's response promise. The callee queues calls until the requested
result capability is available; `ResultsHook::set_pipeline()` can publish it
before the method finishes. Self-adoption exposes the local pipeline without a
loopback wire question.

For references that already carried calls or were encoded as receiver answers,
native enables `Connection::supports_pipeline_join_fence()`. New calls queue
while authenticated multiparty Join shares follow both the old and adopted paths.
The old share follows earlier calls through transparent proxies. Matching at the
same capability supplies both the ordering fence and direct acquisition. Join
targets the promised answers without waiting for their Returns, so migration can
complete after `set_pipeline()`. This preserves the acquired endpoint's policy
and uses existing Join messages; `senderLoopback` remains reserved for capabilities
resolving back to the sender.

The native answer-setup deadline also bounds this fence. Unsupported paths,
failed equality, timeout or disconnection keep the original relay reference.
Queued calls are then sent on that route once; no executed request is replayed.
Other remote holders keep their own references. Networks that do not enable the
fence retain the previous relay behavior. Fresh pipeline references and final
response capabilities still use the direct route immediately after adoption.

A fresh migrated pipeline reference retains the adopted question if the
application drops its original response promise and pipeline. Once Return resolves
it to an imported capability, it owns that import independently and can release
the question. An active reference acquired through the Join fence can own an
independent capability before Return; dropping the original response and pipeline
then permits ordinary parent-call cancellation without revoking that capability.
Successful direct responses received before authorization stay private; even an
early exception retains its question until the rendezvous is consumed or
canceled. Dropping all owners releases both questions, and late Returns do not
resurrect them. New requests on migrated references survive relay disconnection.
Requests built before a connection fails retain the existing stale-request
failure contract; while that connection is live, unsent requests can retarget
after adoption. Invalid result transforms yield broken capabilities.

## Verification

`cargo nextest run --locked --test native_pipeline_migration` checks used and encoded
references, early migration, deadline fallback, queued-call cancellation and
relay disconnection over real Native sessions. `RpcPipelineFence.tla` explores
23 states and supplies 22 native transition-prefix replays, three mutation
controls and fair-progress checks. [Deployment details](Discovery-and-Mobility.md)
describe this additional contract. The earlier caller-pipeline model below checks
the default non-opted-in network behavior.

Run `cargo nextest run --test protocol_models answer_adoption_model -- --exact`. It is also part of
`cargo nextest run --locked --workspace --all-targets`.

* `RpcAnswerAdoption.tla`: 77 states, 114 edge-prefix Rust replays across success
  and exception scenarios. It checks either authorization order, early Return,
  caller drop, callee disconnect, Finish counts and capability retention.
* `RpcAdoptedCall.tla`: 206 states, 380 edge-prefix Rust replays. It checks shared
  invocation ownership across both answers and one child pipeline, completion,
  cancellation and independent disconnections.
* Liveness checks cover eventual response release under fair delivery/drop and
  invocation settlement under fair readiness/Finish actions. Twelve injected
  faults must violate their specified safety invariants.
* The original `CapnpTailCall` third-party configuration and its completion
  witness also run. Separate Rust regressions exercise malformed wire messages,
  peer/kind separation, proxy fallback, self-adoption, direct pipelines, and
  real three-peer authenticated Native RPC with capability calls after the relay
  disconnects.
* `cargo nextest run --test protocol_models caller_pipeline_model -- --exact` checks
  `RpcCallerPipeline.tla`: **2,011 states and 3,311 Rust wire trace replays**,
  plus fair-release liveness and nine detected fault mutations. It covers both
  authorization orders, early success/exception Return, unused/previously used
  references, actual wire targets, retained capabilities after caller drop,
  independent disconnects and Finish ownership. Request drop and relay failure
  occur after adoption in this model. Separate regressions cover earlier drops,
  fresh references, prebuilt requests, encoded references, missing result
  capabilities, self-adoption, and real Native calls before Return and after both
  relay links close. The canonical runtime runner includes this model.

Every graph edge is replayed after a reachable prefix through production RPC
wire handling, observing outcomes, ownership and emitted messages. These are
finite scenarios with drained executor schedules and one authenticated
rendezvous. They do not prove cryptography, arbitrary transport scheduling,
complete C++ API parity or whole-runtime refinement of the composed model.
The TLS verification rules and TLS 1.3 with pinned mutual authentication /
TLS 1.3 with reservation admission profiles are unchanged.
