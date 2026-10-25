# Concrete legacy invocation and generated service dispatch

This isolated no_std fixture drives the public legacy CallContext, PreparedCall
and Promise ownership APIs into the existing generated wire Service/ContextService
bindings. No bounded or hosted RPC protocol driver is instantiated. The fixture
therefore proves the method-dispatch ownership boundary, not allocation-free RPC
connections, capability tables, pipelines or encrypted IO.

Run `cargo test --locked --manifest-path verification/legacy-fixed/Cargo.toml --lib`
and repeat with `--features alloc`. Five tests count both blocks and bytes:
retained inherited-method parameters and real standard-Waker notification;
in-place task cancellation destroying a suspended generated method before its
input owner; independently borrowed context; unknown interface/method and bad
input rejection before application entry/result edits; and 32 simultaneously
suspended calls sharing one service. All five add zero allocated blocks/bytes.

The dispatch future consumes a concrete service adapter. A borrowed `&mut Service`
retains exclusive dispatch when needed. A small per-call generated adapter over
`&SharedServer` allows concurrency without borrowing a shared dispatcher mutably.
This does not make arbitrary service implementations thread-safe. Caller-backed
parameter/result storage, services and pinned task places are declared separately;
no connection record contains a future borrowing that same record/context.

Run `cargo run --locked --manifest-path verification/legacy-fixed/Cargo.toml --bin legacy-fixed-link`
to execute all cases on the host. Build that binary for thumbv7em-none-eabi and
run verification/portable/verify-core-link.py to reject allocator/undefined
symbols. The M4 binary is link evidence, not board startup or execution evidence.
The host build compiler uses std; resolver2 keeps it out of target dependencies.

Connections must still admit the call, select cancellation/streaming protection
from generated metadata before polling, and publish/retain the resulting Return.
The adapter does not supply protected-call execution or send protocol messages.


The input owner now implements capnp_rpc::IncomingMessage and is retained by the
production incoming::Params type. The isolated target dependency on capnp-rpc
has default features disabled; no allocating RpcSystem or bounded driver is
instantiated. The alloc feature separately checks that same ownership path with
hosted dependencies available. Whole-frame read budgets and transport caches
remain the original owner's responsibility.


A sixth test puts both input and output pool guards inside the caller-pinned
future. DispatchResults lends the generated method's result field only after
polling. The intact PreparedCall enforces input-before-result cleanup; completion,
pending cancellation, output-capacity rejection and unpolled drop all release
both pool slots with zero allocations. The test checks actual RefCell borrow
availability during result destruction, not only a synthetic drop counter.
