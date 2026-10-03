# 0.1.0 developer preview (unpublished)

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
