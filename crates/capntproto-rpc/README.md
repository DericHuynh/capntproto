# capntproto-rpc

Capability RPC, pipelining, flow control, cancellation and multiparty protocol machinery. This is a maintained first-party workspace crate derived from
[`capnp-rpc`](https://github.com/capnproto/capnproto-rust), with the original MIT
[license](LICENSE) and [upstream changelog](CHANGELOG.md) preserved.
The upstream starting point and source hashes are recorded in
[provenance](../../vendor/provenance/capnp-rpc-revision.json).

The Cargo package is `capntproto-rpc`. Its Rust library name remains `capnp_rpc`
to preserve compatibility with generated bindings. Use a package alias when
selecting it directly; the root `capntproto` crate already selects this coordinated set.
Standard two-party streams default to ordinary tail reply forwarding for
compatibility with upstream Rust. Known C++/capntproto peers can opt into
`VatNetwork::set_tail_call_optimization(true)`. The independent
[interoperability matrix](../../docs/wiki/Interoperability.md) records the
tested versions, scenarios and upstream limitations.
All maintained crates share the root lockfile and are tested with
`cargo nextest run --workspace`. See the [workspace policy](../../docs/wiki/Fork-Policy.md).

Disable default features to use the `no_std`, no-allocator profile:

```toml
capnp-rpc = { package = "capntproto-rpc", path = "crates/capntproto-rpc", default-features = false }
```

- `capnp_rpc::bounded` exposes caller-backed connections, typed sessions,
  pipelining, streaming acknowledgment tracking and network protocol ownership.
  Its `FixedStorage` bundle initializes ledger and dispatch arrays with explicit
  capacities; the application separately supplies message buffers and IO.
  `Session::drive()` registers one caller-polled driver. Its `poll_with` boundary
  lends the connection for one synchronous poll; application calls and resource
  release wake it after mutable access ends. The application chooses the executor
  and continues to poll IO, maintenance and completion bindings.
- `capnp_rpc::engine` supplies fixed resources, protocol transitions, caller-polled
  IO and completion slots. The optional `alloc` feature adds heap-owned replies
  and fallible graph/ledger growth with explicit per-resource maxima.
  `FixedStorage` with zero graph and ledger arrays can bind these growing
  resources. `Driver::new_growing` attaches dispatch, pipeline, import/export,
  loopback, embargo and request tracking tables with independent limits.
  `Invocations::new_growing` grows reply bindings around caller-owned completion
  slots. `GrowingBuffers` and `with_growing_messages` add stable message storage
  with independent buffer-count, per-message size and retained-byte limits.
  `Queue::new_growing` preserves FIFO order across growth and partial writes.
  `Domain::bind_growing_adoptions` also grows callee-selected questions under an
  independent entry-count limit. `Session::call_owned` and
  `streaming_call_owned` allocate reply slots and retain the same tracked cleanup
  as caller-backed calls, including replies abandoned after delivery. These
  replies still belong to their connection and message scope.
  Fixed providers never fall back to allocation.
- The default `std` feature enables the existing owned clients, server hooks,
  futures integration and two-party transport adapters.
  `capnp_rpc::scoped::ClientHooks` bridges owned generated clients to live or
  frozen routes in a scoped engine graph. It bounds outstanding calls, admits them to
  the shared ordered dispatcher, preserves early pipelines and capability
  payloads, and wakes the caller on cancellation or disconnection. Hook lookup
  checks private identity and keeps policy wrappers opaque. Branded graph
  clients stay inside the scope; only inbox handles can outlive it.
  `export_client` follows promise resolution; `export_snapshot` freezes the
  advertised route. `collect` publishes readiness after resolution and ordering
  barriers, and pending waiters observe terminal shutdown.
  `enqueue_wire` admits owned requests through the portable driver's tracked
  `Invocations`, preserving capability indices, cancellation and early pipelines.
  Poll each `WireCall`'s projections before output/dispatch, then poll invocation
  bindings and its completion alongside protocol maintenance. Null capability
  entries are retained; foreign hooks use a caller-provided import factory.
  `Call::forward` moves the original params/results hooks into a generated server.
  `prepare_received_server` and `prepare_local_server` retain admitted parameter
  capabilities in an owned snapshot; `Prepared::dispatch` forwards method hints
  and the caller's original result context to existing servers. Redirected calls
  still require result completion even when the sender requests only a pipeline.
  Invalid projections from completed results become broken capabilities.
  The default `RpcSystem` still uses the hosted connection implementation.
  Hosted adapters can opt into per-message growth with
  `GrowingStorage::uniform(initial, maximum).with_messages(capabilities, path)`.
  Capability budgets include the target receipt of an incoming Call; path budgets
  count non-noop operations. Const capacities remain inline storage, and zero is
  supported. Fixed constructors remain allocation-free when `alloc` is enabled.
  `Prepared::execute` accepts a `server::Results` producer and the caller's
  executor. Poll its `Execution`, then use `collect_server_results` and
  `Driver::complete_answer` to retain completed replies in the portable ledger.
  `send_answer` (or `send_answer_with_attachments`) publishes those results;
  output rejection retries without rerunning the service.
  Use `poll_maintenance_with` when a producer supplies `Results::pipeline`: it
  prioritizes the explicit early pipeline, including after final failure. Retain
  projected foreign hooks with their policy wrappers. `Results::write_exception`
  preserves error reasons and structured details in Return publication.
  Dropping execution interest respects generated cancellation policy through the
  supplied executor. Generic tail forwarding preserves child pipelines; native
  same-connection tail transfer uses the same bounded question/answer ledgers.
  `server::Worker` now coordinates incoming Call admission, owned service
  futures, cancellation, result collection, Return retries and explicit teardown.
  It bounds active work independently of protocol tables and polls pending
  methods fairly. `Results::publication_bytes` sizes payloads, capability
  descriptors and structured errors before output reservation. Use
  `ClientHooks::server_pipeline` in the driver's maintenance resolver to retain
  the original early-pipeline hooks. Resource-aware publication uses the same
  worker state machine. `wire::Calls` coordinates owned outgoing calls through
  the same invocation ledger, including local service execution, caller
  cancellation, early pipelines, completion delivery and resource-bearing output.
  Admission rejection retains the oldest call; bounded rotating polls allow
  cleanup and ready methods to progress while another operation is blocked.
  Local early pipelines survive a parent exception and late projections, and
  local exceptions retain their reason and details. Independently executed
  protected methods wake the coordinator when they publish an early pipeline.
  Outgoing frames size descriptor tables and promised-answer paths before
  reservation. Terminate both workers before terminating the portable driver.
  The caller still drives IO, maintenance and authenticated network operations.
  `wire::foreign::ForeignPromises` supplies the foreign import callback for
  pending owned hooks. Poll it after graph reclamation and before calls and
  maintenance. Its bounded observers hold weak generational graph identities,
  follow resolution chains fairly, register wakes, and retain completed failures
  across graph-capacity rejection. Calls retain their original proxy even after
  successful settlement. A callable local promise still exports as senderPromise
  until settled; failed endpoints publish an exception and retain their hosted
  diagnostics. Use `export_client_with_metadata(..., foreign::metadata)` for
  direct hosted exports and `collect_wire` with `foreign::local_metadata` for
  local diagnostics plus the application's imported-resource projection. Stop
  observers with `terminate` before terminal driver teardown.
  `wire::coordinator::Coordinator` combines these adapters, maintenance and
  full-duplex byte IO in one caller-polled connection. Its `run` method consumes
  the driver and takes the message pool, FIFO, IO adapters, poll budget and stop
  future from the caller. `Control` supplies bootstrap policy and authenticated
  network work and releases any detached owners on shutdown. The coordinator
  reserves output capacity before polling publishing lanes, retains Busy input
  for ordered retry, and distinguishes bounded scan yields from application
  notifications so idle connections sleep without losing wakes. Normal EOF,
  stop and failures reclaim the pending receive buffer and drain protocol state.
  Dropping `run` before completion requires ending its entire message-pool scope.
  Hard pool/graph capacity exhaustion is reported; configure these limits for
  the accepted concurrency and maximum frame size. `run_transport` accepts
  an `Input` that retains each frame's ancillary resources through admission
  retries, an `Output` policy, and a resource-owning FIFO writer. Byte-only
  `run` uses that same loop with `NoResources`. `FileDescriptors<N>` attaches
  Unix descriptors to Calls, Returns, Bootstrap replies and Resolve messages;
  `capntproto::unix_rpc::{ResourceReader, ResourceWriter}` supplies socket IO.
  Imported metadata is copied before dispatch, response delivery or pipeline
  notification. Unclaimed input resources close on admission and every terminal
  path even when the caller retains the reader.
  `Coordinator::bootstrap` publishes a request and returns a capability on which
  calls can pipeline immediately. `Coordinator::handle` provides a cloneable
  weak handle for typed bootstrap requests while `run` owns the driver. Its
  command queue shares the configured concurrency bound; abandoned commands
  never publish, and shutdown rejects pending commands without extending the
  connection's lifetime. `coordinator::TwoParty` runs a caller-supplied bootstrap
  factory once per admitted request, retains exceptions (including details),
  and retries publication after output pressure. Capture the transport's
  authenticated identity in that factory for peer-specific access policy.
  This handler explicitly rejects multi-party network controls.
  Standalone `Calls`, `Worker` and `ForeignPromises` polls return `Progress::Yield`
  when their budget expires; their caller schedules the next poll. Pending means
  a completed idle scan with application wakeups registered.
  Default migration still requires authenticated multi-party network lifecycle,
  default VatNetwork task creation, idle notifications and complete limit policies.
  Pipeline-only calls without parameter capabilities use separately bounded
  high-ID questions and omit Return. Their stable remote projections retain the
  question until Finish. Published high IDs never wrap or get reused; exhaustion,
  disabled/full high-ID storage, capability parameters or an unexpected Return
  select ordinary questions. Low-level ledger storage exposes the separate
  `pipeline_questions` budget; `FixedStorage` includes it automatically.

The application supplies buffers, IO, scheduling, time and authenticated network
services. The portable target dependency graph contains only the core, engine
and wire crates. Host-side schema generation remains a build dependency.
The hosted ownership migration is still in progress; enabling this profile does
not make the root transport crate or every service allocation-free.

Optional transport resources use `capntproto_rpc_wire::attachments::Receiver` and
`Driver::receive_with_receiver`. The caller supplies the exact message's resource
slots. Rejection leaves them intact; acceptance delivers each index at most once
to its admitted import, including existing aliases. Missing indices are ignored.
The resource receiver adds no allocation; fixed admission remains allocation-free.
Ordinary receive factories discard optional metadata.
The hosted `scoped::wire::resources::Import` keeps the first Unix descriptor;
`ClientHooks::collect_wire` refreshes owned hooks from retained routes before
handing them to application code and on each driver turn. This covers receive
ownership; default RpcSystem integration remains open.

Outgoing resources use `attachments::Sender<R, N>` and
`messages::Queue::with_resources` (or `growing_with_resources`). Reserve the
bundle with its exact message before committing grants. Unused reservations
release their resources; accepted entries retain them until completion or
terminal cleanup after IO stops. `io::poll_queue_resources` takes a caller's
`WriteResources` adapter. Pending and Interrupted transfer nothing; positive
progress transfers resources with the first byte, and all later writes use the
same send cursor without resending them. Byte-only `poll_queue` accepts only
queues without resources.

Portable writers explicitly implement `io::Flush`; unbuffered adapters return
ready, while buffered adapters forward real flush completion. `io::Output`
tracks accepted writes and flushes even partial frames before parking, without
allocating or reoffering attachments. Queue receipts mean byte acceptance;
output-pump completion additionally means the transport buffer has flushed.
The futures adapter forwards flush and close. The Unix resource adapter closes
only the write half.

The hosted coordinator's `run`/`run_transport` retain immediate stop semantics.
Use `run_stream`/`run_connection` for orderly shutdown: stop new work, release
unclaimed input resources, drain accepted output, flush, then close the write
half. Supply a separate abort future for an immediate stop or shutdown deadline;
it stays active during blocked output and close. Protocol storage is reclaimed
on success and failure. On an error or abort, the caller drops the IO; the
coordinator makes only one best-effort Abort attempt for a protocol error on a
live connection; it does not wait for a blocked peer or attempt close. Closure confirms
local transport completion, not peer receipt or execution of outstanding calls.
For a task-owned network lifecycle, use the opt-in `portable::RpcSystem` described
below; the default `RpcSystem` still uses the legacy protocol driver.

Default `RpcSystem` registration and shutdown bookkeeping now live in a shared
hosted registry outside the protocol driver. Transport identities are reserved
before constructing drivers; reentrant registration of an initializing identity
returns Overloaded. A registration owns shutdown completion even after its
connection is detached from lookup. Reusing an identity cannot let an older
completion remove the replacement. Multiple `Disconnector` futures all observe
completion and the first shutdown error. Dropping the system before its tasks
finish reports Disconnected to those observers, including when it was never
polled. This registry uses hosted storage; it does not impose `std` or allocation
on the portable engine. The default still registers the legacy protocol driver.

Portable coordinators now register with `coordinator::network::Network`, which
uses that same registry implementation. Create a network, configure a coordinator
in its pool scope, then consume it with `coordinator.register(&network, id)`.
The ID is supplied by the trusted transport adapter, not by peer wire data.
`network.connection(id)` returns its weak bootstrap handle. Duplicate/closed
registration returns a boxed `Rejected` containing the unchanged coordinator
for retry or cleanup; it never replaces a live driver.

The resulting `Registered` owner exposes only `run_stream` and `run_connection`.
This prevents using immediate-stop APIs where a network expects successful IO
closure. Poll every registered connection while awaiting the network's
`get_disconnector()`. Shutdown stops command admission, wakes parked input,
detaches lookup, drains/flushes/closes output and releases protocol storage
before completing observers. Network drop requests that same orderly shutdown;
it does not run IO. Caller abort/deadline futures stay active during blocked
shutdown, and stop is checked before reserving fresh input so a full pool can
still drain its queued output. Early owner/future drop reports cancellation and
still requires ending its entire pool scope. Protocol/deadline errors retain their reason in network
completion; IO failures report their stage there while the caller receives the
original typed `RunError`.

Existing `VatNetwork` connections can now enter that same coordinator with
`coordinator.register_vat(&network, connection)`. Its `VatConnection` owner
captures the transport's connection ID and authenticated peer, exposes a weak
handle, and provides a managed `run` method. Drive the original network alongside
it. A `TwoParty` factory may capture `peer()` for bootstrap authorization;
multi-party transports still need their authenticated control handler.

The adapter borrows original incoming segments and synthesizes their framing
header while copying directly into the portable message pool. Outgoing
`OutgoingFrame` messages own a copy of the complete serialized segments, queued
in the same FIFO as existing builder messages. Neither direction rebuilds
capability pointers or descriptor tables. TCP (including TLS/QUIC byte-stream
adapters), native routes and Unix transports implement this boundary. Custom
transports opt in with `IncomingMessage::get_segments` and
`Connection::send_frame`; the existing driver remains compatible with their
defaults. No new requirements reach the no_std/no-alloc crates.

Input is bounded by the coordinator's full-frame capacity as well as the
transport's own receive limit. An unfinished receive stays alive through output
drain because canceling it closes both halves on some existing transports;
its bounded prefetched resources are released when the adapter drops. Completed
input resources are still consumed once and released at admission. Output
acceptance owns a transport copy, flush awaits the write receipt, and close is
a separate fence. Independent writer failure observation remains active during
parked input and shutdown. Tests exercise large TCP messages and capability
promises against the default driver, and real Unix descriptor transfers.
Two-party connections report the remote side and their shutdown promise waits
for physical close. Adopted Returns wait for the local answer's completion,
then retain its frozen clients and frame before releasing adoption ownership.
After real progress, the coordinator requires a fresh lane scan before sleeping;
a partial scan started earlier cannot conceal a newly forwarded call.

`portable::RpcSystem` now owns this integration. Construct it with an existing
`VatNetwork`, a bootstrap capability (or peer-specific `BootstrapFactory`) and
explicit `portable::Options`. It drives the network, automatically dials and
accepts connections, reuses transport identities and owns the portable driver's
growing message/table storage inside each task. The generated client/server API
is unchanged. Bootstrap admission works before the system's first poll; protected
server methods run on its call executor. Weak bootstrappers and client handles
do not keep the network or protocol storage alive.

`Options::bounded` takes the maximum connection count, entries per table/queue,
framed message bytes and retained message-buffer bytes. Public fields allow
separate budgets for hooks, calls, capability descriptors, pipeline paths and
output. These bound runtime storage, not memory allocated by application service
futures or requests. Connection limits include construction and draining tasks;
an existing connection remains reusable at the limit. Capacity exhaustion is
explicit and never switches to a different protocol driver.

Poll the system concurrently with client calls and its `get_disconnector()`.
The disconnector stops admission and waits for output drain, physical close and
storage teardown. Its `abort(reason)` can interrupt an already-blocked graceful
close when the caller's own deadline expires. Network writer completion also
waits for registered cleanup before stopping the task set. Unexpected network
termination aborts active connections and preserves the shutdown failure. A
failed accept loop also aborts and retires its connections before failing the
system future; it cannot leave the system pending with no functioning listener.
An accept error after orderly shutdown begins does not interrupt output drain. A
disconnector does not stop a persistent listener; the caller owns the network's
listener policy and can drop the system after connection cleanup.

Live `set_flow_limit` pauses input after an incoming Call crosses the word limit;
Return publication releases its charge even if a pipeline retains the results.
Paused readers resume strictly below the limit, so a zero limit needs a later
increase. This is backpressure, not a hard memory bound: the crossing message is
admitted, and pausing all input can deadlock cyclic calls. The counter is shared
with the allocation-free engine; the hosted adapter supplies wakeups.
`set_trace_encoder` and `clear_trace_encoder` control exception traces on existing
and future connections. Encoding runs once before sizing a failed result and
never automatically echoes an upstream remote trace. Transport idle notifications
require empty protocol references, drained work and flushed output. Reusing an
idle connection marks it active synchronously. Terminal errors preserve their
reason in retained ClientHooks after connection storage is reclaimed.

Live `set_outgoing_call_limit` uses the same engine counter as the legacy driver.
Remote unsent requests reserve a slot immediately; unresolved/local calls reserve
only when routed remotely. Zero rejects new Calls with Overloaded, without closing
the connection or revoking accepted work. A reservation survives retained
responses/pipelines, canceled questions awaiting late Return/Finish, and physical
transport writes. Bootstrap and protocol controls do not consume Call slots.
`diagnostics().snapshot()` reports live record occupancy, incoming words,
outgoing reservations and retained response contexts, sorted by connection ID.
Observers retain only counters and a weak registry. Optional table observation
allocates once per observed table; the fixed no-alloc profiles are unchanged.

`portable::RpcSystem::get_joiner()` joins independent authenticated paths through
the network's `JoinNetwork` profile. It preserves membrane batch authorization,
cancellation, and response guards until the caller acquires the direct capability.
Provide/Accept use the same connection's authenticated rendezvous callbacks;
Accept pipelines wait for the embargo and Finish unregisters the provision.
Control commands are bounded through queued work, active requests, and retained
replies. Control polling rotates between incoming work and completion lanes.

Automatic introductions publish third-party descriptors in Call, Return and
Resolve through the same transactional export ledger. Bounded forwarding vines
own their Provide until the recipient releases the proxy. Acceptance markers
follow previously dispatched calls; they do not wait for method completion.
Network callbacks authenticate contact/recipient metadata, and opaque membrane
hooks remain proxies unless their public resolution exposes an eligible route.

Incoming ThirdPartyAnswer and awaitFromThirdParty rendezvous support either arrival
order, early Return, setup deadlines, cancellation and retained result capabilities.
Unused pipelines switch after authentication, before Return; previously used
paths keep their original ordered route, including repeated projections.

Automatic third-party tail calls now use authenticated introductions to route
results directly to the original caller. The original and adopted answers share
one execution, with independent cancellation and pipeline ownership. The callee
continues for its direct recipient if the relay disconnects; result capabilities
retain their originating connection instead of the introducer. Local destinations
use the same authenticated rendezvous without allocating a wire question. Metadata
and queued/active transfer work are bounded. Transfer publication and adopted
execution share round-robin scheduling with other RPC controls. Calls denied the handoff permission,
unresolved targets, opaque request wrappers, and unsupported networks keep
ordinary forwarding.

The portable system remains opt-in. It supports bilateral Join, native
same-connection tail transfer, and authenticated Join fences for previously
used pipelines. Unsupported or unsuccessful fences retain the ordered original
route. Unknown and obsolete messages receive `Unimplemented`; rejecting optional
Join fails that operation without disconnecting the connection. Protocol errors
attempt an Abort before teardown without waiting on a blocked peer.

The legacy-path regression matrix now covers both drivers:

| Protocol path | Shared regression scenarios |
| --- | --- |
| Bootstrap and obsolete named exports | `bootstrap_factory` |
| Call/Return, scalar and capability payloads | `scoped_vat`, `composed_wire_boundary` |
| Exception details, cancellation, protected execution | `cancellation`, `static_cancellation`, `exception_metadata` |
| Finish, Release, ID reuse and high-ID no-Return hints | `call_hints`, `tail_transfer` |
| Multiple parameter grants, duplicate IDs, refunds and retained peer ownership | `call_hints`; bounded wire tests also reclaim every request buffer before Return |
| Native tail transfer, dependent calls, local resolution | `tail_transfer` (both drivers replay the same traces) |
| Flow credit, concurrency and streaming | `flow_control`, `streaming`, `allocations` |
| Resolve, promise ordering and direct Call/Return/Resolve reflection | `join`, `scoped_vat`, `multiparty_join`, bounded wire tests |
| Provide, Accept, embargoes and third-party answers | `multiparty_join`, `answer_adoption` |
| Bilateral Join, malformed parts/results, relay lifetimes | `join` (both drivers replay the same traces) |
| Authenticated pipeline migration before Return | `answer_adoption` (all eight driver combinations) |
| Opaque membranes and Join delegation | `membrane_join`, `join` |
| Unknown/obsolete messages and optional rejection | `join`, bounded wire tests |
| TLS, QUIC, Unix resources and disconnect cleanup | `secure_rpc`, `scoped_unix`, `unix_facade` |

The bounded driver keeps caller-owned storage and explicit capacity errors;
these are intentional differences from the allocating legacy driver. The hosted
message bridge still copies frames. Published generic calls retain their export
IDs and Return policy independently of those frames, like legacy. Multiple grants
use the configured inline capacity or bounded growth; the output queue owns the
serialized request until physical output completes. Private request builders whose
capability tables fit the wire budget preserve their descriptor indices without
rebuilding the payload. Settled remote calls with native or already-published
parameter owners write directly into the final output frame, preserving null
slots, duplicate grants and same-peer reflections. First-time foreign owners,
unsettled targets and older queued work keep ordinary queued dispatch; retries
do not create speculative promise observers. Opaque parameters and oversized
sparse tables still use bounded collection. Functional trace parity does not
establish instruction, allocation, or throughput parity; benchmark the portable path
explicitly before changing the default driver.

The owning driver's import entries hold weak graph links, like legacy. Binding an
existing import checks its ledger slot and graph generation directly, without a
second table or a linear search. Re-admitting an import clears the old link;
failed admission preserves it. Links retain neither graph clients nor wire grants.
Custom driver `Storage` no longer takes an `imports` binding array, and driver
`GrowingStorage` no longer has a separate import-binding budget. The engine import
ledger and graph node capacities limit imported clients; `FixedStorage` dispatch
slots do not. Import entries carry additional weak-link metadata, so size custom
storage pools for the new layout. Independently branded low-level graphs still
use `clients::Imports`; import `clients::ImportBindings` to call its shared
`bind_capability` method.

Hosted requests release their original parameter builders and capability hooks
after FIFO publication; pending questions retain their separate protocol
obligations. Ordinary calls with zero or one parameter export retain only the
method, return policy and export grant. Larger grant lists use configured inline
storage or bounded growth without retaining the original request frame. Standalone
`ClientHooks` users can configure `with_message_pool` before exporting clients to reuse transmitted request
segments. Queued frames retain their bytes through partial writes, so live
segments cannot be recycled.

Queued admission consumes capability bindings directly into its retained table.
Typed calls do not allocate an intermediate mapping table. The bounded
`enqueue_dynamic_to` APIs accept an `ExactSizeIterator` in capability-index order,
including null entries; use `table.iter().map(Option::as_ref)` for owned bindings
or `table.iter().copied()` for borrowed bindings. Incorrect iterator lengths
reject the invocation and release its retained prefix before dispatch.

Immediate native calls borrow parameter authority from the hosted registry and
request rather than allocating another retained capability vector. The bounded
`CapabilityTable` interface also accepts caller-owned tables with fallible
lookups; `try_send_immediate_with_capability_table` rolls back staged grants on
failure. Its slice convenience API uses the same path. Native hook recognition
checks the private bridge identity; opaque and foreign hooks keep ordinary
policy-preserving admission. Imported reflection IDs are copied once from their
immutable endpoints, while promised-answer transforms stay behind the final
exclusive publication ticket.

Queued native calls use the same table through `enqueue_dynamic_to_table`,
retaining each parameter directly into the invocation's final storage. Encoding,
lookup and dispatch rejection release its complete retained prefix and frame;
tracked admission also preserves the caller's completion sender. Native
eligibility is only a hint: the actual borrow checks the current registry entry
against the private endpoint identity, including after slot removal or reuse.

Native pipeline projections retain their endpoint inline, including clones and
cached direct routes. Opaque projection hooks preserve their original wrapper.
Completing a pipeline retains another hook only when waiting projections need
delivery; its terminal state already owns the completed pipeline. Public
pipeline interests still control parent cancellation and publication ordering.

Local forwarding retains immutable parameter and result readers with their
original traversal budgets. Capability hooks are bound separately and keep their
exact indices, including unused and null slots. Readers remain valid after the
task, driver and allocating message pool are released. Fixed message backing
uses an owned copy because its storage cannot escape the caller's scope.
Network-control replies and their completed pipelines share this same immutable
payload rather than constructing a second message for pipeline readers.

Server-result collection borrows native capability bindings directly into the
final local result table. Foreign hooks keep policy-preserving normalization.
The bounded `local_result_dynamic_table`, `local_answer_dynamic_table` and
`local_answer_prepared_table` APIs accept `CapabilityTable` storage. Lookup
failures and inconsistent iterator lengths reclaim the result frame and every
retained prefix client before returning; answer publication remains retryable.

Promises resolved back to local objects expose their original policy hooks only
after graph ordering barriers open and older queued calls begin forwarding.
Resolution does not wait for those calls' replies. The local capabilities remain
usable when their former connection closes.

Repeated capability descriptors reuse one export ID and retain one grant per
descriptor, as in legacy. Export staging checks that identity before retaining
a graph owner; aliases change only the wire grant count. The graph's read-only
`inspect_for_export` view distinguishes the advertised source and its resolution
obligation from its currently callable destination, preserving local promise
observers and reflected-route barriers. Export slots contain their own identity
index; fixed storage allocates no hash map. Pending aliases share one resolution observer:
the first descriptor introduces `senderPromise`, and later aliases use
`senderHosted`. Output rejection rolls back only unpublished grants. Final peer
Release removes the identity before any retained local call lease completes.
The hosted policy recognizes private local and queued clients; opaque wrappers
keep separate graph identities even when they forward public brand/pointer
diagnostics. Custom `Descriptors` policies supply an `identity` callback, or
return `None` to retain graph identity.

Restricted capability scopes can borrow the export index for hosted imports.
Recognized hooks reuse the original published source before allocating a proxy
or subscribing another foreign promise. Lookup retains the exact graph
generation and creates no wire grant; publication still owns grant accounting.
Pending exports record their producer's weak source separately from their
independent forwarding promise so reuse cannot bypass ordering barriers.
Stale or finally released sources take the ordinary import path. Empty indices
skip owner identification. `export_canonical_promise` takes the original source
key explicitly; export slots store that weak key without extending its lifetime.

Use `send_invocation_with_attachments`,
`Invocations::send_one_with_attachments` and
`poll_maintenance_with_attachments` for automatic Call/Resolve descriptors.
Callbacks inspect retained import metadata or the original local server hook;
policy membranes must remain intact. Return publication accepts the same
resource-bearing queue reservations. If retryable Return admission rejects a
frame, its temporary queue reservation is dropped; re-create the optional
transport bundle before retrying. The frame and grants retain their existing
retry ownership. The root crate's `unix_rpc::ResourceWriter` supports Unix
stream IO; use a resource budget of at most 253 descriptors for this transport.
Fixed bundles do not allocate even if another crate enables `alloc`.
