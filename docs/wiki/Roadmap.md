# Roadmap

Current priority is an ergonomic RPC foundation whose generated APIs reject
invalid ownership and lifecycle combinations. There is no actor runtime in this
workspace. Higher-level runtimes can use the RPC and storage contracts once their
own placement, durable ownership and delivery semantics are defined.

[Runtime status](Runtime-Status.md) lists available behavior. This page lists open
work; [release acceptance](Release-Acceptance.md) owns release qualification and
[correctness](Correctness.md) owns cross-cutting verification. A concrete
bug in a supported contract remains a blocker regardless of roadmap priority.

## RPC and API ergonomics

1. Extend compile-time ownership guarantees where they prevent real mistakes.
   Structured replies, exact-schema tail forwarding, immutable publication,
   borrowed field editors and consuming parameter closures are implemented.
   Keep network, authorization and data errors explicit; do not claim typestate
   can prove remote execution, reliable delivery or application correctness.
2. Complete resource admission beyond the existing per-connection Call count:
   payload bytes, authenticated-principal totals, local unresolved calls and
   bounded output batches. Preserve callback, pipeline and control-message progress.
3. Measure allocations, copies, serialization and batching in integrated workloads
   before changing transport scheduling. QUIC stream fan-out needs an explicit
   ordering/fence design; one stream per RPC connection remains the current profile.
4. Add explicit deadline/receipt ergonomics without automatic replay of uncertain
   mutations. Define supported peer/version profiles for interoperability.

The [RPC research](RPC-Research.md) preserves its protocol probes and ordered
acceptance criteria. [RPC applications](RPC-Applications.md) documents the APIs
already available.

## Storage and ORM

- Integrate the bounded worker with ORM facets while preserving execution-time
  authorization, schema binding, revocation and object ordering.
- Add durable operation receipts and retry lookup. Local CAS or a lost response
  cannot establish whether an earlier mutation committed.
- Add group commit with bounded group bytes/count/delay, preserving each
  transaction's atomicity and reporting uncertain barrier failures.
- Extend component APIs with consistent snapshots/multi-get, typed transactions,
  publication feeds and durable bulk upload. Keep root CAS unless read sets
  justify a finer conflict model.
- Qualify coherent immutable read views, retained-generation bounds and failure
  visibility. Queue replacement requires lifecycle/accounting evidence and
  integrated measurements, not only a synthetic lock-free throughput result.
- Benchmark contiguous manifests and incremental realm metadata before changing
  disk formats. Persistent indexes, shared immutable extents and bounded cold
  recovery are larger structural work.
- Qualify writeback faults and backup restoration on disposable filesystems.
  Multiwriter operation, cross-file transactions and replicated realms require
  new consistency and ownership contracts.

See [storage research](Storage-Research.md), [resilience](Storage-Resilience.md)
and [lock-free research](Lock-Free-Research.md). EAE remains a separate
[research backend](EAE-Comparison.md), not a planned default replacement.

## Transport and deployment

TCP/TLS, quiche QUIC v1/v2, native mutual authentication, shared listeners,
provisioning, arbitration, discovery renewal/failover, STUN rendezvous,
CID rotation and client migration are implemented within their documented scopes.
Open work includes broader hostile-peer/loss/soak qualification, deployment
performance, jitter/shared retry budgets, native macOS descriptor qualification,
other Unix descriptor backends, public bootstrap automation, full ICE/TURN,
portable daemon operations and background identity retirement.

QUIC version selection stays explicit. Application 0-RTT execution, automatic
mutation retry, transparent relabeling of existing vat identities and hard
latency guarantees are not current contracts. TCP has no datagram/path-migration
semantics. Per-capability scheduling and reserved bandwidth are optional research.

## Generator, services and persistence

Optional native specialization of generic/AnyPointer leaves, derives, Serde and
logical comparison remain open. Compiler differential generation should exercise
interacting aliases, annotations and nested generics beyond the existing corpus.
Whole-message infallible validation and recoverable allocator OOM are not supplied.

Realtime grant migration, clock synchronization and hard realtime effects need
new contracts. Persistence uses explicit owner-sealed references and application
factories; generic capability-graph reconstruction and cross-realm translation
are not implied by serialization support.

## Proposed actor database

The [ActorDB proposal](ActorDB-Proposal.md) describes event-sourced actors,
durable changefeeds and incrementally maintained views. The
[ActorDB checklist](ActorDB-Checklist.md) is its future implementation tracker,
with single-host and replicated milestones. Actor execution, distributed
ownership, database indexes and IVM remain unimplemented. These milestones
build on the RPC/storage work above and do not change current release claims.

## Verification and interoperability gaps

Full Rust/C++ equivalence, arbitrary proxy/ID/cancellation/reconnect schedules,
wire/local-state refinement of the composed model, and whole
RPC/schema/transport/storage composition remain unproved. The pinned C++
dispatcher lacks general Join and third-party answers; Rust-to-Rust tests cannot
qualify those as C++ interoperability. Broader platform/storage qualification and
independent native admission/security review remain release work.

The [older incremental roadmap](../archive/Roadmap-History.md) retains completed
items and historical design context; it is not a second active checklist.
