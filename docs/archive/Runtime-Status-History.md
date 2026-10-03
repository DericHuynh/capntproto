> Historical development record, archived 2026-10-01. Commands, counts,
> feature status and security descriptions below belong to earlier snapshots.
> Use the [current guide](../wiki/Runtime-Status.md) for supported behavior and commands.
> Unbundled local run artifacts are not current verification evidence.

# RPC runtime port status

The workspace now builds a maintained fork of `capnp-rpc` in
`vendor/capnp-rpc`. It ports additional behavior from the pinned C++ runtime,
including standard wire handoff and capability membranes. **Complete C++ RPC
runtime parity has not been reached.** This document separates implemented
behavior from remaining work; the earlier application-level introduction API
continues to exist separately.

## Reference and provenance

* C++ reference: `capnproto` commit
  `0de72d8d8cec6b69edaa29de51d3bd490341f9c2`.
* Core serialization/capability dependency: `capnp` 0.25.6 is now vendored in
  `vendor/capnp`, with original-source hashes in `vendor/provenance/capnp-revision.json`.
  Unix hooks are gated on `std`; the alloc-only build remains supported.
* Rust starting point: `capnp-rpc` 0.25.1, upstream commit
  `61f2c7640516f6d74c4b7d6e67257fae4fff9bda`, MIT licensed.
* `vendor/provenance/capnp-rpc-revision.json` records the original inputs.
* `vendor/capnp-rpc/schema/{rpc,rpc-twoparty,persistent}.capnp` are copied
  from the C++ reference. Bindings are generated at build time. This replaces
  the published Rust crate's older boolean Accept embargo with the current
  byte-string ID, and includes the current ThirdPartyAnswer discriminant.
* The generator maps both `Persistent` and its `persistent` annotation to one
  Rust name. The build script renames only the generated annotation module to
  `persistent_annotation`; schema bytes and wire IDs remain intact.

## Status and release scope

This is an implemented-feature inventory. Historical state/trace counts describe
individual bounded checks and are not a completion percentage. Current executable
evidence comes from the [Cargo acceptance tests](../wiki/Testing.md); older reports in
`reports/reproto` describe historical runs.

* [Release acceptance](../wiki/Release-Acceptance.md): criteria and remaining release blockers.
* [C++ parity checklist](../wiki/Cpp-Parity.md): concrete source-audited implementation/API gaps and optional integrations.
* [Roadmap](../wiki/Roadmap.md): missing features, optional extensions and proof obligations.
* [Architecture](../wiki/Architecture.md): ownership, crate/features and verification workflow.
* [Fork policy](../wiki/Fork-Policy.md): supported inputs and upgrade procedure.
* [EAE comparison](../wiki/EAE-Comparison.md): measured storage differences, integration probe and adoption gaps.

## Feature checklist

`[x]` means implemented and exercised within the documented scope; it does not
mean all schedules, platforms or protocol compositions are verified. `[ ]` means
unfinished. Missing implementation, optional design work and verification gaps
are listed in the [C++ parity checklist](../wiki/Cpp-Parity.md) and [roadmap](../wiki/Roadmap.md). Application services
and the custom Native profile are distinguished from standard RPC wire behavior.
The full KJ tree, C++ schema compiler and C++ command-line tools are outside this
RPC-first port inventory.

### RPC connections, calls and lifetime

- [x] **General byte-stream client/server facade** — owned/borrowed streams,
  listener acceptance and cancellation, owned-only drain, disconnect observers,
  bidirectional bootstrap capabilities, trace encoding and queue metrics.
  Executor-neutral drivers plus generic Tokio helpers. Cargo tests and fresh
  TLC/C++ replay: 185 states, 347 scenarios, 1,455 matching observations and
  six detected model faults. Descriptor-aware transports share this driver.
  [Contract and bounds](Cpp-Parity-History.md#two-party-facade-implementation-checks-2026-09-23).

- [x] **Linux descriptor-aware client/server facade** — owned/scoped borrowed
  Unix sockets, listener acceptance, drain, reverse bootstrap and queue metrics.
  Zero FD limit disables sending and receiving; truncation preserves callable
  capabilities. Partial-read and writer cancellation release descriptors without
  closing a borrowed caller socket. TLC: 433 states / 576 scenarios and 2,160
  matching Rust/C++ observations, with ten detected model faults.
  [Contract and bounds](Cpp-Parity-History.md#descriptor-aware-facade-implementation-checks-2026-09-23).

- [x] **Schema-free struct access** — `any_struct` readers/builders, explicit
  allocation, mutable data/pointer sections, size/equality/canonicalization,
  typed/dynamic views and capability-preserving copies. Writable casts reject
  undersized sections. TLC: 1,196 states / 3,045 edge-prefix traces,
  11,541 matching Rust/C++ observations and seven detected model faults.
  [Contract and bounds](Cpp-Parity-History.md#schema-free-struct-implementation-checks-2026-09-23).

- [x] **Schema-free list access** — `any_list` / `any_struct_list` readers and
  builders, all physical encodings, explicit element allocation, checked casts,
  data-only byte views and capability-preserving copies. Pointer-list projections
  retain complete struct elements across builder/reader conversion; bit/non-bit
  casts are rejected. Cargo tests, fresh TLC replay and pinned C++ comparisons
  include an explicit reproduction of C++'s double-offset projection bug.
  [Contract and bounds](Cpp-Parity-History.md#schema-free-list-implementation-checks-2026-09-23).

- [x] **Structural payload equality** — fallible tri-state comparison of any
  pointers and generated/dynamic struct/list readers, schema-independent
  padding rules, physical list kinds and bitwise scalar contents. Capabilities
  remain unknown without resolving or retaining hooks; definite differences
  dominate. TLC: 3,925 states / 3,924 edge-prefix traces, eight detected model
  faults and 2,773 pinned C++ comparison results.
  [API and verification bounds](../wiki/Cpp-Parity.md#serialization-and-rust-code-generation).

- [x] **Multiple connections per RPC system** — Stable connection IDs, registry, continuous accept
  loop, shutdown of all connections, independent failure handling
- [x] **BootstrapFactory** — Per-request authenticated peer selection on incoming, outgoing and
  introduced connections; local self-bootstrap; exceptions and broken pipelines without disconnect;
  obsolete named-export rejection
- [x] **Disconnect cleanup** — Broken state published before capability destructors and diagnostic
  callbacks, detached question ownership, imports resolved outside table borrows, stale
  ordinary/streaming request rejection, best-effort Abort and shutdown despite body allocation
  failure, cancellation of pending transport reads
- [x] **Connection idleness** — Optional network notification based on all five RPC tables,
  reactivation, clean idle EOF without Abort, live-registry removal before output flush,
  disconnector waits for pending shutdowns
- [x] **Tail calls** — Optimized explicit and automatic-proxy same-connection outgoing transfer,
  local/cross-connection result forwarding, early pipeline forwarding, incoming
  `sendResultsTo.yourself` / `takeFromOtherQuestion`, redirect acknowledgements and exception
  propagation, adopted producer ownership through Finish, late adoption after caller cancellation
- [x] **Independent result pipelines** — Owned `PipelineBuilder<T>`, typed and dynamic
  `set_pipeline_from()`, nested path preservation, forwarded RPC pipelines, membrane policy and
  revocation; queued child calls and resolution observers separate from parent completion.
  TLC: 307 states / 787 Rust trace replays; 3,369 pinned C++ observations
  ([contract and bounds](../wiki/Cpp-Parity.md)).
- [x] **Result construction** — Lazy allocation hints, result reinitialization, scoped
  whole-root orphan adoption in typed and dynamic contexts, capability retention/release,
  membrane forwarding and streaming rejection. TLC: 96 states / 163 traces, 978 Rust
  replays and 1,395 pinned C++ observations ([contract and bounds](../wiki/Cpp-Parity.md)).
- [x] **Completion without response data** — Typed, field-operation and dynamic
  `send_ignoring_result()`, immediate pipeline release, normal errors and static
  cancellation policy. TLC: 30 states / 50 traces, 100 local/RPC Rust replays
  and 320 pinned C++ observations ([contract and bounds](../wiki/Cpp-Parity.md)).
- [x] **Call hints** — Generated schema-based noPromisePipelining, typed send_for_pipeline, high
  question IDs, late-Return legacy fallback, noPromisePipelining, forwarding through
  promises/membranes/reconnecting clients, inbound pipeline-only result/context lifetime
- [x] **Static cancellation policy** — Generated method/interface/file allowCancellation, inherited
  policy, protected task ownership beyond Finish/disconnect, retained result capabilities, explicit
  local call executor; Rust wire replay and C++ reference tests
- [x] **Cancellation lifetime** — Final-context cancellation Return, delayed cleanup for retained
  Results and child pipelines, disconnect suppression, first-responder arbitration, legal question
  ID reuse, legacy early-Finish dispatch delay
- [x] **Incoming call flow limit** — Per-connection word accounting until Return, dynamic limits for
  current/future connections, strict wake threshold, all-message backpressure, retained-context
  credit and disconnect wakeup
- [x] **Reference accounting** — Processes Return.releaseParamCaps and releases export objects
  outside table borrows
- [x] **Reconnecting capabilities** — Ordinary and streaming sends, generation captured when a
  request is built, explicit replacement and lazy reset, stale-error suppression, no automatic
  request replay
- [x] **Streaming lifetime** — Queued and membrane forwarding preserve credit-based readiness; fixed
  byte-window controller factory, zero-window readiness, blocked-sender release on controller drop,
  acknowledgement retention, empty/error drain completion and task-driver shutdown
- [x] **Variable/adaptive streaming windows** — shared window getters and independent per-stream
  bandwidth/RTT estimators, startup/steady-state growth and decay, application-limited protection,
  bounded estimates and injectable monotonic clocks. Two-party policies apply to future streams
  before or after connect/accept. Messages still send immediately; dropping credit waits never
  cancels calls. C++ differential: 1,410 observations; TLC/Rust: 1,009 states, 1,653 trace prefixes,
  nine faulty variants and four liveness checks. [Scope and tests](../wiki/Cpp-Parity.md#rpc-flow-control-io-and-diagnostics)
- [x] **Socket-informed streaming windows** — TCP and Linux Unix connections
  sample live send-buffer sizes with a connection-wide 64 KiB fallback after
  the first unavailable query. Queries skip the largest-message allowance and
  failed streams; outstanding controllers do not retain transport IO. Fresh
  TLC: 2,451 states / 5,130 trace prefixes; 27,922 matching Rust/C++ observations
  and seven fault controls. Native socket resizing, cleanup and TCP capability
  tests. [Contract and bounds](Cpp-Parity-History.md#socket-informed-flow-policy-implementation-checks-2026-09-23).
- [x] **Outgoing batches and queue diagnostics** — vectored multi-message writes, one flush per
  batch, partial/scalar writes, pending bytes/count/age excluding active I/O and framing,
  non-owning observers, immediate shutdown admission closure, failure/cancellation cleanup.
  C++ differential: 598 observations; TLC/Rust: 784 states, 2,236 trace prefixes,
  six faulty variants and fair-progress shutdown liveness.
  [Scope and tests](../wiki/Cpp-Parity.md#rpc-flow-control-io-and-diagnostics)
- [x] **Buffered byte-stream input** — default two-party prefetch into aligned reusable storage,
  shared short-lived control messages, independent Call/Return storage including capability
  tables, large-frame direct input, partial-read cancellation and sticky EOF/errors.
  C++ differential: 102 observations; TLC/Rust: 810 states, 2,529 trace prefixes,
  four faulty variants and EOF-progress liveness. Linux ancillary input uses the same framing
  with descriptor-aware prefetch (see Linux Unix transport below).
  [Scope and tests](../wiki/Cpp-Parity.md#rpc-flow-control-io-and-diagnostics)
- [x] **Exception diagnostics** — Owned remote trace and detail metadata, single remote prefix,
  unknown-type clamping, connection-failure classification; optional current encoder on
  Call/Bootstrap/Resolve/Abort errors, default-off and explicit disable, C++ remote-trace
  interoperability
- [x] **Capability-wrapper diagnostics** — synchronous `debug_info()` on untyped, generated
  (through `FromClientHook`) and dynamic clients; descriptions include promises, membranes,
  local/shortened/broken servers, RPC hooks, reconnect and persistence wrappers. Inspection
  never drives resolution, connects or accepts a handoff. Hook traversal is bounded and the
  text retains no capabilities. Pinned C++: 32 observations; TLC: 460 states / 1,250 Rust
  trace replays and four detected faults. [Contract and tests](../wiki/Cpp-Parity.md).
- [x] **Broken promises and import aliases** — Rejections emit Resolve.exception and fail resolution
  waiters; repeated imports share lookup/stream lifetime after alias release
- [x] **Hostile Return IDs** — Bounds checks and per-connection Abort rather than vector-index panic

### Capabilities, handoff and distributed equality

- [x] **Standalone membrane copies** — `Membrane::copy_into` / `copy_out` return
  scoped orphan owners for generated/dynamic structs, lists and AnyPointers.
  Nested/unknown capabilities cross the same policy, reverse crossings unwrap,
  revoked wrappers remain broken, and substitutions retain their own rules.
  Failed unpublished transformations release their authority. TLC: 91 states /
  140 edge-prefix traces, four payload forms, 1,960 matching C++ observations and
  eight detected model faults. [API boundaries](Cpp-Parity-History.md#membrane-copy-implementation-checks-2026-09-23).
- [x] **Standard Provide/Accept** — Connection-scoped provision registration, Accept before Provide,
  authentication delegated to VatNetwork, result capability export and pipelining
- [x] **Accept embargo** — Byte-string IDs, either arrival order, duplicate-ID rejection, pipelined
  calls blocked until release, pending acceptance fails if Provide ends
- [x] **Native introductions** — Deferred `ThirdPartyHosted` acceptance; pre-accept contact
  forwarding and reflection; independent and self acceptance; lazy FD retrieval; cached transport
  errors; vine retention and Finish; Disembargo forwarding to the original Provide
- [x] **Promise shortening** — Keeps bootstrap pipelines alive, handles late resolution waiters,
  handles a promise resolving to a third-party capability after earlier calls
- [x] **Capability identity** — Canonical import/export references; reverse crossing and
  re-acceptance at an existing host recover the same resolved local identity
- [x] **Multiparty Join** — Independent secret shares over transparent proxy paths, object/host
  discrimination, mutual share-possession proofs, direct Native acquisition, cancellation and Finish
  retention; 392 production-profile trace replays ([details](../wiki/Multiparty-Join.md))
- [x] **Bilateral Join** — Standard two-party Join through local resolution and independent RPC
  systems, collect-before-forward, one returned capability, retained downstream results until
  Finish, cancellation and ID reuse; 536 wire trace replays ([details](../wiki/Capability-Join.md))
- [x] **Opaque-wrapper Join policy** — Explicit membrane opt-in for complete batches sharing
  one boundary and direction, nested authorization, protected results, cancellation/revocation
  and retained downstream guards; default policies reject delegation. TLC: 246 states,
  640 Rust wire trace replays and eight detected faults; native Native regression coverage.
  Individual remote secret shares remain opaque ([details](../wiki/Capability-Join.md#policy-preserving-membrane-join)).
- [x] **Third-party answers** — Caller/relay/callee support for authenticated level-3 tail returns,
  both arrival orders, direct result capabilities, self-adoption, retained execution across both
  answers and child pipelines; 494 wire trace replays ([details](../wiki/Third-Party-Answers.md))
- [x] **Answer setup deadlines** — network-provided timers for either arrival order
  and self-adoption; native Native defaults to ten seconds, canceled on authenticated
  match. Late-answer rejection, Finish cleanup and 4,096 adopted questions per
  connection. Fresh TLC exploration: 47 states / 59 replayed transitions.
  [Details](../wiki/Connection-Recovery.md).
- [x] **Early caller pipelines** — Authenticated adoption migrates fresh and unused public
  references before Return, including self-adoption and retained capabilities after request drop;
  new calls survive relay disconnect. Non-opted-in networks keep used/encoded references
  on the relay. TLC: 2,011 states, 3,311 Rust wire trace replays and nine detected faults
  ([details](../wiki/Third-Party-Answers.md#early-caller-pipelines)).
- [x] **Active caller-pipeline migration** — native Native authenticates a Join fence
  along old and direct paths, including before Return; queues later calls and
  retains the relay on bounded failure. Used/encoded references, cancellation,
  timeout and relay loss are covered by native tests and 22 fresh TLC traces.
  [Details](../wiki/Discovery-and-Mobility.md).
- [x] **Discovery and NAT rendezvous** — recipient-bound expiring capability
  directory, named key replacement for new connections, STUN mapping discovery
  and bounded same-socket punching. 918 fresh TLC traces and real Native setup.
- [x] **Deployment lease maintenance** — owned directory renewal and idle shared
  listener STUN refresh, generation-safe cancellation, stale-hint withdrawal and
  recovery without replacing authenticated RPC routes. Two additional TLC models
  drive 576 Rust traces, with six mutation controls. [Details](../wiki/Discovery-and-Mobility.md).
- [x] **Managed discovery advertisements** — coordinate mapped addresses,
  provider retirement, hidden name claims and fresh provisioning on recovery.
  Revocation/replacement and shutdown are terminal for the old owner. TLC checks
  86 states and 133 Rust traces, including driver cancellation, with four detected
  faults; real UDP/RPC covers changed endpoints and retained authenticated calls.
- [x] **Discovery control-route failover** — up to four explicitly delegated
  readers, bounded sequential lookup, authoritative-error and pinned-key checks,
  cancellation and late-response rejection. TLC: 67 states, 110 Rust trace replays
  and three detected faults; generated directory RPC followed by real Native RPC.
- [x] **Transport mobility** — CID rotation with shared-listener aliases and
  validated client path migration preserving RPC and datagram ownership. Failed
  and canceled probes keep the existing path. [Limits](../wiki/Discovery-and-Mobility.md).
- [x] **Local transport scheduling** — configurable packet bursts, bounded
  datagram credit pacing, live updates and weak session-specific controls.
  Shutdown discards unreliable backlog; reconnect gets an independent policy.
  TLC: 3,915 states, 9,135 Rust trace replays and three detected faults. Real
  ordinary/arbitrated RPC, large responses and migration run under pacing.
  [NATIVE_SCHEDULING.md](../wiki/Transport-Scheduling.md)
- [x] **Revocable servers** — Explicit/owner-drop revocation, synchronous cancellation of suspended
  methods including protected calls, first-error retention, duplicate-owner rejection, is_in_use;
  local/wire replay and pinned C++ reference tests
- [x] **Server lifecycle hooks** — Weak self-capabilities, canonical local aliases, ordered
  shortenPath, failed-resolution behavior and replacement authority; local/wire traces and pinned
  C++ reference tests
- [x] **Server-set lookup** — Async lookup waits behind earlier streams; sync lookup returns None
  while blocked; later streams do not extend the captured barrier
- [x] **Membranes** — Inbound/outbound policy decisions, recursive parameter/result capability
  wrapping, wrapped pipelines, reverse unwrapping, wrapper identity reuse by policy object, related
  root policies with reverse-crossing hooks, resolve-before-redirect and cached resolutions, custom
  import/export substitutions, policy-owned revocation futures and callbacks after revocation,
  revocation of pending/future calls and retained inner references
- [x] **File-descriptor capabilities** — Owned local descriptors, async get_fd,
  Call/Return/Bootstrap/Resolve attachments, single-use ancillary slots, first-descriptor import
  retention, missing-FD fallback and membrane opt-in
- [x] **Linux Unix transport** — Standard Cap’n Proto framing with SCM_RIGHTS, bounded message/FD
  reception, close-on-exec, partial writes, cancellation boundary, queued descriptor ownership,
  bilateral C++ interoperability. Descriptor-aware buffered input preserves message ownership
  and resets the descriptor budget at frame boundaries; cancellation, truncation and malformed
  input release prefetched descriptors. Pinned C++: 63 observations; TLC: 485 states / 1,007
  traces replayed in 2,095 real-socket scenarios. [Details](../wiki/Cpp-Parity.md).

### Compiled reflection and detached ownership

- [x] **Dynamic capabilities** — Compiled generic interface/method reflection, owned capability
  values, reflected requests/results/pipelines, dynamic server dispatch and explicit cancellation;
  local/wire replay and pinned C++ comparison
- [x] **Reflection lookup and names** — Optional enum/method/superclass lookup,
  declaring-owner and brand preservation, borrowed display-name suffixes and
  checked byte prefixes. Inheritance uses 64 total visits with visited errors
  propagated and unused branches left unresolved. Diamond dispatch omits duplicate
  interface arms. TLC: 1,117 states; 66 terminal Rust/C++ queries and 132 total
  C++ observations ([contract and bounds](../wiki/Cpp-Parity.md)).
- [x] **Dynamic disown/adopt** — Message-scoped scalar/pointer/group/list ownership, checked brands
  and capability contexts, lossless inline movement, failure ownership recovery; 2,560 trace replays
  and C++ reference cases ([details](../wiki/Dynamic-Orphans.md))
- [x] **Detached allocation and access** — Independent allocation/copy, scoped readers/editors,
  checked typed release and shallow list/blob resize; private capability ownership survives returned
  errors and panics; 5,012 trace replays ([details](../wiki/Dynamic-Orphans.md))
- [x] **Detached group views** — Fieldwise and contiguous typed views, defaults, nested union
  selection, checked field ownership transfer and independent group copying; 11,652 trace replays
  and C++ group-copy reference ([details](../wiki/Dynamic-Orphans.md))
- [x] **External data and arena resizing** — Zero-copy immutable arena segments, retained mmap/word
  owners, checked mutation rejection, tail reclamation and in-place growth; 2,453 trace replays
  ([details](../wiki/Dynamic-Orphans.md))
- [x] **List concatenation and shrinking** — Copy concatenation preserves maximum physical element
  dimensions and unknown capabilities; shrink retains payload addresses and erases tails/padding;
  10,680 trace replays and five pinned C++ concatenation cases ([details](../wiki/Dynamic-Orphans.md))

- [x] **Scalar orphan access** — scalar builders return values by copy, matching
  C++; aggregate edits retain completed changes after callback errors or unwinding.
- [x] **External-data safety** — immutable `Arc<[Word]>`/static backing and an
  unsafe stable-owner constructor for read-only mmap; retained arena ownership,
  alignment/padding validation, serialization, checked `ReadOnlySegment` errors
  and independent writable copies. Dropping an orphan does not promise erasure.

### Dynamic schema transmission and runtime loading

- [x] **Catalog capability** — immutable `(schema ID, revision)` publication,
  atomic compiler-request import, explicit missing responses and capability-scoped
  access. See [SCHEMA_EXCHANGE.md](../wiki/Schema-Exchange.md).
- [x] **Dependency retrieval** — bounded parallel requests, shared/cyclic
  dependencies, out-of-order replies, revision validation and cancellation.
- [x] **Atomic bundle activation** — complete validated dependency closures;
  `Bundle::load()` publishes no partial runtime registry on failure.
- [x] **Runtime schema registry** — owned arenas, borrowed handles, node/word
  bounds, transactional validation, typed dependency stubs and explicit lazy
  loading. See [SCHEMA_LOADER.md](../wiki/Schema-Loader.md).
- [x] **Compatible schema evolution** — newer/older version selection, native
  registration, group-size propagation, slot-to-group and list-element-to-struct
  constraints across either dependency arrival order.
- [x] **Loaded reflection** — structs, groups, lists, enums, defaults, generics,
  implicit method parameters, conservative capability hints and native downcasts.
- [x] **Native list casts** — registered loaded lists cast to native readers and
  builders without copying storage or extracting capabilities; nested lists
  check base kinds/depth and erase generic arguments. Shared type checks and
  compiled capability-list downcasts complete both conversion directions.
  299 TLC states / 1,065 traces, 3,032 Rust/C++ observations and seven rejected
  mutations, plus native ownership and borrow-lifetime tests. [Details](../wiki/Cpp-Parity.md).
- [x] **Native capability/pipeline transfers** — loaded capability sharing and
  consuming compiled/loaded conversions preserve hooks and return owners on
  failure. Native clients and pipelines outlive loaders; pending nested paths
  and membrane denial/revocation work locally and over RPC. Compiled capability
  lists reject mismatched interface IDs even when nested. 322 TLC states / 401
  traces, 1,481 Rust/C++ observations and nine rejected mutations. [Details](../wiki/Cpp-Parity.md).
- [x] **Individual enum native casts** — compiled/loaded values check the enum
  ID without registration or brand/owner matching. Generated open enums support
  introspection and dynamic conversion; closed Rust enums safely reject unknown
  values. Source-version member lookup retains loader ownership. All 65,536
  ordinals tested, 494 TLC states / 917 traces, 3,402 Rust/C++ observations and
  nine rejected mutations. [Details](../wiki/Cpp-Parity.md).
- [x] **Reflection cache identity** — immutable `Eq + Hash` snapshots for
  compiled/loaded schemas, fields, methods and enumerants. Keys preserve owner,
  brand, index and inherited declaring interface; loaded keys borrow their
  loader. Construction is bounded/fallible and rejects incomplete metadata.
  201 TLC states / 649 edge-prefix traces, 3,608 Rust/C++ cache observations,
  211 identity comparisons and five rejected mutations. [Details](../wiki/Cpp-Parity.md).
- [x] **Enums in generic scopes** — native/compiled enum brand erasure, loaded
  enum wire-brand binding and generated default annotation callbacks. Nested
  enums compile and their identity keys work; loaded assignments reject wrong
  brands. 298 TLC states / 770 traces, 2,914 Rust/C++ state observations, 50
  identity comparisons and six rejected mutations. [Details](../wiki/Cpp-Parity.md).
- [x] **Default-sensitive presence** — `HasMode::NonNull` / `NonDefault` on
  compiled and loaded dynamic readers, builders and detached groups. Primitive
  defaults compare by encoded bits; pointers use nullness and union tags gate
  both modes. 2,081 pinned C++ observations, 261 TLC states, 4,437 Rust trace
  replays and four rejected model mutations. [CPP_PARITY.md](../wiki/Cpp-Parity.md)
- [x] **Checked dynamic conversions** — explicit `try_convert()` for compiled
  and loaded values: numeric bounds/integrality, enum names/ordinals, borrowed
  Text-to-Data and existing aggregate/capability brand checks. 1,723 defined C++
  comparisons, 545 TLC states, 26,160 conversion/assignment trace replays and
  five rejected mutations. [Contract and limits](../wiki/Cpp-Parity.md)
- [x] **Loaded constants and annotations** — typed constant values, node/field/
  method/enumerant annotations, bound generic and implicit method arguments,
  named enumerant lookup, lazy declaration errors and schema-authority checks.
  29 metadata records agree with pinned C++; 152 TLC states produce 308 Rust
  trace replays and three rejected model mutations. [SCHEMA_LOADER.md](../wiki/Schema-Loader.md)
- [x] **Brand and union introspection** — explicit/default/symbolic generic
  arguments, scope lists, brand erasure, union subsets, optional field lookup
  and discriminant lookup. Inactive reads are rejected by loaded and compiled
  reflection, including physically retained capability pointers. 181 C++ records,
  84 TLC states, 336 Rust trace replays and four rejected model mutations.
- [x] **Loaded-schema orphan parity** — message/schema-scoped disown/adopt,
  allocation, copying/concatenation, resizing, external data, fieldwise/contiguous
  group access and native conversion with exact generic arguments. Capability
  ownership, failed-transfer recovery and lifetimes are checked by 19,224 loaded
  trace replays, runtime regressions and compiler acceptance cases.
  [SCHEMA_LOADER.md](../wiki/Schema-Loader.md)
  Unknown future tags remain inspectable; unsupported writes are rejected.
- [x] **Loaded capability RPC** — owned capability reads, reflected
  clients/servers, requests, results, pipelines, streaming, tail calls and
  explicit cancellation policy; capabilities survive response disposal.

### Rust generator and message APIs

See [RUST_GENERATOR.md](../wiki/Rust-Generator.md) for opt-in flags and compatibility.

- [x] **Reader/editor facade** — sealed modes, typed borrowed readers, move-only
  editors/field handles, defaults, nullable fields, open enums and lazy errors.
- [x] **Field operations** — `set`, `copy_from`, `edit`, `ensure`, `init`,
  `replace`, `clear`, schema-bound descriptors and explicit presence checks.
- [x] **Cached occupied field editors** — built-in text/data, struct/list,
  generic/AnyPointer and capability entries retain acquired editors; consuming
  compatible entries performs no further arena access or arena allocation.
  Undersized layouts require explicit `ensure`; readonly errors and capability
  ownership are retained. TLC: 44 states, 84 Rust replays, seven fault mutations.
  [Details](../wiki/Rust-Generator.md#cached-occupied-entries).
- [x] **Lists and unions** — checked lengths/indices, fallible iteration, lending
  mutation, staged list initialization/replacement, tag-only/decoded unions,
  unknown tags and explicit group-arm selection/reset.
- [x] **Staged group construction** — generated union-group `replace_with`
  builds before selecting/replacing the arm; callback errors and unwinding
  preserve the old arm and siblings, release temporary capabilities and retain
  independently held clients. Successful publication moves descendant payloads.
  TLC checks 192 states and 339 transitions, replayed in 1,356 Rust cases.
- [x] **Message owners** — custom/scratch allocators, consuming freeze,
  exact/prefix frame acquisition, shared traversal budgets and `compact_copy`.
- [x] **Custom segment and capability-context owners** — `MessageReader`
  owns arbitrary `ReaderSegments` providers and supplied capability tables;
  mutable/frozen messages support custom context owners and exclusive borrows.
  Moves and reader transfers preserve authority and consumed traversal budgets;
  copies acquire independent hooks. TLC: 4,808 states, 12,602 Rust replays and
  six fault mutations. [Details](../wiki/Rust-Generator.md#custom-message-owners).
- [x] **Ownership and publication** — checked orphan transfers, invariant scoped
  brands, Draft/Ready staging, chunked final-storage filling and lossless
  fixed-stride struct copying; failed publication preserves the destination.
- [x] **Generated RPC conveniences** — `method_call()` editors, awaitable
  responses, pre-response pipelines, streaming readiness and cancellation/hint
  annotations without requiring local clients to be `Send`.
- [x] **Borrowed projections** — optional decoded scalar/header projections with
  borrowed text/data, lazy nested access and explicit unknown union tags.
- [x] **Native values** — optional allocating snapshots, explicit unknown-field
  discard, pointer-absence preservation, recursive limits, owned capabilities,
  opaque generic/AnyPointer leaves and atomic encode into a fresh message.
- [x] **Field diagnostics** — schema names and ordinals on fallible generated
  reads, projections, field/entry operations and staged edits; list indices and
  nested field context on native conversion failures. Error kinds, exception
  metadata, lazy validation and failed-publication cleanup are preserved.
  [Details](../wiki/Rust-Generator.md#field-diagnostics).
- [x] **Generic alias generation** — deterministic lexical parameter ordering
  and inherited/unused group parameters in generated RPC and union aliases.

### Native transport and native connection management

See [IMPLEMENTATION.md](../wiki/Native-Transports.md) for the custom
wire profile; this is not TLS-secured, standard-interoperable QUIC.

- [x] **Native backend in quiche** — bootstrap
  TLS 1.3 with pinned mutual authentication and introduced-session
  TLS 1.3 with reservation admission, using pinned upstream TLS verification rules
  (`use-curve25519`, `use-chacha20poly1305`, `use-blake3`) with defaults disabled.
- [x] **Authentication and profile binding** — pinned peers, imported-key
  consistency checks, PSK/context binding, experimental version/application
  profile, native stream preface and rejection of first-flight application data.
- [x] **Packet and async IO machinery** — packet/header protection, key updates,
  retransmission, flow/congestion control, pacing, duplicate handling, bounded
  ordered RPC bridge, timers and optional bounded unreliable datagrams.
- [x] **Native multiparty adapter** — stable endpoints, authenticated
  provision/answer rendezvous, Accept-before-Provide, retirement/replay rejection,
  self-introduction, explicit disconnect and independent route lifetimes.
- [x] **On-demand connections** — authenticated connectors and a pinned directory,
  shared pending dials, peer validation, quotas, timeout, cancellation and stale
  generation protection.
- [x] **Connection retry and recovery policy** — bounded attempts, exponential
  backoff, per-attempt/overall deadlines and opt-in failed-generation replacement
  on connect or attach. Retained old clients stay broken; old cleanup cannot
  remove a fresh route. Fresh TLC exploration: 113 states / 150 replayed
  transitions, plus UDP and virtual-time tests. [Details](../wiki/Connection-Recovery.md).
- [x] **Shared UDP listeners** — bounded single-use reservations, context/peer/PSK
  binding, packet routing, retired IDs, expiry from allocation and cancellation.
  See [NATIVE_LISTENER.md](../wiki/Shared-Listeners.md).
- [x] **Capability provisioning** — recipient-bound fresh reservations/PSKs,
  introducer control relay, lease revocation and readiness after authenticated
  route installation. Existing accepted routes survive provider revocation.
  See [NATIVE_PROVISIONING.md](../wiki/Provisioning.md).
- [x] **Crossed-dial arbitration** — opt-in selection/ack/commit before RPC
  publication, stable pending connection identity, winner datagrams and cleanup
  of losing candidates. See [NATIVE_ARBITRATION.md](../wiki/Session-Arbitration.md).

- [x] **Peer-acknowledged graceful shutdown** — bounded writer drain and authenticated
  byte receipts on dedicated/shared-listener and selected arbitrated sessions;
  network admission stop, pending-reservation retirement, cancellation and stale
  generation isolation. Crossed shutdown preserves reciprocal receipt ordering.
  TLC: 807 states / 1,496 Rust replays; extended listener coverage: 23,976 replays.
  Receipts acknowledge transport delivery, not method completion.
  See [NATIVE_SHUTDOWN.md](../wiki/Shutdown.md).

### Application authority and introductions

These application APIs coexist with native RPC handoff; they do not replace it.

- [x] **Authority grants** — object/generation/holder binding, per-method rights,
  attenuation, delegation and shared parent-lineage revocation.
- [x] **Ticket introductions** — recipient-bound tickets, fresh PSK/context,
  forwarding over authorized capabilities, direct owner connection and pipelined
  acceptance. The private bootstrap is installed only by `handoff::serve()` after
  the pinned handshake, on the same session's stream; caller-supplied peer bytes
  cannot assert authentication. See [IMPLEMENTATION.md](../wiki/Architecture.md#route-ownership).
- [x] **Application handoff fence** — explicitly supplied old-route completion
  fence, delayed direct authority, duplicate acceptance and revocation; the
  high-level ticket API does not discover arbitrary in-flight calls automatically.

### ORM, mmap storage and durable persistence

- [x] **Parameterized `Object(T)` capability** — `get`, `put`, `publish` and
  `subscribe`, plus pull `history`, schema binding and rights checked on each operation.
- [x] **Revision/publication semantics** — durable whole-entry writes with head
  compare-and-swap, separate publication compare-and-swap and reads of published
  revisions. Lost replies do not imply that a write failed to persist.
- [x] **Subscriptions** — latest-snapshot delivery, coalescing for slow observers,
  one callback in flight, bounded subscriptions, authorization rechecks and
  cancellation/drop. This is not a durable event log.
- [x] **Durable publication history and resumable consumer cursors** — typed pull
  capabilities, caller-owned checkpoints, retries across full server restart,
  publication-only ordering, explicit retention gaps, history-preserving
  compaction, cancellation/revocation wakeups and shared subscription quotas.
  130 storage states / 241 native traces and 405 RPC states / 931 traces through
  both local and wire capabilities; seven rejected mutations and fair wait
  termination. [Contract and limits](../wiki/Publication-History.md).
- [x] **Mmap-friendly mutable store** — aligned, append-only whole-entry revisions,
  immutable mapped snapshots, retained file locks, checksums, commit markers,
  fsync-before-acknowledgment and bounded payload/file sizes.
- [x] **Storage/ledger compaction and configurable growth** — atomic checkpoints,
  publishable/latest retention, preserved mapped snapshots and stable writer
  locks, explicit realm compaction and quotas beyond the former fixed limits.
  4,190 storage trace replays plus ledger compaction interleavings;
  [details](../wiki/Storage-Compaction.md).
- [x] **Storage recovery** — validated-prefix scanning, torn-tail truncation,
  rejection of corrupt complete records, poisoned writers after IO failure and
  readable snapshots after the Store is dropped. See
  [IMPLEMENTATION.md](../wiki/Storage-and-ORM.md#storage-format-version-4).
- [x] **Persistence realm** — Standard generic save plus durable owner-sealed references,
  Native-authenticated restoration, owner key rotation, revocation, asynchronous factory guards and
  typed ORM reconstruction; 1,288 trace replays ([details](../wiki/Persistence.md))
- [x] **Owner-sealed restoration** — stable owners, key epochs, authenticated
  Restorer bootstrap, durable reference tombstones, checks before/after async
  factories, guarded renewal and cancellation/revocation/close handling.
- [x] **Persistence owner deletion and reference expiration** — atomic deletion,
  fresh epochs on owner recreation, durable host-driven logical time, shortened
  and inherited deadlines, expired/revoked slot collection and non-reused token
  serials. Strict current-format ledgers, byte-truncation recovery, 3,112 model states
  and 8,674 Rust trace replays including ledger compaction ([details](../wiki/Persistence.md)).
- [x] **Typed ORM reconstruction** — `ObjectFactory<T>`, exact rights/generation
  preservation and shared publication state. Explicit SturdyRefs can be stored
  as data; live connection capability hooks cannot.

### Bulk transfer

See [BULK_RUNTIME.md](../wiki/Bulk-Transfer.md); this is an ordinary RPC service.

- [x] **Transfer capabilities** — one bounded opaque-byte transfer per capability,
  `describe`, ordered `write`, `done` and `cancel`, with independent authority.
- [x] **Payload credit** — reservation before send, matching acknowledgment/error
  settlement, out-of-order replies, duplicate-ack protection and sticky failure.
- [x] **Completion and cancellation** — exact-length validation, idempotent
  completion, atomic in-memory publication, ordered cancellation and receiver
  owner cleanup; canceled waits retain outstanding RPC ownership.
- [x] **Resumable file transfer and atomic ORM commit** — durable chunk journals,
  explicit checkpoints and exact retries across reconnect/restart, file upload
  client, digest/type validation, generation-bound authority, pinned target
  versions, and one-store atomic publication plus completion receipt. History
  compaction preserves staging; terminal receipts survive latest compaction.
  1,625 TLC states, 2,971 native and 2,971 RPC trace replays, nine detected faults;
  [contract and bounds](../wiki/Durable-Bulk.md).

### Realtime snapshots and datagrams

See [REALTIME_RUNTIME.md](../wiki/Realtime-Snapshots.md); payloads contain opaque bytes.

- [x] **Snapshot capability** — `describe`, `offer`, `cancel` and `close`, shared
  sender sequence allocation, per-key replacement and authoritative receipts.
- [x] **Deadline and freshness guards** — apply-time skew-aware checks, stale and
  duplicate suppression, immutable sequence identity, cancellation tombstones,
  clock-regression rejection and bounded queues/history/payload/waiters.
- [x] **Explicit application semantics** — atomic latest-value publication,
  expiration, optional ticking worker and preserved terminal outcomes; local
  timeout means unknown execution and does not roll back accepted work.
- [x] **Datagram capability adapter** — session-local bearer tokens, bounded
  unreliable Native data, validated ingress and reliable status/cancel/close RPCs.
- [x] **Retry and lifetime handling** — explicit identical retries, canceled or
  retired token rejection, and revocation on control-capability/driver release;
  replacement sessions require fresh grants.
- [x] **Fragmented realtime snapshots** — up to 59,392 bytes across 64 datagrams,
  bounded capability-local reassembly, immutable metadata and payload commitments,
  complete-value publication, exact retries and atomic local batch admission.
  Partial transfers remain Unknown; cancellation, expiry and closure free buffers.
  TLC: 1,321 states, 10,738 Rust trace replays, eleven detected fault mutations.
