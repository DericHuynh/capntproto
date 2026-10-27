# Runtime architecture

For package ownership, dependency direction, and file placement, see the
[repository layout](Repository-Layout.md).

Capntproto is an unreleased 0.x protocol/runtime. Breaking Rust APIs and rejecting
obsolete private formats is allowed. Standard Cap'n Proto wire behavior,
capability authority, schema evolution, and C++ interoperability remain contracts.

The [portability design](#planned-portability-boundaries) below describes the
`no_std` / allocation-free migration. The shared policy engine supports these
profiles. The portable wire driver supports caller-backed RPC dispatch; default
hosted `RpcSystem` ownership and the root package still require allocation and
the standard library. Owned generated clients can reach scoped graph routes,
including pending promises, through `capnp_rpc::scoped::ClientHooks`. Its wire
adapter uses tracked portable invocations for payloads, pipelines and replies;
it also prepares admitted parameters for existing owned server hooks without
replacing their result contexts. The default network runtime has not switched
to that bridge.

Legacy and portable now share the bounded atomic notification FIFO in
`capntproto-rpc-engine::ready_queue`. Its storage and transitions require only
`core`; callers own the consumer cursor, wakeups and task generations.
This helper is available on targets with byte and pointer atomics; the rest of
the caller-polled engine keeps its existing target requirements. Hosted
drivers add their executor wakers around the same implementation. Portable keeps
its exact generation checks, and larger hosted operation sets retain the bounded
overflow mailbox. Ordinary wakeups no longer lock that mailbox. This ports the
legacy algorithm into the allocator-free engine while preserving the existing
portable protocol and storage contracts. Owned `ClientHook` and server-future
adapters still require allocation; their lifetimes must be preserved as further
legacy paths move into caller-backed engine components.

The answer and import ledgers also use legacy's direct-index lookup technique
for dense wire IDs, validating the candidate slot before using it. Sparse IDs,
out-of-order admission and IDs displaced by retained retired answers keep their
ordinary lookup. This adds no index allocation, table capacity or trusted wire
identity; generation and lifetime checks remain authoritative.

Portable admission now reuses legacy's lowest-free-index heap algorithm. Its
heap cells live inside the existing caller-owned answer, import, export, graph,
operation, invocation and exchange slots. Preflight only inspects an available
index; commit consumes it, and final retirement returns it. A displaced-record
count also lets answer/import lookup reject an absent dense ID directly, while
sparse IDs and retained generations preserve their bounded fallback search.

Hosted promised-answer calls retain their graph route in the originating
operation instead of immediately allocating a hosted endpoint binding. The
original graph still owns remote promise ordering and reflected-call barriers.
Descriptor export, general forwarding and Return materialize that same route
when a hosted binding is needed. Direct routes consume the existing endpoint
budget; private operation and endpoint identities prevent slot reuse from
retargeting a hook. Terminal cleanup releases the route and rejects its observers.

Completed hosted pipelines can retain the immutable response payload directly;
ordinary completion needs no boxed pipeline adapter or cloned adapter. Server
publication keeps one retained capability reference inline, with the ordinary
bounded path for larger tables. Known call, admission and worker transitions
preserve unrelated scheduler idle certificates; route/policy controls retain
the broader invalidation path. Operation notifications still own parked futures,
and cooperative budgets and physical output ordering remain unchanged.


## Legacy scheduling, framing and deep copying

The active hosted driver uses Tokio. Its task set distinguishes ordinary ready
work from connection output: ordinary RPC transitions run before the output
batch, following KJ's end-of-turn write policy. Separate activity notifications
let newly ready work defer a pending batch, with a bounded turn count to prevent
writer starvation. The transport supplies the executor's deferred-wake hook;
the RPC task set retains scheduling and cancellation ownership.

TCP exposes dedicated `ReadHalf` and `WriteHalf` adapters. Small reads drain
readiness, scatter/gather writes reach the socket intact, and the framer can
report the exact number of bytes missing from an incomplete message body. On
Linux, large incomplete bodies use a bounded receive low-water hint while TCP
receive autotuning remains enabled. Completing the body restores ordinary
readiness; dropping the adapter restores the original socket option. Other
transports keep the default no-op hint. These public half types replace the
earlier compatibility-wrapper aliases, an intentional unreleased API change.

Bootstrap settlement retires replaced capability routes outside mutable table
borrows, so their Finish obligations can join the pending batch. Shutdown
observers are polled only after their own notification, and completed observers
are consumed. Physical write completion, response interest, pipeline interest,
and producer lifetime remain separate ownership obligations.

Deep copies validate a near pointer's target and complete extent in one arena
operation. This removes a repeated segment lookup without removing bounds,
nesting, traversal-budget or amplification checks. Far-pointer landing pads
keep their existing validation. The new
[deep/concurrent benchmark suite](../../benchmarks/rpc/README.md) measures these
costs through real TCP service chains, including eight concurrent client
connections sharing a downstream service connection.

## Portable hosted execution boundaries

The October 17 redesign starts from the measured `866610259cf5` runtime.
Its dedicated campaign passes 11 of 16 latency targets. In the pipelined-read
trace, portable and C++ already issue the same steady-state frame groups and
byte counts, while portable client processing takes 33,143 ns p50 versus
25,802 ns for C++ and 10,839 ns for legacy. The portable server is faster than
C++ in that trace. Matching socket writes alone therefore cannot explain or
close the remaining gap. These instrumented timings identify where to look;
they are not acceptance measurements. See the
[benchmark evidence](Quality-and-Benchmarks.md#portable-architecture-reset-october-17-2026).

The architecture has three sources of optimization friction. The hosted bridge
adapts owned hooks, portable graph references, invocation owners and response
futures; each has distinct cancellation and lifetime obligations. The connection
coordinator previously mixed six work lanes, frame admission, network policy,
completion and physical flushing in one 2,184-line module. Finally, optimizations
relied on numeric lane masks and separate policy booleans, making it difficult
to establish which idle scans a transition could safely preserve. Allocation
reductions in local response-sharing prototypes did not establish a general
latency improvement. Those prototypes are preserved as evidence and are not
part of this architectural baseline.

The protocol remains in the allocation-free engine and wire crates. Hosted
execution is divided inside `capntproto-rpc::scoped::wire::coordinator`:

| Component | Responsibility and boundary |
| --- | --- |
| `scheduler` | Typed `Lane`/`WorkSet`, bounded round-robin selection, idle-scan cursor, coalesced notifications and executor-task registration. It imports no protocol tables, transport or application hooks. |
| `control` | Bootstrap/network integration and the public `Activity` contract. `Control::preserves_idle(activity)` describes effects on pending policy work. Admission bypass and hook visibility retain separate contracts. |
| `work` | Outgoing invocation, incoming worker, foreign-promise and maintenance adapters; preserves their existing budget charges and ownership transitions. It does not poll physical IO. |
| `ingress` | Validated frame admission, Busy-frame retention, resource handoff and response delivery. |
| `pump` | Arbitration between ready work and output capacity, batching, partial writes, flush completion and transport idle reporting. |
| `lifecycle` | Public run methods, stop/abort polling, receive loop and terminal cleanup. |

`Coordinator` composes these components in the existing generative scope. The
split adds no background tasks, dynamic dispatch, locks, allocation sites or
second protocol ledger. Public constructors and run methods remain usable. Custom controls now implement
`preserves_idle` and receive the restricted `ControlContext`; the old per-activity
adapter and arbitrary coordinator-mutation API have been removed. A control can
return false from `preserves_idle` when it needs conservative rescanning. A policy must notify its registered waker when
independent work becomes ready, including reentrant destructor callbacks.

Scheduling state and wake state are deliberately distinct. A ready bit means
that a lane must be polled, not that progress is guaranteed. Pending certifies
idle; Yield retains readiness and wakes the executor without invalidating other
certificates. A new event prefers its lane only when the preceding scan is fully
idle, preserving cleanup fairness during hot traffic. Notifications register
the current executor before consuming the pending mask, and replace that
registration when the task changes. Terminal teardown releases both notifier
and cached parent-waker ownership. All 64 ready subsets and six starting lanes
are checked against an independent rotated-order model.

Ownership stays with its existing authority: the engine owns question/answer
and capability ledgers, `Calls` and `Worker` own application execution, input
retains ancillary resources until admission, and `io::Output` owns accepted
send cursors through partial write and flush. A module boundary does not permit
callbacks under table borrows, premature result recycling, duplicate resources,
or treating local flush as peer acknowledgement. Existing lifecycle, early
pipeline, retained-result, resource and backpressure tests cross these boundaries.
The coverage gate compares the combined production components with the original
coordinator floor, in addition to existing per-file and crate-group checks. Every
extracted component must have instrumentation; test modules cannot dilute this
comparison. The existing baseline and coverage tolerance are unchanged.

Further optimization should target one boundary at a time. Measure admission to
dispatch, dispatch to publication, and publication to physical write alongside
the existing socket trace; account separately for hook synchronization and
completion ownership. Collapse an owned-hook/graph conversion only after its
authority, early-pipeline and cancellation roles are explicit. Keep optimized
and general paths on the same scheduler and frame/output owners. The response
ownership bridge itself has not been replaced in this change, and the new
boundaries do not constitute a claim of 5% performance parity.

## Portable ownership and integration review

The follow-up to `ea6ed2d8d974` examines the ownership path itself. Separating
coordinator modules did not remove the work performed between a socket read and
the next write. The C++ implementation's `ImportClient` directly owns its import
identity and descriptor; its destructor removes that import and emits Release.
`QuestionRef` directly owns response interest and emits Finish before allowing
question-ID reuse (`vendor/capnproto/c++/src/capnp/rpc.c++`). The portable engine
correctly keeps those protocol obligations in branded, generational tables,
but its hosted adapter has additional ownership and synchronization boundaries.

The frozen `ea6ed2d8d974` local whole-process pipeline profile attributes
2,463,558 of 75,637,856 client instructions (3.26%) directly to `collect_hooks`.
Other synchronization is inlined into callers, and these totals include setup
and warmup. This is a useful target, but removing that one function's self cost
cannot by itself explain the much larger latency gap. The review therefore
also covers execution ownership and integration contracts rather than assuming
that reclamation alone will reach parity.

| Boundary | Current cost or constraint | Architectural direction |
| --- | --- | --- |
| Hosted endpoint to graph route | `ClientHooks` owns a graph reference while generated hooks own an `Rc<Endpoint>`. Previously every collection discovered releases by upgrading every weak endpoint. Export also triggered those collections. | Record final-owner release explicitly; consume records without inspecting unrelated live routes. Keep protocol retirement in the engine. |
| Graph route to hosted readiness | Pending hooks must follow resolution and reflected ordering barriers, then retain the settled endpoint before notifying observers. | Keep settlement separate from reclamation. A future graph change feed must identify dependent promises and barrier openings, including indirect aliases. |
| Import owner to hosted metadata | `Output::metadata_source()` now supplies a stable revision source. A settled import can receive its first descriptor through a later alias. Local hooks can also change their diagnostics. | Consume explicit revisions; the current consumer still walks live entries. Caching an absent descriptor or treating settlement as metadata immutability is incorrect. |
| Hosted call to portable invocation | `Call`, `Calls::Entry`, `WireCall`, optional local execution, response state and pipeline observers carry different interest and cancellation lifetimes. | Introduce one execution owner with separate response and pipeline interests before removing adapters. Keep early pipelines, tail adoption, membranes and protected execution in the design. |
| Readiness to execution | `Calls` and `Worker` use rotating slot scans; a general wake invalidates multiple idle certificates. High-water table capacity can outlive a burst. | Use generation-checked ready tokens for individual execution owners, with an allocation-free bounded overflow/full-rescan path and explicit fairness. Merely adding a live count does not establish a latency gain. |
| Network policy to coordinator | `Control` now receives a restricted context. Built-in active multiparty work still uses a private broad mutation boundary. | Refine internal network effects without reopening arbitrary external mutation; preserve invalidation on Pending and error paths. |
| Parameters/results to physical output | Plain payloads already use admitted shared storage and prepared output. Capability-bearing payloads still build owned hook tables. | Preserve admitted storage and exact receipts through execution; measure capability-table construction separately from byte copying. Do not add another generic message wrapper. |

The first implemented step is an endpoint release journal in the hosted bridge.
Each registry slot reserves room for one release record before an endpoint is
published. The last `Endpoint` owner records its private slot and then notifies
the driver, after releasing the journal borrow. Drop does not allocate, release
graph authority, invoke application code under a table borrow, or send a frame.
A slot cannot be reused until its record has been consumed. The private slot is
only a retirement notification; authenticated lookup still checks the inbox and
exact `Rc` identity, and graph authority remains branded and generational.

`ClientHooks::reclaim_released()` consumes only those records. `collect()` adds
promise settlement and skips the route walk when no unsettled endpoint exists.
`collect_with_metadata()` and wire collection still refresh live metadata before
publishing readiness, including already-settled imports. The unsettled count and
entry state are committed before resolution callbacks. Collection also consumes
releases caused by metadata/resolution callbacks without holding the journal
borrow across graph operations. Normal maintenance still sends Finish/Release
under the same ordering and output-backpressure rules. Storage grows with the
registry's admitted high-water slot count, not its configured maximum.

The initial endpoint-journal change was additive. The subsequent integration
redesign below deliberately breaks server, control and metadata adapter APIs. It does not
make all collection constant-time: metadata refresh still scans live entries,
pending settlement scans the table when needed, and export identity lookup and
free-slot selection remain linear. Those costs should not be confused with the
now-explicit final-owner release path.

### Mandatory integration contracts and C++ reassessment

The earlier [three experiments](../../research/reports/portable-performance/2026-10-17-architecture/README.md)
remain immutable historical evidence. The project has no API compatibility
obligation at this stage, so all three contracts are now mandatory production
interfaces. This is a larger integration change than applying the combined
prototype: generated bindings and application services use the new call context,
and the optional borrowed-dispatch and metadata-polling compatibility paths are
removed. See the [migration guide](RPC-Applications.md#generated-service-dispatch).

* `Control::poll`, `received` and `shutdown` accept `ControlContext`.
  Public operations perform bootstrap completion/publication and record effects;
  dropping the context commits invalidation even after Pending or an error.
  An independent control wake does not invalidate application idle certificates.
  Active built-in multiparty work still records a broad network effect.
* `Output::metadata_source` provides a stable `Rc<MetadataSource>` for imports
  that can acquire metadata, including an initially absent descriptor. Revisions
  replace repeated metadata reconstruction. Sources are consulted before exposing
  imported hooks, including pending aliases and synchronous dispatch. `None`
  declares that an import has no metadata; it is not a transient empty snapshot.
* Generated server methods receive `FooContext<'_>` and return `Promise`.
  `complete` borrows admitted parameters and the original result sink during
  synchronous dispatch; `into_owned` transfers the hooks before suspension.
  Structured reply bindings use an affine `ReplyContext`: completion consumes it,
  builders cannot escape, and ownership transfer yields `Reply`, not `Results`.
  The same local dispatch path enforces cancellation, streaming and revocation.
  Non-cancellable wire methods can borrow under the existing immediate-poll
  permission with a supplied executor. Local calls and opaque wrappers defer
  entry until protected execution is admitted, and streaming methods defer until
  their ordering barrier is installed.

The C++ implementation provides a useful ownership comparison. In
`vendor/capnproto/c++/src/capnp/rpc.c++`, `Question` and `Answer` directly track
response/pipeline interest and running execution (around lines 690 and 720).
`RpcCallContext` owns the admitted message, capability table and parameter reader
(line 3159), explicitly releases parameters (`releaseParams`, line 3370), and
lazily constructs results in the outgoing message (`getResults`, line 3373).
`sendReturn` handles answer interest and capability exports together (line 3243).

The new Rust context moves service invocation closer to that model: synchronous
methods at a permitted non-streaming wire dispatch borrow the real result
producer and do not allocate a boxed result hook or an async method future.
Protected methods require the existing immediate-poll permission and a supplied
executor; local calls and opaque wrappers retain deferred entry so executor
rejection precedes application side effects. The normal generator produces this path; no benchmark
specific interface/method dispatcher is needed. Opaque policy hooks explicitly
take ownership, so membranes and other wrappers still intercept calls.

Prepared calls and returns now read the payload through the same selected pointer
used for building it. This follows `RpcRequest::getRoot`,
`RpcResponseImpl::getResults` and `RpcCallContext::getParams`: retain the payload
location instead of decoding the Call/Return envelope again on each access.
`SelectedBuilder::get_reader` reconstructs a borrowed reader from the owner's
segment and offset, so Rust moves cannot invalidate a cached arena reference.
The owning selection prohibits ancestor replacement. The reader retains the
ordinary builder-reader policy; publication still validates capability indices,
traversal limits and nesting through an independent checked reader. Hosted
parameter hooks still imbue their capability table after selecting the content.

An inline admitted-parameter owner was also tested. It removed one allocation per
eligible call but increased server instructions, so the boxed owner remains.
Allocation count alone is insufficient evidence for changing dispatch ownership.
The [selected-reader measurements](../../research/reports/portable-performance/2026-10-17-cpp-readers/README.md)
record both experiments and map the retained change to the pinned C++ source.

Three architectural costs remain after these API changes:

1. **Execution ownership remains distributed.** The parameter adapter is still
   boxed, and concurrent/deferred dispatch still transfers owned hooks. `Calls`,
   graph routes, wire questions, worker execution and pipeline interests remain
   separate owners. A next design should bind one admitted invocation to its
   parameter storage, result producer and separately counted response/pipeline
   interests. It must cover tail adoption and protected execution before removing
   existing owners. The borrowed context enables that work but does not finish it.
2. **Metadata is revisioned, not event queued.** Wire refresh still walks the
   endpoint registry; unchanged settled imports now skip weak-reference upgrades,
   but each metadata source still adds ownership overhead. A generation-checked dirty-route queue with bounded
   overflow/rescan and explicit wakeup would remove unrelated settled-route work.
   Promise settlement and local dynamic projections need their own events.
3. **Readiness is still primarily per lane.** General application wakes and
   active multiparty effects can invalidate broad scans. Individual execution
   readiness should carry generation-checked tokens with fair rotation and an
   allocation-free overflow path. A live count or a narrower trait alone does
   not make a scheduler event driven.

No further wire-format, schema, generative-authority or allocation-free-engine
break is indicated. These remaining changes should stay within the new ownership
boundaries where possible. Copying C++ direct connection access into arbitrary
Rust destructors would conflict with scoped borrowing and bounded output;
explicit lifecycle records provide comparable information safely.

The existing pipelined-read fixture sends the child before awaiting the parent
and retains parent response interest. Dependent-read retains the parent while
awaiting the child; callback exercises bidirectional execution and fanout-32
concurrency. None is a large live-registry scaling fixture. Add separate scaling
diagnostics for settled hooks, sparse aliases, late descriptors and post-burst
traffic while keeping the 12 remote/four loopback acceptance cells unchanged.
Matching socket write groups is necessary evidence about batching, but internal
read-to-write CPU work and uninstrumented latency determine the remaining gap.

The [mandatory integration measurements](../../research/reports/portable-performance/2026-10-17-integration/README.md)
confirm two fewer allocations per eligible synchronous call. Pipeline server
malloc calls fall from 34,314 to 29,913, including startup; one control owner is
added at startup. The metadata contract adds 1,103 pipeline client allocations.
Total pipeline instructions change by -0.738% client and +1.646% server, so the
new boundaries do not establish a broad speedup. The final local pipeline p50
is 43.044 microseconds, against 31.026 for C++ and 30.255 for current legacy.
All six baseline/candidate peer traces preserve complete write bytes and groups.
The steady pipeline still matches C++ batching: 1,368-byte
Finish/Release/Call/Call writes and 1,216-byte paired Returns. Startup and cleanup
remain distinct and are retained in the traces.

Work remains spread across scheduling, wire/engine transitions, checked layout
access, allocation and copies. The annotated 95% of pipeline self-instructions
attributes about 13% on each peer to scoped hosted code, 8-12% to wire code,
12-15% to core layout/value code, and 2-3% to Tokio/Mio source locations. Inlining
and unattributed standard-library work limit that classification; these are not
exclusive causal latency budgets. In particular, scheduler ordering can change
latency without dominating instruction counts. Repetition modes vary greatly,
including the separately instrumented processing CPU windows, so retain all
samples instead of treating pooled timing differences as causal speedups.

### Compio integration boundary

Tokio remains the production host and the selected runtime for further portable
optimization. Compio is an opt-in benchmark experiment (`compio-experiment`),
with no migration of the production transports planned. Preserve its measurements
as architectural evidence while comparing new portable changes on Tokio.

The portable engine, hosted `portable::RpcSystem`, protected call executor and
task set already run without Tokio. The [Compio benchmark host](../../benchmarks/rpc-compio/README.md)
demonstrates that boundary with the production driver and shared workflow code.
It runs a Compio local executor, TCP sockets and deadlines, with a current-thread
Tokio control in the same executable. The two Compio adapters use either
`PollFd` with borrowed buffers and vectored writes, or the buffered completion
compatibility layer. This does not migrate the root package's TLS, QUIC, Unix
descriptor passing, timers or application services.

Compio fits the local `Rc` ownership model, but its completion API is not the
same as KJ's Linux socket implementation. The pinned `kj/async-unix.c++` uses
epoll readiness. Compio supports both a polling driver and io_uring; the local
container rejects `io_uring_setup` with EPERM, so only its Poll backend is
measured. An explicit io_uring request fails visibly rather than silently
falling back.

A native completion transport should retain input buffers until the operation
completes and retain outgoing frame owners through every partial write, canceled
observer and drain. The existing `IncomingFrameStream`, owned `OutgoingFrame`
and `OutgoingMessageStream` are the useful integration boundary. The generic
completion-to-`futures-io` bridge adds staging copies and boxed operation futures;
replacing Tokio names alone does not remove them. A future completion experiment
would belong in a frame-oriented `VatNetwork` at that boundary. No additional
core API break is needed to run the measured
Compio hosts; a direct completion implementation must prove buffer lifetime,
FIFO publication, backpressure and cancellation before becoming the default.

The same-executable experiment does not support replacing the current host with
either adapter as a performance improvement. Pipeline instructions rise about
13% with readiness and 23-26% with the completion bridge. More significantly,
both Compio hosts split a steady pipeline client write into 40 bytes of Finish
and 1,328 bytes of Call/Call/Release. Tokio and C++ retain one 1,368-byte
Finish/Release/Call/Call group. The server still sends paired 1,216-byte Returns.
Thus the Compio read-to-first-write CPU interval ends before much of the next
call's work and is not comparable to the complete Tokio/C++ group. The bridge's
rustix direct syscalls are also outside the libc CPU probe; its empty probe
journal is rejected, not reported as zero processing cost.

This reveals a fourth architecture issue: **publication batching depends on
executor wake order**. Give a connection an explicit, bounded publication turn
that can combine retirement and ready application commands before flushing.
Control-only traffic must still make progress when no application call follows;
output backpressure, protected calls and early pipelines must retain their
ordering. A blanket extra yield would not establish that contract. Validate this
boundary with decoded Tokio and C++ wire groups. Tokio is the selected executor;
the Compio comparison remains available for reproducing the scheduling evidence.

## Route ownership

`transport::Identity` owns an immutable, construction-checked Ed25519 pair.
Private fields live in a restricted child module; callers can generate a pair,
derive one from a private seed, or import a pair with a matching public key.
Diagnostics and `public_key()` expose only the public key. Transport configuration
copies the validated secret into its own key state, so dropping the source
identity does not revoke existing configurations or authenticated sessions.

Application object handoffs enter through `handoff::serve()`. It obtains an
`AuthenticatedSession` using immutable introduction bindings, then installs a
private bootstrap on that same session's IO. The captured introduction ID prevents
replacing the shared state from rebinding an existing connection. `Serving` owns
both drivers and cancels them on drop or canceled `wait()`; the recipient's
`Direct` owner uses the same lifetime boundary. No public API accepts bytes as
proof of an introduction's authenticated peer.

`semantics::HandoffState` keeps its counters private and exposes a named
`HandoffPhase`. Acceptance is derived from the phase, and revocation preserves
progress while allowing already admitted calls to drain. Enqueue reserves space
in the cumulative drain counter so admission cannot create unfinishable work.
The state cannot be deserialized or constructed from arbitrary fields. It is
re-exported from the allocation-free `capntproto-rpc-engine` implementation;
Kani checks its initial state and inductive transition invariant over full-width
counters. The host and portable fixture execute this same implementation.
`NativeStreamGate` uses a typed role and one explicit state; a failed preface
cannot be reset by another authentication notification. These pure policies do
not themselves establish peer authentication; the transport/session boundary does.

`native_rpc::session::SessionTask` owns every task for one route generation.
Workers and diagnostic observers retain only its `Lifecycle`, not the task owner.
Dropping the owner cancels its tasks and closes arbitration admission. Endpoint
removal uses the captured endpoint identity, so stale work cannot remove a new
route. `RouteObserver` retains the first terminal cause after removal.

The state owns its applicable data: Connecting, Authenticated(control),
Draining(control), or Terminal(cause). Only the lifecycle changes these states.
Direct sessions and selected connector/arbitration sessions pass through the same
identity-checked installation. Arbitration selects a session; it does not mutate
RPC lifecycle state or own another shutdown-control slot.

The two-party queue terminator drains admitted messages. `output_closed()` then
confirms local half-close. `drive_until_shutdown()` also waits for disconnect.
Native shutdown separately confirms peer byte receipt. None proves method completion.
Native QUIC confirms reciprocal receipts with an explicit nonce/count echo on a
control stream or authenticated close reason, using upstream Quiche public APIs.

## Transport transitions and buffers

`rpc::tcp` adapts ordinary TCP connections to the two-party RPC engine.
`rpc::tcp::tls` uses rustls/Tokio for TLS 1.3 over TCP, and `rpc::quic` uses
the quiche engine for standard QUIC v1 with TLS 1.3. Both secure transports use
the `capntproto-rpc/1` ALPN, explicit trust roots, and optional mandatory client
certificate verification. Connection setup completes before a bootstrap is
installed; application-specific authorization uses the verified certificate chain.
Bounded concurrent listeners isolate failed and stalled handshakes. Canceling a
listener drops pending handshakes while the common ServerDriver retains accepted
RPC sessions. Each QUIC session uses one client-initiated bidirectional stream;
write shutdown queues bytes/FIN into Quiche, and dropping an open writer closes
its connection. After shutdown, pending transmission retains a driver until
stream collection or connection close/timeout. These are two-party
transports; Native-specific multiparty routing remains separate. See
[TCP TLS and QUIC](TCP-TLS-and-QUIC.md) for configuration and tests.

`transport::engine::Engine` owns the quiche connection, native RPC stream gate,
shutdown protocol, scheduling and pending unreliable packet. Its synchronous
`step(now)` advances the production protocol without socket IO or executor waits.
The async adapter performs packet delivery, application reads/writes, pacing and
half-close, then reports completions. Mobility decisions also receive explicit
time. Upstream Quiche recovery, idle/path timers and packet pacing use the system
clock. Runtime scheduling, migration and shutdown deadlines use Tokio time.
In production both advance together; Quiche `SendInfo.at` is converted to a Tokio
deadline. Tests that require QUIC recovery must let real time advance.

In unit tests, a private `DatagramSocket` variant supplies in-memory UDP queues,
readiness, errors and closure to the same async driver. A local executor polls
drivers only when woken; packet ticks advance real recovery time and align the
paused Tokio clock for pacing. Scripted larger advances test application deadlines.
Generated packet scripts check delivered bytes and receipts across processes.
Crypto entropy and internal `select!` order remain uncontrolled; event logs are
diagnostic rather than claims of identical executions. Active dedicated paths,
migration candidates and shared-listener owners use this same datagram boundary.
Each listener retains one receive loop and shares its raw socket among routed
sessions and auxiliary mapping probes. Simulated send readiness wakes all waiting
senders; canceled sends cannot emit packets. Candidate sends poll
once: backpressure drops the probe for quiche's normal loss recovery, preserving
established traffic and migration deadline/cancellation checks. Only a shared
listener has a session-wide shutdown watcher; replacing a dedicated socket
must not terminate the new path. TLC traces exercise real encrypted validation,
corruption, retirement and late replies under both Native patterns. Shared-listener
traces run real reservation authentication, CID demultiplexing and concurrent
session drivers through drain, failure and close. The private `DatagramIo` trait
also lends raw UDP or simulated IO to one-shot STUN discovery without transferring
socket ownership. Its send future stays inside the deadline/receive selection,
so backpressure cannot hide the overall timeout or a response to an earlier
probe. Mapping traces compose real listener packets with mapping observers and
recipient-scoped discovery publications. Route arbitration, capability handoff,
provisioning dials and authorized NAT rendezvous still need simulated coverage.

Send and receive stream enums own independent reusable buffers. Partial progress
retains the remaining bytes, mutable buffer access is denied while bytes are
pending, and EOF cannot close the consumer before pending delivery completes.
Application reads and quiche receives write directly into these buffers. A
graceful shutdown suppresses ordinary RPC FIN in favor of the receipt protocol.

An acknowledged local close enters `Flushing`, retaining the receipt and its
control until the adapter observes packet-output exhaustion. A packet-burst
yield after sending CONNECTION_CLOSE must not reinterpret quiche's draining
state as an unacknowledged stop. IO errors and owner cancellation still use the
driver's existing failure path.

`DatagramSender::try_send_owned` transfers a `Vec` into the bounded queue and
returns the same allocation through `DatagramSendError::into_parts` on rejection.
Borrowed admission reserves capacity before copying. Neither operation proves
delivery. Unix RPC reuses its serialization buffer between completed messages,
retaining at most 64 KiB of scratch capacity after a write; larger allocations
are released. Descriptor attachment and partial-write offsets retain their
existing message ownership boundary.

Bulk receiving, completion, failure and cancellation carry their applicable
storage/error in `ReceivePhase`. Only completion owns published bytes. Cancellation
releases staging and retains any prior error; it cannot undo completion.

## Core reader boundary

`capnp::message::CheckedStructReader` owns a reader in a private `Rc` allocation
and caches validated root metadata without a self-reference. It encapsulates
the unsafe arena lifetime contract. Views borrow the live arena;
consuming it returns the original reader and its consumed traversal budget.
Field API owners supply capability authority only to lifetime-bound root copies.
Changing providers, authority, or budget rules belongs at this core boundary.

## Build boundaries

| Feature/package | Responsibility |
|---|---|
| base `capntproto` | RPC adapters, authority and semantic helpers |
| `tls` | Certificate configuration, TLS 1.3 over TCP, optional mandatory client certificates |
| `quic` | Standard QUIC v1 RPC streams with quiche; enables `native` and `tls` |
| `native` | Native/quiche transport, listener, arbitration, multiparty routing, provisioning, discovery, NAT rendezvous and path control |
| `services` | Bulk, realtime and schema exchange |
| `storage` | mmap store, typed ORM and persistence |
| `native` + `services` | Capability-authorized realtime datagrams |
| `native` + `storage` | Application object introduction/handoff |
| `services` + `storage` | Durable bulk/ORM publication |
| `capntproto-test-support` | Generated fixture schemas and trace-file contract; dev dependency only |

The default enables `native`, `services`, `storage`, and `tls`. Native directory
routes select TCP/mTLS; `rpc::tcp::connect` selects standard plaintext TCP. The `quic`
feature explicitly enables conventional two-party QUIC. Native still builds
the quiche driver for explicitly selected native QUIC routes.
Plain TCP works without default features. TLS and QUIC also run their secure RPC
tests independently with `--no-default-features --features tls` or `quic`.
Feature-specific library builds use
`cargo check --no-default-features --features FEATURE --lib`. Fixture modules are
not re-exported from the production package; tests import `capntproto_test_support`.
The production build script compiles only schemas used by enabled features.

## Verification workflow

`cargo nextest run --locked --workspace` runs the native acceptance tests.
`cargo test --locked --workspace --doc` runs doctests.
See [Testing](Testing.md)
for targeted checks and external tool requirements.

`test-support/verification/models.json` declares bounded model configurations,
named negative controls, corpus digests, scope and limits. Rust test support runs
TLC, canonicalizes state/transition graphs, checks the graph-bound regression
corpus and supplies it directly to the Rust replay tests. Model checks cache
only matching source/configuration/tool inputs and validated result-log bytes;
Rust replay tests always execute. Trace-only tests are no longer ignored.

Use [Kani](Formal-Verification.md) for local arithmetic, ownership and transition
properties in the portable engine. Keep distributed scheduling, message ordering
and fair-progress properties in TLC. New distributed scalar models can use
`verification::exploration` to obtain fresh reachable
edge-prefix traces directly from TLC. Answer setup and route recovery use this
path, with native Rust replay, named mutation controls and temporal checks.
Discovery/renewal and reader failover, managed advertisements, rendezvous/mapping refresh, path commitment and active pipeline fences also use fresh
scalar graphs; [Discovery and Mobility](Discovery-and-Mobility.md) identifies which checks
replay policies and which drive real UDP/Native/RPC.
The [local transport scheduler](Transport-Scheduling.md) similarly replays credit,
policy-change, packet-burst and closure guards; live tests check paced datagrams
alongside RPC, path migration and shutdown.

`RpcRouteLifecycle` checks lifecycle/output/receipt ordering and immutable terminal
causes. Route and shutdown models exercise complementary component boundaries;
UDP tests cover their integration. This is bounded evidence, not whole-runtime
refinement. `test-support/verification/models.json` and the individual feature
guides record each check's scope and limits. Release decisions use
`RELEASE_ACCEPTANCE.md`, not checkbox or state totals.

The distribution boundary is a coordinated source bundle with explicit core,
RPC, generator and futures paths. `cargo nextest run --test release isolated_release_qualification -- --ignored --exact` reconstructs and tests the
bundle with fresh outputs. `storage/io.rs` owns private write/sync seams; fault
injection exists only in test builds. The production Store validates independent
record framing before tail repair and stabilizes file and pathname on recovery.

## Planned portability boundaries

Status: migration in progress, designed from `d2b163dbf50c377dedf8c38519fa08374eb65451`.
The `capntproto-rpc-engine`, with no runtime dependencies, now owns streaming
credit, adaptive windows, question Return/Finish lifecycles, peer-chosen
answer/import storage and local question/export/embargo allocation used by the
hosted runtime. An isolated Cortex-M
fixture links those transitions, bounded peer/local tables, caller-indexed reading,
resumable stream framing, generated borrowed field views, fixed-buffer serialization
and validated outgoing capability publication without a global allocator.
The hosted RPC crate now reexports the portable `capntproto-rpc-wire` schemas.
Complete capability dispatch and the rest of RPC still require the hosted
implementation; this is not yet a complete
allocation-free RPC engine.
The target is allocation-free serialization, fixed-buffer construction and a
`no_std`, no-`alloc` capability RPC engine. Applications supply storage, IO,
polling, time and transport services. The current hosted defaults must use these
same interfaces and the same engine. This supersedes the earlier proposal to
keep RPC heap-backed and merely evaluate `no_std` support. An embedded QUIC
implementation and actor hosting are not part of this migration.

The distinction matters: `no_std` removes the standard library requirement;
it does not prohibit an allocator. A fixed-buffer API must also avoid hidden
heap fallback and define what happens when its storage is exhausted. See the
[Rust Embedded Book](https://docs.rust-embedded.org/book/intro/no-std.html).

### Support contract

| Component | Intended minimum environment | Contract |
|---|---|---|
| Wire decoding, pointer traversal and validation | `core` only | Borrow supplied bytes; retain traversal, nesting, alignment and overflow checks |
| Generated struct readers | `core` only | Borrow fields and defaults; return structured errors without allocating diagnostics |
| Fixed-buffer builders | `core` only | Borrow exclusive, checked storage; report capacity exhaustion through `Result` |
| Growing builders and owned messages | `core` + `alloc` | Own dynamic storage; require no OS services |
| Capability RPC engine and generated bounded RPC APIs | `core` only | Caller-supplied resources, IO, polling, time and transport; explicit capacity errors |
| Owned clients, async convenience APIs and growable RPC storage | `core` + `alloc` where feasible | Adapters over the same engine; no separate protocol implementation |
| TCP, TLS, quiche, native routing and filesystem storage | `std` + `alloc` | Keep platform dependencies outside the portable crates |
| Schema compiler, generator and development tools | Host `std` | Host requirements do not become target requirements |

Ordinary data readers do not require a capability runtime. Decoding an interface
reference does not provide a usable capability without an appropriate capability
table and authority context. Generated bounded clients and dispatch must work
without allocating hooks; current owned clients and boxed server futures become
optional convenience adapters. Reflection and owned field values need their own
feature audit; support for a generated borrowed reader does not imply that all
dynamic reflection APIs are allocation-free.

### One engine, separate storage and runtime adapters

Keep a single wire implementation and a single capability RPC implementation.
Extract the protocol transitions into a small engine crate with no required `std` or
`alloc` feature. Its optional `alloc` reply owner wraps the same transitions. This boundary is warranted because both the embedded and default
hosted runtime must use it. It is an extraction of the current implementation,
not a second embedded RPC implementation.

| Package | Responsibility after migration |
|---|---|
| `capntproto-core` | Portable wire representation, validation, borrowed views, fixed-buffer construction; optional allocating owners and `std` adapters |
| `capntproto-rpc-engine` | Target: allocation-free RPC transitions, capability accounting, pipelining, flow control and lifecycle over caller-owned resources; credit, question lifecycle and peer/local table extraction implemented |
| `capntproto-rpc-wire` | Shared RPC schemas, bounded frame inspection and binding export reservations to immutable outgoing messages; no default target features |
| `capntproto-rpc` | Current public API adapted to the engine; growable storage, owned clients, promises and server-task integration |
| `capntproto-futures` | Async message IO adapters implementing the engine's transport contract, with their IO/allocator requirements explicit |
| `capntproto` | Hosted integration: Tokio, TCP/TLS, upstream quiche, native routing, authority integration, services and storage |
| `capntproto-codegen` / `capntproto-compiler` | Host tools producing code for the selected target profile |

Hosted RPC depends on the engine and wire crates. The wire crate depends on the
core and engine; the engine itself has no runtime dependencies. Embedded
applications use these portable crates with fixed storage and their own driver.
The portable target has no dependency on the RPC convenience crate, futures
channels, Tokio, quiche, storage or the compiler. The wire crate runs the Rust
compiler and generator on the host using the same pinned RPC schemas as before;
neither the C++ executable nor a vendor checkout is needed for that build step.

```text
Embedded application                  Current hosted defaults
fixed storage, manual/embedded poll   growable storage, owned clients, Tokio
custom IO, clock and transport        TCP/TLS, quiche, native transport
                \                     /
                 shared engine interfaces
             capntproto-rpc-wire -> capntproto-core
                      |
             capntproto-rpc-engine (no dependencies)
```

The hosted provider may allocate when growing storage or scheduling application
futures. The fixed provider must not allocate during construction, normal work,
errors, cancellation or teardown. Both providers use identical protocol
transitions. A `no_std` engine is not, by itself, proof that an arbitrary supplied
provider is allocation-free.

The public `capntproto-rpc` crate now has positive `alloc` and `std` features.
With defaults disabled, `capnp_rpc::bounded` exposes the portable wire driver and
`capnp_rpc::engine` exposes its caller-owned resources; only core, engine and
wire remain in the target dependency graph. The optional `alloc` feature adds
shared completion owners and fallible graph/ledger growth without requiring an executor
or operating system.
The default `std` profile retains the existing owned API. Schema generation runs
on the host, and the hosted persistent API is compiled only in that profile.
The isolated ARM fixture exercises streaming through this public crate.
`bounded::owned::connection::FixedStorage` bundles caller-owned ledger, graph
and dispatch arrays with explicit capacities. Binding consumes a single domain
constructor and lends a driver from that storage; separate message buffers,
completion slots and application tasks remain under caller control. Applications
which need unequal table sizes may still supply the individual storage slices.
With `alloc`, a zero-node bundle can use `bind_growing_clients(initial, maximum)`
to keep fixed ledgers and dispatch while its capability graph grows. Both graph
providers use the same ownership and promise transitions. Growth preserves slot
indices and generations, never consumes only half an imported promise, and
respects the explicit maximum. A bundle with both `NODES` and `LEDGERS` zero can
use `bind_growing` with `connection::GrowingStorage` to grow imports, exports,
ordinary questions and answers as well. Each resource has independent initial
and maximum capacities. `Driver::new_growing` attaches optional growing dispatch
and binding storage with per-table budgets. Dispatch reserves room for protocol
progress before admitting application work, including when starting with zero
allocated slots. Loopback receipt storage grows by entry count, independent of
the numeric wire ID. `Invocations::new_growing` supplies growing invocation reply
bindings. Message buffers and send queues also have optional growing providers.
`Domain::bind_growing_adoptions` gives adopted questions a separate sparse entry
budget; numeric wire IDs cannot enlarge it or consume ordinary question slots.
`Session::call_owned` and `streaming_call_owned` allocate reply slots and use the
same tracked completion path as caller-backed calls. The owned endpoints move
into that path without allocation or extra endpoints, retain their original
value lifetimes, and preserve registered wakeups. A canceled receiver leaves
accepted capability/message receipts for the driver to reclaim. Replies remain
scoped to their originating connection, and application-held results still need
explicit release. These conveniences do not yet replace hosted `ClientHook`.
Fixed providers never fall back to allocation.
Preflight can reserve empty capacity, but rejection preserves protocol owners,
reference counts and generations. Maintenance refreshes its scan after growth
without resetting the current cursor, so new cleanup slots cannot be missed.
The hosted default local-ID tables now use the same `no_std` + `alloc`
`GrowingSlots` provider as the portable question ledger. Iteration visits only
issued slots rather than unused reserved capacity.
Native and 32-bit Miri tests cover live calls, cancellation, batch rollback,
retained pipelines and cleanup beyond initial capacities. A fault control that
disables maintenance refresh must fail the cleanup regression. Kani checks
full-width growth arithmetic, not the allocator or the full driver.
The portable fixture's `growing` feature replays owning-driver conversations
from zero initial graph, ledger, dispatch and binding capacity using the same
limits as the fixed fixture. It covers third-party publication, rendezvous,
pipelines, control reserves, completion bindings and shutdown. Tracking a new
third-party answer reserves its exchange slot before taking any incoming rights.
Maintenance refreshes pipeline, outgoing, exported-promise, tail and invocation
scan limits after growth. A separate regression detects missed pipeline cleanup.
The allocation-counting assertion remains enabled in the core-only and `alloc`
fixed profiles; the separate growing replay intentionally allocates.
`messages::GrowingBuffers` owns stable, segmented buffer records outside the
connection scope. `with_growing_messages` and its async counterpart lend them to
the same message transitions with fallible send-slot growth. Suspended readers
and writers retain their direct buffer views while other records are added.
Reservations enforce independent limits on buffer count, usable bytes per buffer
and aggregate retained allocation, including alignment padding. Free buffers can
grow after a fallible replacement allocation; existing readers, writers and sends
prevent reuse. Validating a replacement temporarily holds both allocations, so
the retained-byte budget is not a bound on transient peak allocation. Metadata is
bounded separately by buffer count. No unsafe code, extended lifetime or leaked
allocation is used. Scope teardown recovers reservations, and a subsequent
exclusive scope can reset deliberately forgotten borrow flags.
`Queue::new_growing` shares the fixed FIFO transitions. Growth rotates a wrapped
queue before changing its capacity, preserving the oldest partial frame. Fixed
queues never allocate. The growing replay now uses these message and queue
providers with the same per-resource limits as the fixed replay. Checks cover
borrowed views across segment growth, cancellation, failed reservations and
zero-allocation warm reuse. Kani proves full-width segment indexing; 32-bit Miri
checks actual buffer loans, and an injected missing queue rotation must fail the
FIFO regression. Growing adopted-question and shared reply ownership replay
covers third-party authorization, late returns, typed calls, streaming and
capability pipelines. Application execution storage and the default hosted
client-hook bridge remain integration work.

Use the portable profiles rather than claim that
`capntproto --no-default-features` is portable: today the root still requires
Tokio, sockets, `std` futures and other hosted dependencies. Keeping it as the
hosted integration package avoids a large file move and a misleading new feature
contract at the beginning of this work. A separate transport package can follow
when a consumer needs it independently of the root; it is not a prerequisite for
the engine extraction. Moving the defaults onto the engine interfaces is a
requirement of this migration, regardless of where their adapter files live.
Any future actor runtime belongs in a separate crate above these layers.

Use positive, additive `alloc` and `std` features. In the existing core they are
independent; preserve that unless a specific API requires both. Do not introduce
an exclusive `no-alloc` switch. Features from different dependents are unified,
so dependency edges into the portable core must use `default-features = false`
and enable only what they need. Host tools can enable their own features. See
[Cargo feature unification](https://doc.rust-lang.org/cargo/reference/features.html#feature-unification).
Fixed-buffer types must retain their no-allocation contract even when another
dependent enables `alloc` in the same build. The same applies to the fixed RPC
resource provider and bounded generated APIs. Dependency feature checks alone
cannot prove this; exercise these paths with allocation counting as well.

### Verified foundation and remaining gaps

The upstream comparison was made against
[`capnproto-rust` revision `81bc1b8`](https://github.com/capnproto/capnproto-rust/tree/81bc1b815d0f450c9114f9cc2e2274182d210df2).
Its serialization runtime separates positive `std` and `alloc` features, uses
borrowed readers and custom IO traits, and retains structured error kinds
without owned diagnostic strings. Those are already foundations of our imported
core and remain the model for this migration. Its `capnp-rpc` still uses `std`
futures and owned capability hooks; it is not an allocation-free RPC engine to
adopt. Upstream `SingleSegmentAllocator` is also infallible and panics when
exhausted. A scratch-space allocator with heap fallback cannot meet the bounded
contract. See the upstream
[feature manifest](https://github.com/capnproto/capnproto-rust/blob/81bc1b815d0f450c9114f9cc2e2274182d210df2/capnp/Cargo.toml),
[fixed allocator](https://github.com/capnproto/capnproto-rust/blob/81bc1b815d0f450c9114f9cc2e2274182d210df2/capnp/src/message.rs#L983)
and [RPC dependencies](https://github.com/capnproto/capnproto-rust/blob/81bc1b815d0f450c9114f9cc2e2274182d210df2/capnp-rpc/Cargo.toml).

The existing core already has `#![no_std]` and `alloc` gates. The following
library checks passed both on the host and with
`--target thumbv7em-none-eabi` using Rust 1.97.0:

```sh
cargo check --locked -p capntproto-core --no-default-features --lib
cargo check --locked -p capntproto-core --no-default-features --features alloc --lib
```

These are library compile checks. They do not establish that a generated client,
the full project, or a final embedded binary works without an allocator.

The important gaps found in the current source are:

- The legacy `message::Allocator::allocate_segment` cannot return an error;
  `SingleSegmentAllocator` can panic and `ScratchSpaceHeapAllocator` can fall back
  to the heap. `message::FixedBuilder` now uses a separate caller-owned arena
  with fallible reservations and restricted editors over the shared wire encoder.
  It supports structs, scalars, text/data, lists, recursive data copies and
  explicit schema upgrades. Capability construction and bounded dispatch use
  explicit capability tables and validated publication transactions.
- `NoAllocBufferSegments` optimizes a single segment, but multi-segment lookup
  repeatedly walks the segment table. `IndexedSliceSegments` now offers O(1)
  lookup using caller-supplied `SegmentOffset` entries. Its constructor validates
  the complete frame before publishing offsets or advancing the input, and
  reports insufficient metadata capacity without a heap fallback.
- `serialize::FrameReceiver` now retains stream framing progress in caller-owned
  storage without IO, an executor or an allocator. The synchronous fixed-buffer
  reader and the async fixed-buffer adapter use this receiver. The owned and
  buffered hosted readers also share its validated first-word parser. Bounded
  RPC dispatch and message ownership now compose these interfaces in rpc-wire;
  replacing the default hosted ownership path remains migration work.
- Borrowed `field_api` data views and in-place editors now compile without
  `alloc`. Owners, capability hooks, native conversions and staged replacement
  remain allocation-gated; existing publication guarantees are unchanged. The
  same generated data schema is tested under core-only, `alloc`, `std`-only and
  unaligned/synchronized-reader profiles and linked without an allocator on ARM.
  The coverage baseline is repartitioned between `field_api.rs`, `staged.rs`
  and `calls.rs`: their combined covered/executable counters exactly retain the
  original 989/1179 lines, 1422/1770 regions, 191/232 functions and 75/101 branches.
  No aggregate coverage totals or regression thresholds change in this split.
- Without `alloc`, field diagnostics preserve structured error kinds and expose
  `Location` formatting into caller-supplied storage. With `alloc`, field errors
  still format owned strings; RPC exception metadata also uses owned strings,
  maps and byte vectors. Bounded diagnostics and exception views need a contract
  that remains allocation-free even when another consumer enables `alloc`.
- Generated interface dispatch uses `Rc`, owned parameters and allocating hooks.
  Add bounded generated clients/dispatch over engine handles and message leases,
  then make the current owned API an adapter over the same operations.
- `Connection`, `OutgoingMessage` and `VatNetwork` currently return boxed
  messages, heap builders, allocating promises and `Rc` lifetime guards. These
  are hosted adapter APIs, not suitable engine interfaces as they stand.
- The shared engine now implements typed answer/import storage. Its fixed
  provider borrows caller slots, returns rejected/replaced owners, and reclaims
  remaining owners on teardown. The hosted provider uses the same table with a
  sparse `HashMap`; its first 16 wire IDs retain the direct-indexed fast path.
  Local question/export/embargo tables also use shared bounded/growing providers.
  Capability owners, task
  sets, sender queues, pipeline hooks and third-party exchanges still retain
  `Vec`, maps, `Rc`, boxed futures and oneshot channels. Extract their
  protocol state into caller-supplied slots and queues; leave heap-backed
  convenience ownership and application task scheduling outside the engine.
  Hosted sender queues now retain monotonically unique identities across drains;
  canceling a drained promise cannot remove later work. Identity exhaustion
  reports overload, and input destruction and resolution wakeups occur after
  releasing mutable queue/client borrows.
- The current synchronous QUIC `transport::engine` still owns a quiche connection
  and uses hosted types. Its separation from socket IO is useful for testing, but
  does not make it a portable RPC engine.

### Fixed-buffer construction must be fallible end to end

The first fallible construction path is `message::FixedBuilder`. It borrows one
aligned word buffer (or validates a byte slice) and never calls the legacy
allocator. Shared layout helpers reserve new storage before clearing an existing
object or publishing its pointer. Heap-backed APIs use these same helpers,
preserving their infallible convenience signatures. Neither `catch_unwind` nor
a guessed size hint is used for capacity management.

`try_init_root::<Schema>()` returns a `field_api::FixedStruct`, which cannot expose
the legacy growing editor. Generated scalar descriptors drive `scalar(Schema::ID)`;
existing pointer descriptors drive `field(Schema::NAME)`. Nested struct, list, text and
data operations return `Result`, including compact capacity/length errors even
with `alloc` enabled. Union tags change only after the replacement succeeds.
`try_get_root` and nested `edit` reject missing or undersized layouts instead of
silently copying defaults or upgrading them. This profile is opt-in; it does not
change the legacy `SingleSegmentAllocator` exhaustion behavior.

Reservations are monotonic: successful replacement uses new words, and clearing
an object does not reclaim them. Rejection consumes no words and preserves the
published message. `reset()` reclaims all reservations except the root slot;
Rust borrowing prevents reset while any reader/editor remains live. Reserved
words are zeroed when created, reset or dropped; unused caller capacity is not
modified. Arbitrary application implementations of schema/scalar traits remain
responsible for their own behavior.

`FixedList` covers primitives, bits, open enums, inline structs, text/data lists
and nested lists. Its element editors stay bounded; `get_mut` checks indices and
returns `None` out of range. Generated group and Void descriptors provide explicit
selection/reset without exposing a legacy growing editor. Group reset preserves
schema-unrelated sibling data and pointers. Child edits after group selection
commit independently, rather than forming a transaction across the whole group.

`try_copy_root` and bounded struct/list field `copy_from` stage recursive copies
before publication. They follow far pointers using the source reader's traversal
and nesting limits, preserve unknown fields and roll back unpublished reservations
on failure or unwind. The fixed arena rejects capability copying before extracting
a client hook, even when `alloc` is enabled. An explicit root/field struct or list
`upgrade` reserves larger inline storage and moves existing child pointers without copying
their payloads. Lists can promote compatible primitive/pointer encodings; bit-list
promotion is rejected. Failed upgrades retain the old object; successful upgrades preserve
unknown fields. Copy/upgrade success consumes a staging word until reset/drop.

Generated generic fields use `typed()` to map an owned type marker to bounded
operations, including generated structs, enums and nested lists. AnyPointer uses
an explicit `cast::<Kind>()` followed by checked operations or staged `copy_from`.
Neither path exposes a legacy growing editor. Arbitrary casts are restricted to
AnyPointer; a schema-known generic Text parameter cannot be cast to another kind.

The remaining construction work includes bounded capability slots and atomic
multi-field group updates. Multiple
supplied segments can be added after the single-segment failure contract is
qualified across those operations.

Operations which may grow storage, including getters that upgrade an older
struct, must expose failure. Use explicit fallible operations in generated APIs
and keep infallible scalar access only where no growth is possible. Preserve the
current convenience API for heap-backed callers while introducing this path;
do not silently change existing setters to discard errors.

Capacity failure must leave the previously published field/root valid and must
not change a union discriminant before its payload is ready. Stage fallible work
before publication. Make reservation rollback reclaim unused space where its
ownership permits it; if rollback is not supported for an operation, specify and
test the consumed-space behavior rather than promise full transaction rollback.
Recursive copies, list upgrades and far pointers need explicit coverage because
they can require more than one internal reservation.

Use compact error variants with numeric context; format them through `core::fmt`
into a caller-supplied sink. Distinguish insufficient buffer/segment capacity
from malformed data and traversal-limit errors. Resource exhaustion is an
ordinary error, including when it occurs while constructing an error reply.
This work does not promise recoverable global allocator failure for all existing
heap-backed APIs or freedom from every programming-error panic.

The engine carries structured error codes and bounded context. Remote exception
text, traces and detail values remain borrowed under a message lease or are
copied into explicitly supplied storage. Hosted adapters can turn these views
into today's owned errors. Avoid allocating an existing rich error and then
converting it to a compact one; the allocating operation must be outside the
bounded path, including when the core's `alloc` feature is enabled.

Zeroing and reuse remain part of the builder's safety contract. Expose only the
used message range, clear reused pointer storage correctly, and retain exclusive
access until all derived views are gone. Avoid large hidden stack arrays; the
caller decides whether buffers live on the stack, in static storage or in an
application-owned arena. No global allocator or mandatory atomic operations are
needed by this profile.

### Reader and generator boundaries

Retain the zero-copy single-segment reader. Offer a second constructor accepting
caller-owned segment metadata: validate the frame and fill checked offsets once,
then perform constant-time segment lookup. Report an explicit capacity error
when the metadata table is too small. Keep the existing low-memory reader for
callers that prefer less scratch storage over faster repeated lookups.

Untrusted segment-table metadata must pass through a validating constructor;
publicly supplied counts and lengths are not proof that a backing buffer is
valid. Reader lifetimes must keep both the immutable message and its index alive.
Caching must not bypass pointer bounds, traversal budgets or nesting limits.
Reuse `capntproto-core::io` and its optional `embedded-io` adapter instead of
inventing a second serialization IO trait family.

Test generated data-only and bounded RPC code in isolated target crates. Keep
wire/schema IDs and layouts identical across profiles. A generator option should
select the API surface (borrowed data, bounded RPC or owned convenience APIs),
not depend on an application's accidentally matching Cargo feature names.
Checked-in schema fixtures can exercise compilation without requiring a target
compiler to run the host generator. Separately verify fresh generation using the
host compiler, including schemas that contain interfaces.

### Caller-supplied engine interfaces

Use a single-owner, explicitly advanced state machine. The engine consumes
typed inputs and exposes effects; adapters perform IO and application work and
report completion. It must not read sockets, sample a platform clock, spawn
tasks, allocate a future, or invoke arbitrary application callbacks while its
tables are mutably borrowed. `core::future::Future` and `core::task` are available
without `std`, but an executor and a future per call are not requirements of the
engine.

These are interface responsibilities, not final Rust trait signatures. Prove
them with a generated call/return implementation before freezing the public API.

| Boundary | Caller supplies | Engine guarantees |
|---|---|---|
| Resources | Typed slot storage, message buffers, segment metadata, bounded queues and scratch space | Fallible reservations; no hidden heap fallback; stable handles instead of pointers into growable storage |
| Incoming IO | Complete framed message leases, EOF and transport failures | Validation and ordering; retained bytes stay leased until no protocol/user reference needs them |
| Outgoing IO | Send acceptance and completion through message leases and send tokens | Backpressure; explicit ownership until completion/failure; no silent message loss |
| Polling | Calls to advance with a work budget, and delivery of readiness/completion inputs | Bounded work per turn, pending effects and a distinction between more runnable work and waiting for an external event |
| Time | A monotonic timestamp for advancement | Next required deadline; checked deadline arithmetic; no internal OS clock reads |
| Dispatch | Generated method handlers; caller-owned storage for any pending application tasks | Typed call/reply handles, parameter/result leases, explicit completion and cancellation |
| Network services | Connect/accept, authenticated session identity, introductions, rendezvous and any cryptographic token generation | Capability/protocol state transitions bound to the correct connection and operation generation |

Stream framing belongs in a reusable allocation-free adapter over the existing
core IO primitives. TCP and QUIC supply bytes through this adapter; a custom
message transport can provide complete frames directly. The RPC engine must not
assume Tokio IO traits, a QUIC stream implementation or a particular executor.
Native split-plane adapters retain their existing ordering and receipt contracts
when mapping streams into engine inputs.

Implemented receive boundary: `capnp::serialize::FrameReceiver` borrows an aligned
byte buffer, including its segment table. `input()` lends one bounded `ReadWindow`;
the caller writes bytes and consumes the window with `commit(count)`. Dropping an
uncommitted window preserves progress. Windows never extend into the next frame.
The receiver checks segment counts, size arithmetic, traversal limits and buffer
capacity before exposing a complete message. Truncation, invalid completion counts
and explicit transport failures are terminal; the first error kind is retained.
After reading a message, `next_message()` reuses storage only once all borrowed
views have been released. A compile-fail example enforces that lifetime boundary.

`capnp_futures::serialize::try_read_message_into()` drives these same windows using
the caller's `AsyncRead`. Canceling its future while pending preserves committed
progress in the external receiver; resumption requires the same stream. Its error
conversion keeps compact kinds without formatting heap strings. The convenience
`try_read_message_no_alloc()` owns progress inside its future, so cancellation
cannot be resumed with a fresh receiver on that partially consumed stream.
These adapters have no heap fallback, including when `alloc` is enabled elsewhere;
the IO provider remains responsible for its own allocation behavior. The existing
hosted owning APIs retain their dynamic buffers and scheduling requirements.

Effects live in reserved caller-supplied storage or are exposed one at a time;
returning a fresh `Vec<Effect>` would violate the boundary. An effect remains
pending until its transfer is acknowledged. If the adapter cannot accept a send,
the engine retains it and does not report it as transmitted. Budget exhaustion
with more runnable work causes the driver to arrange another poll; waiting for
IO or time must not busy-spin. A hosted `Future` wrapper translates this into
waker registration and timers, including readiness rechecks to prevent lost
wakeups. A manual loop uses the same progress/deadline information.

Represent time as an explicit monotonic value based on `core::time::Duration`.
Specify equal-timestamp handling, reject backwards time without corrupting
state, and report deadline overflow instead of wrapping. Hosted adapters map
their clock to this value. Virtual-clock tests can exercise the same deadlines;
quiche's own internal transport clock remains outside this RPC engine contract.

### Resource ownership and exhaustion

IO, polling and time injection alone are insufficient: an allocation-free engine
also needs explicit memory accounting. The fixed provider borrows caller-owned
tables and arenas; the default provider owns growable equivalents. Use a small
set of typed storage operations rather than an abstract replacement for the
global allocator. Dynamic growth must not invalidate outstanding handles.

Define limits for connections, questions/answers, imports/exports, pipeline
operations, retained messages/bytes, queued sends, timers, rendezvous entries,
pending dispatches and local capability references. Caller buffers can live in
static memory, on the stack or in a supplied arena. Const-generic helpers may be
offered, but must not force large arrays or application futures onto the stack.
Each provider reports its actual limits; no fixed compile-time global limits are
hidden inside the protocol.

Separate typed wire IDs from internal slot handles. Internal handles include a
generation and connection epoch so stale replies, sends and timer completions
cannot act on a reused slot. Generation exhaustion must retire a slot or return
an error; do not wrap to a previously valid generation. Peer IDs remain sparse:
even a peer ID of `u32::MAX` occupies one bounded entry rather than determining
an array size. Preserve the current low-ID fast path where useful, and benchmark
fixed sparse tables and growable providers independently. Any hashing needs a
documented strategy for attacker-controlled keys and supplied entropy.

Implemented peer-table boundary: `peer_table::PeerTable` has a caller-selected
inline low-ID range and a `SparseStorage` provider. `BorrowedSparse` borrows an
exclusive slice of `Slot` entries; no allocation or implicit heap fallback is
possible. Low-ID lookup is O(1); bounded sparse lookup is O(capacity). The hosted
adapter retains Rust's randomized `HashMap`. Inline and sparse capacities are
separate; select `LOW = 0` to use one sparse budget for the full wire-ID range.
`try_insert_unique` distinguishes duplicate IDs from full storage and returns
the rejected owner unchanged. `try_insert` explicitly permits replacement and
returns the old owner. Removal returns ownership as well, so runtime callbacks
and destructors can run after releasing table borrows. Nonempty backing is
rejected at construction and returned intact. Dropping the borrowed provider
releases retained entries and empties its slots for reuse; user value destructors
have their own allocation and reentry requirements. This is storage extraction,
not yet bounded answer dispatch or end-to-end admission/error-reply handling.
Storage diagnostics format only occupancy/capacity into the caller's formatter;
they neither require `Debug` on capability owners nor expose owner contents.

Implemented local-table boundary: `local_table::LocalTable` uses a contiguous
`Storage` provider plus independent sparse providers for pipeline-only and
adopted questions. `BorrowedSlots` borrows the application's fixed buffer; the
hosted provider grows a `Vec`. Every dense slot also carries one free-heap cell,
independent of its value. The engine maintains a min-heap in those cells:
ordinary IDs reuse the lowest vacancy, lookup is O(1), and removal/reuse is
O(log capacity) without allocation or a second free-list buffer. Dense slot
metadata costs one `u32` plus alignment padding per slot. Sparse providers retain
the peer table's bounded linear lookup or hosted randomized `HashMap` behavior.

Ordinary IDs stay below `2^30`; only question tables expose pipeline-only
allocation (`2^31..=u32::MAX`) and adopted insertion (`2^30..2^31`). Capacities
are independent. Exhaustion, invalid adopted ranges and duplicate adopted IDs
return the caller's owner with existing entries and allocation cursors intact.
High-ID wraparound skips live entries, checking at most occupancy plus one
candidates. That scan can still take quadratic time with linear sparse storage;
applications needing large bounded tables can provide a different map.
Empty backing is checked at construction, including storage recovered after a
forgotten table. Retained owners are cleared when borrowed providers are dropped.
As with peer tables, detach values before running reentrant destructors. The
hosted convenience API retains its existing panic policy at wire-ID exhaustion;
bounded callers receive structured errors. Full connection admission and cleanup
reservation are separate work, and wire IDs themselves do not prevent stale reuse.

The coverage floor review transfers the unchanged identifier counters to the
engine. Moving `WireId` across a
crate boundary also removes LLVM
call-site regions from Join, multiparty Join, RPC dispatch and table-replay
code. Those five region floors are normalized to the new mapping; none gains
an unhit region. Other existing file/metric floors remain in place. Measured
portable-table and expanded compiler-contract coverage establish new floors.
The local-table extraction transfers branch coverage to the portable allocator;
the hosted adapter now contains no branches. Its existing line, region and
function floors stay in place, and the allocator's new floor includes its unhit
guards. Every aggregate group/metric floor is retained or increased. This accounts for
the extraction rather than excluding the affected source from coverage.

Admission reserves all resources needed to represent the operation and its
immediate effects before publishing it. Reservation failure leaves the existing
protocol state intact and returns the unsent caller-owned input. Reservation
rollback must be testable; it cannot leak slots on repeated unsuccessful calls.
Wire responses use existing Cap'n Proto error behavior, not a new capacity
negotiation extension. Receiving an already admitted peer message which cannot
be represented must produce a bounded rejection where supported, or terminate
the connection explicitly. Reliable protocol traffic cannot simply be dropped.

Reserve cleanup capacity separately from normal calls. Finish, Release,
cancellation, error reporting and shutdown must remain representable when normal
queues are full. Use pending flags on existing records and bounded draining
where that avoids an additional queue allocation. If a peer exceeds even the
reserved control capacity, the engine must be able to enter a terminal state
without allocating or requiring another message buffer. Do not promise delivery
of an error over a broken or permanently blocked transport. Drain/reclaim records
incrementally within the work budget, preserving the first terminal cause.

The engine owns protocol reference counts and pending-call state. Bounded public
handles use explicit retain/release/cancel operations through a context, or
scoped ownership where possible; do not smuggle `Rc` into this interface.
Hosted `Clone`, `Drop` and future cancellation translate into these operations.
Any deferred release used by a hosted wrapper needs capacity reserved with the
reference it represents, so dropping a handle cannot lose cleanup work when a
queue is full. Refcount and byte-accounting overflow are checked errors.

Implemented wire-reference boundary: `capntproto_rpc_engine::references` owns
`ExportRefs` and `ImportRefs`, now used by the hosted RPC runtime. They count
capability descriptors, separately from application handle clones. Export grants
and peer releases check overflow/underflow before changing state. A fully released
export cannot be revived; the caller detaches its owner and retires the slot.
Import admission also checks the full `u32` range before accepting another grant.

An import's `pending_release()` lends an exclusive, single-use `ReleaseTicket`.
Dropping it without `commit()` retains the pending count, allowing a bounded
output adapter to reject cleanup temporarily. Commit means the adapter accepted
ownership of the Release message, not that bytes were flushed or acknowledged.
The caller must retain this state and reserve cleanup capacity or terminate the
connection. The ticket does not allocate, run callbacks or provide a queue.

Hosted descriptor serialization reserves export references before publication.
A returned error rolls back every grant already reserved for that unsent batch,
including new exports and repeated IDs. Call admission then returns an error
without publishing a question; reply serialization sends an exception. Imports
reject count overflow without replacing existing aliases. The hosted adapter
still uses `Rc`, growable tables and owned messages. Wire-reference counters alone
do not establish complete capability ownership.

Implemented bounded import ownership: `imports::with_imports()` borrows a caller
slice of `imports::Slot<T>` for one connection scope. `Local` handles are neither
copyable nor constructible by callers; `retain()` checks the local count and
`release()` consumes a handle. Local cloning does not add a wire reference.
`receive()` admits an additional descriptor for an existing wire ID, checking
both counts before mutation; `admit()` rejects duplicate IDs and returns the
new owner unchanged. These operations reuse the hosted runtime's `ImportRefs`.
Wire-ID lookup/admission are O(capacity); local handle/key lookup is O(1).

An invariant lifetime separates each connection scope without global counters,
atomics or a caller-chosen epoch. Handles and copyable delayed-work `Key`s cannot
escape that scope or enter another table. `resolve(key)` rejects the key after
the final local release. Receiving a new grant while cleanup is pending advances
the slot generation before reviving it; committed cleanup followed by slot reuse
also advances it. Generations never wrap. Exhausted vacant slots are retired, and
an exhausted pending slot rejects revival even if another slot is available.
Keys are local work tokens, never serialized or substituted for wire IDs.

Cleanup needs no additional queue: a slot with no local owners retains its wire
reference count and owner. `pending_release(index)` examines exactly one slot,
allowing the driver to budget scanning across polls. Its exclusive ticket carries
the ID/count; rejecting output leaves the slot occupied. `commit()` detaches and
returns the owner after the adapter accepts that exact Release message. Drop
the returned owner outside other runtime borrows. New grants cannot race a live
ticket in safe Rust, and no application callback runs during these transitions.

This scoped interface supports a caller-driven loop, including polling futures
inside the scope. Handles cannot be put into a future returned from the scope.
Dropping or forgetting a `Local` intentionally retains its count; release it
explicitly, or terminate the connection and end the scope. Scope teardown clears
remaining entries and runs owner destructors, including on driver unwind. It
does not send wire cleanup or terminate IO itself. Owners/destructors and IO
providers retain their own allocation requirements. Generated bounded dispatch,
integration with message leases and migration of hosted `Rc` handles remain
open; this import table is not a complete allocation-free RPC connection.

Implemented bounded export ownership: `exports::with_exports()` borrows caller
slots for a separate invariant connection domain. New exports reserve the lowest
vacant ID below `2^30`, advancing a checked generation; exhausted slots are never
reused. `lookup(wire_id)` authorizes only published exports and returns a local
`Key`. `lease(wire_id)` additionally reserves ownership for an active call.
`retain()` checks local lease overflow, and `release_local()` consumes a lease.
Final peer `release()` immediately invalidates wire lookup and non-owning keys,
but leases keep the owner alive. The last release returns that owner to the caller
for disposal outside runtime borrows. Local leases never manufacture wire grants.
Wire IDs still require protocol ordering; generation checks protect local work
tokens, not delayed or duplicated raw wire messages after ID reuse.

An exclusive `batch()` reserves grants for one outgoing message. `export(owner)`
takes a new owner, while `grant(key)` reserves another descriptor of a live or
newly staged export, checking published plus pending counts. Repeated IDs share
per-slot pending counters, so no additional journal is allocated. Commit changes
counts only after the adapter accepts the message containing every reservation.
An individual reservation failure changes no counts; the caller can drop the
batch to abort the whole message. Dropping or unwinding the batch restores live
counts without running application destructors. Newly staged owners become
retired and keep their slots until `take_retired(index)` returns them. Their
generations remain consumed, so provisional keys cannot revive on a retry.

Forgetting a batch leaves its grants unpublished. The next mutating operation
recovers pending state; immutable lookup never exposes a staged export. New owner
reservation failures return the owner unchanged. Wire/local lookup and individual
release/draining are O(1), except recovery of an abandoned batch; allocation,
batch commit/abort and recovery scan O(capacity). The caller chooses that bound
and scans retired owners incrementally. Capability identity deduplication remains
the caller's responsibility; an explicit `export()` creates a fresh export.

This table uses the same `ExportRefs` as hosted RPC, but the hosted `Rc` ownership
and growable descriptor tables have not migrated to it. The adapter must serialize
each grant exactly once and bind commit to acceptance of that complete message;
the table cannot infer those facts from IO. Complete acceptance and commit in one
driver step, or terminate the connection if the accepted message cannot be
accounted for. The validated publication API below now binds hosted/promise
reservations to the actual sealed frame. The low-level batch API remains
available for adapters and by itself does not enforce that binding.

Implemented outgoing publication: `capntproto_rpc_wire::publication::prepare()`
consumes one export batch and one message lease. It decodes exactly one framed
`Call.params`, `Return.results` or `Resolve.cap` under caller-supplied reader
limits. `senderHosted`, `senderPromise` and `none` descriptors are admitted;
receiver-hosted/third-party descriptors, attached file descriptors and capability
results marked `noFinishNeeded` are rejected. The bounded publication subset
does not reduce the capabilities of the existing hosted runtime.

`Batch::descriptors()` counts decoded IDs in caller scratch with one `u32` per
export slot. Checks are O(slots + descriptors); no temporary map, heap allocation
or per-message journal is needed. Every reserved grant must appear exactly once,
including repeated IDs. Missing, unreserved and excess descriptors fail. An
invalid descriptor poisons the check even if a low-level caller ignores that
error. Validation changes only scratch, never published or pending counts.

The private `Prepared` fields retain the batch, immutable frame and offered send
under exclusive table/pool borrows. Before `accept()`, callers can inspect bytes
but cannot mutate the frame, commit the batch separately or obtain its send key.
Dropping, rejecting or unwinding aborts the batch and releases its message/send
references; existing exports retain their prior counts and new owners await
`take_retired()`. Other message readers retain their own references. Forgetting
a prepared value cannot publish it, but retains message storage until terminal
pool teardown; normal export recovery aborts its forgotten reservations.

After the driver reserves ordered output capacity, consuming `accept()` commits
the exact batch and exposes that frame's `SendKey` in an infallible step without
application callbacks. Subsequent byte completion or IO failure never undoes a
possibly observed capability grant. If accepted output is lost, terminate the
connection; do not reuse a partially written stream. The driver still owns queue
admission/order and must pair the same connection's table, pool and transport.
Low-level APIs must not independently send a frame whose grants remain pending.
This validator checks descriptor multiplicity and reachable capability-pointer
indices. It does not check application schemas, call targets or the full RPC lifecycle.

Message leases outlive every parameter/result view and every send that refers
to their bytes. An IO write completing, output half-close, peer byte receipt and
application completion remain different events. Preserve those distinctions in
the existing shutdown and native receipt behavior. A receive buffer cannot be
reused merely because dispatch returned if a pipeline or application task still
retains it. Never hold a borrowed input slice across advancement without an
associated lease.

Implemented message storage: `messages::with_messages()` borrows caller-owned
byte buffers and send slots in an invariant pool scope. `reserve()` returns an
exclusive writer after clearing the selected buffer. `seal()` consumes it and
publishes an immutable prefix; invalid lengths return the intact writer. A
noncopy `Lease` retains readable bytes, while `retain()` checks the reader count
before creating another lease. `release()` consumes one lease. Reader borrows
prevent releasing that lease while a view is live, while allowing IO and allocation
of other pool buffers. Per-buffer `core::cell` guards preserve immutability without
unsafe code or an allocator. Forgetting a guard reserves its bytes until terminal
scope recovery; `try_write()` and `seal()` report a forgotten mutable guard.
The caller supplies alignment; the decoder still validates framing and wire data.

`with_messages_async`, `with_imports_async`, `with_exports_async`,
`with_questions_async` and `with_answers_async` keep these invariant domains
across caller-polled suspension. They do not box futures or choose an executor.
The portable fixture holds a generated parameter reader across suspension while
another buffer is sent and recycled for a nested call. Dropping an enclosing
scope future is terminal teardown, so the caller must quiesce its IO first.

`offer()` reserves a send slot and an independent reference to the same bytes.
Rejection releases that reservation; `accept()` returns a checked `SendKey`.
Acceptance does not mean any bytes have been written. An exclusive `SendWindow`
exposes the unsent suffix and consumes one matching write result through
`advance(count)`. Dropping a window preserves progress. Invalid counts and zero
progress leave records unchanged; the driver chooses terminal failure handling.
Final byte completion or `fail_send()` releases only the send reference. The
buffer cannot be reused until every reader and send reference has gone.

Keys identify whole sends, including a generation; late or duplicate completion
of a finished send cannot affect a replacement. Each partial IO result must
still be applied to its matching window. Buffer/send generations never wrap;
exhausted slots are retired. Scoped tokens cannot cross pools or escape their
driver scope. Dropped or forgotten writers, leases and offers retain storage
until explicit cleanup or terminal scope teardown. Teardown clears records and
preserves generations; the caller must stop IO first. It neither cancels network
operations nor securely erases bytes. Reservation scans the buffer slots and
zeroes the selected capacity; offer scans send slots. Normal lookup, progress
and release are O(1), and teardown is linear in supplied slots.

The futures IO layer exposes `poll_queued_message()` and
`write_queued_message()` using this exact send state. A nonzero caller-supplied
budget bounds write attempts per poll, including interruptions; exhausted
budgets self-wake. Successful partial writes commit immediately. Canceling the
future preserves the send and its exact suffix, so it can resume on the same
stream. IO errors, zero writes and over-reported counts release only that send;
a partially written framed stream must then be terminated before another frame
is sent. The adapter does not flush, half-close or report RPC completion. Its
compact errors and future need no allocation, but the supplied IO/waker retain
their own requirements. The convenience future exclusively borrows the pool;
manual polling allows a driver to schedule separate sends across polls.

The portable fixture sends a real generated fixed-buffer message one byte at a
time into `FrameReceiver`, then retains a readable lease after byte completion.
This verifies composition of framing and storage, not generated RPC dispatch.
Another fixture serializes a real `Return` capability table with the shared
generated fixed-buffer API, accepts its bound grants, sends it byte by byte and
admits the decoded descriptors to the bounded import table. Local Release counts
then retire the corresponding exports while an active call lease retains the
owner. Generated dispatch, detaching receive progress into the pool, wider
descriptor kinds and migration of hosted owned messages remain open.

Keep the engine single-owner. Concurrent callers use adapters and queues; the
engine requires neither a mutex nor mandatory atomics. Application dispatch and
destructors that may reenter the runtime execute after engine borrows are
released. This retains the existing reentry safeguards without heap-based
interior ownership in the engine.

### Generated APIs and hosted defaults

Generated bounded clients use typed capability handles and caller-provided
request storage. Generated dispatch borrows parameters and receives a reply
permit that can be consumed once by completion, failure or cancellation.
Calls pending beyond dispatch retain a message lease and a task handle in
caller-supplied storage. They do not require `Box<dyn Future>`, oneshot channels,
`Rc<Server>` or an allocator-backed capability table. Pipeline paths and deferred
calls must also fit the supplied resources and fail admission explicitly.

Allow manually polled handlers as the baseline. An optional executor adapter can
poll statically typed or caller-pinned tasks; heterogeneous task storage needs an
explicit capacity and lifecycle contract. The hosted adapter may box application
futures while using the same dispatch and completion inputs. Generated data and
method IDs, result layouts, cancellation annotations and authority checks must
remain identical across these API surfaces.

Move the current `capntproto-rpc` conveniences onto these boundaries: owned
clients retain engine handles, promises observe completion records, `TaskSet`
drives application/adapter work, and `VatNetwork` implementations become
transport/network service adapters. The default Tokio, TCP/TLS, Unix and quiche
paths must exercise the extracted engine in production and existing tests.
Do not leave an optional embedded implementation beside an unchanged default
protocol engine. A transitional implementation may coexist only during migration;
the old state transitions are removed once the default path is switched and
equivalence checks pass.

The target includes cancellation, pipelining, tail calls, import/export
accounting, embargoes, flow control and third-party handoff within configured
limits. Transport-dependent features still require an appropriate adapter:
Unix file descriptors and authenticated multiparty introductions cannot be
promised by every transport. File descriptors remain opaque adapter resources,
not `std::os::fd` types in engine state. Authentication must come from a trusted
adapter; a decoded identifier is not proof of identity. Revocation and late
rendezvous completion use explicit engine inputs and generation checks.

Quiche 0.30.0 requires hosted types and dynamic memory. Keep the unmodified
upstream dependency, current TLS/mTLS providers and existing wire contracts.
The allocation-free claim covers the engine with fixed providers; it does not
extend to the default QUIC/TLS stack. Likewise, separating a storage record codec
would not make the mmap/filesystem store available on a bare-metal target.

### Ordered implementation and acceptance

The bounded call path now has generated wire interfaces, caller-owned question
and answer ledgers, and FIFO output admission. Enable `bounded_rpc(true)` on the
generator to retain interface identity, method parameter/result schemas and
reflection without emitting boxed clients or servers. Generated method constants
such as `echo::Owned::ECHO` bind the declaring interface, ordinal, parameter and
result types, cancellation policy and pipelining hint. Incoming dispatch checks
both interface ID and ordinal before exposing the corresponding parameters.
These bindings are wire views; they do not silently construct a hosted client.
Streaming methods instead expose `StreamingMethod` and empty acknowledgements;
their types provide no result reader or pipeline constructor. Typed paths carry
their root result schema and current struct schema through caller-owned `u16`
storage. Only capability fields can finish a path. Groups add no wire operation;
union selections cannot be expressed by Cap'n Proto pipeline transforms and fail
explicitly. Decoding a promised target copies and validates its entire transform
before retaining the source answer, so queued work does not borrow reusable input.

`FixedBuilder` can encode capability indices in roots, typed fields, lists and
generic fields. Indices refer to the enclosing payload's descriptor table and
are not authority on their own. Copying a capability-bearing tree still requires
a descriptor remapper and is rejected by ordinary fixed-buffer copying.
Publication validates every reachable index under the reader's traversal and
nesting limits, then validates hosted/promise grant multiplicities.
Receive admission likewise validates all descriptors before committing any import.
Caller-owned receipt slots contain staged new owners, aliases or tombstones; a
capacity/count failure preserves every prior owner, generation and reference.
Typed capability readers also carry message-reader identity. `reception::Received`
rejects a capability pointer from another reader even if its numeric index fits.

`inbound::call` preflights the target lease, answer slot, parent pipeline
dependency and complete parameter-import batch before changing any committed
state. Dropping the ticket preserves every count and generation. Acceptance
returns the borrowed incoming call, target lease, answer key and bound payload.
Application dispatch and eventual target release still belong to the connection.

`inbound::returned` similarly binds ordinary Return delivery to the retained
immutable original Bootstrap/Call, its implicit parameter releases and its result
imports. Invalid IDs, indices, receipts or release counts preserve all three
ledgers. Canceled questions do not import result capabilities; their pending Finish
owns remote cleanup. Export release batches keep retired owners in caller slots
for explicit draining, so acceptance runs no destructors. These bindings currently
handle hosted/promise imports, level-1/2 forwarding vines, ordinary results,
exceptions and canceled replies. Receiver-owned descriptors, tail-call adoption
and third-party results remain separate migration work. The caller must supply
the exact retained original message.

`reception::full` adds receiverHosted and receiverAnswer descriptors using
parallel caller-backed import, export-lease and answer-pipeline receipt arrays.
Every identity, transform, count and storage reservation is checked before any
table commits. Its accepted capability view distinguishes a remote import, a
local export lease and a promised-answer lease/path, with the same message-origin
check as ordinary reception. Invalid later descriptors cannot leave an acquired
prefix. Releases only decrement local counts; `take_retired`/`take_finished`
detach completed owners explicitly outside the transaction.

`connection::with_connection` and its async counterpart introduce a single
connection brand for caller-supplied import, export, question and answer storage.
Roles remain distinct; handles cannot escape into another connection. Split
field borrows support joint admission, and terminal scope cancellation detaches
all remaining owners. IO, messages, scheduling and time remain caller-owned.
This scope composes the ledgers; it is not yet the complete RPC driver.

`inbound::full::call` includes one extra export/answer receipt for its target,
admitting all payload references and the new answer together. Repeated references
to the same local export or answer are checked as a total. A parent retains its
old generation through Finish and wire-ID reuse. `inbound::full::returned` also
supports these descriptor kinds, atomically acquiring reflected local leases
before applying implicit parameter releases. Thus a Return can release the last
remote grant and still deliver that same capability locally. These APIs currently
cover caller-directed and redirected calls, results, exceptions and canceled replies;
`inbound::full::resolve` applies the same ownership transaction to all two-party
replacement descriptors, including late resolution after the observer is gone.

`Batch::export_promise` reserves a promise descriptor. `resolve_promise` stages
one Resolve without granting another reference to the original export. Publication
checks the promise ID, replacement kind and exact grant multiplicities together
with the sealed frame. Rejected output leaves resolution retryable; accepted
output resolves that ID once and prevents re-exporting it as a new promise.
`inbound::resolve` applies the matching import obligation and replacement grants
atomically. Hosted and bounded imports share `promise::ImportState`: hosted
aliases preserve a pending obligation and repeat resolution is rejected until
the old import is actually released. An absent/abandoned promise still requires
replacement cleanup; the accepted view explicitly retains that reference until
transfer or release.

`clients::Graph` owns capability edges in caller-backed slots. Its scoped handles
cannot cross runtime domains; generations reject stale callbacks, and checked
references keep endpoints alive through queued calls and resolution. Cyclic or
duplicate resolution, full storage and reference exhaustion leave the replacement
available for retry. Retired endpoint owners are explicitly drained outside graph
borrows; releasing a long promise chain uses constant stack space.

`snapshot` freezes the endpoint advertised for a remote promise, rather than
following that promise's later resolution. Export and post-Return pipeline records
must retain this snapshot. This follows the C++ forwarding rule for the Tribble
four-way race. Descriptor provenance is also retained: reflected local/answer
capabilities require an embargo after a used remote promise resolves, even if
their current endpoint routes back to the same peer. An affine barrier keeps the
original route and blocked producer alive until the embargo ledger accepts its
matching echo; canceled observers cannot destroy that ordering obligation.

`clients::dispatch::Runtime` adds caller-backed task slots and explicit polling.
Every task enters one non-wrapping sequence. Polling selects the oldest ready task
while unrelated ready routes can progress past blocked promises. Rejected output
keeps the task and its payload in place. A running task owns a frozen endpoint;
completion/cancellation releases it explicitly. The original route's calls and
sender-loopback task use this same sequence. Opening the barrier cannot let new
calls overtake queued ones. The framed fixture covers one-byte writes, Pending,
Interrupted, dropped output and duplicate echoes. This is an owning dispatcher
component, not yet the complete connection driver or migrated hosted runtime.

The dispatcher can also borrow an existing graph after the connection's export
and answer scopes have been established. Its queued owners can then contain
those scopes' capability receipts. Graph-wide task sequences never reset when
dispatcher storage is reused. `drain_one` detaches blocked owners for explicit
connection cleanup without invoking user code inside a graph transition.

`connection::with_domain` introduces a single-use domain before caller storage is
declared. Consuming its `bind` token creates the graph and connection ledgers
together. Graph endpoints can then own import handles while exports and answers
own graph clients, without a lifetime cycle or heap indirection. The token cannot
be copied, forged, reused or escape its scope; compile contracts check these
properties. Storage sizes and terminal teardown remain caller responsibilities.

`rpc-wire::owned` keeps each immutable incoming frame, descriptor receipts and
call target together. Async request views cannot replace their receipt origin
or release their frame while suspended. An owning outgoing question retains the
exact accepted Call/Bootstrap through Return, even after cancellation; returned
results retain their receipts until explicit release. Answer owners keep their
exact published Return through Finish, pipeline leases and local adoption.
Finish and implicit export releases therefore cannot be paired with a different
frame. Cleanup builders reserve FIFO admission before committing Finish/Release.
Reply `try_send` and `try_send_granting` preserve both the completed frame and
the exact staged export batch on rejection. After output drains, retry does not
rerun application code or reuse an aborted grant ID. If Finish arrives meanwhile,
the retained result may still serve admitted pipelines; wire publication must
instead send cancellation and abort any unsent grants.

`owned::pipelines` freezes local result routes from staged export descriptors.
Its weak graph observers retain the original answer generation through early
Finish and peer-ID reuse. Canceling every observer permits parent cancellation;
invalid paths and failed producers become broken routes. Result completion
checks the answer generation and envelope ID and rejects duplicate completion.
`Published::from_reflected` also snapshots receiverHosted and receiverAnswer
results. These retain the exact original import or outgoing-question route,
even if the application's observed promise has resolved elsewhere. Owning
`try_send_reflected` preserves the completed frame, grant batch and reflection
receipts on rejection, then queues the accepted Return before releasing them.

`clients::outgoing` binds paths on outgoing questions to callable graph clients.
The original question route remains independently retained until queued calls
and frozen result routes have retired. Return settles existing observers with
the correct direct/reflected ordering policy, including after the application's
question cancellation. `clients::exporting` gives each exported local promise an
independent graph identity, settles it once to a frozen route, and publishes its
Resolve through the FIFO. Backpressure preserves the settled route; a nested
promise resolution cannot rewrite the capability originally advertised.

`owned::connection::Driver` owns the joint graph/ledger domain, ordered dispatcher,
outgoing request originals and responses, binding registries, and deferred Call
and Resolve tasks. Its ordinary `send` reserves request tracking before output
admission. Its input coordinator applies Call, Bootstrap, Return, Resolve,
Finish and Release once, preserving unadmitted frames and rejected producers.
Pending Return propagation precedes a following Return or Resolve; otherwise a
newly returned promise could lose a Resolve before its graph observer exists.
Control cleanup can still progress, and local tail adoption does not block the
unrelated replies its producer may need. This ordering is explicit rather than
depending on an executor yield between incoming messages.
`enqueue_call` accepts a generated method and a graph capability rather than a
caller-selected wire target. Queued parameters retain their capability clients;
their reachable indices are validated before dispatch admission. The same
sequence orders incoming calls, application calls and loopback markers.
`send_invocation` publishes the exact question and parameter grants together
with FIFO acceptance, then returns the task/question mapping to the caller.
Rejected output preserves the task. Local destinations expose borrowed typed
parameters and message-bound capability lookup for caller-driven execution.
Cancellation before wire publication owes no Finish or Release. Deferred input
gets a dispatch position before newer application work. The portable fixtures
cover parameter grants through Return/Finish/Release, failed descriptor checks,
unresolved targets, local dispatch, stale cancellation and task-slot reuse.
`invocation_pipeline` creates result capabilities before wire publication.
Caller-backed observers attach them to the accepted question or its already
received Return. Cancellation retains result rights until those observers are
bound or abandoned. Subsequent Resolve cannot overtake that binding. Local
execution resolves the same paths through `poll_local_invocation_pipeline`;
canceling an unsent call breaks its pipelines without sending wire controls.
Tests cover early/late binding, cancellation, graph exhaustion, local completion
and lost-resolution fault injection.

`completion::Slot` supplies a reply future and affine sender in caller-owned
storage. Their lifetimes prevent reuse while an endpoint remains accessible;
the sender can complete only once. Producer cancellation wakes the receiver.
Receiver cancellation before delivery returns the rejected value to its sender.
Cancellation after delivery retains the undelivered value in the slot, which
refuses reuse until `take_undelivered` recovers it. This matters for RPC Responses:
the caller must release their receipts through the driver, and generic value
Drop cannot do that. Wake, waker destruction and recovered-owner destruction run
outside state borrows. The caller supplies the waker and polling policy. The
portable fixture sends real owning responses through these futures, covers
cancellation on both sides of delivery, and reclaims every message buffer.
Kani checks reply ownership, cancellation and reuse; mutation controls require
rejection of sends to closed receivers and retention after accepted cancellation.
`Sender::track` consumes the producer endpoint into a unique `Delivery`. It can
register a producer waker for receiver consumption or cancellation, recover an
abandoned value, and explicitly cancel delivery during teardown. This works
while network IO is idle. Waker callbacks run after publishing state and outside
the slot borrow; tracking cannot create competing cleanup observers.
`Slot::try_pair` claims a slot through a shared reference for application-owned
slot arrays. Both endpoints keep it busy, even after consuming the result, until
they are dropped. A tracked delivery replaces its sender without changing that
ownership count. This lets a service context remain usable while its caller
awaits a reply, without letting stale endpoints observe a later call. Forgotten
endpoints require exclusive recovery or ending the storage scope. Native tests
also check endpoint cleanup when a receiver's wake hook panics.

`owned::connection::Replies` binds accepted questions to those slots using a
caller-supplied `ReplySlot` array. Capacity rejection returns the original sender
for retry. Polling waits for pipeline observers before handing out a response,
cancels closed receivers through the driver, and releases abandoned responses
and their capability receipts. Cancel bindings before terminating the driver;
responses already taken by the application still need explicit release. The
portable fixture covers capacity retry, cancellation before/after Return,
consumption, forgotten receivers and teardown with actual imported capabilities.

The engine's optional `alloc` feature adds `completion::shared::channel` with
heap-owned endpoints over those same send, cancellation, polling and wakeup
transitions. It needs no standard library or executor. Hosted RPC replies,
local calls, pipeline queues, task-drain notifications, third-party rendezvous
and streaming-credit waiters use this adapter; the allocation-free
profile continues to use caller-owned slots. Heap-owned replies use normal Rust
destruction, so affine wire receipts still require the borrowed tracked-delivery
API and explicit driver cleanup. Hosted flow-control paths detach waiters before
waking them, allowing synchronous callbacks to inspect the controller. Native
and Miri tests cover cancellation, abandonment, exact value transfer and
reentrancy; a held-borrow fault must fail the reentrancy check.

`Invocations` establishes completion bindings while calls are still queued.
Its caller-owned slot array tracks output acceptance, local execution and reply
observation. `CallOutput` retains either a wire Response or a LocalResult, so a
call can wait on a promise before its eventual location is known. Local results
retain graph clients and validate capability-pointer origins and generated
method identity. Complete every local result pipeline before delivering or
discarding its original result; cancellation of that reply does not cancel
calls already queued on its result capabilities. Transient resolver errors keep
the observer and local result available for retry.

`accept_local` returns an affine LocalCall. Its `params_async` borrows only that
call and its immutable message bytes across suspension. Other pool buffers and
driver operations remain available. `complete_local` returns both owners on a
method mismatch; success releases parameters and retains the completed result
until propagation/delivery. `Invocations::terminate` cancels pending receivers,
reclaims unconsumed outputs and reports externally running calls which must be
released before driver termination. Local and remote fixtures exercise these
paths, including output rejection, abandoned/forgotten replies, exceptions,
zero allocation and suspended parameter reads during independent buffer reuse.
Generated public completion adapters and hosted ownership migration remain open.
`Session::drive` supplies the exclusive wake registration for a caller-polled
driver. `Drive::poll_with` lends connection, pool and invocation bindings for
one synchronous poll, preserving the operation when access is busy. Application
calls, admitted input and released results wake the driver after the mutable
borrow ends. Polling an idle connection does not generate another wake. Ready
and handle destruction remove the registration, and Rust prevents session
teardown while that handle remains live. Waker replacement and callbacks run
outside the session borrow. Real call/return fixtures cover consumed and abandoned
results, zero allocation, wake counts, ARM linking and 32-bit Miri. Fault controls
remove a wake or restore a held borrow and must fail their named assertions.
The application still polls IO, maintenance and completion bindings with that
context; this registration does not introduce an executor or background loop.
Admitted operations blocked on graph/task capacity remain owned for retry;
ordinary control messages may still progress. A budgeted round-robin maintenance
poll propagates promises, drains retired owners, emits Finish/Release and cancels
queued work without executing the service. Its cursor advances on resource errors
so blocked work cannot starve cleanup. The two-domain portable fixture exercises
this coordinator over partial, Pending and Interrupted IO, including a saturated
dispatch queue, cancellation, late replies and complete resource reclamation.
The driver also owns reflected loopback obligations. It queues senderLoopback
markers through the same ordered dispatcher as calls, and accepts an echo only
for its exact, non-reusable embargo ID. Incoming senderLoopback requests retain
their original target until an echo is accepted into output. Unresolved answer
targets wait for publication and survive early Finish; canceled parent results
are retained locally while their wire Return reports cancellation. Echoes must
route back to the same peer. Deferred echoes prevent newer incoming calls from
overtaking them.

Driver storage requires at least two dispatch slots, one loopback binding and
one embargo receipt. One dispatch slot is reserved for senderLoopback markers;
otherwise calls awaiting an echo could consume the storage needed to send its
marker. Ordinary calls and incoming echoes share the remaining capacity.
Output rejection retains queued controls and graph barriers, and rejected
embargo IDs are never reused. The portable fixtures exercise two driver domains,
early Finish, duplicate echoes, full send storage and calls arriving before
loopback output under partial/Pending/Interrupted IO. IO polling and application
futures remain caller-owned. The hosted coordinator also integrates authenticated
third-party answer handoff and automatic tail transfer, described below.

Same-connection tail calls use `send_redirected` followed by `tail_answer`.
Output rejection preserves the child for retry; accepted adoption is once-only.
Caller-backed tail bindings retain the redirected child until the original
answer generation retires, including accepted pipelines after early Finish.
Pipeline observers freeze the child question and path rather than consuming its
ResultsSentElsewhere acknowledgement as a result. Reusing the parent's wire ID
cannot redirect a new producer. Portable tests exercise these paths under IO
faults, saturated output and exhausted graph storage; a generation fault control
must fail the reused-ID assertion. Terminal cleanup drains the retained children.

Terminal cleanup consumes the driver and borrows its output queue exclusively.
The resulting `Terminating` state exposes only budgeted cleanup polling. After
transport IO stops and externally executing tasks/results are returned, cleanup
discards unsent output without claiming transmission, releases pending frames and
receipts, and transfers producers, import owners and local servers to the caller.
It drains task ownership before invalidating ledger/graph entries. Completed
cleanup leaves the queue permanently failed while making this connection's
message buffers and send slots reusable in the enclosing pool. Dropping cleanup
early still requires ending that pool scope. The portable fixture combines a
partial write, pending and returned questions, published results, a deferred
Resolve, queued/deferred Calls and one-step polling budgets.

Incoming Abort records the first terminal failure and blocks further coordinator
admission, sends and maintenance output. The caller stops IO on `Received::Aborted`
and consumes the driver for cleanup. An echoed Unimplemented Resolve releases
its sender-owned replacement grant while retaining any already admitted call
lease. An Unimplemented response to a required RPC message is terminal. The wire
codecs also construct bounded Abort and capability-free Unimplemented controls.

`Messages::receive` reserves receive-only storage without clearing reused bytes.
Its `Receiving` owner exposes only the prefix committed by the read adapter;
the decoder cannot inspect previous contents. A buffer-independent `FrameCursor`
shares framing validation with the borrowed `FrameReceiver`. The wire task seals
only complete frames, and discards EOF, errors and explicit cancellation.
`Messages::write_owned` transfers a cleared builder reservation into a caller-pinned
build task. Both paths leave the pool available while Pending. Receive writes
directly into its final buffer; async generated replies are built directly after
the framing header. Existing `reserve` and `reserve_exact` still clear storage.
Dropping an incomplete IO task is terminal connection teardown, not a new frame
boundary. The owning conversation fixture combines bootstrap, graph dispatch,
generated async calls, typed returns, cancellation and cleanup with partial,
Pending and Interrupted IO. Separate fault cases cover truncated frames, invalid
read counts, transport errors, capacity failure, builder errors and cancellation.
Third-party orchestration, automatic connection driving and default hosted
ownership migration remain unfinished.

`rpc-wire::clients::Imports` binds admitted import receipts to shared graph
clients. Hosted aliases preserve an outstanding promise obligation from the
import ledger. A weak, scoped alias map avoids an ownership cycle; the immutable
remote endpoint owns the actual local import reference until explicit retirement.
Two-slot promise admission preserves the receipt and graph on capacity failure.
`clients::reflected` retains a receiverHosted graph client independently of its
export lease, so final peer Release cannot revoke that local reference. The
sealed `Capabilities` facade supports both graphs and ordered runtimes without
exposing a route that bypasses the runtime's task queue. `bind_capability`
handles imported, reflected local and promised-answer receipts and preserves
their ordering provenance. An owning Resolve task admits its replacement once,
then applies it to the graph with that provenance. Resource rejection returns
the same task for retry; abandoned targets still release replacement grants.
Reflected resolutions return a barrier to the ordered loopback dispatcher.
Complete connection-driver ownership is still being integrated.

`publication::reflected` binds receiverHosted and receiverAnswer output to retained
import/question references. Descriptor IDs and complete normalized transforms
must match those references. Rejection preserves them for retry; acceptance puts
the exact frame into the FIFO before making any resulting Release or Finish
eligible. Questions returned with noFinishNeeded cannot be reflected after the
peer has permission to discard their answer. The combined `question` binding also
reserves the new question and validates the target; `answer` binds a Return to its
answer lifecycle. The portable fixture transfers reflected Resolve through
partial/Pending/Interrupted IO and checks replacement survival through cleanup.

The `embargoes` ledger uses caller-backed or hosted sparse storage. Reserved
owners become echoable only on output acceptance. IDs never wrap or repeat
within one connection, so delayed echoes cannot release a later waiter after
storage reuse; exhaustion returns the rejected owner. `admission::embargo` binds
the offer to its sender-loopback ID and original target before FIFO acceptance.
Cancellation leaves a tombstone until echo, while terminal draining detaches all
owners. Hosted promise clients now use this ledger and wake detached owners
outside table borrows. The portable fixture exercises framed loopback and promise
exchange through short writes, Pending and Interrupted; determining when the old
route has drained still belongs to the complete dispatcher being migrated.

`reserve_redirected` reserves a question for `sendResultsTo.yourself`; outgoing
admission verifies that exact result destination. Incoming redirected calls
retain results for one `takeFromOtherQuestion`. Adoption preflight acquires a
generation-bound producer dependency atomically with Return and implicit
parameter releases. The dependency survives Finish and source wire-ID reuse.
A canceled observer still consumes the once-only adoption without retaining
an extra dependency. `resultsSentElsewhere` is accepted only for a redirected
question; ordinary results/exceptions are rejected there. `admission::tail_answer`
binds the original answer to a retained redirected question on the same
connection. The driver must keep that question until the original Finish and
keep polling the adopted producer. The portable fixture exercises these messages
through short/Pending/Interrupted IO, dropped output and early Finish. Received
`onlyPromisePipeline` remains a hint: this profile always sends Return, as the
protocol permits, and does not originate calls using that optional optimization.

`inbound::finished` binds a Finish to the retained Return and all implied result
releases before retiring its answer. Rejected input changes neither ledger;
unknown/duplicate Finish is ignored without replaying releases. Explicit Release
admission likewise leaves retired owners for draining outside table borrows.

`Messages::write_borrowed` ties a mutable buffer view to its unique writer, so
other buffers can progress while a generated result builder is suspended. Rust
prevents sealing/discarding that writer while the view is live.
`FixedBuilder::into_segment` consumes the builder and transfers its initialized
segment to a borrowed slice, avoiding its normal clearing Drop. The caller can
reserve header space and construct results directly in a future send buffer.
Framing, descriptor ownership and publication validation remain required.

Bounded code generation now emits borrowed async `Server` methods and a
stack-owned `ServerDispatch` implementing `wire_capability::Service`. Dispatch
matches declaring interface and method ordinal, including inherited branded
interfaces. Metadata exposes cancellation/streaming policy before the future is
polled. `Incoming::serve` validates the payload and constructs results directly
inside a fixed Return envelope. Temporary envelope editors transfer their borrow
with `FixedStruct::into_field`; request/result buffers and service remain borrowed
through suspension. Dropping the future cancels execution; discard that partial
output. Unknown methods fail before touching output or invoking the service.
For redirected calls, retain the result locally and publish ResultsSentElsewhere;
the normal answer publication transaction rejects transmitting ordinary results
for that redirected answer. Capability receipts, result descriptor publication,
tail adoption and scheduling remain connection responsibilities.

Generated `ContextServer<C>` and `ContextServerDispatch<S, C>` adapters implement
`wire_capability::ContextService<C>` when a service needs caller-provided context.
Both incoming wire calls and local invocation parameters expose
`serve_with_context`. Inherited and branded methods forward the same context;
streaming methods keep their distinct signature. The serialization layer does
not depend on an executor, transport or capability graph. The portable fixture
resolves an actual parameter capability, awaits a nested invocation, and returns
that capability while the parent is suspended. Driver and message-pool borrows
end before every suspension, so nested work can progress without allocation.

`Session` now wraps caller-backed driver, message-pool and invocation storage.
Its synchronous `access` closure cannot lend mutable connection state to an async
future. Reentrant access returns the original closure for retry, preserving any
captured affine owners. `call` returns a method-typed reply future; `read` exposes
that method's generated result schema. `capability` checks the result's message
origin, binds its retained receipt, and returns a `TypedClient` with the generated
field's interface. `pipeline` accepts a generated path rooted in that reply's
result schema. Both clients remain valid after releasing the original reply;
interface changes require an explicit cast or use of the raw graph API.
Reply and client release preserve the original owner if the session is busy.

`LocalResultDraft` reserves the final result buffer before service execution.
`write_async` borrows only that buffer; the draft itself remains outside the
future. Result capability insertion stores an owning graph reference only after
the field write succeeds. `finish` validates every reachable index before moving
the bytes and references into an owning local result. Cancellation, service
failure, invalid indices and explicit discard recover all inserted references.
A deliberately forgotten future keeps its byte borrow unavailable until the
pool scope ends, while other buffers remain usable. The cleanup fixture checks
retirement before connection termination, and a missing-release fault control
must fail that check. Fixed-field reborrows preserve union-selection semantics.

Generated streaming methods enter the same ordered invocation registry through
`streaming_call`. They publish `noPromisePipelining`, reject result pipelines, and
return an acknowledgment owner with no result-schema API. Local acknowledgments
retain their generated method identity; remote exceptions retain their frame and
map to portable error kinds through `check_ack`. Caller-supplied flow control
still uses the engine's byte-window accounting; the session does not create a
clock or task executor. `Session::streaming` binds a caller-owned array of
acknowledgment slots to that exact session. Its budgeted `poll_drained` registers
wakeups, checks local and remote errors, and releases each accepted reply's
capability receipts. The first observed failure rejects new calls but cannot
report drain completion until all earlier admitted calls settle. Capacity and
busy-session rejection preserve pending owners and never run a rejected call's
parameter filler. Dropping this owner cancels consumer interest; the invocation
registry still reclaims accepted results and wire obligations. Byte-window
policy remains caller supplied, independently of the outstanding-call limit.

Third-party provision primitives now use these same bounded ledgers. A distinct
outgoing `Provision` handle owes Finish but exposes no result pipeline and never
awaits Return. An incoming Provide retains its immutable recipient metadata and
target receipt while the caller prepares a network registration; Finish returns
that registration owner without requiring a sent result frame. Owning Accept
requests preserve completion metadata and distinguish a null embargo from a
present empty byte string. Accept results use the normal Return, capability
grant, pipeline and cleanup machinery. Caller-owned embargo storage admits each
identifier once, tolerates either message arrival order, and cancels pending
acceptances when the provision closes. Released acceptances retain independent
authority. The hosted third-party exchange uses the same embargo transition
policy. Network authentication and automatic cross-connection orchestration
remain caller integration work; these codecs do not authenticate identifiers.

The owning driver admits Provide and Accept into a bounded network-job slot.
The network takes the owning job, authenticates its metadata, and registers it
against the exact live answer generation. Registration holds its graph authority
inside that answer's cleanup owner. A callback cannot run after Finish, and an
acceptance which already retained authority survives registration cleanup.
`Accept::authorize` binds the supplied authority to the embargo in that immutable
request. Only a successful poll produces `ReadyAccept`, which the driver accepts
for capability publication. Rejected output returns that same ready owner.
Early Finish uses a canceled wire Return while already-retained pipelines keep
their local completed results. Denied or revoked acceptances use the ordinary
exception and pipeline-failure path. Outgoing provisions use distinct sender
handles without occupying reply-tracking slots; FIFO admission keeps their
Finish behind the Provide even when output is saturated.

`ThirdPartyAnswer` reserves a callee-selected ID in separate caller-supplied
sparse question storage. Its owning network job retains the completion metadata
even when Return arrives first. The driver gates response delivery, reply
registration, and pipeline creation until the caller authenticates the exact
peer/rendezvous through `authorize_adoption`. An identifier alone never opens
that gate. Authentication failure retains the job for retry or cancellation;
closing it cancels the question. Early Finish cannot recycle the ID before its
late Return. Results admitted before denial release their grants once; results
arriving after cancellation are discarded without importing grants. Ordinary
connections can supply zero adoption slots. Cross-connection metadata matching
and peer authentication belong to the caller-provided network.

`send_third_party` reserves a distinct result destination from ordinary calls
and same-connection tail calls. The immutable Call must match that reservation;
a third-party question cannot be adopted as a local tail source. A callee may
send `awaitFromThirdParty` only when the original Call opted in. Its owning
`AwaitingResult` job keeps immutable metadata and the original question generation
through authentication, cancellation and retry. `complete_await` validates the
generated method before calling the network's rendezvous validator. A successful
validator binds the supplied local result to that authenticated peer and call;
foreign connection capabilities must become local proxy clients, never cast
between graph brands. Rejected authentication returns both owners intact.
If the original Return declared noFinishNeeded, completion permits scalar
results but rejects capabilities before invoking the authentication callback.

`take_output`, typed invocations and `Replies::track_output` deliver ordinary or
handed-off results through the same cleanup path. Old-route pipelines remain
embargoed until their ordered loopback completes. Canceled, abandoned and
disconnected handoffs reclaim metadata, results and retained capabilities.
`complete_network_answer` supplies the original callee's retained pipelines
after publishing Await or ResultsSentElsewhere. It requires the exact live
request and method and refuses premature or duplicate completion. The portable
three-vat fixture uses six independent connection domains and exercises either
arrival order, authentication before exposure, all Finish obligations and full
buffer recovery. Automatic rendezvous orchestration and hosted default runtime
migration remain integration work.

The sender's `send_adopted_answer` reserves a callee-selected answer and its
`ThirdPartyAnswer` announcement in the same FIFO publication. Invalid metadata
or insufficient storage returns the unadmitted producer. Ordinary Call and
Bootstrap cannot occupy this reserved range. The adopted answer then uses the
normal Return, cancellation, pipeline and Finish lifecycle.

Provide-targeted acceptance markers now traverse the ordinary dispatcher before
becoming network jobs. A queued marker cannot be passed to the embargo-release
API: only `poll_network` can produce the ordered marker type. Older calls,
pending promises, and an occupied network-job slot preserve the marker for a
later poll. The callback receives its exact registration generation, so Finish
and wire-ID reuse cannot authorize a stale marker. Provide targets require an
empty transform. Imported targets must resolve to an explicit `Route::Vine`
proxy; ordinary local services cannot authorize acceptance forwarding. An export
lease pins the exact proxy generation while its marker traverses the dispatch
queue. `forward_vine_embargo` receives an ordered marker, copies its identifier
into caller-provided scratch, and asks the proxy to enqueue it on the same
upstream path used by older calls. Failure preserves the marker and lease.
`send_provision_embargo` binds that output to an affine outgoing Provision,
keeping Finish and ID reuse behind the accepted marker. Peer Release and
disconnect revoke callbacks; terminal cleanup returns the proxy owner. The
caller network still supplies authenticated proxy routes and forwards ordinary
calls before acknowledging dispatch.

Incoming Join jobs own their key-part metadata and target receipts. The caller
supplies network validation, contribution routing and the registration guard in
the answer's producer. `complete_join` publishes capability-free result metadata
and retains the target through Finish; `reject_join` publishes an ordinary
exception. Full output or invalid metadata returns the original job unchanged.
Early Finish emits a canceled Return without running the metadata callback.
Join result-capability pipelines fail because Join results are network metadata.
These operations supply bounded wire ownership; authenticated part aggregation,
proxy forwarding and acquisition remain the network provider's responsibility.

An outgoing question reservation and its immutable frame enter ordered output
together. Failed preparation rolls back the question and send. `Finish` and
`Release` obligations stay pending until the exact matching frame is accepted;
a full queue cannot lose cleanup or prematurely recycle an ID. Answer tickets
similarly allow one Return and preserve owners across rejected output. Admitted
pipeline dependencies retain the answer through Finish/Return and suppress
execution cancellation while a dependent call still needs the result.
After both Return acceptance and Finish, an answer's wire identity is detached.
Its local pipeline leases retain a slot/generation identity instead, allowing safe
peer reuse of the wire ID without redirecting old work or losing the new owner.

The FIFO accepts non-copyable send offers, exposes only its oldest suffix, and
fails terminally after a transport error. `rpc-engine::io` defines caller-polled
read/write interfaces and bounded-attempt output polling. `rpc-wire::io` drives
the fixed receiver with the same interfaces. The hosted futures adapter delegates
leased-message polling to this engine code. A portable fixture runs generated
bootstrap, call, return, cancellation, Finish and Release conversations with
short IO, interrupted operations and pending polls. The `bounded_cpp_interop`
example also drives the public bounded API against the existing C++ TCP peer,
covering bootstrap/result pipelines, returned and reflected capabilities,
exceptions, cancellation, Finish/Release and complete buffer recovery. Its socket
adapter deliberately fragments IO, and the ordinary C++ interoperability test
runs it alongside the hosted example. This checks protocol interoperability;
the complete RPC conversation is also checked for zero allocations with hosted
features enabled. It is not a performance measurement. The full hosted RPC runtime
has not yet migrated to these ledgers and wire bindings.

1. [x] Establish isolated consumer fixtures and CI jobs for allocation-free core
   and engine, optional allocating conveniences, and hosted profiles. Add the
   engine crate with extracted production state transitions (for example the
   pure flow-control calculation), immediately used by the current defaults.
   Keep normal native tests under nextest.
   Implemented: fixed/adaptive streaming credit and question Return/Finish
   lifecycles are used by the hosted runtime and linked on bare-metal ARM without
   an allocator. The fixture also exercises caller-indexed wire reading and
   resumable stream framing with generated borrowed views. It now links a
   generated bounded RPC conversation, including construction and teardown.
2. [x] Separate borrowed field views from allocating owners. Implement fallible
   arena/pointer operations and generated fixed builders for scalar, text/data,
   nested struct, list and union operations. Add validated caller-indexed
   multi-segment reading. Cover exact-fit/short buffers, copying/upgrading,
   reuse, failed publication and error formatting with no heap allocation.
   Implemented: bounded primitive/pointer/struct/nested lists, group/Void selection,
   transactional recursive data copying, explicit non-truncating struct/list
   upgrades and bounded generic/AnyPointer data writes, plus explicit bounded
   capability indices. Cross-message capability remapping is explicit, bounded and transactional.
3. [ ] Implement fixed and growable resource providers, generation-checked
   handles, admission reservations, control reserves and message leases. Move
   one complete generated bootstrap/call/return/cancel/release path into the
   engine and run it through both manual IO and the default hosted adapter.
   Treat this as a partial migration until all protocol operations are covered.
   Implemented: generated manual-IO conversations and scoped question/answer
   ownership, validated output transactions and FIFO polling, plus optional
   growing graphs, ledgers, dispatch and bindings with explicit limits. The
   hosted leased send adapter shares the polling implementation. Stable growing
   message buffers, send records and FIFO queues use the same lease transitions.
   Adopted questions now grow under an independent entry-count bound, preserving
   late-Return and Finish obligations. Owned completion endpoints move into the
   same tracked delivery interface, and typed sessions can allocate their reply
   slots. Migration of default RPC handle, receive/dispatch and task ownership
   remains.
4. [ ] Move pipelining, tail calls, flow control, embargoes, third-party answers,
   joins/handoffs, revocation and disconnect cleanup onto the same resource and
   event interfaces. Preserve authentication, cancellation and receipt semantics.
   Replay the same traces using fixed and growable providers; compare observable
   results with the previous implementation and pinned C++ peers.
5. [ ] Complete the public owned API and generated server adapters; switch every
   current default transport/runtime path to the engine. Remove migrated legacy
   transitions and mandatory boxed futures/channels from the engine dependency
   graph. Preserve hosted ergonomics while documenting bounded API differences.
   Implemented: generated borrowed async server adapters, caller-backed capability
   graph and ordered task dispatcher, owning wire coordination and third-party
   handoff primitives. The main ledger, dispatch, invocation, message and
   output-queue resources have fixed and growing providers; sessions also accept
   owned reply slots. `scoped::ClientHooks` now retains live or frozen graph routes for
   owned generated clients without extending any scope lifetime. Its private
   inbox identities cannot be forged from public hook pointers/brands, and
   opaque parameter hooks retain their interception policy. Call admission and
   running ownership have explicit limits; engine queue rejection retries the
   oldest call; final hook/call drops wake the caller for reference reclamation.
   Pending local/imported promises distinguish routability from settlement and
   observe reflected ordering barriers. `enqueue_wire` uses tracked portable
   invocations, carries null capability slots, projects pipelines while local
   execution is pending, and converts completed wire receipts into owned hooks.
   `Call::forward` preserves the original server context. Owned server preparation
   retains exact admitted parameter receipts across frame release, and dispatch
   preserves the original result hook's executor, cancellation and tail-call
   behavior. Invalid completed projections break rather than retrying forever;
   redirected calls still complete when a sender requests only a pipeline.
   Per-message capability receipts, local results, drafts, frozen publications
   and pipeline paths now support explicit bounded growth. Resource-table growth
   alone preserves fixed message limits; `GrowingStorage::with_messages` opts in.
   Fixed profiles never spill to the heap merely because Cargo enables `alloc`.
   Owned requests, Return payloads, reflected routes and network control targets
   use the same budgets, with fallible allocation before ownership admission.
   Identity capability copies validate indices without materializing a map.
   Portable Call/Return/Resolve admission now delivers optional transport resources
   to exact import receipts only after references commit. Caller-backed resource
   slots survive rejection, consume repeated indices at most once, and ignore
   unavailable indices as the protocol requires. The hosted metadata adapter
   retains Unix descriptors through owned clients, late aliases and resolution.
   Output FIFO entries now own resource bundles alongside their exact accepted
   sends. The original message cursor determines first-byte eligibility, so
   partial writes cannot re-emit descriptors. Fixed and explicitly growing
   bundles share Call/Resolve generation and Return publication. The caller-
   polled Unix writer reuses existing ancillary IO without another protocol
   ledger. Pending, interruption, rejection and terminal cleanup preserve the
   stated ownership boundaries. Hosted `server::Results` now collects completed
   payloads and explicit early pipelines without branded graph references. Its
   execution future preserves generated cancellation policy through the caller
   executor, including retaining partial results after caller interest disappears.
   `collect_server_results` retains original policy hooks; `complete_answer`
   moves immutable result ownership into the portable answer. `send_answer`
   stages descriptors/resources and accepts Return atomically, preserving results
   on output rejection. Finish racing publication emits Canceled while keeping
   results for admitted child pipelines; redirected calls emit ResultsSentElsewhere.
   `poll_maintenance_with` resolves explicit early pipelines before final payload
   lookup, including after service failure, with outer resource errors retryable.
   Exception callbacks preserve hosted reason/type/details. Generic tail calls
   forward their child request and pipeline through the original hook.
   The incoming-call `server::Worker` now owns accepted dispatcher operations
   through preparation, application polling, collection and Return acceptance.
   Capacity rejection leaves queued work in place; round-robin polling registers
   every pending method without spinning after a complete idle scan. Finish
   cancels caller interest through the original generated cancellation executor,
   and explicit teardown returns requests before driver reclamation. Output
   sizing includes descriptor tables and exception detail data. Fragmented
   caller IO, partial resource writes, large replies and early child calls are
   exercised together with that worker. Outbound invocations and authenticated
   network controls still belong to the caller; no second protocol ledger is
   introduced.
   The outgoing `wire::Calls` coordinator now admits owned requests to tracked
   invocations and polls local methods, remote output, early projections and
   completion delivery. A rotating bounded scan preserves progress during
   capacity rejection; the oldest unadmitted call remains owned until retry or
   cancellation. Local result producers resolve existing and late pipelines
   before fallback to completed payloads, preserving explicit early routes after
   failure. Owned completions preserve local error reasons/details, and protected
   methods notify the coordinator when independently publishing early pipelines.
   Output sizing accounts for descriptors and promised-answer transforms;
   resource-aware output shares the same coordinator and invocation transaction.
   Termination cancels bindings and returns running local requests before driver
   reclamation. `wire::foreign::ForeignPromises` now supplies policy-preserving
   foreign imports and drives their pending resolution through caller polling.
   Its bounded weak registrations cannot keep abandoned graph routes alive;
   chained resolution, capacity rejection and wake-driven idle scans preserve
   progress. Successful settlement retains the original callable proxy, so
   admitted calls cannot be overtaken by a new direct path. Export snapshots
   retain a callable local promise's resolution obligation while keeping remote
   routes frozen. Failed application owners retain hosted exception diagnostics
   and publish a wire exception. Direct exports use metadata-aware collection
   before publishing readiness; remote resource metadata remains caller supplied.
   `wire::coordinator::Coordinator` now combines calls, workers, foreign promises,
   maintenance and full-duplex byte IO in one caller-polled loop. The caller
   supplies buffers, IO, stop/deadline futures and a `Control` implementation for
   bootstrap policy and authenticated network operations. Output queue pressure
   pauses publication while input and output continue. Busy input retains its
   original frame. Explicit Yield outcomes and a separate application wake
   notification prevent idle scans from spinning or losing a promise wake.
   `owned::tasks::receive_until` adds allocation-free terminal cancellation that
   returns its writer before teardown. Normal stop, EOF and failure drain the
   hosted work and portable driver; early drop instead ends the pool scope.
   Fragmented duplex calls (including 64 KiB), one-slot execution/output,
   interrupted IO, idle wakeups, partial-input cancellation, input/output faults
   and rejected controls have integrated regression coverage. The same loop
   now supports resource-bearing IO through `run_transport`. Its input owns
   resources through admission retries; its output policy supplies bundles for
   Calls, Returns, Bootstrap replies and Resolves. The Unix adapters use
   caller-polled recvmsg/sendmsg without prefetching a second frame's resources.
   Metadata reaches hooks before synchronous server dispatch and client
   completion. Unclaimed resources close on admission, EOF, faults and stop.
   Real-socket tests cover bootstrap, pipelined calls, returned capabilities,
   delayed promise resolution and zero-descriptor fallback. Regression tests
   cover frame isolation, duplicate indices and partial-input terminal cleanup.
   Buffered output now shares an allocation-free `io::Output` state with an
   explicit `Flush` contract. It flushes partial frames as well as complete ones,
   preserves attachments across retries and parks without polling clean IO.
   The futures adapter forwards flush/close; the Unix adapter closes its write
   half. Managed `run_stream`/`run_connection` stop new work, release pending
   input resources, drain accepted output and await write-half closure. A
   separate caller abort future remains active throughout drain/flush/close,
   after which terminal cleanup returns protocol storage. Immediate
   `run`/`run_transport` cancellation is still available. Tests cover buffered
   duplex calls, each blocked shutdown stage, flush/close failures and real Unix
   resource connections. A one-byte Kani harness checks buffered completion and
   once-only attachment acceptance for Pending/Interrupted/failure/success;
   it does not prove the entire async coordinator or network liveness.
   Bootstrap now uses a distinct admitted token, and its completion shares the
   answer's retained result, pipeline, Return and Finish ownership. It cannot be
   completed with an ordinary method's token. Fixed-storage tests cover pool
   exhaustion, output pressure, Finish-before-Return and wire-ID reuse with zero
   allocations. The bare-metal conversation uses the same completion API.
   `Coordinator::bootstrap` exports the root promised capability; its weak owned
   `Handle` also admits typed bootstrap commands while the loop is running. The
   bounded command lane participates in fair polling, wakes idle connections,
   skips abandoned commands and rejects pending commands during shutdown.
   `TwoParty` supplies concrete bootstrap policy with a factory closure capturing
   the authenticated peer identity. It executes policy once and retains results
   across output pressure; factory exceptions preserve wire reason/details and
   leave the connection usable. Integration tests cover immediate calls,
   pending bootstrap capabilities, failed factories and subsequent success.
   It does not supply authenticated multi-party network operations.
   Default `RpcSystem` now shares hosted registration/shutdown ownership outside
   the protocol driver. Registration reserves the transport ID before callbacks,
   separates live lookup from final IO completion, and uses an owned guard to
   retire canceled tasks, even before first poll. Old registrations cannot remove
   a replacement using the same transport ID. Multiple shutdown observers share
   completion and the first failure; cancellation cannot leave the count stuck
   or claim that IO closed successfully. Registry borrows end before transport
   callbacks and observer wakeups. Bootstrap, accepted connections, Join and
   third-party adoption all use it, while still constructing the hosted legacy
   protocol driver. This hosted registry does not change portable dependencies.
   Portable coordinators now share that registry through
   `coordinator::network::Network`. Consuming a coordinator with `register`
   returns a `Registered` owner exposing only managed stream/resource run APIs;
   duplicate or closed registration returns the original coordinator for retry.
   Trusted connection IDs index weak bootstrap handles. Disconnectors stop
   command admission and wake parked IO, then wait through drain, close and
   protocol cleanup. Lookup detaches before drain, while registration completion
   remains owned; stale completion cannot remove a replacement connection.
   Early owner/future drop reports cancellation, including after detachment.
   Network drop requests orderly shutdown but still needs caller polling and
   caller deadlines. Stop is checked before reserving fresh input, allowing a
   full pool to drain queued output during shutdown. Regressions exercise 64 KiB
   pipelined RPC, partial-input cancellation, blocked close, errors, ID reuse and
   real Unix resources.
   Existing message-oriented VatNetwork connections now adapt through
   `Coordinator::register_vat`: transport-supplied identity and peer, bounded
   original-segment input, immutable frame output in the existing transport FIFO,
   descriptor ownership, write receipts and independent writer failure monitoring.
   The network's own driver still needs concurrent polling. No capability
   pointer is rebuilt across this boundary. Unfinished receives stay alive
   through drain so receive cancellation cannot prematurely close both halves.
   TCP interoperation and Unix resource tests exercise the adapter. Custom
   transports implement `IncomingMessage::get_segments` and
   `Connection::send_frame`; their defaults preserve existing driver compatibility.
   Allocated output frames retain immutable pool buffers through physical write
   completion. Hosted parameters and replies retain their bounded input reader
   and exact capability receipts; fixed borrowed storage keeps the bounded copy
   path. Shared owners pin their charged buffer against reuse, including after
   the driver scope ends. Receive reservations publish only the prefix committed
   by IO; control builders clear their exposed prefix. Neither can seal unused
   capacity. These changes
   do not yet establish benchmark acceptance.
   The hosted API now separates configuration, execution and live policy.
   `portable::RpcSystem` is no longer a Future. Transfer systems into a bounded
   `portable::Runtime` with `runtime.handle().add(system)` and drive related
   application work with `runtime.run(application)`. A turn progresses ready
   systems and drains ready application continuations, then polls physical output.
   Buffered protocol input resumes in the next turn, allowing earlier output to
   overlap work at the next service. All turns in one poll share a 32-round
   fairness bound. Ready work produced by output starts the next turn directly,
   sharing the same fairness allowance. Connections in a service chain can share
   this scheduling domain. Application, protocol and output readiness are tracked
   independently, so a wake does not poll unrelated connections. Application
   completion returns immediately and is not
   an implicit transport drain; await disconnectors inside the application for
   orderly shutdown. Dropping the runtime aborts outstanding systems.
   `system.run_with(application)` provides the single-system form;
   `system.run()` drives a system whose application is externally scheduled.
   `system.control()` supplies live limits and trace settings independently of
   the runtime's exclusive ownership. Old Future and policy-setter APIs are
   removed, without compatibility adapters.

   `VatNetwork::take_output_driver` explicitly transfers physical output ownership.
   A transferred network observes completion during lifecycle progress without
   polling the writer there. TCP and Unix networks support this split; native
   authenticated session transports retain their independent transport owners.
   In-memory transports have no physical writer to transfer. This split preserves
   physical completion, failure and shutdown accounting and does not relax the
   fixed-storage output contract: pending flushes complete before further writes.
   An owning writer can explicitly permit bounded queue admission during a pending
   flush through `WriteResources::can_write_while_flushing`. The hosted adapter
   retains accepted bytes, attachments and admission credit until physical
   completion, and admits at most 64 outstanding frames, allowing control traffic
   alongside application bursts within the existing byte/credit budgets. This lets
   ready protocol work extend the transport batch without waiting for a previous frame's write.
   Flush and idle still certify completion of every accepted frame; queue
   admission alone cannot complete shutdown.
   Calls and pipeline work use their own transactional capacity checks instead
   of treating an unfinished flush as an admission barrier. An actual blocked
   attempt is retried when physical completion releases retained buffers.
   Publication distinguishes a blocked attempt from an inapplicable direct path
   and from failure. Temporary buffer/queue exhaustion retains the original call
   for retry, without falling through to another allocating admission path or
   disconnecting the peer.
   A blocked attempt waits only while accepted output can release storage.
   Without that completion fence, capacity failure remains an error; an
   impossible allocation cannot leave a call asleep with no wakeup source.

   Inbound application content is now checked on access through its original
   bounded reader. Admission still validates the RPC envelope and capability
   descriptors transactionally, but no longer walks the entire application graph.
   `SharedReader` pins an immutable arena and retains selected pointers with the
   same traversal allowance and remaining nesting depth. Reopening or cloning a
   view cannot reset that allowance. Capability lookup uses the admitted receipt's
   exact descriptor table. Invalid unused application fields can be admitted and
   fail when accessed; malformed envelopes and descriptors still fail admission.
   The former bytes-plus-options retain API and unlimited admitted-reader mode
   are removed. Fixed storage shares an explicit `ReadBudget` across reopened
   views and keeps its bounded copy path.

   Protocol questions bind their generation explicitly to the local exchange
   slot. Wire IDs resolve through the authoritative question table; they never
   size adapter storage. Pending and returned exchanges verify that binding
   before access, rather than relying on matching free-list positions or scanning
   unrelated entries. Adapter ownership pins the generation until response
   transfer or release. Protocol obligations and application references retain
   their separate retirement rules.

   The opt-in `portable::RpcSystem` now owns the connection tasks and automatic
   VatNetwork dialing/acceptance. Each task owns growing storage and runs the
   portable coordinator; ordinary generated ClientHooks remain the public API.
   Explicit options bound connections, tables, calls, hooks, messages and retained
   bytes. Connection limits count constructing and draining tasks while permitting
   reuse of a live transport identity. A weak bootstrap handle admits calls even
   before the first system poll, and protected methods use the system executor.
   Peer factories receive the captured transport peer, without requiring Clone.
   Shutdown observers wait for drain, physical close and protocol storage teardown;
   caller deadlines can abort an already-started drain. Network completion waits
   for registration cleanup rather than canceling tasks before close receipts are
   consumed. Accept-loop failure also retires connections and terminates the system
   with its original error, without disrupting a drain already started by the
   caller. Regressions cover cancellation before first poll, reentrant shutdown,
   capacity, duplicate dial/accept, close faults, TCP legacy-driver interoperability
   and real Unix descriptor transfers through bootstrap/Call/Return/Resolve.
   Live incoming flow limits now use an allocation-free affine charge counter.
   Calls release credit on Return publication, independently of retained pipelines;
   policy changes wake paused readers. Incoming backpressure pauses all messages
   and may deadlock cyclic calls; zero requires a later limit increase.
   Opt-in trace encoding runs once before sizing each exception and never echoes
   remote traces automatically. Idle notifications require empty protocol references,
   drained work and flushed output; reuse marks the transport active immediately.
   Retained ClientHooks preserve terminal error reasons after driver teardown.
   Live outgoing admission uses a shared engine counter with affine reservations.
   Unsent remote requests reserve immediately; unresolved destinations reserve
   when routed remotely. Rejection fails one Call without disconnecting. Credits
   remain attached to requests/responses/pipelines, question records and physical
   writes, so cancellation and late Return cannot bypass the limit.
   Diagnostics use optional record-lifetime counters, including retired owners
   awaiting cleanup, rather than cached snapshots from a previous driver poll.
   Observers cannot retain branded storage or protocol owners. The no-alloc
   profile has no observer fields or allocations.
   Authenticated independent-path Join now uses the captured network context
   through bounded control commands. Provide/Accept retain authenticated
   rendezvous and embargo ownership in the portable answer ledger. Finish
   unregisters provisions and cancels unmatched Accept waiters. Membrane batch
   authorization remains mandatory; a returned direct capability survives relay
   disconnection. Input adapters can report validated frame sizes before pool
   reservation, bounding receive capacity before IO. Receive-only reservations
   avoid redundant clears while retaining framing, size and ownership checks.
   Automatic introductions now publish Call/Return/Resolve descriptors with
   exact export grants. Forwarding vines retain their Provide and order acceptance
   markers after older calls have been dispatched. Incoming answer adoption
   authenticates the cross-connection rendezvous before exposing results or new
   pipelines; used pipeline routes remain frozen through the old connection.
   Setup deadlines end at authentication, while method execution can continue.
   Outgoing third-party tail calls now introduce the original caller to the
   callee through exact authenticated connections. One service execution is shared
   by the original and adopted answer, with independent cancellation and retained
   pipelines; loss of the relay does not cancel the direct recipient's work. Local
   destinations adopt without a wire question. Queued invocation templates retain
   destination metadata across output retries. Previously exposed pipeline routes
   stay on the relay; copying a direct result preserves its original hosted owner.
   Bootstrap completion also resolves queued clients from the driver, so forwarding
   no longer requires application polling of a reply promise.
   Ordinary successful replies with no capability descriptors now use
   `noFinishNeeded`, matching C++ cleanup. FIFO acceptance commits retirement;
   rejected publication leaves the answer intact. Already-admitted pipeline
   leases retain their original generation. Exceptions, redirects, provisions
   and callee-selected answer IDs keep explicit Finish. This avoids a separate
   cleanup packet for data-only round trips.
   Remaining integration: bilateral Join (including fences to migrate previously
   used pipelines), native tail transfer and the
   high-ID/no-Return pipeline-only optimization before switching default `RpcSystem`.
   Caller-supplied endpoint metadata preserves hosted errors and their reasons;
   wire-created broken routes still retain only an ErrorKind. Full wire promise
   diagnostics need a retained error owner. Direct remote call exceptions
   preserve their reason/type/details.
6. [ ] Qualify and document the full profiles. Require allocator-free RPC linking,
   bounded failure/teardown checks, hosted regression suites and comparable
   release benchmarks before declaring this migration complete.

Acceptance requires a final bare-metal fixture linked without a global allocator,
exercising generated RPC dispatch, call/return, cancellation and fixed-buffer
building, as well as isolated compile checks. Exercise engine construction and
shutdown too; allocating once during initialization is not strict no-`alloc`.
Use a separate allocating fixture to avoid feature unification masking an
accidental dependency. Host tests must count allocations around successful,
malformed-input, capacity-failure, pipeline and third-party cleanup operations
while excluding harness setup. Test fixed providers both with and without the
core's `alloc` feature enabled by another consumer. Compile-fail checks should
cover incompatible lifetimes, mutation while a view is borrowed and double reply
completion. Run pointer changes through Miri, fuzz targets and C++ interop tests.

Exhaust every resource independently at its limit and one beyond it. Check
rollback, full-queue cancellation/disconnect, delayed writes, stale generations,
large peer IDs, timer overflow/backwards time and completion races. Extend the
Kani resource/ownership proofs and TLA+ lifecycle models with Rust trace replay
to cover slot reuse and control-reserve progress. Liveness claims must state fair polling and transport
progress assumptions. Default hosted tests must instantiate the same engine;
passing a separate synthetic embedded test is insufficient acceptance evidence.

Measure release-mode latency and throughput at 0 B, 64 B, 1 KiB and 64 KiB, plus
multi-segment traversal, allocation counts, static/stack memory and code size.
Allocation-free construction may improve predictability and reuse; `no_std`
alone is not a speed optimization. The current hosted acceptance baseline is
matched plaintext TCP at no more than 1.15× C++ latency at every canonical
payload size. The [local TCP comparison](Quality-and-Benchmarks.md#tcp-default-and-vectored-io)
passes that latency gate; it does not qualify the unfinished allocation-free RPC
profile. Retain comparable hosted benchmarks throughout this work.
