# 0.1.0 developer preview (unpublished)

- Validate near struct pointers and their complete targets with one segment
  lookup, preserving traversal and bounds checks without caching movable storage.
  Install schema compiler prerequisites in the dedicated benchmark report job.

- Reuse outgoing RPC segments in a pool bounded to 128 KiB and 16 segments per
  connection, clearing used words before reuse. Avoid protected background-task
  allocation for immediately completed non-streaming, non-pipelined RPC methods.
  Pending methods retain cancellation protection and local calls remain deferred.
  Bound task admission and reap ready work before allocating scheduler nodes.

- Run the complete CI graph on PRs and pushes to main: validation, platform
  checks, Cargo/coverage, models, fuzzing and extended checks, then release
  benchmarks and cleanup. Fork/Dependabot PRs compile benchmarks without cloud
  credentials. Publish distinct lane artifacts from one CI run atomically to
  the reports branch; preserve independent graphs, schedules and manual runs.
- Inline the first message-builder segment's metadata, following C++, removing
  its metadata allocation while preserving multi-segment and external-buffer
  ownership. A fitting scratch builder now requires no heap allocations.

- Add opt-in split control/bulk planes on the existing authenticated quiche
  session. Single-use capability grants authorize bounded payload streams;
  capability RPC, pipelining, cancellation and publication stay ordered on
  stream 0. Preserve old-peer/TCP bulk fallback, reserve control credit, expose
  bulk counters and drain reliable streams before graceful connection close.
  See `docs/wiki/Split-Plane.md` for limits and completion semantics.

- Reuse RPC write-batch storage, keep small framing tables on the stack, and
  complete immediately-ready non-pipelined calls without a background completion
  task. Native transport samples application clocks only for pending datagrams
  and migration, plus deadlines for active bulk transfers. Keep single-segment receive metadata inline, retain one input
  cancellation registration per connection, generate QUIC packets directly into
  bounded send batches, and construct disabled-pipeline errors only on use.
  The 1.2× C++ latency target has not yet been met.
- Retain outgoing call admission permits in the queued write instead of a
  separate completion task on the built-in transports. Unobserved sends avoid
  completion channels. A separate connection-level write completion signal
  fails RPC calls promptly even if input and close remain blocked. The output
  closure fence still waits for close; the network driver waits for disconnect.
- Use bounded initial segments for unhinted calls and two-party responses, avoiding
  tiny multi-segment responses and oversized empty-call arenas. Honor explicit
  request size hints with space for the RPC envelope. Reap RPC tasks directly and reuse a local task
  admission queue, with wake and destructor callbacks outside queue borrows.
- Use bounded local byte streams between native RPC and its TCP/QUIC drivers.
  The existing local runtime can split the stream without mutexes, while keeping
  vectored writes, backpressure, cooperative scheduling, and half-close behavior.
  Peer receipt validation still determines graceful shutdown completion.
- Build local result wrappers only when an RPC pipeline can observe them, and
  reuse the call executor per connection. Tail-call redirection retains its
  explicit result ownership and cancellation behavior.
- Keep the first buffered input segment's range directly, following the pinned
  C++ implementation's common-segment optimization. Retained frames own their
  word storage without a separate reference-count allocation; short-lived views
  retain safe shared ownership. Preserve multi-segment bounds and empty segments.
- Avoid scheduling idle checks while a live import or export proves that an RPC
  connection remains active. Preserve deferred checks on final release and
  reentrant table changes, following the C++ connection's early activity check.
- Submit native TCP/TLS frame headers and payloads in one vectored write instead
  of three separate TLS writes. Preserve partial-write handling, exact wire bytes,
  flush errors, and the receipt acknowledgement fence without copying payloads.
- Imported the maintained Cap’n Proto Rust runtime, RPC engine, async framing and
  generator as `capntproto-{core,rpc,futures,codegen}` workspace crates, retaining
  upstream licenses and generated Rust import names.
- Replaced the Quiche fork with unmodified crates.io quiche 0.30.0. Explicit QUIC
  v2 requests fail without downgrade. Native QUIC uses `capntproto/3` and explicit
  receipt confirmations; conventional QUIC write shutdown now reports local FIN
  submission. Recovery uses upstream system time. TCP/TLS remains supported.
- Capability RPC runtime and field-oriented Rust generator with bounded TLC,
  Rust trace replay and selected C++ reference/interoperability checks.
- TCP/TLS and quiche QUIC v1 transport with native mutual authentication, dynamic schemas,
  bulk/realtime services and explicit three-party introductions.
- Typed durable objects, publication history, atomic batches and revocable realms.
- Opt-in bounded async storage owner for V4/V5, with size/principal admission
  budgets, queued cancellation/deadlines, unread reply accounting, I/O quarantine
  and drain/cancel shutdown. Existing ORM RPC handlers remain synchronous.
- Opt-in RPROTO05 component manifests share unchanged data, support atomic edits
  and publication, typed local reads and RPC facets, and rebase shared references
  during compaction. Existing RPROTO04 files remain supported by `Store::open`.
- Owned RPC connections and native vats with typed bootstrap access after startup,
  peer-specific bootstrap factories, async connector closures, capability joins
  and bounded shutdown. Applications can build higher-level runtimes on these
  APIs; the experimental actor workspace crate has been removed.
- Opt-in outgoing RPC Call count limits per connection, retained through
  question/write ownership, plus queued/active output metrics and protocol
  resource snapshots. Payload-byte and local unresolved-client budgets remain
  separate work; the default remains unlimited.
- Opt-in generated structured replies make writing and forwarding mutually
  exclusive and freeze results on pipeline publication. Typed tail calls,
  directly awaitable calls and consuming parameter-construction closures improve
  client/server ergonomics without changing the wire protocol.
- Storage format **RPROTO04** independently checks record framing before tail
  recovery and stabilizes the recovered file and directory before serving.
  Versions 1–3 are rejected; there is no automatic migration.
- Coordinated core/RPC/generator/futures dependencies work from an external
  application without root Cargo patches. Distribution is a source bundle.
- EAE comparison and private integration probe; the default backend remains
  the whole-entry store, with component storage available explicitly.
  See `docs/wiki/EAE-Comparison.md` and `docs/wiki/Component-Storage.md` for measurements.

- Canonical guides now live in `docs/wiki/`, with task-oriented navigation, a
  validated GitHub Wiki export, and an archive for superseded development ledgers.

This is an experimental preview, not complete C++ API parity or a production
security qualification. See `docs/wiki/Release-Acceptance.md` for outstanding gates.

## Review follow-up (2026-10-04)

- Replace repository Python tooling with the `capntproto-dev` workspace crate,
  invoked through `cargo run --locked -p capntproto-dev -- <subcommand>`. Port CI,
  reporting, cloud benchmark lifecycle, fuzzing, model generation and research
  probes, with Rust regression tests. Retire obsolete archived Python runners;
  historical measurements retain their original provenance.
- Use independent s2n-quic peers for QUIC v1 interoperability checks, including
  Retry, key rotation and stream shutdown. The production backend remains quiche.
- Separate generated CI evidence into an orphan `reports` branch. Keep the source
  README editable and discover documentation automatically instead of maintaining
  a duplicate page inventory. Existing measured history is retained as a frozen seed.
- Move the unimplemented Capnt Actors design to `docs/proposals/`; correct QUIC v1,
  unsupported v2 and the distinct native TCP/QUIC ALPN identifiers.
- Enforce unsafe documentation for application code; forbid unsafe in RPC/codegen/
  futures and track source-bound core-runtime documentation debt in CI. Document
  arena and descriptor ownership contracts without claiming a full unsafe audit.
- Split RPC dispatch and capability handling, and schema-node generation, into
  focused modules. Replace unresolved RPC size hints and clarify constructor checks.
- Document minimal TCP/TLS builds separately from QUIC and full verification tools.
  Retain nextest process isolation and the existing feature/transport contracts.
