# Runtime status

This is an unpublished 0.x developer preview. The table describes implemented
behavior in this source tree; it is not a release certification or a claim of
complete C++ equivalence. [Release acceptance](Release-Acceptance.md) owns the
qualification gates. [Architecture](Architecture.md) explains ownership.

| Area | Available | Boundary |
| --- | --- | --- |
| Capability RPC | Calls, streaming, promise pipelines, tail transfers, cancellation policy, membranes and revocation | Drivers must run; cancellation does not undo effects |
| Generated APIs | Field operations, directly awaitable calls, parameter closures and opt-in structured replies | Runtime errors remain for received data, authorization and application failures |
| Bilateral transport | Plain TCP, CA-validated TLS 1.3/mTLS, quiche QUIC v1/v2, Linux/macOS descriptor transport | Plain TCP is unauthenticated; native macOS qualification remains open |
| Native multiparty network | Pinned Ed25519 mutual TLS over TCP or quiche, direct introductions, authenticated Join and answer adoption | Connector authority is configured; this is a project-specific network profile |
| Native QUIC controls | Bounded datagrams, shared reservations, scheduling, CID rotation and validated client migration | TCP does not provide datagrams or QUIC path controls |
| Resource controls | Per-connection outgoing Call limits, incoming flow control, queue/active-write metrics and protocol snapshots | No process-wide memory limit; byte/principal and local unresolved-call budgets remain open |
| Schemas | Rust text compiler, generator, compiled/loaded reflection and capability-based exchange | Bounded compiler corpus; no full language-equivalence claim |
| Compatibility | Opt-in JSON/text, ByteStream, HTTP-over-capabilities, WebSocket framing and JSON-RPC adapters | Applications supply executor and network integrations |
| Storage | V4 whole-entry revisions, V5 component revisions, atomic publication, mapped reads, history and compaction | One cooperating writer; local filesystem durability assumptions |
| Async storage | Opt-in bounded worker, cancellation-before-start, explicit outcomes and shutdown | Existing ORM RPC handlers remain synchronous; group commit is pending |
| Persistence | Owner-sealed SturdyRefs, factories, owner rotation, revocation and expiry | No generic capability-graph persistence or replicated realm |
| Application services | Bounded bulk, resumable durable bulk and replaceable realtime snapshots | Logical receipts/deadlines do not imply hard realtime or exactly-once execution |

Defaults are `quic-quiche`, `services` and `storage`; the QUIC feature transitively
enables `native` and `tls`. Plain TCP is available without defaults. Quiche is the
only QUIC engine. There is no actor runtime, placement service or activation scheduler.

Use [RPC applications](RPC-Applications.md), [transport setup](TCP-TLS-and-QUIC.md),
[native routes](Native-Transports.md), [storage](Storage-and-ORM.md) and the
[roadmap](Roadmap.md) for details. The superseded incremental checklist is retained
as a [historical record](../archive/Runtime-Status-History.md).
