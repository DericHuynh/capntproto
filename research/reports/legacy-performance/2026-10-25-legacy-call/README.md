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
