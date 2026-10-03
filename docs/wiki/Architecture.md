# Runtime architecture

For package ownership, dependency direction, and file placement, see the
[repository layout](Repository-Layout.md).

Capn't Proto is an unreleased 0.x protocol/runtime. Breaking Rust APIs and rejecting
obsolete private formats is allowed. Standard Cap'n Proto wire behavior,
capability authority, schema evolution, and C++ interoperability remain contracts.

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
The state cannot be deserialized or constructed from arbitrary fields.
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
The quiche fork exposes `stream_send_acknowledged()`; successful receipt remains
observable after collection, whereas STOP_SENDING and local resets never count.

## Transport transitions and buffers

`rpc::tcp` adapts ordinary TCP connections to the two-party RPC engine.
`rpc::tls` uses rustls/Tokio for TLS 1.3 over TCP, and `rpc::quic` uses
the quiche engine for standard QUIC v1/v2 with TLS 1.3. Both secure transports use
the `capntproto-rpc/1` ALPN, explicit trust roots, and optional mandatory client
certificate verification. Connection setup completes before a bootstrap is
installed; application-specific authorization uses the verified certificate chain.
Bounded concurrent listeners isolate failed and stalled handshakes. Canceling a
listener drops pending handshakes while the common ServerDriver retains accepted
RPC sessions. Each QUIC session uses one client-initiated bidirectional stream;
write shutdown waits for acknowledged bytes/FIN, and dropping the stream closes
its connection even when a diagnostic handle remains. These are two-party
transports; Native-specific multiparty routing remains separate. See
[TCP TLS and QUIC](TCP-TLS-and-QUIC.md) for configuration and tests.

`transport::engine::Engine` owns the quiche connection, native RPC stream gate,
shutdown protocol, scheduling and pending unreliable packet. Its synchronous
`step(now)` advances the production protocol without socket IO or executor waits.
The async adapter performs packet delivery, application reads/writes, pacing and
half-close, then reports completions. Mobility decisions also receive explicit
time. The root dependency enables the fork's `tokio-clock` backend, so quiche
recovery, idle/path timers and packet pacing share Tokio's clock with runtime
scheduling, migration and shutdown. Construct and drive connections within the
same time domain; `SendInfo.at` converts directly to a Tokio deadline. Paused-time
tests control these timers together. The standalone fork retains its OS-clock
default and private deterministic packet-test scope. This clock feature enables
no test utilities or deterministic entropy in production.

In unit tests, a private `DatagramSocket` variant supplies in-memory UDP queues,
readiness, errors and closure to the same async driver. A local executor polls
drivers only when woken; paused Tokio time controls recovery and shutdown waits.
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
| base `reproto` | RPC adapters, authority and semantic helpers |
| `tls` | Certificate configuration, TLS 1.3 over TCP, optional mandatory client certificates |
| `quic` | Standard QUIC v1/v2 RPC streams with quiche; enables `native` and `tls` |
| `native` | Native/quiche transport, listener, arbitration, multiparty routing, provisioning, discovery, NAT rendezvous and path control |
| `services` | Bulk, realtime and schema exchange |
| `storage` | mmap store, typed ORM and persistence |
| `native` + `services` | Capability-authorized realtime datagrams |
| `native` + `storage` | Application object introduction/handoff |
| `services` + `storage` | Durable bulk/ORM publication |
| `reproto-test-support` | Generated fixture schemas and trace-file contract; dev dependency only |

The default enables `native`, `services`, `storage`, `tls`, and `quic`.
Plain TCP works without default features. TLS and QUIC also run their secure RPC
tests independently with `--no-default-features --features tls` or `quic`.
Feature-specific library builds use
`cargo check --no-default-features --features FEATURE --lib`. Fixture modules are
not re-exported from the production package; tests import `reproto_test_support`.
The production build script compiles only schemas used by enabled features.

## Verification workflow

`cargo test --workspace` runs the default native acceptance tests and doctests.
See [Testing](Testing.md)
for targeted checks and external tool requirements.

`test-support/verification/models.json` declares bounded model configurations,
named negative controls, corpus digests, scope and limits. Rust test support runs
TLC, canonicalizes state/transition graphs, checks the graph-bound regression
corpus and supplies it directly to the Rust replay tests. Model checks cache
only matching source/configuration/tool inputs and validated result-log bytes;
Rust replay tests always execute. Trace-only tests are no longer ignored.

New scalar models can use `verification::exploration` to obtain fresh reachable
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
RPC, generator and futures paths. `cargo test --test release isolated_release_qualification -- --ignored --exact` reconstructs and tests the
bundle with fresh outputs. `storage/io.rs` owns private write/sync seams; fault
injection exists only in test builds. The production Store validates independent
record framing before tail repair and stabilizes file and pathname on recovery.
