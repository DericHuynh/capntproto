# C++ / Rust implementation parity

Source audit completed: **2026-09-23**. Reference: the clean local C++ checkout at
`0de72d8d8cec6b69edaa29de51d3bd490341f9c2`, matching
[the schema pin](../vendor/provenance/revision.json). Rust is the coordinated fork of
`capnp`, `capnp-rpc`, `capnp-futures` and `capnpc` under `vendor/`, plus the
application and transport modules under `src/`. It incorporates the upstream
Rust implementation; it is not a line-by-line translation of the entire C++ tree.
[Rust provenance](../vendor/provenance/capnp-rpc-revision.json) records that distinction.

**The core RPC operations are implemented, but this is not a complete C++ API
replacement or a proved equivalent runtime.** Every message arm explicitly
handled by the pinned C++ RPC dispatcher has a corresponding Rust dispatch path.
There are concrete public API, adapter and platform gaps below. Some Rust
features implement schema-defined operations which this C++ revision does not
implement; others are project-specific protocols.

This document replaces the previous incremental checklist. The comparison used
C++ headers **and implementations**, Rust entry points and dispatchers, and the
actual test/verification harnesses. Scope is Cap'n Proto serialization,
reflection, capabilities, RPC and adjacent adapters. The whole KJ library and
compiler toolchain are separately identified as outside the RPC-first port.
This is a feature/symbol audit, not an exhaustive equivalence result for every
overload, malformed input or asynchronous interleaving.

`[x]` means the described implementation exists within the stated boundaries.
`[ ]` means an identified implementation or verification gap. Tests listed as
evidence are available in the repository; their existence alone does not mean
the full suite was freshly run for this audit. Fresh results are recorded at
the end. There is no defensible overall parity percentage from this inventory.

## RPC protocol and capabilities

The primary comparison is [C++ rpc.c++](../vendor/capnproto/c++/src/capnp/rpc.c++)
(`handleMessage`, `handleCall`, `handleReturn`, capability tables) versus
[Rust rpc.rs](../vendor/capnp-rpc/src/rpc.rs)
(`handle_message`, call/return arms, `receive_caps`, `write_descriptor`).

| Status | C++ behavior | Rust implementation and evidence | Boundary |
|---|---|---|---|
| [x] | Bootstrap, Call, Return, Finish, Resolve, Release, Disembargo, Abort and Unimplemented dispatch | `ConnectionState` in [rpc.rs](../vendor/capnp-rpc/src/rpc.rs); [runtime](../tests/runtime.rs), [cancellation](../tests/cancellation.rs), [disconnect cleanup](../tests/disconnect_cleanup.rs) tests | Dispatch coverage does not exhaust all field combinations or races. |
| [x] | Hosted/promise imports and exports, reference counts, promised-answer calls and embargo ordering | [rpc.rs](../vendor/capnp-rpc/src/rpc.rs), [queued.rs](../vendor/capnp-rpc/src/queued.rs); [import aliases](../tests/import_alias.rs), [runtime](../tests/runtime.rs), [call hints](../tests/call_hints.rs) | Capabilities retain live hooks/tables; serialized pointer indices alone do not convey authority. |
| [x] | Tail calls, `sendResultsTo.yourself`, `resultsSentElsewhere`, `takeFromOtherQuestion` | [rpc.rs](../vendor/capnp-rpc/src/rpc.rs); [tail transfer](../tests/tail_transfer.rs), [cancellation](../tests/cancellation.rs) | Includes forwarding when a request resolves locally and redirected-answer ownership. |
| [x] | Streaming calls and pipeline-only/no-pipelining hints | [capability.rs](../vendor/capnp/src/capability.rs), [local.rs](../vendor/capnp-rpc/src/local.rs), [rpc.rs](../vendor/capnp-rpc/src/rpc.rs); [streaming](../tests/streaming.rs), [call hints](../tests/call_hints.rs) | Streaming acknowledgements and ordinary result-bearing calls have distinct completion contracts. |
| [x] | Cancellation policy, parameter release, retained contexts and dependent pipelines | [capability.rs](../vendor/capnp/src/capability.rs), [local.rs](../vendor/capnp-rpc/src/local.rs), [generator](../vendor/capnpc/src/codegen.rs); [cancellation](../tests/cancellation.rs), [static cancellation](../tests/static_cancellation.rs) | Rust futures and protected local calls require a driven executor; this does not reproduce KJ scheduling. |
| [x] | Per-peer bootstrap factory, incoming word limit, exception details/trace encoder, idle notifications | [RpcSystem/Connection](../vendor/capnp-rpc/src/lib.rs), [rpc.rs](../vendor/capnp-rpc/src/rpc.rs); [bootstrap factory](../tests/bootstrap_factory.rs), [incoming flow](../tests/incoming_flow.rs), [exception metadata](../tests/exception_metadata.rs), [trace](../tests/exception_trace.rs), [idle](../tests/idle.rs) | Incoming limits can block all further messages; this is not a deadlock-free admission scheduler. |
| [x] | Provide/Accept, third-party descriptors, deferred acceptance and ordered introductions | [rpc.rs](../vendor/capnp-rpc/src/rpc.rs), [third_party.rs](../vendor/capnp-rpc/src/third_party.rs); [runtime](../tests/runtime.rs), [deferred handoff](../tests/deferred_handoff.rs), [Noise multiparty](../tests/noise_multiparty.rs) | Requires VatNetwork introduction hooks. The ordinary two-party byte-stream adapter does not create third-party connections. |
| [x] | Local server identity, `thisCap`, shortening, server sets and revocable servers | [C++ capability API](../vendor/capnproto/c++/src/capnp/capability.h); [Rust server hooks](../vendor/capnp/src/capability.rs), [server set](../vendor/capnp-rpc/src/lib.rs), [revocable.rs](../vendor/capnp-rpc/src/revocable.rs); [server hooks](../tests/server_hooks.rs), [server set](../tests/server_set.rs), [revocable server](../tests/revocable_server.rs) | Rust uses owned/Rc handles and asynchronous lookup rather than C++ reference lifetimes. |
| [x] | Bidirectional membranes, reversal, substitutions, related policies, redirection and revocation | [C++ membrane](../vendor/capnproto/c++/src/capnp/membrane.c++); [Rust membrane](../vendor/capnp-rpc/src/membrane.rs); [membrane](../tests/membrane.rs), [runtime](../tests/runtime.rs), [FD](../tests/fd_capabilities.rs) | Calls/results/pipelines and standalone object copies cross the same boundary. |
| [x] | Public `copyIntoMembrane` / `copyOutOfMembrane` | [C++ helpers](../vendor/capnproto/c++/src/capnp/membrane.h); `Membrane::copy_into` / `copy_out` in [Rust](../vendor/capnp-rpc/src/membrane.rs), [orphan transformation](../vendor/capnp/src/dynamic_orphan/access.rs); [tests](../tests/membrane_copy.rs) | Copies generated/compiled dynamic readers, lists and AnyPointers into a scoped orphanage. Preserves nested/unknown capabilities and reverse crossings. Rust finishes structural copying before policy callbacks; inline groups use its existing known-field/active-arm rules. Details below. |
| [x] | Eager/lazy reconnect wrappers and reset | [C++ reconnect](../vendor/capnproto/c++/src/capnp/reconnect.c++); [Rust reconnect](../vendor/capnp-rpc/src/reconnect.rs); [tests](../tests/reconnect.rs) | Reconnection does not promise automatic replay or exactly-once external effects. |
| [x] | Standard `Persistent(SturdyRef, Owner)` interface | [C++ schema](../vendor/capnproto/c++/src/capnp/persistent.capnp); generated [Rust persistent module](../vendor/capnp-rpc/src/lib.rs); [runtime interface test](../tests/runtime.rs) | The durable realm/store below is a project implementation, not a C++ storage-engine port. |

### Schema-defined operations beyond this C++ implementation

These are not missing C++ ports, and Rust-only tests do not establish external
C++ interoperability for them.

| Operation | Actual pinned C++ behavior | Actual Rust behavior |
|---|---|---|
| General Join/distributed capability equality | `handleMessage()` has no Join case; its default sends Unimplemented. | [join.rs](../vendor/capnp-rpc/src/join.rs) implements local/bilateral equality; [multiparty_join.rs](../vendor/capnp-rpc/src/multiparty_join.rs) and [multiparty.rs](../vendor/capnp-rpc/src/multiparty.rs) support independently routed authenticated shares. Unsupported routes return errors, not inequality. [Tests](../tests/multiparty_join.rs). |
| Third-party answer adoption | `handleCall()` accepts only Caller/Yourself destinations; `handleReturn()` has no AwaitFromThirdParty case; the dispatcher has no ThirdPartyAnswer case. | [answer_adoption.rs](../vendor/capnp-rpc/src/answer_adoption.rs) and dispatch paths implement these schema operations when the network supports them. [Tests](../tests/answer_adoption.rs). |
| Active caller-pipeline migration through Join fencing | No corresponding general-Join path in the pinned dispatcher. | Rust fences old/new paths before migration and retains the old route on unsupported/failed setup. [Tests](../tests/noise_pipeline_migration.rs), [contract](ANSWER_ADOPTION.md). |

The reservation of `[2^30, 2^31)` for callee-allocated question IDs in Rust
answer adoption comes from [rpc.capnp](../vendor/capnproto/c++/src/capnp/rpc.capnp),
not a new wire-ID partition. Authentication/rendezvous tokens are VatNetwork-specific.

## RPC flow control, I/O and diagnostics

| Status | C++ implementation | Rust counterpart and evidence | Actual difference or limit |
|---|---|---|---|
| [x] | Fixed, variable and adaptive `RpcFlowController` in [rpc.c++](../vendor/capnproto/c++/src/capnp/rpc.c++) | [flow_control.rs](../vendor/capnp-rpc/src/flow_control.rs), [adaptive.rs](../vendor/capnp-rpc/src/flow_control/adaptive.rs); [differential tests](../tests/flow_control.rs) | TCP and Linux Unix adapters now default to a socket-informed variable window. Generic streams without metadata fall back to 64 KiB. Rust uses per-network policy setters instead of C++'s process-wide adaptive switch. |
| [x] | General byte-stream `TwoPartyClient` / `TwoPartyServer` in [rpc-twoparty.c++](../vendor/capnproto/c++/src/capnp/rpc-twoparty.c++) | [facade.rs](../vendor/capnp-rpc/src/twoparty/facade.rs); [tests](../tests/twoparty_facade.rs) | Owned/borrowed acceptance, listener cancellation, drain, remote/reverse bootstrap, disconnect observers, trace encoding and queue diagnostics. Rust explicitly polls a separate server driver; borrowed byte IO uses bounded copying. The same facade now accepts descriptor-aware networks. |
| [x] | `AsyncCapabilityStream` convenience overloads and `listenCapStreamReceiver` in [rpc-twoparty.c++](../vendor/capnproto/c++/src/capnp/rpc-twoparty.c++) | [Unix facade](../src/unix_rpc/facade.rs), [common transport facade](../vendor/capnp-rpc/src/twoparty/facade.rs); [Cargo/TLC/C++ tests](../tests/unix_facade.rs) | Linux/macOS Unix socket clients, owned/scoped borrowed accepts, listener, limits, drain, metrics and disconnect. Borrowed sockets use an owned duplicate; zero FD limit disables sending as well as receiving. Native macOS qualification remains pending; see the platform section below. |
| [x] | Queued batches, size/count/age in [rpc-twoparty.c++](../vendor/capnproto/c++/src/capnp/rpc-twoparty.c++) | [write_queue.rs](../vendor/capnp-futures/src/write_queue.rs), [twoparty.rs](../vendor/capnp-rpc/src/twoparty.rs); [queue](../tests/outgoing_queue.rs), [completion](../tests/output_completion.rs) tests | Pending metrics exclude active batches. Rust batches at driver polling, not KJ `evalLast`; throughput parity is not established. |
| [x] | Scatter/gather serialization and buffered reuse in [serialize-async.c++](../vendor/capnproto/c++/src/capnp/serialize-async.c++) | [asynchronous serialization](../vendor/capnp-futures/src/serialize.rs), [buffered_read.rs](../vendor/capnp-futures/src/buffered_read.rs); [buffered input](../tests/buffered_input.rs), [queue](../tests/outgoing_queue.rs) tests | Shared control-message storage, retained Call/Return storage, partial I/O and cancellation paths exist. Standalone and buffered readers accept caller word storage; Linux `FdReader` also accepts caller descriptor slots. Buffered direct reads still own their payload, as in pinned C++. |
| [x] | Descriptor-bearing messages and capability FDs | [C++ RPC](../vendor/capnproto/c++/src/capnp/rpc.c++), [Unix I/O](../vendor/capnproto/c++/src/kj/async-io-unix.c++); [Rust fd.rs](../vendor/capnp-rpc/src/fd.rs), [unix_rpc.rs](../src/unix_rpc.rs); [FD](../tests/fd_capabilities.rs), [buffered socket](../src/unix_rpc/buffered_tests.rs) tests | SCM_RIGHTS is implemented for Linux/macOS. Native macOS qualification remains pending; other Unix platforms are not enabled. |
| [x] | Capability wrapper debug descriptions | [C++ ClientHook](../vendor/capnproto/c++/src/capnp/capability.h); [Rust hook collector](../vendor/capnp/src/private/capability.rs); [debug tests](../tests/capability_debug.rs) | Rust limits collector traversal to 64 layers and can report busy state. Diagnostic text/type names are language-specific, not a stable protocol. |

## Dynamic reflection and result construction

| Status | C++ surface | Rust counterpart and evidence | Actual scope |
|---|---|---|---|
| [x] | SchemaLoader validation, version selection, stubs, native registration | [C++ loader](../vendor/capnproto/c++/src/capnp/schema-loader.c++); [Rust loader](../vendor/capnp/src/schema_loader.rs), [compatibility](../vendor/capnp/src/schema_loader/compatible.rs), [validation](../vendor/capnp/src/schema_loader/validate.rs); [tests](../tests/schema_loader.rs) | Binary schema nodes/compiler requests, transactional loads and resource bounds. The separate Rust frontend also provides validated, immutable [parsed-schema snapshots](../crates/capnp-compiler/README.md#runtime-reflection); it does not mutate a live parser through reader handles. |
| [x] | Generic brands, methods/superclasses, constants, annotations and union metadata | [C++ schema API](../vendor/capnproto/c++/src/capnp/schema.h); [compiled schemas](../vendor/capnp/src/schema.rs), [loaded brands](../vendor/capnp/src/schema_loader/brands.rs), [metadata](../vendor/capnp/src/schema_loader/metadata.rs); [metadata](../tests/schema_metadata.rs), [introspection](../tests/schema_introspection.rs), [lookup](../tests/reflection_lookup.rs) tests | Loaded/compiled APIs are separate; loader mutation requires an exclusive borrow. |
| [x] | Schema/member equality and hashing | [C++ schema.c++](../vendor/capnproto/c++/src/capnp/schema.c++); [identity.rs](../vendor/capnp/src/schema/identity.rs); [tests](../tests/schema_identity.rs) | Fallible snapshot keys retain owner/brand/member index; loaded keys borrow the loader. Construction has a 128-visit budget. |
| [x] | Enum casts and generic-scope enum identity | [C++ DynamicEnum](../vendor/capnproto/c++/src/capnp/dynamic.c++); [dynamic_value.rs](../vendor/capnp/src/dynamic_value.rs), [loaded casts](../vendor/capnp/src/schema_loader/dynamic/native.rs), [field generator](../vendor/capnpc/src/codegen/field_api.rs); [native enum](../tests/native_enum.rs), [enum brands](../tests/enum_brand.rs) | Individual casts check only enum ID. Open Rust enums preserve UInt16; closed enums return `NotInSchema`. Loaded identity/assignment retains wire brands while native casts erase them. |
| [x] | Dynamic field/list access, presence, unions and conversions | [C++ dynamic.c++](../vendor/capnproto/c++/src/capnp/dynamic.c++); [compiled structs](../vendor/capnp/src/dynamic_struct.rs), [lists](../vendor/capnp/src/dynamic_list.rs), [loaded views](../vendor/capnp/src/schema_loader/dynamic.rs); [presence](../tests/field_presence.rs), [conversion](../tests/dynamic_conversion.rs) tests | Explicit fallible `try_convert`; existing downcasts/setters are not silently coercing or universally transactional. |
| [x] | Native struct/list casts, dynamic capability casts and pipeline `releaseAs` | [C++ dynamic.h](../vendor/capnproto/c++/src/capnp/dynamic.h); [loaded type checks](../vendor/capnp/src/schema_loader/native.rs), [loaded conversions](../vendor/capnp/src/schema_loader/dynamic/native.rs), [compiled capabilities](../vendor/capnp/src/dynamic_capability.rs); [native lists](../tests/native_list.rs), [native RPC](../tests/native_rpc.rs) | Loaded named aggregates require native registration; casts erase generic arguments. Failed consuming conversions return the owner. |
| [x] | Orphan allocation, copy, adoption/disowning, resize, concat and external data | [C++ orphan.h](../vendor/capnproto/c++/src/capnp/orphan.h); [dynamic orphans](../vendor/capnp/src/dynamic_orphan.rs), [access](../vendor/capnp/src/dynamic_orphan/access.rs), [loaded orphans](../vendor/capnp/src/schema_loader/dynamic/orphan.rs), [field owners](../vendor/capnp/src/field_api/owners.rs); [access](../tests/orphan_access.rs), [concat](../tests/orphan_concat.rs), [external](../tests/orphan_external.rs), [loaded](../tests/loaded_orphans.rs) tests | Arena/context tokens and scoped editing replace C++ ownership syntax. External data is explicitly owned/static, not unrestricted borrowed memory. |
| [x] | `getResults`, `initResults`, size hints, orphanage and result adoption | [C++ capability.h](../vendor/capnproto/c++/src/capnp/capability.h); [Rust Results](../vendor/capnp/src/capability.rs), [root editor](../vendor/capnp/src/dynamic_orphan/root.rs), [dynamic contexts](../vendor/capnp/src/dynamic_capability.rs); [tests](../tests/result_construction.rs) | These APIs exist and must no longer be listed as generally missing. Rust caps advisory preallocation at `2^20` words; messages can grow further. |
| [x] | Independent PipelineBuilder, early publication and forwarding | [C++ capability.c++](../vendor/capnproto/c++/src/capnp/capability.c++); [pipeline_builder.rs](../vendor/capnp-rpc/src/pipeline_builder.rs), `set_pipeline_from` in local/RPC hooks; [tests](../tests/result_pipeline.rs) | Publication does not complete the call. Early/final capability consistency is a server obligation. Rust rejects a second publication; pinned C++ preserves the first without the same Rust error. |
| [x] | Typed/dynamic `sendIgnoringResult` | [C++ capability.h](../vendor/capnproto/c++/src/capnp/capability.h), [dynamic.h](../vendor/capnproto/c++/src/capnp/dynamic.h); [Rust requests](../vendor/capnp/src/capability.rs), [dynamic requests](../vendor/capnp/src/dynamic_capability.rs), [field API](../vendor/capnp/src/field_api.rs); [ignore-result](../tests/ignore_result.rs), [field RPC](../tests/field_api_rpc.rs) tests | Completion/error is still observed; this is neither a detached call nor a streaming send. |

### Deliberate differences from the C++ API

- Rust offers `get_or_load(&mut self, ...)`; C++ has a retained
  `LazyLoadCallback` and concurrent `loadOnce()` through a const loader. Rust
  does not promise mutation through live schema/view borrows.
- C++ `computeOptimizationHints()` caches hints. Rust's loaded
  `may_contain_capabilities()` walks schemas conservatively with a depth bound.
  The safety purpose exists; the API and cost differ.
- Schema identity snapshots and exact loaded assignments are distinct from
  native-cast compatibility. A same-ID native enum cast does not permit
  interchange of arbitrary loaded aggregate owners/brands.
- Closed Rust enums cannot safely represent arbitrary integer discriminants;
  generated open enums provide that C++ behavior without invalid Rust values.
- Conversion tests exclude C++'s undefined float-to-integer casts at `2^63`
  and `2^64`; Rust rejects those values. See the exclusions in
  [the differential adapter](../tests/conversion/verification.rs).
- Borrowed orphan ownership, null adoption and recovery are not identical to
  C++ exceptions/RAII. Rust preserves rejected owners and rejects foreign arenas
  even for null orphans; [result tests](../tests/result_construction.rs)
  distinguish these cases.
- The unchecked-schema message accessor and deprecated nongeneric
  `Schema::getDependency()` are not planned compatibility shims.

## Serialization and Rust code generation

- [x] Binary/packed framing, segment readers/builders, traversal limits and
  canonicalization exist in [message.rs](../vendor/capnp/src/message.rs),
  [serialize.rs](../vendor/capnp/src/serialize.rs),
  [serialize_packed.rs](../vendor/capnp/src/serialize_packed.rs) and
  [layout.rs](../vendor/capnp/src/private/layout.rs), corresponding to C++
  [message](../vendor/capnproto/c++/src/capnp/message.c++),
  [serialization](../vendor/capnproto/c++/src/capnp/serialize.c++) and
  [layout](../vendor/capnproto/c++/src/capnp/layout.c++). This does not establish equal
  fuzz coverage, allocation behavior or performance.
- [x] Native readers/builders/clients/servers, generics and RPC annotations are
  generated by [codegen.rs](../vendor/capnpc/src/codegen.rs). The field API,
  owned values, projections and open enums are additional Rust interfaces in
  [field_api](../vendor/capnpc/src/codegen/field_api.rs), with
  [compiler contracts](../tests/tooling.rs) and [field tests](../tests/field_api.rs).
- [x] **Structural message equality.** C++ [any.c++](../vendor/capnproto/c++/src/capnp/any.c++)
  now has Rust counterparts: `Equality::{Equal,NotEqual,UnknownContainsCapabilities}`,
  [`AnyPointer::Reader/Builder::equals`](../vendor/capnp/src/any_pointer.rs), and
  [`raw::struct_equals` / `raw::list_equals`](../vendor/capnp/src/raw.rs).
  Generated and compiled/loaded dynamic readers share the same
  [comparison implementation](../vendor/capnp/src/private/layout/equality.rs).
  It ignores trailing zero/null struct fields and unused bit-list padding,
  preserves physical list encoding/count distinctions, and compares floating
  point storage by bits. Capability pairs remain unknown even for the same
  hook; a definite difference elsewhere wins. Hooks and capability tables are
  not inspected. This does not perform distributed capability Join.
  Reader errors propagate until the first definite difference; comparison is
  not full validation of unequal inputs. Use bounded readers for untrusted
  messages. The pointer-section list metadata now records one pointer per
  element; [tests](../tests/structural_equality.rs) cover that correction.
  Schema-free struct and list allocation/views are implemented below.
- [x] **Public schema-free struct views.** Rust
  [`any_struct::Reader` / `Builder`](../vendor/capnp/src/any_struct.rs) implement
  C++ `AnyStruct` data/pointer sections, size, equality, canonicalization and
  generated/compiled-dynamic views. `AnyPointer::init_as_any_struct()` allocates
  explicit word/pointer counts; `get_pointer_type()` exposes pointer kinds.
  Section pointers retain capabilities. Copies preserve physical unknown fields.
  Writable casts reject insufficient storage instead of exposing unchecked
  setters; read-only casts supply schema defaults. Reader erasure also accepts
  loaded schemas, and loaded builders provide consuming `into_any_struct()`
  erasure. Details and bounded verification below.
- [x] **Public schema-free list views.**
  [`any_list`](../vendor/capnp/src/any_list.rs) and
  [`any_struct_list`](../vendor/capnp/src/any_struct_list.rs) implement the C++
  `AnyList` / `List<AnyStruct>` view family: physical encoding/count, size,
  data-only raw bytes, equality, struct-element access and explicit allocation.
  Native/compiled-dynamic casts check layout and writable section sizes;
  schema erasure and copying preserve unknown fields and capabilities.
  Reader and builder erasure accept loaded schemas. Bit/non-bit reinterpretations are
  rejected. The pointer-list builder/reader offset bug is repaired in Rust and
  separately reproduced in pinned C++; the comparison boundary is detailed below.
- [ ] The schema-language compiler is an **experimental Rust frontend**, not fully qualified for parity.
  The new [capnp-compiler crate](../crates/capnp-compiler/README.md) compiles
  structs/enums, unions/groups, imports/aliases, annotations, generic structs/interfaces
  and methods, brands, nested references, lists, scalar and composite constants/defaults into `CodeGeneratorRequest`, including
  deterministic IDs, wire layout, embedded files, extended string forms, documentation
  comments, node/member byte ranges and per-file identifier references.
  [CompilerCommand::run](../vendor/capnpc/src/lib.rs) invokes external
  `capnp compile` for the vendored RPC crate and downstream example. Root and
  test-support builds use Rust compilation; both Rust APIs render schema Rustdoc.
  [Parsed-schema snapshots](../crates/capnp-compiler/README.md#runtime-reflection)
  connect textual compilation to runtime reflection, direct nested lookup and
  dynamic messages. `SchemaSession` adds lazy declaration/alias loading with an
  exclusive borrow and transactional validation. `ConcurrentSchemaParser` adds
  worker-owned sessions, bounded request queues and immutable shared snapshots.
  Distributable frontend packaging and mutation through C++-style shared runtime
  handles remain open; [grammar qualification](SCHEMA_COMPILER_QUALIFICATION.md)
  records the tested corpus and remaining differences.
  The crate is separate from the RPC-first runtime port.

## Concrete remaining implementation work

These entries replace the previous unspecific “remaining reflection audit.”
Priority here concerns C++ runtime/API replacement, not the product roadmap.

- [x] **Automatic socket-informed flow policy.** The
  [TCP adapter](../src/rpc/tcp.rs) and [Linux Unix transport](../src/unix_rpc.rs)
  query live `SO_SNDBUF`, share a sticky 64 KiB fallback across streams, and
  skip queries when the largest-message allowance suffices or the stream failed.
  The generic network's `new_with_send_buffer()` exposes the transport hook;
  opaque byte/Noise streams have no socket metadata and use the fallback.
  Explicit policy overrides remain available on the byte-stream network.
  Linux socket tests and bounded TLC/pinned C++ comparisons are recorded below;
  cross-platform TCP execution and throughput parity are not established.
- [x] **Caller-supplied asynchronous word storage.**
  [`read_message_with_scratch` / `try_read_message_with_scratch`](../vendor/capnp-futures/src/serialize.rs)
  mirror the pinned C++ standalone async readers: use caller words when the
  payload fits, otherwise allocate owned payload storage. The framing table
  does not consume scratch capacity. Rust borrows scratch for the reader's
  lifetime, including the fallback case; segment metadata still allocates.
  [Native and pinned-C++ checks](../tests/async_scratch.rs) cover framing,
  fallback, limits, errors and reuse. Allocation budgets are checked separately.
- [x] **Scratch integration in buffered/descriptor adapters.** C++
  [`MessageStream::tryReadMessage`](../vendor/capnproto/c++/src/capnp/serialize-async.h)
  has counterparts in Rust's
  [buffered input](../vendor/capnp-futures/src/buffered_read.rs) and Linux
  [`FdReader`](../src/unix_rpc.rs). Fitting retained buffered frames borrow caller
  words (including framing); direct reads remain owned. Caller FD slots receive
  bounded, move-only descriptors with excess closed. Pending input/FDs survive
  canceled reads inside the reader. Existing RPC receive methods keep owned
  messages. [Contract and evidence](#buffered-and-descriptor-scratch-checks-2026-09-28).
- [ ] **Non-Linux ancillary transport qualification.** The macOS implementation
  and native CI tests are in place; actual macOS execution remains pending.
  [Platform details](#macos-descriptor-transport-2026-09-29) distinguish local
  cross-checks from native qualification. Other Unix platforms remain disabled.

### Optional C++ adapters and codecs

The opt-in [capnp-compat crate](../crates/capnp-compat/README.md) implements the
following compatibility APIs. These adapters are not dependencies of ordinary
capability RPC. The checkmarks describe the documented port scope and bounded
tests; they do not certify arbitrary KJ scheduling or application integrations.

| Status | C++ implementation | Rust counterpart |
|---|---|---|
| [x] | [`ByteStreamFactory`](../vendor/capnproto/c++/src/capnp/compat/byte-stream.h) | [`byte_stream`](../crates/capnp-compat/src/byte_stream.rs): explicit end/abort, futures I/O, optional executor-owned legacy EOF, TLS callback, local unwrapping, substreams and path resolution. |
| [x] | [`HttpOverCapnpFactory`](../vendor/capnproto/c++/src/capnp/compat/http-over-capnp.h) | [`http`](../crates/capnp-compat/src/http.rs): level-2 pipelined bodies, common headers, fixed/unknown lengths, upgrades and CONNECT. |
| [x] | [`WebSocketMessageStream`](../vendor/capnproto/c++/src/capnp/compat/websocket-rpc.h) | [`websocket`](../crates/capnp-compat/src/websocket.rs): bounded binary-message framing and futures I/O for two-party RPC. |
| [x] | [`JsonCodec`](../vendor/capnproto/c++/src/capnp/compat/json.h) | [`json`](../crates/capnp-compat/src/json.rs): dynamic values, annotation/type/field handlers, owned orphan decode, compact/pretty output and input/work limits. |
| [x] | [`JsonRpc`](../vendor/capnproto/c++/src/capnp/compat/json-rpc.h) | [`json_rpc`](../crates/capnp-compat/src/json_rpc.rs): bidirectional named calls, notifications, errors, cancellation and Content-Length framing; no capabilities or batches. |
| [x] | [`TextCodec`](../vendor/capnproto/c++/src/capnp/serialize-text.h) | [`text`](../crates/capnp-compat/src/text.rs): dynamic text encode/decode through the Rust compiler lexer, standalone orphan values and pretty output. |
| [ ] | [`SchemaParser`](../vendor/capnproto/c++/src/capnp/schema-parser.h), [implementation](../vendor/capnproto/c++/src/capnp/schema-parser.c++): schemas/imports | The separate [Rust frontend](../crates/capnp-compiler/README.md#runtime-reflection) provides snapshots with metadata/dynamic messages, lazy declaration/alias loading, and [concurrent caches](../crates/capnp-compiler/README.md#concurrent-parser-caching). [Custom callbacks](../crates/capnp-compiler/README.md#custom-source-providers) supply identity/import resolution and bounded readers. [Optional file IDs](../crates/capnp-compiler/README.md#optional-file-ids) support configuration schemas. Cache handles retain immutable generations; runtime reflection materializes locally. [Qualification](SCHEMA_COMPILER_QUALIFICATION.md) records remaining grammar/metadata differences. |

The rest of KJ, C++ generators and CLI commands are outside RPC-first scope.
Their absence is different from a missing RPC message handler.

## Rust-specific features and residual limits

These implementations must not inflate C++ parity:

| Feature | Actual location | Relationship to C++ |
|---|---|---|
| Noise transport, authenticated introductions, discovery/NAT, mobility and provisioning | [transport.rs](../src/transport.rs), [noise_rpc](../src/noise_rpc.rs), [discovery](../src/noise_discovery.rs), [NAT](../src/nat.rs), [quiche fork](../vendor/quiche/quiche) | Project transport/profile. `Noise_IK_25519_ChaChaPoly_BLAKE3` is explicit in transport code, with Snow features in [Cargo.toml](../Cargo.toml). No equivalent transport in this C++ checkout. |
| Dynamic schema transmission | [schema_exchange.rs](../src/schema_exchange.rs) | Project RPC service, not a port of an existing C++ schema-exchange service. |
| Bulk/durable bulk and realtime snapshots/datagrams | [bulk.rs](../src/bulk.rs), [durable_bulk.rs](../src/durable_bulk.rs), [realtime.rs](../src/realtime.rs), [datagrams](../src/realtime_datagram.rs) | Additional schemas/semantics; not C++ ByteStream/HTTP compatibility. |
| ORM get/put/publish/subscribe, mutable whole-entry storage, history and durable realms | [orm.rs](../src/orm.rs), [storage.rs](../src/storage.rs), [persistence.rs](../src/persistence.rs) | Application/storage implementation. The standard Persistent interface does not specify this engine. |

The source audit also tracks Rust-specific options separately from missing C++
runtime features:

- [x] Optional `rpc_try` now supports synchronous `Result?` through
  `FromResidual` with error conversion. The panic-only `Promise: Try`
  implementation is removed; direct `promise?` fails compilation and async
  code uses `.await?`. See the [feature contract and checks](#rpc-try-contract-checks-2026-09-28)
  for this deliberate experimental-API change.
- [x] The generator `capnp_root` override now covers Text/Data lists and
  nongeneric interface ownership, and is exposed through `CompilerCommand`.
  [Downstream tests](../tests/capnp_root.rs) compile and run default, renamed,
  re-exported and bootstrap runtime paths through both compiler entry points;
  see [runtime-path verification](#generated-runtime-path-checks-2026-09-28).

Not every “unsupported” string is a gap: C++ dynamic lists also reject
`List(AnyPointer)`, and generated default server methods returning Unimplemented
are normal dispatch fallbacks. These were checked against their call sites.

## Verification: what is actually established

The repository contains three different forms of evidence:

1. **Rust runtime tests**, including real `RpcSystem` instances, sockets,
   capability calls and generated-code compilation. For example,
   [tests/support](../tests/support/mod.rs) transports real runtime messages,
   rather than only testing another handwritten state machine.
2. **Pinned C++ differential/reference tests.**
   [The harness](../test-support/src/verification/cpp.rs) checks reference-source
   hashes and builds selected libraries/tests. C++ self-tests alone do not
   compare Rust. Programs in [tests/cpp](../tests/cpp) compare observations where
   invoked by Rust tests. `installed_cpp_rpc_interoperability` in
   [tooling.rs](../tests/tooling.rs) instead uses installed C++ libraries;
   that is a different reference.
3. **Bounded TLA+/TLC component checks and trace replay.**
   [exploration.rs](../test-support/src/verification/exploration.rs) produces one
   reachable prefix ending at each explored edge, not every possible execution.
   Fault controls mutate models; they do not establish Rust mutation coverage.
   Bounds and fairness assumptions matter.

Representative source-to-verification mappings:

| Area | Rust/C++ comparison entry point | TLA+ component |
|---|---|---|
| Two-party connection ownership and drain | [Rust replay](../tests/twoparty_facade/verification.rs), [C++ oracle](../tests/cpp/twoparty-facade.c++) | [TwoPartyFacade](../verification/TwoPartyFacade.tla) |
| Descriptor-aware facade, limits and retained authority | [Rust replay](../tests/unix_facade/verification.rs), [C++ listener/accept oracle](../tests/cpp/unix-facade.c++) | [UnixFacade](../verification/UnixFacade.tla) |
| RPC windows | [flow_control.rs](../tests/flow_control.rs) | [variable](../verification/RpcVariableWindow.tla), [adaptive](../verification/RpcAdaptiveWindow.tla) |
| Socket-informed window, query timing and shared fallback | [Rust replay and socket tests](../tests/flow_control/socket_window.rs), [C++ network oracle](../tests/cpp/socket-window.c++) | [RpcSocketWindow](../verification/RpcSocketWindow.tla) |
| Queue/batching and buffered input | [outgoing_queue.rs](../tests/outgoing_queue.rs), [buffered_input.rs](../tests/buffered_input.rs), [Unix buffers](../src/unix_rpc/buffered_tests.rs) | [queue](../verification/RpcOutgoingQueue.tla), [input](../verification/RpcBufferedInput.tla), [FD input](../verification/RpcBufferedFds.tla) |
| Diagnostics, presence, conversions | [capability_debug.rs](../tests/capability_debug.rs), [field_presence.rs](../tests/field_presence.rs), [dynamic_conversion.rs](../tests/dynamic_conversion.rs) | [debug](../verification/RpcDebugInfo.tla), [presence](../verification/DynamicFieldPresence.tla), [conversion](../verification/DynamicConversion.tla) |
| Reflection identity/lookup, native casts | [schema_identity.rs](../tests/schema_identity.rs), [reflection_lookup.rs](../tests/reflection_lookup.rs), [native_enum.rs](../tests/native_enum.rs), [native_list.rs](../tests/native_list.rs), [native_rpc.rs](../tests/native_rpc.rs) | [identity](../verification/SchemaMemberIdentity.tla), [lookup](../verification/SchemaLookup.tla), [enums](../verification/NativeEnumCast.tla), [lists](../verification/NativeListCast.tla), [RPC casts](../verification/NativeRpcCast.tla) |
| Structural message equality | [Rust replay](../tests/structural_equality/verification.rs), [C++ oracle](../tests/cpp/structural-equality.c++) | [StructuralEquality](../verification/StructuralEquality.tla) |
| Membrane object copies | [Rust replay](../tests/membrane_copy/verification.rs), [C++ oracle](../tests/cpp/membrane-copy.c++) | [MembraneCopy](../verification/MembraneCopy.tla) |
| Schema-free struct views | [Rust replay](../tests/any_struct/verification.rs), [C++ oracle](../tests/cpp/any-struct.c++) | [AnyStruct](../verification/AnyStruct.tla) |
| Schema-free list views | [Rust replay](../tests/any_list/verification.rs), [C++ oracle](../tests/cpp/any-list.c++); projection discrepancy explicitly tested | [AnyList](../verification/AnyList.tla) |
| Results, independent pipelines, ignore-result calls | [result_construction.rs](../tests/result_construction.rs), [result_pipeline.rs](../tests/result_pipeline.rs), [ignore_result.rs](../tests/ignore_result.rs) | [ownership](../verification/RpcResultOwnership.tla), [pipelines](../verification/RpcIndependentPipeline.tla), [ignore result](../verification/RpcIgnoreResult.tla) |

- [ ] **Composed-model wire/local-state refinement remains incomplete.**
  The original `WireReleaseEffect` and `WireRequestMethod` diagnostics were
  repaired on 2026-09-25; [conformance checks](../tests/conformance.rs) run all
  12 requirements without ignores. A relation for generated annotations and
  request identity, annotation-isolation checks, and
  [native replay](../tests/composed_wire_boundary.rs) now cover explicit Release
  and method/data dispatch. Implicit releases, paired identity and other
  metadata still use auxiliary state. This is not whole-model refinement or a
  new C++ differential result. [Exact scope](CONFORMANCE.md).
- [ ] **No full Rust/C++ runtime equivalence proof.** Component tests do not
  exhaust arbitrary proxy chains, cancellation/disconnect/reconnect schedules,
  hostile wire data, ID reuse or memory-ownership combinations.
- [ ] **No external pinned-C++ Join/third-party-answer interoperability proof.**
  The reference lacks those operations. Rust-to-Rust tests and schema
  correspondence are separate evidence.
- [ ] **No whole RPC/Noise/schema/bulk/realtime/storage composition proof.**
  Cryptography, OS/network behavior and arbitrary durable-storage failures are
  outside the finite component models. [Verification backlog](ROADMAP.md#verification-and-interoperability-gaps).

### Checks rerun for this source audit

| Command | Observed result on 2026-09-22 |
|---|---|
| `git -C vendor/capnproto rev-parse HEAD` and `git -C vendor/capnproto status --short` | Pinned commit above; clean C++ working tree. |
| `cargo test --locked --test tooling schema_pin_and_wire_inventory -- --exact --nocapture` | Passed; checks pins/discriminants, not protocol semantics. |
| `cargo test --locked --test conformance -- --nocapture` | 7 passed, 2 explicitly ignored. |
| `cargo test --locked --test conformance -- --ignored --nocapture` | Both diagnostics failed as described; Cargo exits 101, TLC reports invariant violations. |

Those two rows record the 2026-09-22 audit. On 2026-09-25, handler repairs and
new checks superseded the diagnostic result: 12 passed, none ignored, plus
279 states / 1,092 Rust edge-prefix replays and three model fault controls.

The source-audit pass itself changed no runtime implementation; the full workspace,
C++ differential and release suites were not rerun for that pass. Historical state counts
are not summed into parity or code coverage. Use [TESTING.md](TESTING.md) for
component commands and [RELEASE_ACCEPTANCE.md](RELEASE_ACCEPTANCE.md) for the
separate release gates.

### Structural equality implementation checks (2026-09-23)

`cargo test --locked --test structural_equality --test native_list --test field_presence -- --nocapture`
passes 24 tests. The new `StructuralEquality` TLC exploration has **3,925 states
and 3,924 edge-prefix traces**. Its 36 representative values cover all ordered
pairs in both directions: **2,592 Rust/C++ comparisons**, plus **181 supplementary
comparisons** for primitive widths, floating bits, far pointers, malformed data,
reader limits and aggregate helpers. All eight named model fault controls fail
the intended invariant. Native tests also cover live capability hook tripwires,
authority release, unknown schema fields and both dynamic APIs.

The formatting/Clippy/workspace-doctest Cargo gate passes, as do 15 capnp
doctests (four existing examples remain ignored). Both alloc-only and
no-default-feature capnp builds pass; the latter retains the pre-existing
unused `fmt` warning. The full workspace runtime and release suites were not
rerun for this component change.

The finite model checks structural comparison of bounded acyclic values; it
does not establish arbitrary-message safety, an exhaustive parser result,
capability identity or RPC scheduling. Error comparisons check success versus
failure, not identical exception text or exact traversal accounting.

### Membrane copy implementation checks (2026-09-23)

The new `Membrane::copy_into` and `copy_out` methods return message-scoped
`dynamic_orphan::Orphan` owners. Pass an `Access` from the destination's orphanage;
its message must have a capability table when copying capabilities. Normal
adoption/type/arena checks still apply. The source can be discarded after copying.
Capabilities use the existing import/export machinery: opposite crossings unwrap,
ordinary wrappers are revoked, and policy substitutions retain their own rules.
Copying data remains possible after revocation; a substitution error becomes a
broken capability. Neither copying nor adoption calls the service or awaits its
promise resolution.

`Access::copy_with_capability_transform` first copies the complete value, then
transforms its privately owned hooks before returning the orphan. Structural
errors therefore occur before callbacks, unlike C++'s transformation during
extraction. Callback errors/unwinding release the unpublished copy; callback
side effects cannot be rolled back. Parent siblings are excluded when copying
Rust inline groups; independent aggregates preserve unknown fields. Loaded
values can cross through their underlying AnyPointer, but these methods do not
return loaded-schema orphan owners.

Eight new [Cargo tests](../tests/membrane_copy.rs) cover these contracts, including
unknown fields, repeated/nested references, rejected foreign adoption, promise
polling, substitutions and failure cleanup. Together with `membrane`,
`result_construction`, `orphan_access` and `orphan_groups`, **37 distinct tests
pass**. Formatting, Clippy, workspace doctests, 15 capnp doctests and alloc-only /
no-default-feature builds pass (the latter retains the existing unused `fmt`
warning). This is not a full workspace runtime or release qualification run.

Fresh `MembraneCopy` TLC exploration has **91 states / 140 edge-prefix traces**.
Replaying each prefix with struct, struct-list, AnyPointer and capability-list
payloads yields **560 scenarios / 1,960 matching Rust/C++ observations**.
All eight named model fault controls are detected. The model bounds execution
to four ownership/copy/revoke actions on one service, with calls settled between
actions. It checks directions, reversal, source preservation and authority
lifetime; arbitrary pending RPC schedules, policy substitutions and malformed
payloads are covered by native tests rather than this finite model.

### Schema-free struct implementation checks (2026-09-23)

`any_struct::Reader` and `Builder` expose physical data and pointer sections
without requiring a schema. `AnyPointer::init_as_any_struct(data_words,
pointer_count)` zero-initializes an explicit layout. Mutable section borrows are
exclusive; raw bytes never expose pointer slots. Pointer sections use ordinary
AnyPointer access, including capability extraction, replacement and clearing.
`get_pointer_type()` follows far pointers but does not validate an entire target.

Readers can erase generated, compiled-dynamic and loaded-schema struct views;
builders can erase generated and compiled-dynamic views. `get_as()` and
`get_as_dynamic()` attach generated/compiled schemas. Read-only casts retain
schema-evolution defaults. Writable casts require both physical sections to fit
the requested schema and return `TypeMismatch` otherwise, without growing or
mutating storage. This check replaces C++'s unchecked layout assumption. Use
the owning pointer's typed getter when storage needs upgrading.

Pass readers to setters with `any_pointer::Owned`; no fictitious named struct
schema is introduced for introspection. Copying keeps unknown physical fields
and capability references, independent of the source's lifetime. Canonicalization
returns a root-prefixed single segment, trims trailing zero/null sections and
rejects capabilities. It retains normal traversal/nesting checks. The existing
AnyPointer pipeline implementation is also exposed as `any_struct::Pipeline`.
Schema-free list wrappers are described below. Neither view family introduces
loaded dynamic builder conversions or new schema-free orphan owner types.

`cargo test --locked --test any_struct --test structural_equality --test membrane_copy --test orphan_access -- --nocapture`
passes **32 distinct tests**, including seven new struct-view tests. Native
regressions cover undersized casts, generated/dynamic/loaded erasure, unknown
fields, capability calls after source disposal, malformed/cyclic messages,
far pointers, reader limits, and primitive-list virtual struct elements.
Formatting, Clippy, workspace doctests, 17 standalone capnp doctests, and
alloc-only / no-default-feature builds pass. The latter retains the existing
unused `fmt` warning. No full release qualification was rerun for this change.

Fresh [AnyStruct](../verification/AnyStruct.tla) exploration finds **1,196 states
/ 3,045 edge-prefix traces**. Rust replays every prefix and compares **11,541
observations** with the pinned C++ `AnyStruct` implementation. The model bounds
each scenario to four operations, zero to two data words and pointer slots,
first/last byte writes, and null/data/capability pointers. It checks section
layout, independent copies and canonicalization; seven named fault controls
are detected. Capability slots are exercised with actual service calls during
replay. Arbitrary layouts, malformed inputs, Rust borrowing and executor/RPC
schedules are outside this finite model; the native and compile-fail tests
cover selected cases. These checks do not establish complete C++ parity.

### Schema-free list implementation checks (2026-09-23)

`any_list::Reader` / `Builder` and `any_struct_list::Reader` / `Builder` expose
all eight physical list encodings, size/count, equality, borrowed data-only raw
bytes, and schema-free struct elements. `AnyPointer::init_as_any_list()` and
`init_as_list_of_any_struct()` allocate explicit layouts and reject wire
count/size overflow before replacing the existing value. Struct-list readers
iterate fallible element reads; builders use sequential exclusive reborrows.
Compatible primitive/pointer lists expose virtual struct sections without
upgrading storage. Bit lists cannot become struct elements.

`get_as()` attaches native list types, including primitive, enum, text/data,
nested, struct, AnyPointer and capability lists; `get_as_dynamic()` accepts a
compiled element type. Casts validate storage, and writable struct views require
both sections to fit. Missing read-only struct fields use schema defaults.
These views do not validate pointed-to children until accessed. The null/empty
Void view has no elements and permits primitive/pointer views; writable struct
casts still check section sizes. New checked casts reject bit/non-bit
reinterpretation. Ordinary typed readers retain C++'s more permissive behavior
for reading larger primitive encodings as bits; ordinary writable getters now
match C++'s rejection. The model checks the two getter contracts separately.

Erasing native or compiled-dynamic builders preserves the whole physical list;
reader erasure also accepts loaded-schema lists. Copies use `any_pointer::Owned`
and retain unknown fields and capability references after source disposal.
Raw byte access rejects elements with pointer slots, including null slots and
zero-length pointer lists. Size includes the inline-composite tag and nested
objects, and counts capability occurrences. Rust charges child traversal during
size calculation, consistent with its existing struct size helper; C++ refunds
that accounting. Exact exhaustion points can therefore differ. Nesting failures
on schema-free element reads remain errors; casts to infallibly indexed native
struct lists check the extra nesting step before returning the view.

The underlying pointer-list builder representation now retains the complete
element base. Pointer access applies its data offset once, so `into_reader()`
and schema erasure keep valid pointers and unknown data. Pinned C++ still shifts
the base in its typed writable getter and adds the offset again in its reader.
The [C++ regression mode](../tests/cpp/any-list.c++) explicitly reproduces that
upstream bug. For that trace action, the oracle writes through C++'s typed
`List<Data>` getter and observes through a fresh root reader; Rust exercises the
repaired builder-to-reader conversion directly. C++ `List<AnyPointer>` has no
root getter, so the typed data list supplies the equivalent pointer layout.
This is comparison of resulting message contents with a documented adapter,
not a claim that the broken C++ conversion behaves identically.

The [Cargo tests](../tests/any_list.rs) include nine new cases covering all
encodings, safe casts, sub-word virtual elements, unknown fields, capability
calls after source disposal, overflow, maximum zero-sized list counts, loaded
erasure, far pointers, malformed/cyclic inputs and reader limits. Compile-fail
doctests reject overlapping mutable element views. Separate schema-free loaded
builder and orphan owner types are not introduced.

`cargo test --locked --test any_list --test any_struct --test native_list --test structural_equality --test orphan_access --test orphan_groups -- --nocapture`
passes **47 regression tests**. The standalone capnp crate also passes **39 unit
tests / 19 doctests** (four existing examples remain ignored). Formatting,
Clippy, workspace doctests and alloc-only / no-default-feature checks pass; the
no-default-feature build retains the existing unused `fmt` warning. This was
not a full workspace runtime or release qualification run.

Fresh [AnyList](../verification/AnyList.tla) exploration finds **2,858 states /
8,704 edge-prefix traces** and produces **32,628 matching observations** using
the C++ comparison boundary above. All ten named model fault controls are
detected. Each scenario has at most four actions, twelve possible physical
layouts and zero to two elements. Writes touch the
first/last data byte (bits for packed lists) or pointer slot. Copying, clearing,
raw byte inspection, pointer-list conversion and ordinary bit casts are modeled;
capability pointers make actual service calls during Rust/C++ replay. Native
tests cover selected cases beyond those bounds. This is bounded conformance,
not a proof of arbitrary schemas, executor schedules or the full RPC protocol.

### Two-party facade implementation checks (2026-09-23)

[The public facade](../vendor/capnp-rpc/src/twoparty/facade.rs) implements general
byte-stream client/server conveniences from C++ `rpc-twoparty.{h,c++}`.
`TwoPartyClient` drives one connection, exposes the remote bootstrap on either
side, allows a local bootstrap for callbacks, and provides disconnect, explicit
shutdown, trace-encoder and outgoing-queue handles. The driver releases runtime
ownership before waking disconnect observers. Output failure ends the facade
even when the peer keeps input open. The underlying `VatNetwork`'s separate
output-fence and connection-lifetime contracts are unchanged.

`TwoPartyServer::new()` returns an accept handle and a `ServerDriver`; Rust
requires explicit polling. The driver owns accepted streams. Dropping it
cancels those connections; dropping the accept handle stops new acceptance and
lets the driver finish when its connections drain. Borrowed accepts are driven
separately and do not delay drain. Their drivers may outlive the accept handle.
An already-empty drain completes immediately, including when another connection
is accepted before that future is polled. Pending drains include newly accepted
owned connections. Multiple drain observers are supported; C++ restricts its
`TaskSet::onEmpty()` to one pending observer.

The scoped borrowed adapter retains no borrowed pointer in a `'static` task:
it stages at most 16 KiB in each direction and polls the real stream only inside
the borrowed driver. It adds copying and does not claim C++ performance parity.
Cancellation releases the borrow without destroying or closing the caller's
stream; normal shutdown may close its write half. Already-consumed input is
not returned. Owners must still close or continue reading a completed borrowed
stream if the peer needs to flush shutdown output. Owned streams use the normal
network without this adapter.

The listener accepts a Rust stream of IO results: errors propagate, dropping
the listener future stops acceptance without canceling accepted connections,
and finite listener EOF completes. C++ socket listeners have no EOF operation.
Connection failures are isolated and available through an explicit error
callback; the default ignores them instead of logging like C++. The Tokio
[helpers](../src/rpc.rs) now accept any compatible ordered stream and use this
facade, including the existing Noise duplex adapter.

The [Cargo tests](../tests/twoparty_facade.rs) cover a real TCP listener,
cancellation before and after pending IO, empty/pending drains, overlapping
owned connections, retained capabilities/observers, reverse bootstrap,
capabilities invoked and returned across RPC, 100,000-byte responses through a
37-byte pipe, exception traces and read/write failure isolation. Compile-fail
documentation checks borrowed-stream lifetime enforcement.

Fresh [TwoPartyFacade](../verification/TwoPartyFacade.tla) exploration finds
**185 states / 347 edge-prefix scenarios**, with **1,455 matching Rust/C++
observations**. All six named model faults are detected. The model bounds each
scenario to five actions, one owned and one borrowed connection, one call per
connection and one drain observer. It checks ownership, owned-only drain,
capability calls, peer closure, borrowed cancellation and owner destruction.
Each operation settles IO before observing state. The C++ oracle uses real
socket pairs and polls the KJ IO event port; scheduling only `evalLater` callbacks
is insufficient to observe socket closure. This does not verify arbitrary IO
interleavings, listener implementations, descriptor transport, performance or
the composed RPC protocol. The composed-model limitations above remain open.

Validation: **32 targeted regression tests pass** across `twoparty_facade`,
`rpc`, `output_completion`, `outgoing_queue` and `disconnect_cleanup`, including
the eight new facade tests and existing Noise/UDP capability handoffs. The final
facade rerun passes all eight. The repository formatting/Clippy/workspace-doctest
gate passes, including both new facade doctests; four pre-existing workspace
examples remain ignored. No full workspace runtime or release qualification run
was performed for this change.

### Descriptor-aware facade implementation checks (2026-09-23)

The Linux [Unix facade](../src/unix_rpc/facade.rs) adds `client()`,
`client_borrowed()` and `TwoPartyServer` with owned/borrowed `accept()`,
`listen()`, `drain()`, options and trace encoding. It uses the common RPC driver
through the new `TwoPartyNetwork` interface and `listen_networks()`; connection
ownership, disconnect observation and bootstrap behavior are shared with the
byte-stream facade. Client queue diagnostics use `QueueDiagnostics`, which can
observe either transport without retaining messages, sockets or descriptors.
Unix queue metrics exclude the active message and clear on rejection/cancellation.
This writer sends individual frames; matching C++'s batching performance or
identical metrics during different active batch schedules is not claimed.

Borrowed sockets use an owned CLOEXEC duplicate with an exclusive Rust lifetime
on the caller's socket. No borrowed reference enters a `'static` RPC task.
Canceling a partial read terminates the RPC connection and cancels its writer,
releases consumed ancillary descriptors, and closes the duplicate without
shutting down the caller's socket. Explicit protocol shutdown may affect the
shared write half. Consumed bytes/ancillary data are not restored; reusing the
socket as a new RPC connection after partial consumption is not supported.
An owned connection still shuts down its socket on a canceled/invalid read.

The pinned C++ comparison found and fixed a pre-existing difference in
`Options::max_fds`: **zero disables sending as well as receiving descriptors**,
matching C++ `OutgoingMessageImpl::setFds()`. Positive values limit reception per
message while outgoing messages retain the separate Linux limit of 253 FDs.
Truncated descriptors do not remove the associated RPC capabilities. Descriptors
already obtained by an application remain usable after RPC disconnect; capability
revocation cannot revoke independently held OS authority.

Fresh [UnixFacade](../verification/UnixFacade.tla) exploration finds **433 states
/ 576 edge-prefix scenarios**, with **2,160 matching Rust/C++ observations** and
ten detected model fault controls. Bounds: one owned or borrowed connection,
independent receive limits 0–2, two FD-bearing capabilities in each direction,
at most two transfers and five actions. Calls invoke every capability even if
its FD was truncated. Observations check descriptor values, actual capability
invocations, socket ownership, drain and retained descriptors after disconnect.
The C++ oracle uses real capability sockets, descriptor-aware borrowed accepts
and `listenCapStreamReceiver` for owned acceptance. Rust also exercises its real
Unix listener separately. Each modeled action settles IO; this is not an
exhaustive scheduler, kernel resource or composed-protocol proof.

Native tests additionally cover both peers borrowing sockets, reverse bootstrap
with a descriptor-bearing capability, listener cancellation, unpolled cancellation,
partial ancillary input, a blocked writer, queue cleanup and descriptor disposal.
Compile-fail documentation enforces the borrowed socket's lifetime. This
implementation remains Linux-only; non-Linux descriptor transport and
caller-provided async scratch storage are still unchecked above. Automatic
socket window selection is implemented in the following section.

Validation: **33 targeted regression tests pass**: 16 Unix transport/unit tests
and 17 tests across `unix_facade`, `twoparty_facade`, `fd_capabilities` and
`output_completion`. The formatting/Clippy/workspace-doctest gate passes (three
doctests, four pre-existing ignored examples), as does
`cargo check --locked --no-default-features --lib`. This was not a full workspace
runtime or release qualification run.

### Socket-informed flow policy implementation checks (2026-09-23)

The pinned C++ `TwoPartyVatNetwork::getWindow()` reads the message stream's
send-buffer hint and permanently falls back to 65,536 bytes after the first
unavailable result. All streams on that connection share this fallback. C++'s
variable controller skips the query while in-flight bytes fit within its largest
message and after stream failure. Rust's existing controller sampled eagerly;
this implementation corrects that timing as well as adding socket integration.
Zero remains a valid size. Resize alone never wakes blocked credit promises;
a later successful acknowledgement reevaluates them. Sends remain immediate.

[`SendBufferWindow`](../vendor/capnp-rpc/src/twoparty/send_buffer.rs) implements
the shared hint/fallback. The generic byte network's `new_with_send_buffer()`
queries its live output through a weak reference, retaining neither socket nor
write half in outstanding flow controllers. Its ordinary constructors use the
same variable policy with unavailable metadata. `set_window_size()`,
`set_variable_window()` and `set_adaptive_window()` still override future streams.
[`rpc::tcp`](../src/rpc/tcp.rs) supplies `network()`, `client()` and `listen()`;
the connected Tokio TCP socket is split without duplication and queried through
`socket2`. The common server accepts it via `accept_network()`, or `listen()`
feeds accepted networks to its existing driver. Linux Unix networks, including
owned/borrowed facade accepts, sample their socket automatically. Generic
`rpc::client()`/`serve()` and erased or Noise byte streams do not discover hidden
sockets; use `rpc::tcp` for socket-informed TCP flow control.

Fresh [RpcSocketWindow](../verification/RpcSocketWindow.tla) exploration finds
**2,451 states / 5,130 edge-prefix scenarios**, producing **27,922 matching
Rust/C++ observations** of query counts and credit outcomes. Bounds: two streams,
at most three/two sends respectively, first-call success/error acknowledgements,
two hint changes and six actions; hints are unavailable, zero, 16 KiB or 64 KiB.
Every scenario runs through a Rust network and the pinned C++ `TwoPartyVatNetwork`
with a controlled `MessageStream`. Seven fault controls detect eager queries,
retry after unavailability, per-stream fallback, missing ack queries, stale
windows, treating zero as unavailable, and waking credit on resize.
These are settled controller actions, not a model of kernel IO or executor
schedules; the composed-protocol proof gaps above remain open.

Native tests additionally resize real Linux TCP/Unix send buffers and observe
public credit promises, confirm socket/output destruction with pending acks,
and check successful acks arriving after an earlier failure. A real TCP facade
test invokes capabilities in both directions, then checks listener cancellation,
continued calls, disconnect and drain. TCP uses portable `socket2` APIs but only
Linux execution is verified here. This does not establish throughput parity or
an end-to-end congestion estimate across RPC proxies.

Validation: **33 targeted regression tests pass** across `flow_control`,
`streaming`, `tcp_rpc`, `twoparty_facade`, `unix_facade`, `rpc` and
`output_completion`. This includes the existing adaptive/variable TLC checks,
facade C++ comparisons and Noise/UDP capability handoffs. The repository
formatting/Clippy/workspace-doctest gate and
`cargo check --locked --no-default-features --lib` pass. No full workspace runtime
or release qualification run was performed for this change.


## Async word scratch implementation checks (2026-09-27)

The standalone async serialization API now accepts `&mut [capnp::Word]`. It
reuses only the needed prefix, retains segment boundaries, and leaves unused
words untouched. Oversized messages use owned storage without changing scratch.
The existing segment-table parser enforces counts and total-word traversal limits;
it also rejects payload sizes that cannot fit a Rust allocation/slice before
allocation. Returned readers retain their normal traversal and nesting limits.

[Seven native tests](../tests/async_scratch.rs) cover fragmented reads with pending
polls, consecutive messages, zero-length segments, the 511-segment boundary,
exact/insufficient capacity, all truncated prefixes of selected frames, clean EOF,
injected I/O failures and cancellation followed by reuse on a fresh stream.
A compile-fail doctest rejects a reader escaping its scratch lifetime. Cancellation
can consume a partial frame and modify scratch; restarting that stream requires
external restoration of the frame boundary.

The [pinned C++ oracle](../tests/cpp/async-scratch.c++) matches **168 cases** for
success/EOF/rejection, consumed bytes, borrowed-versus-owned payload storage,
segment contents and unused scratch preservation. Error strings and allocation
counts are language-specific. The [allocation regression](../tests/allocations.rs)
requires an 8 KiB single-segment message to allocate only its segment index pair
when scratch fits; a fallback control must allocate payload storage.

Validation: seven async-scratch tests, four allocation tests, three synchronous
framing regressions, nine capnp-futures unit tests and the lifetime doctest pass.
Targeted Clippy passes with warnings denied. All tests remain reachable through
`cargo test --workspace`. This is a bounded standalone byte-stream comparison;
buffered/descriptor scratch integration was completed in the
[subsequent adapter work](#buffered-and-descriptor-scratch-checks-2026-09-28).
Broader platform qualification remains open. The full workspace and release
qualification were not rerun for this change.


## Rust schema compiler implementation checks (2026-09-27)

The first-party [capnp-compiler crate](../crates/capnp-compiler/README.md) contains
a bounded lexer/parser, lexical type resolution, deterministic child IDs,
non-union data/pointer layout, default-value checking and schema-request emission.
It has a library API and `capnp-compile` CLI, with no C++ invocation. The existing
`capnpc` backend can generate working Rust bindings directly from these requests.

The focused [reference suite](../tests/schema_compiler.rs) passes **107 positive
schemas and nine shared rejection cases** against pinned C++. It compares all
fields known to our schema bindings, canonical default-value bytes (including
null pointer defaults), and requested-file metadata. Cases include all pairs of
data widths/pointer slots, 32 longer layouts, nested/recursive types, reordered
ordinals, explicit/derived IDs, primitive limits, floating-point special values
and UTF-8 strings/filenames. C++ source-location extensions, source-info comments
and compiler-version metadata are outside this comparison.

Generated Rust bindings compile and pass a defaults/serialization round trip.
Six standalone integration tests and one doctest cover loader acceptance,
invalid/unsupported syntax, diagnostic positions, resource limits, truncation and
the CLI. These run in the workspace test command and platform smoke jobs; the
root differential suite also needs the pinned C++ build prerequisites. LLVM
inventory and source-bundle enumeration include the crate, and CI checks the
CLI's embedded auditable dependency metadata.

This initial slice did not establish full language parity. Import/alias support
and union/group support were completed in the follow-ups below. Constants,
annotations, interfaces/methods, generics/brands, composite defaults and source
comments remain open. Existing build scripts continue to use C++. The full
workspace qualification and hosted platform jobs were not rerun for this slice.


## Rust schema parser imports and aliases checks (2026-09-27)

The [Rust frontend](../crates/capnp-compiler/README.md) now provides `SchemaParser`
for registered in-memory files and `FileCompiler` for disk files. Imports resolve
relative to the source or through explicit ordered import roots. Canonical disk
identities deduplicate symlink aliases; virtual paths normalize `.` and `..`.
The CLI accepts multiple requested files, `-I`/`--import-path`, `--src-prefix` and
`--`. File IDs may appear anywhere at file scope.

Implemented alias forms include explicit/shorthand `using`, direct import member
expressions, nested aliases, re-exports, primitive/list aliases and aliased `List`
constructors, file-scope `.Name` and parenthesized type expressions. Alias lookup
uses its declaration scope. Cyclic imports and recursive cross-file types work;
cyclic aliases, invalid targets, ID collisions and missing dependencies fail with
source diagnostics. Unused transitive imports are loaded lazily, and unused
imported type nodes are omitted from requests. Import roots reject `..` escapes;
filesystem compilation is not a sandbox. Bounds cover aggregate source/node/file
counts, alias expansion and resolution work.

Fresh Linux checks passed:

- **27 import/alias graphs and 12 shared rejection cases** against pinned C++,
  alongside the original **107 positive schemas and nine rejections**. Known
  node/request fields and canonical default bytes match. Source-location
  extensions, comments and compiler-version metadata remain excluded.
- **16 native integration tests and two doctests**, including search precedence,
  import/alias cycles, symlink identity, lazy dependencies, failed-parse recovery,
  aggregate bounds and the multi-file CLI. The symlink case is Unix-only.
- Both single-file and cyclic multi-file requests generate Rust bindings that
  compile and pass serialization/default-value round trips.

All tests remain reachable through `cargo test --workspace`; platform CI already
runs the crate suite, and the existing LLVM inventory covers the new resolver
and source modules. Existing repository build scripts still use C++ until the
remaining grammar is implemented. Full workspace qualification and hosted
platform runs were not rerun for this change.


## Rust schema unions and groups checks (2026-09-27)

The [Rust frontend](../crates/capnp-compiler/README.md) now parses named and
unnamed unions, ordinary groups, group alternatives and nested unions. Fields
share the enclosing struct's ordinal namespace; emitted group IDs depend on
field-list position, matching C++. Legacy explicit named-union ordinals are
supported, including the restriction on retroactively unionizing old fields.

Layout uses shared data locations and pointer slots with per-alternative usage.
It follows ordinal order for allocation, discriminants and schema fields while
preserving declaration order in `codeOrder`. All groups inherit their enclosing
struct's data/pointer sizes. Imported structs bring their group nodes into the
request; groups cannot be used as independent field types. Invalid declarations,
empty groups, small/duplicate unnamed unions and conflicting/gapped ordinals
produce source diagnostics. Nesting, node and shared layout-work limits bound
compilation. Historical issue #344 layouts rejected by pinned C++ also return
diagnostics; the Rust implementation always enables this compatibility check.

Fresh Linux checks passed:

- **212 successful union/group schemas and 15 shared syntax/semantic rejections**,
  including five unmodified layout fixtures from the pinned C++ `test.capnp`,
  all pairs of data widths/pointer types and 64 interleaved four-group layouts.
- **128 deterministic nested layouts**: 97 accepted requests match; 31 historical
  issue #344 rejections agree. Comparisons cover all known node/request fields
  and canonical default bytes; source-location extensions, comments and compiler
  version remain excluded.
- **29 import/alias graphs and 12 shared rejections**, including imports of group
  nodes, plus the existing **107 original schemas and nine rejections**.
- **19 native integration tests and two doctests**, including group ID stability,
  shared sizes, discriminants, truncated schemas and layout/depth limits.
- **Three generated Rust acceptance tests** compile and serialize bindings from
  single-file, cyclic-import and union/group requests. The new test switches
  alternatives, checks initialization/defaults and preserves non-union fields.

The focused compiler tests, Clippy, formatting and auditable CLI inventory checks
pass. Tests remain included in `cargo test --workspace` and native platform smoke
jobs; no additional test runner is required. Evidence is written under
`target/verification/schema-compiler/`, including `unions.txt`, `nested-unions.txt`
and `unions-rust.log`. Full workspace qualification, LLVM collection and hosted
platform jobs were not rerun for this change. Full schema-language parity still
requires annotations, interfaces, generics, composite values and source-info
emission. Scalar constants and Data defaults were completed in the follow-up below.


## Rust schema constants and Data checks (2026-09-27)

The [Rust frontend](../crates/capnp-compiler/README.md) supports scalar, Text, Data
and enum constant declarations at file/struct scope, explicit/derived IDs,
qualified references, forward references and aliases. Imports used only in values
resolve lazily and are inlined; they do not appear in requested-file import tables
unless also used in a type or `using` declaration. Imported constant nodes are not
emitted merely because their values are referenced. Cycles, type mismatches,
invalid aliases and name collisions produce source diagnostics.

Typed evaluation preserves intermediate Float32 rounding, checked integer
conversions, null versus explicit empty defaults and raw Data bytes. Data values
accept UTF-8 strings or hexadecimal byte pairs. Adjacent strings concatenate.
Constant evaluation shares the bounded resolver work budget; reference depth is
limited to 64 and emitted Text/Data expansion to 16 MiB. Composite values,
annotations, interfaces/methods, generics, embeds and further string forms remain.

Fresh Linux checks passed:

- **123 successful constant/Data schemas and 63 shared rejections**, including
  all scalar numeric conversion pairs, integer boundaries, precision changes,
  scoped names, defaults, binary literals and canonical constant/default bytes.
- **15 constant import graphs and eight shared rejections**, including aliases,
  re-exports, lazy transitive imports, constant cycles and both requested-file
  orders. Value imports also used in types are retained in the import table.
- **24 native integration tests and two doctests**, plus **ten root integration
  tests**. The full focused reference corpus has **583 matching accepted schemas
  and 138 matching rejections**, excluding source-info/version extensions.
- A fourth generated Rust acceptance test validates public constants, typed
  defaults, imported Data inlining and serialization without imported bindings.
- Formatting, Clippy and the CLI's embedded auditable inventory pass.

Two reference behaviors are explicit. Floating-point overflow produces infinity.
Enum constant references expose their raw ordinal in pinned C++, so the Rust
frontend follows its numeric conversions and rejection as enum-typed defaults.
Bare enumerant defaults and enum constant declarations remain supported.

One deliberate difference is tested separately: the literal `18446744073709551616`
is rejected in Rust while pinned C++ wraps it to zero. Rust retains checked integer
parsing. This case is not counted as a matching acceptance or rejection.

Evidence is under `target/verification/schema-compiler/`, including `constants.txt`,
`constant-imports.txt` and `constants-rust.log`. All tests remain part of
`cargo test --workspace`; platform smoke tests and the LLVM inventory include the
new evaluator module automatically. Full workspace qualification, LLVM collection
and hosted platform jobs were not rerun for this change.

## Rust composite schema values (2026-09-27)

The [Rust frontend](../crates/capnp-compiler/README.md) supports list and struct
constants/defaults, nested lists, inline struct lists, group assignments, union
alternatives, qualified references and one-level implicit struct wrapping.
Typed struct/list constants can initialize AnyPointer. Imports used only in
values remain inlined, including when an imported type is erased to AnyPointer;
the compiler computes the layout without adding unused schema dependencies to
the emitted request.

The [wire encoder](../crates/capnp-compiler/src/wire.rs) uses safe builder APIs.
It preserves scalar default masks, null versus explicit empty pointers, source
assignment order and C++ group/union storage behavior. Cached values carry their
expanded depth and work costs, preventing exponential constant references from
bypassing limits. Encoded storage has a separate 16 MiB budget, checked before
allocation, and encoding shares a 1,000,000-step budget including group clears.

An end-to-end regression exposed existing Rust generator behavior that ignored
AnyPointer defaults. The maintained generator now emits AnyPointer constants
and applies explicit defaults: readers borrow the validated constant, builders
copy it into null fields, and existing values take precedence. The infallible
AnyPointer getter signatures are preserved.

Fresh Linux checks passed:

- **69 accepted composite schemas and 22 shared rejections**, comparing known
  request fields and canonical constant/default bytes against pinned C++.
- **17 composite import graphs and five shared rejections**, covering erased
  types, transitive references, lexical scope, nominal typing and requested-file
  order. The complete focused corpus now has **669 matching accepted schemas
  and 165 matching rejections**.
- **29 native integration tests and two doctests**, plus **13 root integration
  tests**. The fifth generated Rust acceptance test reads and mutates defaults,
  serializes them, checks union selection and verifies constant/message isolation.
- Formatting, Clippy and the CLI's embedded auditable inventory pass.

AnyPointer literal inference is unsupported by C++. Referencing an AnyPointer
constant also fails in the pinned compiler (an assertion failure); Rust returns
a source diagnostic. Typed struct/list references assigned to AnyPointer are
supported and tested. This does not establish full schema-language parity:
annotations, interfaces/methods, generics, embeds, extended strings and emitted
documentation/source locations remain.

Evidence is under `target/verification/schema-compiler/`: `composites.txt`,
`composite-imports.txt`, `composites-rust.log` and the existing comparison logs.
The tests are included in `cargo test --workspace`; native tests run in the
Linux/macOS/Windows smoke jobs. Full workspace qualification, LLVM collection
and hosted platform jobs were not rerun for this change.

## Rust schema annotations (2026-09-27)

The [Rust frontend](../crates/capnp-compiler/README.md) now compiles annotation
declarations with explicit/derived IDs, all twelve target flags, wildcard and
empty target lists, lexical/imported names, aliases, scalar/composite arguments
and repeated applications in source order. It validates target eligibility,
argument types and annotation/constant dependency cycles. Named group and union
annotations belong to the containing field; unnamed unions reject annotations.
Applications carry explicit empty brands. Generic declarations remain unsupported.

Request dependencies include used annotation declarations, their types and their
own annotations. Annotation-name imports appear in the requested-file import
table. Argument-only imports remain inlined; C++ also omits imports used solely
in an annotation declaration's type from that table. Unused transitive imports
remain lazy. Annotation evaluation shares the existing depth, work, payload and
encoding limits.

Fresh Linux checks passed:

- **56 accepted annotation schemas and 37 shared rejections**, comparing all
  known request fields and canonical annotation/default/constant bytes.
- **22 annotation import graphs and eight shared rejections**, including the
  unmodified `rust.capnp` and `c++.capnp` files, aliases, cycles, lazy dependencies
  and multiple requested-file orders. The full focused reference corpus totals
  **747 matching accepted schemas and 210 matching rejections**.
- **34 native integration tests and two doctests**, plus **16 root integration
  tests**. The sixth generated Rust acceptance test verifies `$Rust.name`,
  `$Rust.option`, `$Rust.parentModule`, annotation reflection and serialization
  without generating the imported annotation file's bindings.
- The eight existing reflection lookup tests pass, including their bounded
  model exploration and pinned C++ comparison.
- Formatting, Clippy and the CLI's embedded auditable inventory pass.

The generated-code test exposed an existing field reflection bug: the lookup
index was sorted by renamed Rust identifiers, although binary search compares
original schema names. The maintained generator now sorts by schema names.
Regression checks cover ordinary fields and named group/union fields.

Evidence lives under `target/verification/schema-compiler/`, including
`annotations.txt`, `annotation-imports.txt` and `annotations-rust.log`.
All tests remain included in `cargo test --workspace`, and the native tests run
in the Linux/macOS/Windows smoke jobs. Full workspace qualification, LLVM
collection and hosted platform jobs were not rerun for this change. Remaining
language work includes interfaces/methods, generics, embeds, extended strings
and emitted documentation/source locations.


## Rust schema interfaces and methods (2026-09-27)

The [Rust frontend](../crates/capnp-compiler/README.md#interfaces-and-methods)
now emits nongeneric interfaces, multiple inheritance, nested declarations,
inline parameter/result schemas, explicit struct signatures and streaming
results. It resolves interface types and `Capability`, applies typed defaults,
and checks interface/method/parameter annotations against their target flags.
Inline input/output schemas have zero wire scope IDs and deterministic IDs based
on interface ID, ordinal and role, preserving identity through method renames.

`-> stream` resolves the pinned StreamResult through explicit import roots,
including its standard C++ annotation dependency. The fixed StreamResult ID is
checked rather than emitting a dangling reference for a substituted schema.
Import traversal stays lazy and handles cross-file recursion and multiple
requested files with auxiliary method nodes.

Fresh Linux checks passed:

- **52 accepted interface schemas and 38 shared rejections**, comparing known
  request fields and canonical method/parameter annotation and default bytes.
- **12 interface import/streaming graphs and six shared rejections**. The full
  focused corpus now totals **811 matching accepted requests and 254 matching
  rejections**. Two accepted requests contain inheritance cycles; both runtime
  loaders reject these, while their emitted request fields agree.
- **38 native integration tests and two doctests**, plus **20 root integration
  tests**. The seventh generated Rust acceptance test executes inherited calls,
  explicit parameter/result structs, defaults, pipelined capability calls and
  streaming uploads, using the runtime's call executor.
- Existing branded diamond lookup and explicit generic alias RPC regressions
  pass, checking that the generator change preserves their behavior.
- Formatting, compiler/generator/integration Clippy checks and the CLI's embedded
  auditable inventory pass.

C++ emits cyclic inheritance requests without rejecting them at compilation.
The maintained Rust generator previously recursed without a bound on these;
it now reports cycles, inheritance depth of 64 or greater, and more than 65,536
expanded inherited edges per interface. Regressions exercise cycles, deep chains
and exponential diamond expansion. Branded paths remain distinct within the
bounds. This is a focused superclass traversal fix, not general validation of
arbitrary code-generation requests.

Evidence is under `target/verification/schema-compiler/`: `interfaces.txt`,
`interface-imports.txt`, `interfaces-rust.log` and `auditable.json`.
The tests remain included in `cargo test --workspace`; the native tests run
in the Linux/macOS/Windows smoke jobs. Full workspace qualification, LLVM
collection and hosted platform jobs were not rerun for this change.
Generics/brands (including generic method parameters), embeds, extended strings
and emitted documentation/source locations remain. The frontend can emit
`List(Capability)` type metadata, but the existing Rust generator rejects that
list type as `List(AnyPointer)`.

## Rust generic schema compilation (2026-09-27)

The [Rust frontend](../crates/capnp-compiler/README.md#generic-types-and-brands)
now compiles generic struct/interface declarations, nested and imported aliases,
explicit and inherited brands, generic interface inheritance, implicit method
parameters, and branded constants/defaults and annotation applications. Parameter
scope IDs, ordering, detached method schemas and `isGeneric` metadata match the
pinned compiler. AnyStruct/AnyList constraints and pointer-parameter `null`
defaults are supported. Keyword-shaped field/group/method names are distinguished
from declaration keywords, allowing the standard schema file to compile unchanged.

Generic binding trees share their contents, and substitutions and expanded types
consume the existing resolution work/depth budgets. Tests reject compact alias
diamonds before they can expand into unbounded brands. Constant evaluation keeps
shared nongeneric values intact and substitutes branded values only when their
type changes, retaining the existing composite value limits.

Fresh Linux checks passed:

- **56 accepted generic schemas and 28 shared rejections**, comparing every
  known request field and canonical constant/default/annotation bytes.
- **17 generic import graphs**, including nested aliases, distinct instantiations,
  annotation arguments, lazy dependencies, erased values and cross-file recursion.
- **Four unmodified standard schemas**: `schema.capnp`, `rpc.capnp`,
  `rpc-twoparty.capnp` and `persistent.capnp`.
- **All 21 repository schemas** under `schemas/` match C++ requests. This checks
  their actual sources and configured standard/Rust annotation imports.
- **43 native integration tests and two doctests**, plus **25 root integration
  tests**. The eighth generated Rust acceptance crate executes two tests covering
  generic defaults/groups/serialization, inherited RPC, implicit method parameters
  and explicit nested signatures with distinct type arguments.
- Formatting, compiler/integration Clippy, local documentation links and the CLI's
  embedded auditable inventory pass. The focused corpus totals **909 matching
  accepted requests and 282 matching rejections**.

A separate regression records an intentional difference: pinned C++ rejects
`List(T)` for its first two parameters but accepts it at later parameter indices.
Rust consistently rejects lists whose element is an unbound parameter. A list of
a concrete generic struct, such as `List(Envelope(T))`, is supported. The existing
Rust generator exposes implicit method values through AnyPointer; the acceptance
test verifies those calls. Generator support for constrained AnyPointer lists
remains a separate boundary.

Annotation value types are bound for checking and encoding. Request selection
matches C++: types mentioned only in an annotation application's brand are not
selected automatically, so reading these annotation values through the runtime
loader may require additional schema dependencies.

Evidence is under `target/verification/schema-compiler/`: `generics.txt`,
`generic-imports.txt`, `repository-schemas.txt`, `generics-rust.log` and
`auditable.json`. All tests remain part of `cargo test --workspace`; native tests
are included in Linux/macOS/Windows smoke jobs. Full workspace qualification,
LLVM collection and hosted platform jobs were not rerun for this change.
Embeds, extended string forms and emitted documentation/source locations remain;
existing build scripts continue using C++ while migration is evaluated separately.


## Rust schema embeds and string forms (2026-09-27)

The [Rust frontend](../crates/capnp-compiler/README.md#strings-and-embedded-files)
now supports `embed` for Text, Data and serialized structs. Both source APIs use
one path namespace and the same relative/import-root rules; `SchemaParser::add_file`
registers raw bytes. Embeds load lazily, resolve beside the declaring source,
share canonical identities and do not populate schema import tables. Unused
embedded files in imported declarations are not opened.

Struct embeds read unpacked framed messages, preserve unknown fields and retain
schema defaults for absent fields. Inline struct lists copy the intersection of
the input storage and the list's element layout. Generic brands, annotations,
constant references, mutable defaults, multi-segment far pointers, empty/null
roots and whole-word trailing bytes are covered. Group embeds return a source
diagnostic where the reference aborts.

Quoted strings preserve bytes and support C-style simple, hexadecimal and octal
escapes. Adjacent strings concatenate, including incomplete UTF-8 byte sequences
that become valid when joined. Backtick lines are literal and normalize line
endings to LF. BOMs and ASCII whitespace follow the reference lexer. Embedded
NULs and invalid UTF-8 Text bytes are preserved; callers validate UTF-8 separately.

Fresh Linux checks passed:

- **65 matching string requests and 13 shared rejections**.
- **30 matching embed requests and 16 shared rejections**, including imported
  values and malformed serialized inputs.
- **52 native integration tests and two doctests**, plus **28 root integration
  tests**. The ninth generated acceptance crate checks constants, byte strings,
  mutation of embedded defaults, serialization and unknown-field preservation.
- Formatting, compiler/integration Clippy, local documentation links and the
  CLI's embedded auditable inventory pass. The focused corpus now totals
  **1,004 matching accepted requests and 311 matching rejections**.

Unlike C++'s unlimited embed reader, Rust bounds individual files at 4 MiB,
combined source/embed input at 16 MiB, distinct schemas/embeds at 256 each,
struct nesting at 64 and traversal at 2,097,152 words. Existing expansion and
encoding budgets apply to repeated values. Malformed messages, cycles,
capabilities and budget exhaustion produce diagnostics. The in-memory API reads
no files; filesystem compilation retains the documented import path boundary.

Evidence is under `target/verification/schema-compiler/`: `strings.txt`,
`embeds.txt`, `embeds-rust.log`, `native.log`, `full.log` and `auditable.json`.
These tests run through `cargo test --workspace`; native tests remain in the
Linux/macOS/Windows smoke jobs. Full workspace qualification, LLVM collection
and hosted platform jobs were not rerun. Documentation/source-info emission
remains unfinished, and existing build scripts continue to use C++.


## Rust documentation and source-info emission (2026-09-27)

The [frontend](../crates/capnp-compiler/README.md#documentation-and-source-ranges)
now emits documentation comments and source positions on schema nodes and their
`Node.SourceInfo` records, including fields, enumerants, groups, methods, parameter
lists and individual parameters. The runtime's maintained `schema.capnp` and
bootstrap-generated bindings expose the additive `startByte`/`endByte` fields;
older requests continue to read zero defaults. The existing AnyPointer value
representation is unchanged.

Comment attachment follows C++: file docs follow the ID statement, line docs
follow `;`, block opening docs take precedence over closing docs, and blank lines
break attachment. Comments strip one optional space after `#`, preserve indentation
and CR bytes, and append LF even at EOF. Member metadata follows schema order.
Groups duplicate documentation on the node and containing field, with shared
parse storage and a separate 16 MiB expanded documentation budget. Diagnostics
retain their original spans. The metadata index is iterative and bounded by the
existing input/token limits.

Fresh Linux checks passed:

- **52 new documentation requests** match C++, covering blank lines, BOMs,
  Unicode, CRLF/CR, empty/EOF comments, literal comment markers, and parameter ranges.
- **56 native tests and two doctests**, plus **29 root compiler integration tests**.
  The full **1,056 accepted request / 311 shared rejection corpus** now compares
  node positions, comments, member ranges and canonical source-info as well as
  previous schema/default/constant/annotation fields.
- **35 runtime schema-loader, metadata, introspection and reflection tests** pass
  with the regenerated schema bindings, including their existing model replays.
- Formatting, compiler/integration Clippy, documentation links and the CLI's
  embedded auditable inventory pass. The core crate package verifies with the
  regenerated bindings. Its no-default-features check passes with the existing
  unrelated unused `fmt` variable warning in the runtime.

One reference quirk is isolated in the comparator: C++ preloads `StreamResult`
from an older compiled schema with zero Node offsets, while its source-info has
the correct parsed offsets. Rust preserves the parsed range in both. The test
normalizes only this built-in node and still compares its source-info exactly.

Evidence is under `target/verification/schema-compiler/`: `source-info.txt`,
`native.log`, `full.log`, `schema-bindings.log` and `auditable.json`.
Tests remain included in `cargo test --workspace`; no full workspace, LLVM or
hosted platform qualification was rerun. Per-file identifier-resolution tables,
rendering source-info as Rustdoc in the generator, and build-script migration
remain separate work. Existing build scripts continue using C++.


## Rust per-file identifier references (2026-09-27)

The Rust frontend now emits `RequestedFile.fileSourceInfo.identifiers` for every
requested file, including initialized empty tables. References retain UTF-8 byte
ranges and resolved declaration IDs for built-ins, named types, imports, aliases,
constants and annotations. Generic applications report constructors and arguments;
parenthesized names retain the inner expression's start byte. Parameters and
synthetic stream signatures do not invent declaration references. Value-only
imports can reference declarations omitted from generated-code dependencies.

The maintained schema and bootstrap-generated runtime bindings now include the
additive `FileSourceInfo` extension, including its reserved member-target variant.
Pinned C++ emits only type-ID targets. Old requests remain readable with empty
tables. Rust emits sorted, unique references; the comparison normalizes C++'s
traversal order and repeated entries. Import/embed retries discard partial tables,
and identifier collection shares the existing bounded resolution work.

Fresh Linux checks passed:

- **60 new expression requests and three multi-file request graphs** match C++.
  All **1,119 accepted request comparisons** now check identifier ranges and
  targets, alongside the existing **311 shared rejections**.
- **60 native tests, two doctests and 31 root compiler integration tests** passed
  across the existing and added suites. These remain part of `cargo test --workspace`.
- **35 schema-loader, metadata, introspection and reflection tests**, including
  existing model replays, pass with the regenerated bindings.
- Compiler/integration Clippy, formatting, documentation links, package
  verification and CLI auditable metadata pass. The no-default-features build
  passes with the existing unrelated unused `fmt` variable warning.

Evidence: `target/verification/schema-compiler/identifiers/`, plus
`target/verification/schema-compiler/identifiers.txt`. This run did not repeat the
full workspace, LLVM coverage or hosted platform jobs. Rustdoc rendering and
build-script migration remain; existing build scripts still use C++. Passing
this bounded corpus does not establish complete schema-language parity.


## Generated Rustdoc and first-party Rust schema builds (2026-09-28)

The maintained `capnpc` generator now renders source-info in standard bindings
and the field API. Comments follow node IDs and schema member indices through
renaming, ordinal reordering, groups/unions, projections, native values and RPC
methods. File comments get an `include!`-compatible `schema_documentation` page
with collision handling. One indexed lookup handles absent/partial metadata;
duplicate source-info IDs and invalid UTF-8 return errors. Escaped attributes
prevent comment text from becoming generated Rust code.

CommonMark normalization marks untagged/indented schema examples as text and
keeps explicit Rust doctests. It preserves nested lists, quoted examples and
long fences, and turns bare HTTP(S) URLs in prose into links. This fixes actual
workspace Rustdoc failures from indented URLs and language-neutral schema text.

Root and test-support build scripts now use `FileCompiler` and
`CodeGenerationCommand`. `compile_with_dependencies` reports loaded schemas and
embeds, including symlink spellings and canonical targets, for Cargo rebuild
tracking. Repeated invocations start fresh and unused lazy imports stay excluded.
Standard annotation/stream imports come from the maintained source bundle; the
unmodified pinned `stream.capnp` is included in the core crate package. Import
search directories must be watched separately when precedence changes matter.

Fresh Linux checks passed:

- **62 native compiler tests and two doctests**; new build-input regressions cover
  embeds, failed/repeated compilations, search precedence and Unix symlinks.
- **33 root compiler integration tests**, including the existing **1,119 accepted
  request comparisons and 311 shared rejections**. The additional Rustdoc fixture
  produces matching Rust/C++ generated bodies in both API modes.
- **30 field-API, native-value, RPC and reflection regressions** pass using the
  migrated test-support build. The generated documentation fixture passes **13
  explicit Rust doctests** and checks actual HTML on the corresponding items.
- Workspace Rustdoc builds with `-D warnings`; root/RPC/test-support doctests,
  generator/compiler/integration Clippy, the root no-default-features build,
  formatting and documentation links pass. Core packaging verifies **94 files**,
  including the bundled stream schema. Both compiler executables pass embedded
  auditable inventory checks.

Evidence: `target/verification/schema-compiler/rustdoc/`; generated HTML lives at
`target/schema-compiler-acceptance/doc/rust_schema_docs/`. The optional storage
benchmark's EAE checkout is absent, so its lockfile additions were reconciled
against Cargo-resolved root/RPC benchmark entries; that benchmark was not built.
Other affected standalone lockfiles were refreshed through Cargo.

The vendored RPC crate, downstream example and `CompilerCommand` still use the
external compiler. Migrating separately packaged crates needs a distributable
frontend dependency. The frontend remains experimental and separate from live
runtime schema-loader integration. This run did not repeat the full workspace
suite, LLVM coverage, performance benchmarks or hosted platform jobs.


## Parsed-schema runtime snapshots (2026-09-28)

The Rust frontend's `SchemaParser` and `FileCompiler` now expose
[`parse_schemas()`](../crates/capnp-compiler/README.md#runtime-reflection).
It compiles source, validates the request with the runtime loader and returns an
owned, immutable `ParsedSchemas`. Callers can navigate direct declarations,
inspect source metadata and construct dynamic messages without generated Rust
bindings. Handles borrow the snapshot; source files and the input parser can be
deleted independently. Compiler requests, identifier tables and filesystem input
dependencies remain available.

Compilation diagnostics and loader rejection have distinct error variants.
Explicit loader limits apply after the frontend's existing bounds, and failure
returns no partial snapshot. Repeated parsing starts fresh. The underlying loader
is read-only, preventing schema replacement from making retained metadata stale;
callers can clone it for independent native registration or schema changes.

Fresh Linux checks passed:

- **67 native compiler tests and four doctests**, including five new runtime
  snapshot regressions and a compile-fail lifetime check. Tests cover imported
  brands, groups, implicit method nodes, defaults, dynamic wire roundtrips,
  dependency selection, loader limits, failure/retry isolation, source deletion
  and embed changes across independent snapshots.
- The new [C++ SchemaParser comparison](../tests/schema_compiler/parsed.rs)
  matches **15 declarations** for IDs, kinds, node/member ranges, comments and
  direct nested lookup. A dynamic message containing branded pointers, a group,
  a union and struct lists has identical canonical wire bytes. This checks the
  actual C++ parser/reflection API in addition to earlier request comparisons.
- Compiler and integration Clippy with warnings denied, strict compiler Rustdoc,
  formatting, local documentation links and the CLI auditable inventory pass.

Evidence: `target/verification/schema-compiler/parsed/`. These additions run
under `cargo test --workspace`; the native tests are also included in existing
Linux/macOS/Windows smoke jobs. Fixtures normalize checkout line endings before
the comparison; separate lexer/source-info tests cover CRLF preservation.

This is not C++'s live parser cache. Nested lookup exposes direct declarations,
not aliases, and never compiles additional source. A declared but uncompiled
imported child returns an error; explicitly requesting its file includes the full
subtree. Lazy alias/declaration lookup, concurrent caching, random file IDs,
custom filesystem callbacks and distributable frontend packaging remain open.
The existing 33 root compiler integration tests, full workspace suite, LLVM
coverage, performance benchmarks and hosted platform jobs were not rerun here.


## Lazy declaration and alias sessions (2026-09-28)

[`SchemaSession`](../crates/capnp-compiler/README.md#lazy-sessions) now retains the
frontend's source graph and can load additional declarations by ID or nested name.
It compiles their dependencies and parents while leaving unused children/siblings
uncompiled. Requested-file order and identity remain unchanged. Batch nested loading
commits all direct children together; aliases are not part of that enumeration.

Declaration aliases resolve through lexical scope and imports. This includes file
scopes, structs, enums, interfaces, constants and annotations. Pinned C++'s
`ParsedSchema::findNested()` **does resolve declaration aliases**: the comment in
`Compiler::lookup()` refers to resolution results that are not declarations, such
as parameters. The executable oracle confirms the behavior. Generic bindings are
discarded when returning a declaration, also matching C++; an alias of a generic
parameter returns no schema. Built-in/list aliases return errors because they have
no loadable schema node. Immutable `ParsedSchema` snapshots retain their direct
declaration-only lookup; session lookup owns the extra resolution work.

Extensions require an exclusive mutable borrow. A compile-fail doctest prevents
mutation while a schema handle is live. Discovery, resolution and runtime loader
validation are staged before commit; failures leave the previous schemas, metadata
and input dependencies unchanged. Successfully read schemas/embeds remain cached.
Unused files are read on demand, and files discovered only by a failed extension
can be corrected before retrying. Memory sessions borrow their source provider;
disk sessions capture absolute search paths and outlive the compiler configuration.
`into_schemas()` detaches an owned snapshot.

Fresh Linux checks passed:

- **74 native compiler tests and six doctests**, including seven session tests for
  selection, aliases, brands, implicit method metadata, source caching, import/embed
  retry, cumulative loader limits and rollback after compilation or loader rejection.
- **35 root compiler integration tests**, retaining the **1,119 accepted request
  comparisons and 311 shared rejections**, generated Rust acceptance crates and the
  earlier 15-declaration parsed-schema comparison.
- The new [session oracle](../tests/schema_compiler/session.rs) matches **18 session
  snapshots** against C++ `Compiler::lookup()` and its actual lazy loader callback,
  followed by dependency/parent completion with `eagerlyCompile()`. Every selected
  schema node and source-info record has matching canonical bytes. Cases include
  alias binding erasure, file/constant/annotation aliases, generic parameters,
  imported types, methods, groups, embeds and unused invalid declarations.
- Compiler/integration Clippy with warnings denied, strict compiler Rustdoc,
  formatting, local documentation links and the CLI auditable inventory pass.

Evidence: `target/verification/schema-compiler/session/`. All additions run under
`cargo test --workspace`; existing platform smoke jobs include the native tests.
The implementation stages a syntax-graph copy and recompiles the accumulated
selection, so no incremental compilation performance claim is made. Work is bounded
per extension, with graph/input and loader limits across the session. The comparison
does not assert C++ failure atomicity, shared-handle mutation or concurrent caching.
Concurrent parsing, custom filesystem callbacks, random file IDs and distributable
frontend packaging remain open. Full workspace tests, LLVM coverage, benchmarks
and hosted platform jobs were not rerun in this turn.

## Optional compatibility implementation checks (2026-09-28)

`cargo test --locked -p capnp-compat` exercises the crate through ordinary Cargo
tests, included by `cargo test --workspace`. Fourteen accepted codec fixtures
compare canonical wire bytes and exact compact/pretty JSON and text with the
pinned C++ reference, including annotations, unions and floating-point formatting.
Native tests cover invalid input, work limits, handler precedence and brand
identity, substream limits/early end/zero limits, explicit EOF, abort and legacy
executor-owned EOF, body truncation, framing and JSON-RPC cancellation.

Linux loopback tests connect Rust adapters to the pinned C++ ByteStream and HTTP
factories, including a 200 KB streamed upload/echo, WebSocket upgrade/data/close,
and a CONNECT tunnel. A C++ JSON-RPC peer checks renamed methods, notifications,
results and errors. Executed peer tests have a 30-second deadline and child cleanup.
Source copies retain upstream attribution and are generated by the Rust frontend.

Local validation passed 27 compatibility tests, 74 compiler tests and six compiler
doctests, 28 affected orphan/schema-loader tests, and two coverage-report unit
tests. The compatibility suite includes zero-body HEAD/CONNECT responses,
capability identity after wrapping, and failed root adoption retaining ownership.
Strict Clippy and Rustdoc, formatting, actionlint, Zizmor and offline Markdown
links passed. Cargo commands used the auditable wrapper; the compiler executable's
embedded dependency inventory was verified. Ordinary test harnesses follow the
repository's [auditable build limits](QUALITY.md#auditable-cargo-builds).

The portable suite is wired into Linux/macOS/Windows compile/smoke CI; the new
source prefix is mandatory in full LLVM reports. Hosted jobs, complete workspace
verification, full LLVM collection and performance benchmarks were not rerun for
this increment. No blanket C++/KJ replacement or production qualification is implied.

## Buffered and descriptor scratch checks (2026-09-28)

`BufferedRead::try_read_message_with_scratch` returns a
`Reader<BufferedScratchSegments<'_>>`. It keeps the existing classifier API:
short-lived messages share the stream buffer, while retained, fully buffered
frames copy into caller words when they fit. Scratch capacity includes the
segment table, unlike the standalone async API. Large or descriptor-bearing
partial frames take the owned direct-read path even with sufficient scratch,
matching the pinned C++ implementation. `uses_scratch()` and
`is_shared_buffer()` expose the selected storage; `into_owned()` copies borrowed
scratch when the caller needs to release it. Segment metadata still allocates.

Linux `unix_rpc::FdReader` accepts owned, shared or borrowed Tokio Unix sockets.
Its scratch read also receives `&mut [Option<OwnedFd>]`; `ScratchMessage` exposes
the body and only the initialized descriptor prefix. Slots must be empty on
entry. Taking a slot transfers ownership; otherwise the caller's slots retain
the descriptors after the view is dropped. This explicit Rust ownership rule
prevents silently overwriting unrelated descriptors. Shared message/occupied
slot rejections happen before consuming input or changing pending descriptors.

The effective FD limit is the smaller of slot capacity, configured limit and
Linux's 253-FD maximum. Excess descriptors are closed. A smaller subsequent
call also truncates already-prefetched descriptors, and cannot recover FDs
discarded by an earlier smaller capacity. Each reader holds a bounded 253-slot
staging array for partial input and prefetch; recvmsg uses bounded aligned stack
storage. Caller output slots do not require a descriptor Vec allocation.

Caller words/slots remain untouched until successful delivery. Canceling a raw
read releases their borrows and retains partial input/FDs inside the reader for
retry, including with different storage. Stream drop or terminal error closes
staged FDs. The existing RPC network retains its owned-message API and its
connection-closing cancellation contract. This change does not retrofit borrowed
messages into the RPC `IncomingMessage` trait or add non-Linux ancillary support.

Five portable scratch tests and six Linux scratch tests include **1,008 pinned
C++ observations** for storage choice, unused words, message order and FD values.
Allocation tests require one segment-index allocation for fitting retained
messages, including a prefetched-FD case. Native socket tests witness exact
descriptor closure; compile-fail doctests enforce caller-storage lifetimes.
The existing buffered/FD/TLC regressions also exercise the unchanged APIs.
Local validation passed 33 integration tests, 23 Unix unit tests, nine
capnp-futures unit tests and 21 doctests. Strict Clippy and Rustdoc, formatting,
actionlint, Zizmor and offline documentation links passed. Cargo validation used
the repository's auditable wrapper.
Platform smoke CI selects portable tests and Linux descriptor cases; all checks
remain reachable through `cargo test --workspace`. Full workspace verification,
LLVM collection, hosted platform jobs and benchmarks were not rerun here.

## Generated runtime path checks (2026-09-28)

`capnp_root` now substitutes the runtime path for Text/Data list readers,
builders and owned types (including nested lists and generic arguments), and
the `Owned` implementation for nongeneric interfaces. The ordinary
`CompilerCommand` build-script API forwards the same option to the generator;
leaving it unset preserves `::capnp`.

The downstream regression test compiles and executes **16 configurations**:
four runtime paths (`::capnp`, `::runtime`, `crate::wire`, `crate`), two
generation modes (legacy and field API with native values/projections), and
two entry points (Rust frontend plus `CodeGenerationCommand`, installed C++
frontend plus `CompilerCommand`). All bindings are nested under parent modules
and include cross-file imports, pointer constants/defaults, Text/Data lists,
generics, groups/unions, interface inheritance and streaming methods. Roundtrips
check list contents, imported fields, union selection, reflection, projections
and native values. The default-path control is a separate crate; the override
crate has no direct dependency named `capnp`, so hardcoded paths fail compilation.

Before the emitter fixes, the default control passed and the override crate
failed with unresolved `::capnp` references. Afterward all configurations passed.
The focused test runs in existing Linux/macOS/Windows smoke jobs and through
`cargo test --workspace`. It is generator coverage, not additional C++ runtime
parity or a hosted-platform result.

Local validation passed the downstream test, 21 field API/value tests, two
capnpc unit tests and one capnpc doctest (five existing doctests remain ignored).
Strict Clippy/Rustdoc, formatting, actionlint, Zizmor and offline documentation
links passed. Both downstream binaries contain cargo-auditable dependency
inventories. Full workspace/LLVM checks, hosted jobs and benchmarks were not
rerun for this generator fix.

## RPC try contract checks (2026-09-28)

The optional nightly feature now implements
`FromResidual<Result<Infallible, E>>` for `Promise<T, Error>` when `Error: From<E>`.
Successful validation continues synchronously; a failed `Result?` returns an
immediately rejected promise and drops ordinary local work. A deferred promise
is never polled to decide whether `?` should return early. Error kind, diagnostic
text, remote trace and opaque details survive propagation without cloning.

The previous `Try` implementation could only panic in `branch()`. Its removal
intentionally rejects direct `promise?`, `Try` bounds and `Try::from_output` on
promises. Use `.await?` for asynchronous propagation and ordinary Promise
constructors for returned work. This preserves the useful `Result?` feature
through Rust's [residual conversion interface](https://doc.rust-lang.org/std/ops/trait.FromResidual.html)
without pretending a synchronous branch can await a future. The impossible
`Ok(Infallible)` residual is handled by an exhaustive empty match.

Eight [regression tests](../vendor/capnp/tests/rpc_try.rs) check success, borrowed
and non-Unpin output, metadata ownership, exactly-once error conversion, UTF-8
validation, deferred execution and cancellation before/after polling. Two
doctests accept synchronous validation and reject direct `promise?`. The
workspace [tooling driver](../tests/tooling.rs) runs these under both
`std + alloc` and `no_std + alloc`, then checks the feature without allocation.
Native runs use `nightly-2026-08-29`; full LLVM runs select the existing matched
coverage toolchain. The driver is included in `cargo test --workspace` and in
Linux platform smoke CI. This closes a Rust-specific feature gap, not a C++
runtime or stable-Rust language feature.

Local validation passed all 104 capnp tests/doctests with the feature enabled
(four existing doctests ignored), plus the alloc-only checks and the focused
LLVM driver. LLVM export confirms execution of the residual conversion; this
was not a full workspace coverage run. All Cargo commands used the auditable
wrapper; the instrumented test-harness executables do not embed audit inventories
with the existing wrapper. Strict Rustdoc, formatting, workflow lint and offline
links passed; one existing malformed Rustdoc code span was corrected.
The workspace driver and a downstream compilation of the new tests pass strict
Clippy. That pass also exposed 32 package-wide nightly Clippy findings in other
runtime modules, subsequently addressed by the [strict runtime lint pass](#strict-runtime-lint-checks-2026-09-28).
Full workspace verification and hosted CI were not rerun.

## Strict runtime lint checks (2026-09-28)

The maintained runtime now passes `cargo clippy --all-targets -- -D warnings`
as a primary package on Rust 1.97.0 with default features, and on
nightly-2026-08-29 with `rpc_try` enabled.
The workspace's `--no-deps` lint did not cover this excluded package. A dedicated
`capnp_runtime_lints` tooling test closes that gap; platform jobs run the same
stable check and the Linux `nightly_rpc_try_contracts` driver includes nightly
Clippy. Both tooling tests remain part of `cargo test --workspace`.

Nine integer narrowing casts now use checked conversions. Struct dimensions
and parameter counts retain their validated wire/schema bounds; pointer offsets
that cannot fit the host address size return `MessageSizeOverflow`. Alignment,
divisibility and optional-value checks use equivalent standard-library methods,
and the no-allocation segment table reader decodes fixed four-byte chunks.
The allocator-free formatting no-op also explicitly consumes its unused argument,
and the fallback I/O implementations elide redundant lifetimes.

Five intentional cases retain narrow, documented lint settings. Two floating-point
casts keep C++ rounding behavior and the existing range/roundtrip checks. Three
native-release methods retain their public `(Error, Self)` failure results so
rejected conversions return the original owner without an extra allocation.
Those allowances are local to the methods because result sizes vary by target;
no crate-wide lint suppression or public signature change was added.

Local validation passed 94 default-runtime tests/doctests (four existing doctests
ignored), 58 focused integration tests and the pinned C++ numeric comparison:
**1,723 defined conversion observations**, with the same two undefined C++ boundary
cases excluded and tested natively. The optional feature's tests and lint driver
also passed. Strict library Clippy passed locally for `no_std + alloc` and
allocator-free builds, as did strict Rustdoc, formatting, workflow lint and
offline documentation links. Broad TLC/native-reference replay, full
workspace/LLVM verification, benchmarks and hosted platform execution were not
rerun for this cleanup.

## Custom schema source callbacks (2026-09-28)

[`SourceCompiler` / `SourceProvider`](../crates/capnp-compiler/src/source/custom.rs)
close the application-defined input gap alongside the existing in-memory and
disk parsers. A provider resolves requested names and import/embed strings into
opaque identities and logical filenames, then supplies a `Read` stream. Strings
reach the provider without path normalization; there is no implicit disk fallback.
The frontend bounds each read to 4 MiB plus one overflow-detection byte and retains
its shared 16 MiB input limit and existing graph/compilation budgets.

Aliases share identity, cycles reuse already parsed sources, and schema/embed
caches remain separate. `compile()` produces generator requests; reflection
snapshots own their data and sessions borrow the provider for later lazy loading.
Failed compilation or runtime validation discards provisional state, preserving
the last successful snapshot and allowing corrected inputs to be retried. Callback
side effects cannot be rolled back. Custom snapshots expose no inferred disk
dependencies; providers can track their own inputs.

The [native tests](../crates/capnp-compiler/tests/provider.rs) cover opaque paths,
identity deduplication and collisions, recursive aliases, binary embeds, fresh
compilations, lazy selection, cached input deletion, compilation/validation
rollback, read failures, metadata validation and input limits. A compile-fail
doctest prevents a session from escaping its provider's lifetime.

The [pinned C++ comparison](../tests/schema_compiler/provider.rs) exercises
`SchemaFile` callbacks, equality/hash identity, imports, embeds and lazy lookups.
Six complete canonical schema nodes, four available source-info records and one
dynamic message match. C++ does not supply source-info for the imported file and
lazily loaded sibling in this scenario; Rust retains it. The test explicitly
records those two omissions without excluding their schema bytes. Embed read
counts are not equated: C++ can reopen an embed for each use, while Rust caches
its content by identity. Evidence is under
`target/verification/schema-compiler/provider/`.

Local validation passed all **92 compiler tests/doctests**, including ten provider
tests, plus the new C++ oracle and both existing parsed-schema/session C++ suites.
Strict compiler/integration Clippy and Rustdoc passed. All tests remain reachable
through `cargo test --workspace`; the portable tests join the existing three-OS
compiler smoke jobs. Full workspace/LLVM, hosted platform jobs and benchmarks
were not rerun. Concurrent parser caches, complete grammar qualification and
non-Linux ancillary transport remain open; optional file IDs are covered below.

## Optional schema file IDs (2026-09-28)

`SchemaParser`, `FileCompiler` and `SourceCompiler` now expose
[`set_file_ids_required(false)`](../crates/capnp-compiler/src/source.rs) for
configuration schemas without persistent file IDs. The default remains strict.
Missing IDs use `getrandom` 0.4.3 OS entropy with the high bit set; explicit IDs
take precedence even when declared after other declarations. Existing child,
group and implicit method IDs derive from the resulting parent through the
unchanged Cap'n Proto algorithm. Entropy failures and ID collisions return
diagnostics. Source content and declaration byte ranges are not rewritten.

The setting covers requested files and imports in all three providers. Session
creation captures the policy, and cached input identities retain their generated
IDs through cyclic imports, alias lookup and lazy extension. Failed extensions
discard only provisional state; previously committed IDs and metadata remain
unchanged. Fresh compilations generate new IDs, including for unchanged inputs.
Applications needing persistent schema or cross-process RPC identities must
continue supplying explicit IDs. The free `compile()` function and CLI remain
strict. Unlike C++, strict missing-ID diagnostics do not draw randomness merely
to suggest an ID, and the compiler does not retain a cross-session parser cache.

[Native tests](../crates/capnp-compiler/tests/file_ids.rs) cover defaults and policy
reset, explicit declarations, empty sources, fresh identities, dynamic defaults,
all source providers, lazy/cyclic imports, policy capture and transactional
compile/loader failure. Private parser tests inject entropy failure and a collision,
and prove that strict/explicit-ID input does not request entropy. The public API
example is a doctest. Existing lockfile versions are reused, and the compiler
binary's auditable inventory contains `getrandom` 0.4.3.

The [pinned C++ oracle](../tests/schema_compiler/file_ids.rs) exercises real
`SchemaParser::setFileIdsRequired(false)` generation and checks default rejection,
cached identity, fresh-parser identity and explicit overrides. **12 complete
canonical schema/source-info pairs and one dynamic message match.** Since
independent random draws differ, Rust receives the observed C++ file IDs as
explicit declarations appended after the unchanged source. Rust's own random-ID
path is separately compared with an explicit recompile using its generated IDs.
This checks descendant derivation, groups, unions, implicit method structs,
constants and recursive cross-file references without masking schema bytes.
Evidence is under `target/verification/schema-compiler/file-ids/`.

Local validation passed all **104 compiler tests/doctests**, the new C++ oracle
and the existing parsed-schema, lazy-session and provider C++ suites. Strict
Clippy and Rustdoc passed. All tests remain reachable through
`cargo test --workspace`; portable tests run in the existing three-platform
compiler smoke jobs. Full workspace/LLVM, hosted platforms and benchmarks were
not rerun. Concurrent caching, full grammar qualification and non-Linux ancillary
transport remain open.

## Contextual names and numeric grammar (2026-09-28)

The Rust frontend now accepts generic parameters named `import`, including
implicit method parameters, and field types named `group` when the field has an
ordinal. Import expressions still require a following path string; groups without
ordinals and legacy named unions retain their existing syntax. These contextual
name rules match the pinned C++ parser.

The shared lexer also rejects leading-zero numeric prefixes containing `8` or `9`
before a fraction/exponent: `08e1` and `078.5` are errors, while `01e1`, `07.85`
and `0.89` remain valid. C++ tries its octal integer lexer before its floating-point
lexer and splits the rejected spellings into separate tokens. Rust reports an
invalid octal digit directly. The rule also applies to the text-value parser used
by the optional compatibility codecs.

A [shared 295-case corpus](../crates/capnp-compiler/tests/corpus/grammar.rs) covers
contextual keywords/built-ins, duplicate and shadowed generic parameters,
declaration placement, annotation targets, parentheses, trailing commas and
numeric spellings. [Portable tests](../crates/capnp-compiler/tests/grammar.rs)
check acceptance, runtime loading and diagnostic spans, plus concrete generic
bindings through fields and method parameters/results.

The [pinned C++ oracle](../tests/schema_compiler/grammar.rs) agrees on all cases:
**205 accepted schemas and 90 shared rejections**. Accepted requests compare all
known node fields, source-info fields and canonical bytes, canonical values,
requested files and normalized identifier references. Compiler-version metadata
and diagnostic wording are excluded. Evidence, including each source and decoded
request, is under `target/verification/schema-compiler/grammar/`.

Local validation passed **107 compiler tests/doctests**, all **38 schema compiler
integration tests**, ten compatibility codec tests, strict Clippy and Rustdoc.
The three production changes are confined to the shared lexer/parser; no new
dependencies or CI matrix were added. Tests remain reachable through
`cargo test --workspace`, with portable cases in the existing three-platform
compiler smoke jobs. Full workspace/LLVM, hosted platforms and benchmarks were
not rerun. This corpus is bounded evidence; complete grammar qualification,
concurrent parser caching and non-Linux ancillary transport remain open.

## Binary whitespace and text numeric fidelity (2026-09-28)

The shared schema/text lexer now accepts vertical tabs before, between and after
hexadecimal byte pairs. Rust's `is_ascii_whitespace()` excludes that character,
whereas KJ includes it. The binary-literal path now uses the same six whitespace
bytes as token spacing.

An additional [108 lexical cases](../crates/capnp-compiler/tests/corpus/grammar.rs)
check all ASCII controls, space and DEL in token spacing, quoted strings and
binary literals, plus selected Unicode separators. The pinned C++ comparison
agrees on **46 accepted requests and 62 rejections**, including schema/source
metadata and canonical values. The portable suite also checks standalone byte
parsing. Evidence is under `target/verification/schema-compiler/lexical/`.

[`TextCodec`](../crates/capnp-compat/src/text.rs) now checks numeric syntax and
schema type before conversion. Numbers can no longer initialize Text or enum
fields, floating-point literals require floating-point fields, and integer
magnitudes are bounded before conversion to floats. Uppercase `0X` is rejected.
Integer `-0` becomes ordinary zero, including for unsigned fields; floating-point
`-0.0` retains its sign. Integer-to-Float32 conversion rounds directly instead of
first rounding through Float64, fixing wire differences near large half-way
points. The compiler's existing checked positive-UInt64 overflow policy applies
here too; pinned C++ wraps oversized positive literals.

The [shared codec corpus](../crates/capnp-compat/tests/common/text_scalars.rs)
checks decimal/hex/octal spellings, both signs, integer and floating-point zero,
large rounding boundaries, infinity/underflow, numeric type/range rejection and
binary whitespace. The C++ oracle now compares **102 accepted input/format
combinations** using canonical wire and exact compact/pretty JSON/text output,
plus **43 shared rejections**. Portable tests independently assert float bits and
the deliberate positive-integer overflow boundary. Numeric rounding and unsigned
negative-zero regressions were reproduced before the fix. Evidence is under
`target/verification/compat/`, including `codecs-summary.txt`.

Local validation passed **108 compiler tests/doctests**, all **29 compatibility
tests** (including the C++ codec and transport oracles), and all **39 schema
compiler integration tests**. Strict Clippy and Rustdoc passed. Cargo commands
used the auditable wrapper, and the new cases remain reachable through
`cargo test --workspace` and the existing portable smoke jobs. Full workspace/LLVM,
hosted platforms and benchmarks were not rerun.

Implicit scalar-to-struct wrapping is covered in the next section. Full grammar
qualification, concurrent parser caching and non-Linux ancillary transport
remain open.

## Text value wrapping and C++ escaping (2026-09-28)

[`TextCodec`](../crates/capnp-compat/src/text.rs) now accepts an unambiguous scalar
for a struct or group by assigning it to the first field in schema order. For
example, `child = true` creates the same value as `child = (flag = true)` when
`flag` is the child's first field. The conversion supports nested fields, bound
generic types, list elements, union members and typed orphan decoding. Other
fields retain schema defaults; replacing an existing nested value resets its
other fields to those defaults and leaves unrelated parent fields intact.

The conversion is limited to one wrapper, preserves numeric range/rounding rules
and uses the existing nesting budget. It does not search later fields or infer
enum/list/Data literals from a struct's first field. Quoted Text remains Text,
even when that field is Data. Explicit nested expressions can supply ambiguous
values. C++'s mutable-root decoder still requires a struct expression, and Rust
keeps that distinction from `decode_orphan()`.

The [shared wrapping corpus](../crates/capnp-compat/tests/common/text_wrapping.rs)
adds **25 accepted values and 23 rejected forms**. Native tests compare implicit
values with explicit equivalents and check replacement, group defaults, union
selection and arbitrary text bytes. The pinned C++ fixture now generates bindings
to call its public typed orphan decoder, alongside its mutable-root decoder.
The complete codec oracle matches **152 accepted input/format combinations** in
canonical wire bytes and exact compact/pretty JSON/text, plus **66 rejections**.
Generated bindings and evidence stay in `target/verification/compat/`.

Those comparisons also exposed and fixed existing stringifier differences:
valid Unicode is emitted directly in Text, and controls/quotes use C++'s short
escapes (`\a`, `\b`, `\f`, `\v`, `\'` as well as the existing forms). Data high
bytes remain octal-escaped. Malformed UTF-8 Text uses reversible octal escapes
because Rust returns a UTF-8 `String`; C++ can emit invalid bytes literally.
Portable tests explicitly verify that boundary without discarding bytes.

Local validation passed all **33 compatibility tests**, including both C++
oracles, strict Clippy and Rustdoc. Formatting and offline documentation links
passed. Cargo commands used the auditable wrapper. New tests remain part of
`cargo test --workspace`; the portable cases run in the existing Linux/macOS/
Windows codec smoke jobs. Full workspace/LLVM, hosted platforms and benchmarks
were not rerun.

Ordered repeated assignments and successive union selections are covered in the
next section. Full grammar qualification, concurrent parser caching and non-Linux
ancillary transport remain open.

## Ordered text assignments and group failure state (2026-09-28)

[`TextCodec`](../crates/capnp-compat/src/text.rs) now evaluates assignments in source
order, matching C++ for repeated fields and successive union selections. For
example, `(i32 = 1, i32 = 2)` leaves `i32` equal to 2. Replacing a struct pointer or
list creates a fresh value with schema defaults; it does not merge with the old
value. A failing slot assignment retains the old value and union selection,
while earlier successful assignments remain visible.

Groups initialize and fill in place. Reassigning a group resets its non-union
fields and default union alternative recursively, preserving storage exclusive
to inactive alternatives. A failed group assignment exposes that reset and any
successfully assigned prefix, including activation of a group union member.
Syntax errors occur before mutation; semantic errors stop at the first failure.
Orphan decode failures leave the destination unchanged.

The [shared assignment corpus](../crates/capnp-compat/tests/common/text_assignments.rs)
contains **36 cases**, including **15 failures**, checked in compact and pretty
modes. The pinned C++ oracle compares canonical wire bytes and exact JSON/text
for all **72 combinations**, including the destination remaining after an error.
It checks repeated scalar/pointer/list assignments, nested struct replacement,
group defaults, successive union selections, pre-populated roots and typed
orphans. A nested-group fixture catches inactive union storage that is invisible
in printed values. Portable tests independently assert expected logical results.
The repeated-field rejection was reproduced before the fix.

The complete oracle retains the prior **152 accepted combinations and 66 shared
rejections**, alongside these 72 state comparisons. Inputs, seeds, diagnostics
and encodings are under `target/verification/compat/assignment-*`; totals are in
`codecs-summary.txt`.

Local validation passed all **34 compatibility tests**, including both C++
oracles, strict Clippy and Rustdoc. Formatting and offline documentation links
passed. Cargo commands used the auditable wrapper. The new cases run through
`cargo test --workspace` and the existing Linux/macOS/Windows codec smoke jobs.
Full workspace/LLVM, hosted platforms and benchmarks were not rerun.

The general dynamic builder's group initialization/clearing is covered in the
next section. Full grammar qualification, concurrent parser caching and
non-Linux ancillary transport remain open.

## Loaded dynamic group initialization and clearing (2026-09-28)

The [loaded dynamic builder](../vendor/capnp/src/schema_loader/dynamic.rs) now
matches C++ and compiled Rust reflection for group initialization and clearing.
`init_struct()` resets group fields before returning a writable view. `clear()`
recursively clears the default union alternative and non-union fields, preserving
storage exclusive to inactive alternatives. Both select a group union member;
unrelated fields and generic bindings survive. The explicit `group()` view
continues to select without resetting. The duplicate TextCodec reset helper has
been removed in favor of this shared runtime behavior.

Orphan adoption retains its stronger cleanup contract: hidden storage and owners
are erased recursively before replacement. Its cleanup now has a separate helper,
so the new selective group clear cannot leave nested inactive pointers behind.
The [portable regression](../tests/dynamic_groups.rs) reproduced both original
group mismatches and this potential adoption regression before their fixes.

Sixteen traces compare **38 canonical wire observations** across loaded Rust,
compiled Rust and [pinned C++](../tests/cpp/dynamic-groups.c++). They cover repeated
clear/init, nested default groups, inactive pointer/data storage, union activation,
non-clearing views and generic fields. Additional portable assertions check typed
defaults, writes through returned views, sibling preservation, retained generic
bindings and orphan cleanup. Inputs, the serialized seed, C++ schema request,
outputs and summary are under `target/verification/dynamic-groups/`.

Local validation passed **34 focused runtime/reflection tests** (group reset,
loaded orphans, schema loading, presence and conversions) and all **34 compatibility
tests**, including the codec/transport C++ oracles. Strict Clippy, Rustdoc,
formatting, offline documentation links and actionlint passed. Cargo commands
used the auditable wrapper. The new portable tests join the existing Linux/macOS/
Windows smoke job and remain in `cargo test --workspace`. Full workspace/LLVM,
TLC replay, hosted platforms and benchmarks were not rerun.

Loaded mutable getters on inactive union arms are covered in the next section.
Full grammar qualification, concurrent parser caching and non-Linux ancillary
transport remain open.

## Loaded mutable getter union checks (2026-09-28)

[`get_struct()` and `get_list()`](../vendor/capnp/src/schema_loader/dynamic.rs)
now reject inactive union arms before touching pointers, materializing defaults
or upgrading storage. They share the loaded reader's selection check. Group
getters return an existing view without activating another arm; `group()` remains
an explicit selection operation. Valid active access preserves default
materialization, writable views and generic bindings. Unknown tags select no arm,
but do not prevent access to non-union fields.

The [new suite](../tests/dynamic_getters.rs) compares **53 states**, including
**35 rejections**, with compiled Rust reflection and pinned C++. Cases cover
structs, groups, primitive/struct/nested/capability lists, bound generic fields,
defaulted null pointers, populated values, incompatible pointer kinds retained
behind other arms and unknown tags. Results and canonical bytes match. Rejected
native cases also preserve the complete serialized message. Live capability tests
check that failure neither exposes a builder nor alters hook identity/table
ownership; explicit clearing subsequently releases the owner exactly once.
All three portable regressions failed before the fix.

The existing C++ group oracle now optionally records caught errors and resulting
wire states; its original group-reset mode remains covered. Evidence, seeds,
the C++ schema request and a summary are under
`target/verification/dynamic-getters/`.

Local validation passed **42 focused runtime/reflection tests** and all
**34 compatibility tests**, including C++ group, getter, schema, codec and
transport comparisons. Strict Clippy, Rustdoc, formatting, offline documentation
links and actionlint passed. Cargo commands used the auditable wrapper. New
portable cases join the existing Linux/macOS/Windows smoke job and remain in
`cargo test --workspace`. Full workspace/LLVM, TLC replay, hosted platforms and
benchmarks were not rerun.

Existing nested mutable-list access is covered in the next section. Full grammar
qualification, concurrent parser caching and non-Linux ancillary transport remain
open.

## Existing nested mutable lists (2026-09-28)

Loaded [`ListBuilder::get_list(index)`](../vendor/capnp/src/schema_loader/dynamic.rs)
now exposes existing writable nested lists with checked bounds and element types.
Null children return empty typed views. Compatible views retain their storage,
generic bindings and capability tables; smaller struct-list layouts upgrade
without losing values. Root, nested and orphan list access share one layout helper.

The compiled dynamic getter also routes nested struct lists through the
schema-aware layout getter. Previously it invoked the primitive-list getter,
triggering a debug assertion even for ordinary nested struct lists. The new
regression fails against that implementation and passes with the fix.

[Tests](../tests/dynamic_nested_lists.rs) compare **73 results, lengths and wire
states**, including **22 rejections**, with pinned C++. Cases cover ordinary
scalar, bit, enum, text, data, void, capability and branded struct children; null
and empty children; deeper nesting; invalid indices and non-list elements;
primitive/short struct-list upgrades; incompatible bit lists; and larger physical
struct layouts with unknown fields. Portable tests additionally check unchanged
full serialized bytes on compatible reads and failures, preserved siblings and
values, and retained live capability identity/ownership through extraction and
replacement. A compile-fail doctest enforces exclusive parent/child access.
Evidence is under `target/verification/dynamic-nested-lists/`.

Local validation passed **46 focused runtime/reflection tests**, all **34
compatibility tests**, and both loaded-list borrowing doctests. Strict Clippy,
Rustdoc, formatting, offline documentation links and actionlint for the changed
workflow passed. Cargo used the auditable wrapper. The three portable regressions
join Linux/macOS/Windows smoke CI and the suite remains in `cargo test --workspace`.
Full workspace/LLVM, TLC replay, hosted platforms and benchmarks were not rerun.
A broader actionlint invocation also reported the pre-existing `ubuntu-26.04`
label in `full-quality.yml` as unknown to the local tool; that workflow is unchanged.

Mutable text/data access and sized initialization are covered in the next section.
Full grammar qualification, concurrent parser caching and non-Linux ancillary
transport remain open.

## Loaded mutable text/data access (2026-09-28)

Loaded [`Builder` and `ListBuilder`](../vendor/capnp/src/schema_loader/dynamic.rs)
now expose `get_text()`, `get_data()`, `init_text()` and `init_data()`. Getters
borrow existing storage, reject inactive union arms before interpreting pointers,
and materialize only nonempty field defaults. Generic Text/Data fields retain
their bindings. Initializers validate schema type, bounds and wire size before
replacing storage or selecting a union arm. New content is zeroed and text keeps
its trailing terminator. Successful replacement releases retained capabilities.

Compiled dynamic text/data field getters previously materialized empty defaults.
They now preserve null pointers, matching C++. A portable regression fails with
the previous getter and passes with this change.

The [new suite](../tests/dynamic_blobs.rs) compares **100 results, blob contents
and canonical wire states**, including **25 rejections**, against pinned C++.
It covers defaults, null/populated/empty storage, bound generic group fields,
inactive/unknown union selections, mutable edits, sized initialization, bad
pointer/list layouts, missing text termination, invalid UTF-8 bytes and list
bounds. Separate portable checks cover backing addresses, full message bytes,
default ownership, wire-size rejection before allocation, retained capability
identity and cleanup. Two compile-fail doctests check message lifetime and
exclusive parent/element access. Evidence stays under
`target/verification/dynamic-blobs/`.

Loaded initializers retain their stronger prevalidation contract: invalid types
and sizes do not change union selection. These failures are tested locally;
C++ selects the union arm earlier, so failure states are not claimed equivalent.

Local validation passed **50 focused runtime/reflection tests**, all **34
compatibility tests**, and **7 loaded-reflection borrowing doctests**. Strict
Clippy, Rustdoc, formatting, offline documentation links and actionlint for the
changed workflow passed. Cargo used the auditable wrapper. Three portable tests
join Linux/macOS/Windows smoke CI; all new tests remain in `cargo test --workspace`.
Full workspace/LLVM, TLC replay, hosted platforms and benchmarks were not rerun.

Checked mutable AnyPointer/AnyStruct/AnyList field access is covered in the next
section. Full grammar qualification, concurrent parser caching and non-Linux
ancillary transport remain open.

## Loaded AnyPointer, AnyStruct and AnyList fields (2026-09-28)

The [loaded builder](../vendor/capnp/src/schema_loader/dynamic/pointer.rs) now
provides `get_any_pointer()`, `get_any_struct()` and `get_any_list()`, with matching
initializers and `init_any_struct_list()` for inline-composite lists. Each method
requires the field's matching resolved constraint. Constrained views preserve the
outer pointer kind; unconstrained fields expose a raw pointer builder. Generic
AnyPointer fields work, while concrete bindings and capability/interface fields
cannot be accessed through an incompatible raw accessor.

Getters reject inactive arms before pointer access. Populated views retain
physical sections/encodings, unknown fields, backing addresses and capability
tables. Null AnyStruct access materializes an empty struct, matching C++;
null AnyPointer/AnyList access stays null. Initializers select the arm, zero new
storage and release old descendants. Invalid constraints, encodings, element
counts and struct-list word counts leave selection and storage unchanged.

[Verification](../tests/dynamic_pointers.rs) compares **85 results, physical
layouts and canonical wire states**, including **21 rejections**, with compiled
Rust and pinned C++. The C++ oracle performs dynamic lookup followed by the
corresponding raw/AnyStruct/AnyList conversion. Cases cover null/empty/populated
storage, every list encoding, far pointers, groups, generic bindings, union tags,
wrong pointer kinds, mutation and initialization. Additional native checks cover
full message preservation, schema constraints, overflow, live capability-table
identity, descendant ownership and clients retained across replacement. Two
compile-fail doctests enforce message lifetime and exclusive mutable views.
Evidence stays under `target/verification/dynamic-pointers/`.

The Rust API's constraint-specific access and initializer prevalidation are
stronger than C++ raw-pointer reflection. Those rejections are tested locally;
C++ failure states are not claimed equivalent for unsupported accessors or
oversized initialization.

Local validation passed **47 focused runtime/reflection tests** and **9 loaded
reflection borrowing doctests**. Strict Clippy, Rustdoc, formatting, offline
links and actionlint for the changed workflow passed. Cargo used the auditable
wrapper. Four portable tests join Linux/macOS/Windows smoke CI, and all new tests
remain in `cargo test --workspace`. Full workspace/LLVM, compatibility codecs,
TLC replay, hosted platforms and benchmarks were not rerun.

Loaded schemas on schema-free AnyStruct/AnyList views are covered below. Full
grammar qualification, concurrent parser caching and non-Linux ancillary
transport remain open.

## Loaded casts for schema-free struct/list views (2026-09-29)

AnyStruct and AnyList readers/builders now provide `get_as_loaded()` using a
loader-borrowed struct schema or list element type. The
[implementation](../vendor/capnp/src/schema_loader/dynamic/cast.rs) attaches
metadata without copying storage or requiring native registration. Readers use
schema defaults for missing fields. Builders require sufficient physical
sections and never resize the borrowed layout. List casts check encoding,
struct nesting depth and resolved metadata, rejecting mismatched schema kinds
and unknown or symbolic types even within nested lists.

[Verification](../tests/loaded_view_casts.rs) compares **62 casts** against
compiled Rust. **41 compatible values and canonical wire states** also match
pinned C++; **21 stricter layout rejections** are checked against compiled Rust.
Cases include defaults, generic Text/Data bindings, old primitive/pointer list
encodings viewed as structs, inline projections, packed bits, unknown enum
ordinals, nested lists, far pointers and mutation. Separate checks preserve
backing addresses, unknown fields, full messages on failure, capability ownership
and reader budgets. Invalid child pointers are rejected when accessed, not
traversed by the cast. Three compile-fail doctests enforce loader/message
lifetimes and exclusive mutable views.

Pinned C++ lacks a mutable AnyList-to-DynamicList overload taking a runtime
schema. Its owning pointer getter supplies the equivalent mutations only where
storage already fits. The oracle uses fresh readers after mutation to avoid the
previously identified C++ projected-builder reader-offset defect. C++ unchecked
casts and storage-upgrading getters are not used as failure-state oracles.
Evidence stays under `target/verification/loaded-view-casts/`.

Local validation passed **56 focused runtime/reflection tests** and **3 new
borrowing doctests**. Strict Clippy, Rustdoc, allocator-free and alloc-only builds,
formatting, offline links and actionlint for the changed workflow passed. Cargo
used the auditable wrapper. Five portable tests join Linux/macOS/Windows smoke
CI; all new tests remain in `cargo test --workspace`. Full workspace/LLVM, TLC
replay, compatibility codecs, hosted platforms and benchmarks were not rerun.

Loaded mutable view erasure is covered below.

## Loaded mutable view erasure (2026-09-29)

Loaded `dynamic::Builder::into_any_struct()` and
`dynamic::ListBuilder::into_any_list()` now transfer their existing layout into
schema-free builders. They require no native registration, allocation or child
traversal. Unknown fields, physical list encoding, capability tables and the
original exclusive storage borrow survive. Null children stay null. For groups,
erasure exposes the complete containing struct, including sibling fields;
inline list projections likewise retain all physical sections.

The [erasure suite](../tests/loaded_view_casts/erasure.rs) compares raw edits for
**15 mutable layouts** with compiled Rust and **13 supported cases** with pinned
C++ dynamic-to-raw constructors. The two inline pointer projections affected by
the known C++ offset defect are tested against compiled Rust. Additional checks
cover empty/void/byte/word lists, null nested children, malformed pointees, group
storage, reborrowed edits and unknown fields. Capability hook tripwires verify
that erasure does not extract, clone, identify, resolve or call clients, and that
raw replacement releases only the table's original reference. Three compile-fail
doctests enforce message lifetime and exclusive mutable access.

Local validation passed **35 focused runtime/reflection tests** and **3 new
borrowing doctests**, plus strict Clippy, Rustdoc and no-default-feature/alloc-only
builds. Formatting and offline documentation links passed. All Cargo commands
used the auditable wrapper. The existing loaded-cast smoke command now includes
nine portable tests on Linux/macOS/Windows; its C++ oracle runs on Linux. All
remain in `cargo test --workspace`. Full workspace/LLVM, TLC replay, hosted
platforms and benchmarks were not rerun. Erasure logs and counts are under
`target/verification/loaded-view-casts/`.

The macOS descriptor implementation follows below. Full compiler grammar
qualification and concurrent parser caching also remain open.

## macOS descriptor transport (2026-09-29)

`unix_rpc` is now enabled on Linux and macOS. Its clients, server facade, caller
FD slots and bounded staging share one ownership implementation. Platform calls
are isolated in [ancillary.rs](../src/unix_rpc/ancillary.rs); Windows and other
Unix targets retain their previous module gates. The transport keeps a common
253-FD per-message ceiling.

Darwin control-header lengths use their native types. Receives always provide
space for 512 descriptors, including when the caller accepts none, then adopt
and explicitly close extras. This follows the pinned KJ workaround for truncated
rights and the bound in [Apple's XNU Unix-socket implementation](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/uipc_usrreq.c).
Parsing clamps each header to the returned control-buffer length. Received FDs
are marked close-on-exec before exposure; if configuring one fails, the entire
receipt is closed, including later entries. This fallback is not atomic with
concurrent fork/exec. Linux retains `MSG_CMSG_CLOEXEC` and kernel truncation.
macOS sends set the socket's `SO_NOSIGPIPE` option rather than changing global
signal disposition; see [Apple's socket options](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/setsockopt.2.html).

Linux retains its qualified ancillary-barrier prefetch. macOS reads exact frame
headers and bodies so stream coalescing cannot attach capabilities to a later
bare frame. Its bodies use owned direct-read storage, even with caller word
scratch; FD slots still use bounded staging without a descriptor Vec. Buffered
sharing, prefetch counts and allocation budgets are therefore Linux-only
contracts. Descriptor limits, cancellation, cleanup and capability calls remain
shared contracts.

Local validation passed **31 focused transport/RPC tests**, including the existing
pinned C++ boundary/scratch comparisons, and **2 borrowing doctests**. New tests
cover the fallback receive policy, explicit excess closure, injected fcntl
failure, truncated control headers, default-SIGPIPE subprocess behavior and
queued/split frame boundaries. Strict native Clippy and Rustdoc passed. The
byte-identical transport module and its unit tests also passed strict Clippy for
`aarch64-apple-darwin` in a disposable dependency fixture. A full-crate Darwin
check was blocked by the missing Apple SDK header required by `ring`.

Linux/macOS CI now runs the portable descriptor, facade and capability tests,
plus strict transport Clippy; Linux keeps its allocation budget job and pinned
C++/TLC coverage. All tests remain in `cargo test --workspace`. **Native macOS
execution has not run here**, so macOS transport is implemented but not yet
qualified. Full workspace/LLVM, TLC replay, hosted CI and performance benchmarks
were not rerun. Logs are under `target/macos-fds-*.log`.

The next acceptance step is a native macOS CI run. FreeBSD transport, full
compiler grammar qualification and concurrent parser caching remain open.

## Schema numeric lexing and lazy imports (2026-09-29)

The Rust frontend now validates numeric token syntax before dependency selection.
Previously, loading an imported file could silently accept `1ee2`, `0b10` or
`1.0.0` in an unused constant: evaluation was the only validation point. Every
opened source now rejects these forms with a source diagnostic. The shared
standalone text-value parser receives the same correction.

Well-formed numbers retain lazy type/range validation. Unneeded files are still
unopened; discovering an invalid file during session extension preserves the
prior snapshot and input dependencies. Correcting the file allows a retry,
including changing its file ID without retaining provisional declarations.
Integers exceeding UInt64 remain rejected when evaluated; the pinned C++ lexer
wraps them, which remains an intentional difference. Incomplete exponents cause
C++ exceptions, so differential tests compare rejection rather than diagnostics.

The shared [numeric corpus](../crates/capnp-compiler/tests/corpus/numbers.rs)
contains 33 valid and 33 invalid expressions, tested in requested files and unused
imported declarations. All **132 comparisons** agree with pinned C++ on acceptance;
accepted requests also match known schema fields, canonical values, source info
and normalized identifier references. Portable tests cover text values and
session rollback/retry. Tests remain in `cargo test --workspace` and the existing
Linux/macOS/Windows compiler smoke jobs. Evidence lives under
`target/verification/schema-compiler/numeric/` and `numeric-imports/`.

Local validation passed **111 compiler tests/doctests**, **17 codec tests** and
the new C++ differential test. Strict compiler/integration Clippy, formatting and
offline documentation links passed. Cargo used the auditable wrapper throughout.
Full workspace/LLVM, hosted platforms and benchmarks were not rerun.

Full grammar qualification and concurrent parser caching remain open.

## Concurrent parser caching and upstream grammar qualification (2026-09-29)

`ConcurrentSchemaParser` now shares a worker-owned lazy session between threads.
Memory/disk frontends expose `into_concurrent` constructors; an owned provider
constructor accepts `Send` providers without requiring `Sync`. A bounded queue
serializes extensions. Successful source reads and positive alias lookups are
cached; failures preserve the current generation and can retry. Existing borrowed
providers and exclusive sessions retain their original ownership rules.

`CachedSchemas` and `CachedSchema` are immutable `Send + Sync` handles. Old
snapshots remain usable after further loads, source deletion, worker failure or
cache destruction. `materialize()` creates local validated runtime reflection
objects; runtime loaders themselves remain thread-local. The final cache client
closes and joins its worker. Callback reentry is rejected; provider panic releases
waiting callers with errors. Persistence, invalidation, parallel compilation and
in-place shared runtime-handle mutation are outside this API.

Eight portable cache tests include 32 simultaneous duplicate lookups (one source
read per identity and one shared compilation), concurrent distinct extensions,
atomic batch failure, limits, source correction/retry, optional IDs, non-Sync
provider ownership, retained snapshots, shutdown, reentry and 24 waiters released
after a provider panic. The pinned C++ lazy-loader oracle now compares all 18
snapshots from both the exclusive and concurrent APIs, including aliases,
generic erasure, source metadata and declaration/dependency closures.

The [qualification report](SCHEMA_COMPILER_QUALIFICATION.md) maps language
production families to tests. The new upstream corpus discovers all 22 real
schemas in the pinned C++ tree, including the language's lexer/grammar schemas,
compatibility definitions, examples and benchmark schemas. Every request matches
under standard source roots. A separate mixed-root regression records two
first-discovered display-name/prefix differences and compares all other fields
unchanged. The import-discovery differences are fixed in the following section.
Full language equivalence is still unqualified.

Local validation passed **120 compiler tests/doctests** and the full **42-test
schema compiler integration suite**, including generated Rust/RPC execution and
all pinned C++ comparisons. Strict Clippy and Rustdoc, touched-file formatting
and offline documentation links passed. Cargo used the auditable wrapper.
Tests remain in `cargo test --workspace`; cache tests also join existing
Linux/macOS/Windows compiler smoke jobs. Full workspace/LLVM, hosted platforms
and performance benchmarks were not rerun. Evidence is in
`target/verification/schema-compiler/upstream/`, `session/` and
`target/compiler-cache-*.log` / `target/compiler-qualification-*.log`.


## Schema import-discovery and compilation-order parity (2026-09-29)

The mixed-root `test-import2.capnp` request now matches pinned C++ including
`src/capnp/c++.capnp`, its `namespace` annotation and their display-name prefixes.
The differential test no longer rewrites those names. Import metadata is loaded
after semantic compilation. An iterative traversal visits declaration dependencies
in schema order, then parents, children in declaration order and aliases in name
order, rather than sorting compilation by source-discovery indices.

Compilation now resolves struct/group slots in their shared ordinal order, applies
member annotations in source order, translates inline RPC signatures during the
interface bootstrap, and evaluates composite pointer defaults after that bootstrap.
Auxiliary group dependencies follow declaration order independently of field
ordinals. Expanded-value limits are checked as defaults are constructed, including
deferred defaults in groups and method parameter/result structs.

The shared [22-case corpus](../crates/capnp-compiler/tests/corpus/discovery.rs)
compares competing relative/absolute import spellings, nested aliases, declarations,
groups, annotations, RPC signatures, generic dependencies and default phases.
All known request fields, canonical values/source info and normalized identifiers
match pinned C++ without display-name adjustments. Portable tests cover in-memory
compilation, concurrent-cache initialization/extensions, immutable old snapshots,
and deferred group/method default budgets. Tests use `cargo test --workspace` and
the existing Linux/macOS/Windows compiler smoke jobs.

Full grammar equivalence remains unqualified. Checked integer overflow, consistent
unbound-generic-list rejection, bounded resources and ordinary errors in place of
C++ exceptions remain deliberate differences; see the
[qualification report](SCHEMA_COMPILER_QUALIFICATION.md).

Local validation passed **122 compiler tests/doctests** (the full compiler suite
plus the added deferred-budget regression) and the full **43-test root schema
compiler suite**, including generated Rust/RPC execution and all pinned C++
comparisons. Strict Clippy, Rustdoc, touched-file formatting and offline links
passed. Cargo used the auditable wrapper. Full workspace/LLVM, hosted platform
jobs and benchmarks were not rerun. Evidence is under
`target/verification/schema-compiler/discovery/` and `upstream/`, with check logs
in `target/compiler-parity-*.log`.
