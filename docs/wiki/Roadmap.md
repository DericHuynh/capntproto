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

## Portable RPC performance gap

**Open: [gap #11](https://github.com/DericHuynh/capntproto/issues/11), recorded October 15, 2026.** For each matching plaintext TCP workload,
portable pooled p50 must be at most **1.05 × min(legacy p50, C++ p50)**. Compare
all peers in the same campaign with matched fixtures, release settings, CPU
placement and sampling budgets. Faster portable results pass without slowing
either reference. Apply the target to all twelve remote application workflows
and separately to the four loopback payloads; never combine those topologies.
Retain p95/p99, throughput, CPU, RSS and every sample alongside the p50 decision.
The existing empty-call instruction qualification is a separate contract.

The [frozen baseline](../../research/reports/portable-performance/2026-10-15/baseline.json)
comes from [run 38763852450](https://github.com/DericHuynh/capntproto/actions/runs/38763852450)
at `2c9548c998fb`. All 350 remote trials and 120 loopback trials were checked
against the published percentiles. Seven remote workflows exceed the new target:

| Workflow | Portable p50 | Faster reference p50 | Portable/reference |
| --- | ---: | ---: | ---: |
| 32 concurrent 1-KiB calls | 1,041.810 µs | C++ 769.963 µs | 1.353× |
| Dependent capability read | 284.675 µs | Legacy 259.339 µs | 1.098× |
| Pipelined capability read | 193.436 µs | Legacy 167.097 µs | 1.158× |
| Callback | 278.755 µs | Legacy 242.305 µs | 1.150× |
| 256-KiB echo | 1,348.376 µs | Legacy 1,093.015 µs | 1.234× |
| 1-MiB echo | 4,549.966 µs | Legacy 4,233.197 µs | 1.075× |
| 4-MiB echo | 19,493.488 µs | C++ 18,188.989 µs | 1.072× |

Investigate batching/syscall frequency under fan-out, capability-bearing call
and result paths, and large-frame copying/allocation/polling. Whole-client
resource logs also show roughly twice legacy's CPU consumption at 4 MiB;
they include setup and warmup and are not a hot-function profile. Keep lifecycle,
admission, cancellation, FIFO, partial-write and shutdown guarantees intact.
Local experiments guide changes; closing this gap requires a fresh dedicated
campaign with every applicable cell passing, not an empty-call result alone.

The latest completed dedicated campaign at `866610259cf5` passes 11 of 16 cells:
remote pipelined reads, callbacks and 64-KiB echo, plus empty and 64-byte loopback,
remain above the limit. Earlier campaigns used different hosts and timing;
their pass counts are historical observations rather than a progression score.

On October 17 the active work shifted to the
[portable hosted architecture](Architecture.md#portable-hosted-execution-boundaries).
Typed scheduling, centralized notification ownership, an explicit control-activity
contract and separated execution/IO/lifecycle modules establish boundaries for
integration and further optimization. Unqualified response-sharing and worker
experiments are archived separately. The 5% target remains open; architectural
refactoring and local profiling do not replace a complete dedicated campaign.

The [ownership and integration review](Architecture.md#portable-ownership-and-integration-review)
separates explicit endpoint retirement from promise settlement and metadata
refresh. The [three architecture experiments](../../research/reports/portable-performance/2026-10-17-architecture/README.md)
are retained as historical evidence. The current implementation makes restricted
control contexts, revision-backed metadata and generated borrowed service
contexts mandatory, with an explicit ownership transfer for asynchronous work.
This deliberately breaks the unreleased Rust APIs. The
[renewed architecture review](Architecture.md#mandatory-integration-contracts-and-c-reassessment)
identifies the remaining parameter/execution ownership, registry scans and broad
readiness invalidation. These contracts enable further work; their presence alone
does not establish the 5% goal.

Tokio is the selected host for continuing portable optimization. The
[runtime comparison](../../benchmarks/rpc-compio/README.md) retains Compio behind
the opt-in `compio-experiment` feature to reproduce the existing measurements.
Prepared call/result readers now reuse the selected payload location, following
C++ ownership and access patterns. The [measured port](../../research/reports/portable-performance/2026-10-17-cpp-readers/README.md)
reduces pipeline-server instructions by 1.04% and callback-client instructions by
0.46%; it does not establish the 5% latency target. An inline parameter owner
saved allocations but increased instructions and was rejected.
Focus further work on portable CPU overhead and matching Tokio/C++ publication
turns. A Compio transport migration is no longer an active roadmap item; the
recorded Poll results do not establish io_uring performance.

The first optimization slice indexes immediate reply owners beyond eight calls,
checking the full question generation before delivery, and copies two-segment
prepared Returns without first clearing the destination. Small-call lookup,
reader validation, resource limits and generic protocol paths remain intact.
Remote reports now show the faster reference and an exact inclusive 5% decision
for each workflow; the report remains advisory while this gap is open. See the
[optimization evidence](Quality-and-Benchmarks.md#portable-workflow-gap-october-15-2026)
for measurements and remaining work.

Related audit findings remain separate: native QUIC's 64-KiB loopback p50 is
211.442 µs versus legacy TCP's 84.101 µs, with encryption/architecture differences;
the serialization decode fixture includes a full byte-validation scan; storage
research points to durability amortization and ORM worker integration. These do
not change the portable plaintext target or justify removing fixture validation.

[The earlier October 16 dedicated checkpoint](Quality-and-Benchmarks.md#dedicated-checkpoint-and-scheduling-follow-up-october-16-2026)
passed seven of twelve remote workflows and three of four loopback payloads.
The later twelve-cell checkpoint above supersedes its status; all earlier misses
and samples remain retained. The gap remains open.

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

TCP/TLS, quiche QUIC v1, native mutual authentication, shared listeners,
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
