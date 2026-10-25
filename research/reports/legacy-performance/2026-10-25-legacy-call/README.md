# Caller-owned legacy invocation contexts

CallContext<'call,P,R,H> now keeps its invocation provider H. CallStorage<P,R>
borrows typed views, transfers the original owners to a suspended handler, and
chooses the completion representation. PreparedCall retains concrete parameter
and result owners; it returns an ordinary Result and never sends a Return or
changes cancellation policy. The protocol driver still owns those decisions.
Parameters drop before the result sink, including an unentered invocation.

The default hosted provider remains &mut dyn CallContextHook, with the same
fields and borrowed/owned hook operations. Its small projection/transfer methods
inline statically, adding no second vtable. Hosted complete retains its original
body and two-editor closure. Generic try_complete lends the context to its
closure; get() then borrows both editors. This avoids an unnecessary static
lifetime requirement from Rust's current higher-ranked GAT limitations when the
provider itself owns borrowed storage. No allocator workaround or unsafe erasure
is used. into_typed keeps the provider and into_owned consumes it exactly once.

Five allocation-counted tests cover a fixed Return with retained answer metadata,
growth failure reaching completion while preserving earlier results, moving
parameters/results into a suspended caller-pinned service and canceling it in
place, parameter-guard release before result cleanup, and traversal failure
before result initialization. All add zero blocks/bytes. The nine actual
alloc-only legacy conversation fixtures still pass, including cancellation, callbacks, fragmented
frames and retained data. Local core
library/wire all-target no-default Clippy and 122 no-default wire tests pass.

verification/portable/legacy-call-link.rs executes one real fixed Call payload
through CallContext, a suspended owned service task and Promise into a fixed
Return on the host. It links for M4 with no undefined/allocator symbols; the
exact scope and hashes are in embedded-link.json. This is one invocation, not a
complete network driver or the required unary/deep/concurrent conversation
matrix. It has not executed on a board. The alternate bounded protocol driver
is not invoked by this probe.

Hosted tests, two-width/profiles Miri and five repeated work/allocation profiles
require the separate immutable qualification. Native latency remains restricted
to dedicated droplets. Actual legacy connection records, ClientHook/PipelineHook,
message/capability storage and generated service dispatch still require caller-
backed integration. This is an ownership boundary extraction, not completion of
no-global-allocator legacy RPC.


Qualification `9292a818a6b8c24b5fd171035c142c8bbe0cf0f2`,
[run 40181910314](https://github.com/DericHuynh/capntproto/actions/runs/40181910314),
passes all 20 strict-provenance invocation Miri executions (five tests across
both widths and no-default/explicit-alloc profiles). Full hosted tests pass;
repeated work/allocation profiling remains running.


## Hosted qualification

Run 40181910314 passes both jobs and all 240 repeated work/allocation trials.
The selected capability-owner/scheduler Ir/Bc/Bi slopes are exact in all cases;
337/337 normalized common captured assembly bodies match. TLS blocks stay
7/57/202 and all bytes/QUIC samples and spreads remain in comparison.json.
Whole Ir ranges -0.031% to +0.058%. These helpers exclude some inlined caller
work, as demonstrated separately by the request code-generation regression.

Simulated Bim regresses in every case: unary +29.994%, dependent +14.338%,
pipeline +11.117%, concurrent +19.949%, callback +11.759%, deep +6.124%.
Bcm also regresses unary +1.554%, callback +2.673% and deep +1.255%, while
improving in the remaining cases. These substantial predictor regressions are
retained; unchanged helper instructions do not establish native performance.

Verified artifact 12594729708: 82,017,079 bytes, SHA-256
`79fa9ad6eefc62faab65784418709e4558a5348dbeaa02921fe17a6f8ae4ab69`.
Raw profiles/captured assembly/symbols remain in that archive. Selected function
attribution reconciles every whole-program delta.
