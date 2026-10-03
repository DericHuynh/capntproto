> Historical development record, archived 2026-10-01. Commands, counts,
> feature status and security descriptions below belong to earlier snapshots.
> Use the [current guide](../wiki/Roadmap.md) for supported behavior and commands.
> Unbundled local run artifacts are not current verification evidence.

# Feature and research backlog

This backlog is separate from release acceptance. Intentional exclusions are not implementation promises.
The [C++ parity checklist](../wiki/Cpp-Parity.md) owns the concrete source comparison and
its remaining implementation/API tasks; this roadmap owns broader extensions.
The [correctness and testing roadmap](../wiki/Correctness.md) lists prioritized
API, fuzzing, memory-safety, simulation and verification improvements.

## Release classification

The [release checklist](../wiki/Release-Acceptance.md) owns concrete release gates R1–R7
(downstream use, artifact, storage integrity/recovery, fault tests, support matrix
and operational documentation) and production qualification P1–P4. They take priority over adding
features below. A Linux 0.x developer preview does not require clearing this list.

| Backlog group | First-release classification |
|---|---|
| Complete C++ API/reflection parity | Conditional: required for a complete-replacement claim |
| Non-Linux transport, broader discovery/NAT/mobility deployment, generator conveniences | Optional extensions to the proposed preview scope |
| EAE, multiwriter/indexing/replication, general object-graph persistence, realtime extensions | Optional features; EAE has a measured [integration decision](../wiki/EAE-Comparison.md) |
| Hostile-wire tests, supported-peer interop, Native review, workload/failure qualification | Qualification of existing behavior; see P1–P4, and R3–R5 for storage correctness |
| Exhaustive schedules, unbounded proofs, arbitrary hardware/compromise models | Longer-term research, not mandatory release acceptance |

Any concrete bug in a supported contract remains a release blocker. A missing
general proof alone is not evidence of such a bug.

## Remaining work

Unchecked items below distinguish missing implementation from optional product
work and unproven behavior. An unchecked proof obligation does not imply that
the corresponding runtime operation is absent. Intentional API/security
boundaries are not promises to add those behaviors.

### Missing runtime/API features and optimizations

- [x] **Component storage and typed facets** — opt-in V5 manifests share unchanged
  data, atomically publish component edits, retain mapped snapshots and rebase
  references during compaction. [Contract and byte comparison](../wiki/Component-Storage.md).
- [x] **Bounded storage owner** — opt-in V4/V5 async API with count/byte and
  principal limits, separate small/large admission pools, queued cancellation,
  deadlines, unread reply accounting, I/O quarantine and explicit shutdown.
  [Contract and tests](../wiki/Storage-Worker.md). Existing ORM RPC facets remain synchronous.
- [ ] **Group commit and ORM worker integration** — share durability barriers
  with bounded group delay, preserve schema/authorization checks at execution,
  and add durable operation receipts. The [performance](../wiki/Storage-Research.md) and
  [resilience](../wiki/Storage-Resilience.md) measurements remain frozen prototype
  evidence; qualify the integrated RPC workload before choosing deployment limits.
- [ ] **Storage failure qualification** — qualify real writeback faults,
  cold recovery and backup restoration independently of process-crash tests.
  V5 component crash/second-recovery checks now run in the isolated storage gate.
- [ ] **Immutable reads and reduced coordination** — qualify an authorized,
  coherent published read view, retained-generation bounds and failure visibility.
  The [lock-free research](../wiki/Lock-Free-Research.md) includes 84 exploratory runs;
  queue replacement still needs lifecycle/credit proofs and integrated measurements.
- [ ] **Component API completeness** — snapshot/multi-get capabilities, typed
  transactions, history/change feeds, durable mutation receipts and bulk uploads.
  Keep root CAS as the default; finer conflicts need explicit read sets.
- [ ] **Structural storage scaling** — incremental realm records and lookup
  indexes, shared immutable extents, persistent metadata and bounded recovery.
  Benchmark contiguous manifests before changing the V5 format.

- [x] **RPC application ergonomics** — owned connections and vats, weak bootstrap
  handles usable after startup, peer-specific bootstrap factories, connector
  closures, capability joins and bounded shutdown. [Guide](../wiki/RPC-Applications.md).
- [x] **Structured RPC reply API** — opt-in generated consuming reply stages,
  exact-schema tail forwarding, immutable pipeline publication, directly
  awaitable calls and parameter-construction closures. Compiler contracts reject
  invalid transitions; legacy mutable bindings retain their runtime checks.
  [Contract](../wiki/RPC-Applications.md#structured-server-replies).
- [ ] **RPC admission and complete resource diagnostics** — bound ordinary and
  unresolved calls while preserving pipelines and callback/control progress;
  account separately for queued messages, active batches and retained contexts.
  The first layer now provides opt-in per-connection Call count admission,
  queued/active output diagnostics, protocol table counts and retained wire
  responses. Byte/principal budgets, local unresolved clients and bounded batches
  remain open. [Admission contract](../wiki/RPC-Applications.md#outgoing-admission-and-resource-observation).
  The [RPC research](../wiki/RPC-Research.md) includes 15 reproducible protocol scenarios,
  a typed tail-call API proposal and allocation/transport qualification priorities.
- [ ] **Higher-level runtime integration** — build future virtual actors on the
  RPC APIs after defining activation, placement, durable ownership and delivery
  contracts. There is currently no actor runtime in this workspace.
- [x] **Bounded third-party answer setup** — native Native deadlines for missing
  redirects/adoptions, cancellation of setup timers after authentication,
  per-connection adopted-question limits and late-answer rejection. Method
  execution retains application deadline policy. [NATIVE_RECOVERY.md](../wiki/Connection-Recovery.md)
- [x] **Variable/adaptive RPC streaming windows** — shared variable estimates,
  per-stream bandwidth/RTT adaptation, two-party policy selection, pinned C++
  differential tests and fresh TLC/Rust replay. [CPP_PARITY.md](../wiki/Cpp-Parity.md)
- [x] **Outgoing RPC batches and queue diagnostics** — scatter/gather batches,
  pending byte/count/age snapshots, shutdown admission and failure cleanup;
  pinned C++ comparisons and fresh TLC/Rust replay. [CPP_PARITY.md](../wiki/Cpp-Parity.md)
- [x] **Buffered RPC byte-stream input** — shared control-message storage, retained Call/Return
  payloads and capabilities, large-frame direct reads, cancellation-safe low-level input;
  C++ comparisons and fresh TLC/Rust replay. [CPP_PARITY.md](../wiki/Cpp-Parity.md)
- [x] **Buffered Linux ancillary input** — descriptor-aware prefetch, per-frame budgets and
  cancellation cleanup; pinned C++ comparison and TLC traces replayed over real Unix sockets.
  [Contract and verification](../wiki/Cpp-Parity.md).
- [x] **Capability-wrapper diagnostics** — client/hook debug descriptions with bounded
  traversal; observation does not drive resolution, reconnect or handoff. C++ comparison
  and TLC/Rust trace replay. [Contract and verification](../wiki/Cpp-Parity.md).
- [x] **Default-sensitive dynamic presence** — compiled/loaded readers, builders
  and detached groups; bitwise defaults, pointer nullness and inactive unions.
  Pinned C++ comparisons and fresh TLC/Rust replay. [CPP_PARITY.md](../wiki/Cpp-Parity.md)
- [x] **Checked dynamic conversions** — compiled/loaded `try_convert()` with
  numeric range checks, enum names/ordinals and borrowed Text-to-Data; pinned C++
  comparisons and fresh TLC/Rust conversion/assignment replay. [CPP_PARITY.md](../wiki/Cpp-Parity.md)
- [x] **Structural message equality** — tri-state pointer/struct/list comparison
  with capability uncertainty, schema-independent padding rules and reader
  limits. Cargo regressions, pinned C++ comparisons and fresh TLC traces.
  [Contract and bounds](../wiki/Cpp-Parity.md#serialization-and-rust-code-generation).
- [x] **Standalone membrane-copy helpers** — copy structs/lists/AnyPointers into
  scoped orphanages while importing/exporting all contained capabilities;
  reverse crossings, revocation and substitution behavior use the existing
  membrane. Cargo regressions, TLC replay and pinned C++ comparisons.
  [Contract and bounds](Cpp-Parity-History.md#membrane-copy-implementation-checks-2026-09-23).
- [x] **Schema-free struct views** — explicit allocation, mutable data/pointer
  sections, checked writable schema casts, unknown-field copying and
  capability-aware canonicalization. Cargo tests, fresh TLC traces and pinned
  C++ comparison. [Contract and bounds](Cpp-Parity-History.md#schema-free-struct-implementation-checks-2026-09-23).
- [x] **Schema-free list views** — explicit list/struct-element allocation,
  physical layout, checked native/dynamic casts, capability-preserving erasure
  and copying, and corrected pointer-list builder/reader conversions. Cargo
  regressions, TLC traces and pinned C++ comparisons.
  [Contract and bounds](Cpp-Parity-History.md#schema-free-list-implementation-checks-2026-09-23).
- [x] **General two-party byte-stream conveniences** — owned/borrowed streams,
  listener cancellation, drain, disconnect observation and bootstrap capabilities;
  Cargo tests, fresh TLC lifecycle replay and pinned C++ comparisons.
  [Contract and bounds](Cpp-Parity-History.md#two-party-facade-implementation-checks-2026-09-23).
- [x] **Descriptor-aware convenience overloads** — Linux Unix client/server
  facade, scoped borrowed sockets, listener acceptance, per-message limits,
  drain and metrics. Cargo tests, TLC traces and pinned C++ comparisons.
  [Contract and bounds](Cpp-Parity-History.md#descriptor-aware-facade-implementation-checks-2026-09-23).
- [x] **Automatic socket-informed flow control** — TCP and Linux Unix adapters
  sample the live send buffer and share C++'s sticky fallback across streams;
  native socket tests, fresh TLC replay and pinned C++ network comparisons.
  [Contract and bounds](Cpp-Parity-History.md#socket-informed-flow-policy-implementation-checks-2026-09-23).
- [x] **Caller-supplied async word buffers** — borrowed payload storage with
  owned fallback, framing/limit/cancellation checks, allocation budgets and
  168 pinned C++ comparisons. [Scope and checks](Cpp-Parity-History.md#async-word-scratch-implementation-checks-2026-09-27).
- [x] **Buffered and descriptor scratch storage** — retained frame words and
  caller-owned FD slots, bounded cancellation-safe descriptor staging, allocation
  budgets and 1,008 pinned C++ comparisons.
  [Scope and checks](Cpp-Parity-History.md#buffered-and-descriptor-scratch-checks-2026-09-28).
- [x] **Optional C++ adapters and codecs** — opt-in `capnp-compat` with JSON/text,
  ByteStream, HTTP level 2, WebSocket message transport and JSON-RPC. Portable
  tests and pinned C++ codec/loopback checks are workspace tests; see the
  [contracts and bounds](../wiki/Compatibility-Adapters.md).
- [ ] **Complete C++ RPC runtime/API parity** — broader transport integrations
  and the documented source/verification gaps remain.
  [CPP_PARITY.md](../wiki/Cpp-Parity.md) maps the source evidence, optional adapters and
  proof gaps. Result construction, pipeline publication and native conversions
  already exist within their documented Rust contracts.
- [ ] **Non-Linux ancillary transport** — implement and exercise descriptor
  transport on other Unix platforms; current SCM_RIGHTS transport is Linux-only.
- [x] **Migration of active caller-pipeline references** — native Native uses an
  authenticated Join fence on the old and adopted paths, queues subsequent calls,
  and migrates after pipeline publication, including before Return. Timeout,
  unsupported paths and failed equality retain the ordered relay. Encoded references
  and relay loss are exercised by native Cargo tests and fresh TLC trace replay.
  [ANSWER_ADOPTION.md](../wiki/Third-Party-Answers.md)

### Transport and deployment features

- [x] **Bounded automatic connection recovery** — opt-in replacement of failed
  generations on connect/attach, shared pending dials, fresh provisioning retries,
  exponential backoff and overall deadlines. Existing calls are never replayed.
  [NATIVE_RECOVERY.md](../wiki/Connection-Recovery.md)
- [x] **Capability directory discovery** — recipient-scoped, expiring named/key
  lookup, compare-and-replace publication, revocation, fresh provisioning on dial
  and name-based identity-key replacement. [NATIVE_DEPLOYMENT.md](../wiki/Discovery-and-Mobility.md)
- [x] **STUN-assisted NAT rendezvous** — same-socket IPv4/IPv6 mapping discovery,
  delegated bounded punching and pinned Native authentication before publication.
- [x] **Connection-ID rotation and validated client path migration** — shared
  listener CID routing, bounded probes, cancellation/timeout rollback, preserved
  RPC generation and datagram lane. [NATIVE_DEPLOYMENT.md](../wiki/Discovery-and-Mobility.md)
- [x] **Owned directory renewal** — automatic half-lifetime renewal, generation
  checks, immediate owner-drop revocation, and permanent retirement after
  replacement, revocation, closure or missed expiry. Fresh TLC/Rust replay.
- [x] **Idle listener mapping maintenance** — periodic same-socket STUN refresh,
  address-change notifications, stale/failure withdrawal, bounded retries and
  cancellation without closing authenticated RPC routes. [NATIVE_DEPLOYMENT.md](../wiki/Discovery-and-Mobility.md)
- [x] **Managed mapped-service advertisements** — publish address/provider pairs
  together, suspend lookup and cancel pending provisioning on mapping failure,
  retain and renew the name claim, and recover with fresh authority. Revocation,
  replacement, drain, shutdown and owner/driver cancellation retire the owner;
  cached old providers cannot regain authority. Multiple recipients share one
  mapping while existing RPC sessions survive advertisement changes.
- [x] **Bounded discovery control-route failover** — ordered, explicitly delegated
  readers with per-attempt and total deadlines; authoritative absence, invalid
  records and provisioning failures never trigger fallback. Cancellation and late
  replies are checked by TLC/Rust replay and real control-route/Native RPC tests.
- [ ] Broader public discovery/bootstrap automation, full ICE candidate selection,
  TURN/endpoint-dependent NAT support and a portable production server daemon.
- [ ] Background identity-key generation/retirement and transparent migration of
  existing capabilities to a new identity. Named discovery follows an explicitly
  published replacement for new sessions; existing vat IDs remain immutable.
  Packet-protection key updates and persistence owner-key rotation are different operations.
- [x] **Local transport scheduling controls** — bounded packet bursts and
  optional per-session datagram credit pacing, live policy updates, preserved
  RPC/path control and shutdown, and isolated controls across reconnect.
  [NATIVE_SCHEDULING.md](../wiki/Transport-Scheduling.md)
- [ ] Adaptive scheduling, per-capability priorities and reserved bandwidth.
  Broader loss/recovery/performance qualification remains required for production
  claims (release P2/P4); local scheduling controls are not latency guarantees.

### Generator and reflection design work

- [x] Replace the optional nightly `rpc_try` panic path with synchronous
  `Result?` propagation, error conversion and explicit async boundaries.
  [CPP_PARITY.md](Cpp-Parity-History.md#rpc-try-contract-checks-2026-09-28)
- [x] Consistent `capnp_root` substitution and a build-script override, verified
  by downstream compilation and roundtrips for renamed/re-exported runtimes,
  bootstrap generation and the optional field API. [CPP_PARITY.md](Cpp-Parity-History.md#generated-runtime-path-checks-2026-09-28)
- [ ] Optional native specialization of generic/AnyPointer leaves, derives,
  Serde integration and logical comparison; opaque owning leaves already work.

### ORM, storage and persistence extensions

- [ ] Optional EAE mutable arena backend, after preserving ORM publication/history,
  durable batch receipts, capability policy and builder ownership. Its tests are
  useful today; integration is not a release prerequisite. [EAE_BENCHMARKS.md](../wiki/EAE-Comparison.md)
- [ ] Multiple writers, cross-file transactions and indexing beyond startup
  scanning in the mmap store.
- [ ] Distributed realm replication/trust and cross-realm translation policy.
- [ ] General application-object/capability-graph reconstruction beyond the
  typed ORM factory. Arbitrary factories must supply their own schema,
  generation and authority checks. [PERSISTENCE.md](../wiki/Persistence.md)

### Bulk and realtime extensions

- [ ] Automatic realtime grant migration across reconnect or three-party handoff.
- [ ] Clock synchronization, reserved-bandwidth/latency scheduling and hard
  realtime execution; current guarantees cover logical publication deadlines.

### Deliberately excluded or deferred contracts

These are boundaries of the present design, not missing standard RPC messages.

- [ ] Authenticated 0-RTT application execution with a defined replay contract.
  First-flight application data is currently rejected; TLS resumption,
  certificate fallback and standard QUIC wire interoperability are not offered.
- [ ] C++-style parser callbacks that mutate the runtime loader through live
  readers and concurrent parser caching. Current
  lazy loading requires an exclusive Rust borrow. The separate
  [Rust schema frontend](../wiki/Schema-Compiler.md)
  compiles structs/enums, unions/groups, imports, aliases, annotations, generic
  structs/interfaces, brands, method parameters and streaming, scalar/composite
  constants/defaults, embeds, extended strings, documentation/source-info, identifier
  references and recursive cross-file types. All 21 repository schemas
  match C++ requests. Immutable parsed-schema snapshots now connect textual
  compilation to validated runtime reflection, nested declarations, source metadata
  and dynamic messages. Sessions add lazy declaration/alias lookup, cached input
  identity and transactional extension under an exclusive borrow. Custom source
  providers supply application-defined import/embed resolution and bounded input
  without implicit disk access. Optional random file IDs support configuration
  schemas, with stable identities inside a successful session. The root and
  test-support builds use Rust compilation with
  input tracking, and generated bindings render schema Rustdoc. Migration of the
  separately packaged vendored RPC crate still needs a distributable frontend. Lists of unbound parameters
  are deliberately rejected regardless of parameter position.
- [ ] Whole-message infallible validation, recoverable allocator OOM and
  returned-error allocation-failure atomicity; validation remains lazy and
  the allocator contract remains infallible.
- [ ] Secure erasure/reclamation of every discarded orphan and historical secret
  copy. Tail reclamation and zeroization of owned PSK buffers do not imply this.
- [ ] Persistence of live connection capability hooks, arbitrary in-place
  mutation of serialized pointers, at-rest encryption and hostile-writer/rollback
  protection. Explicit SturdyRefs and whole-entry replacement are supported.
- [ ] Automatic replay/exactly-once external effects for lost mutation replies,
  or automatic discovery of arbitrary old-route work by the application ticket
  API. It requires the caller's explicit completion fence.

### Verification and interoperability gaps

Finite additional adversarial/failure tests are release qualification, while
“all,” “arbitrary” and “end-to-end refinement” below describe broader research
coverage. See release P1–P4 for bounded, actionable acceptance criteria instead
of treating each broad statement as a prerequisite to shipping.

- [ ] Complete connection-task cancellation and question/answer cleanup race
  coverage, arbitrary static-cancellation/streaming/tail-call combinations and
  all-schedule equivalence with the KJ `evalLast()` workaround.
- [ ] Arbitrary cyclic shortening, redirection chains, membrane transformation
  graphs and asynchronous revocation schedules beyond current bounds.
- [ ] All native handoff proxy-chain/disconnect permutations and arbitrary idle,
  transport and protected-method scheduling. Connection and answer setup now
  have bounded native deadlines; unconditional progress on a stalled executor
  and arbitrary composed-route behavior remain unproven.
- [ ] Full hostile-wire/fuzz coverage and production interoperability testing
  against the pinned C++ development runtime. Current bilateral interop uses
  installed C++ 1.5.0; pinned-source reference tests are separate.
- [ ] External C++ general-Join/third-party-answer interoperability: the pinned
  C++ dispatcher does not implement these paths, so it cannot serve as that peer.
- [ ] Cryptographic security proof of the Native/quiche binding, arbitrary
  compromise/forward-secrecy analysis and exhaustive packet-recovery behavior.
- [ ] Arbitrary filesystem/hardware failure and power-loss validation; current
  storage evidence covers checksums, valid-prefix/torn-tail recovery and fsync
  assumptions.
- [ ] End-to-end refinement of the composed RPC, capability, schema, bulk,
  realtime, transport and storage models against Rust. Component trace replay
  and finite composed-state checks do not constitute whole-program verification.
