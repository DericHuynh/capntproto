# C++ and Rust parity

The reference is C++ Cap'n Proto revision
`0de72d8d8cec6b69edaa29de51d3bd490341f9c2`, recorded in
[provenance](../../vendor/provenance/revision.json). The maintained Rust core,
RPC engine, async serializer and generator are coordinated forks, alongside
first-party compiler, adapters, transports and storage. This is a feature
comparison, not a line-by-line port or a proof of equivalent execution.

## RPC protocol and capabilities

The runtime handles Bootstrap, Call, Return, Finish, Resolve, Release,
Disembargo, Abort and Unimplemented, plus Provide/Accept and third-party
capability descriptors. It supports hosted/promise reference accounting,
promised-answer transforms, tail transfers, streaming hints, cancellation
policies, server identity, membranes, revocation and reconnect wrappers.
See [runtime tests](../../tests/runtime.rs) and [RPC implementation](../../crates/capntproto-rpc/src/rpc.rs).

| Additional operation | Rust support | Reference boundary |
| --- | --- | --- |
| General capability Join | Local, bilateral and authenticated multiparty paths | Pinned C++ dispatcher returns Unimplemented for general Join |
| Third-party tail-call answers | Authenticated rendezvous and direct results | Pinned C++ dispatcher lacks ThirdPartyAnswer / awaitFromThirdParty |
| Active pipeline migration | Native Join fence preserves old-path ordering | No corresponding general-Join path in that C++ dispatcher |

These features use standard schema message arms with network-specific authority
payloads. Rust-to-Rust coverage is not C++ interoperability evidence.
[Join](Capability-Join.md) and [third-party answers](Third-Party-Answers.md) define the contracts.

## RPC flow control, I/O and diagnostics

Variable/adaptive streaming windows, socket-informed TCP flow control,
scatter/gather write batches, buffered reads and caller-supplied scratch storage
are implemented. Two-party client/server owners expose listeners, shutdown and
disconnect observation. Linux/macOS Unix sockets implement SCM_RIGHTS; native
macOS qualification remains pending. Other Unix descriptor backends are not enabled.

Outgoing Call count admission and protocol resource snapshots are available.
Legacy queue metrics cover pending messages; the new `output_snapshot()` includes
queued and active writes. These controls do not bound total process memory.
[RPC admission](RPC-Applications.md#outgoing-admission-and-resource-observation)
and [flow-control tests](../../tests/flow_control.rs) describe their limits.

## Dynamic reflection and result construction

Compiled and loaded schema metadata support brands, constants, annotations,
member identity, checked conversions, dynamic structs/lists/capabilities,
unknown-field-preserving views and detached ownership. The Rust APIs use
lifetimes and explicit checked casts instead of C++ reference ownership syntax.
See [schema loading](Schema-Loader.md) and [dynamic orphans](Dynamic-Orphans.md).

Legacy result builders provide size hints, orphanage access and independent
pipeline publication. Opt-in generated `Reply<T>` stages add exact-schema tail
forwarding and immutable publication. They prevent specific lifecycle mistakes
at compile time; they do not replace runtime validation of messages or policy.
[Structured replies](RPC-Applications.md#structured-server-replies) defines that boundary.

## Serialization and Rust code generation

Serialization, structural equality, schema-free views and generated reader/editor
APIs preserve the documented wire behavior. Capability-bearing equality can be
unknown; distributed capability identity uses Join. The Rust text compiler is
qualified against a pinned corpus, with explicit deviations and resource limits.
See [generator](Rust-Generator.md) and [compiler qualification](Compiler-Qualification.md).

## Optional C++ adapters and codecs

[capntproto-compat](Compatibility-Adapters.md) supplies JSON/text codecs,
ByteStream, HTTP level 2, WebSocket message framing and JSON-RPC. It remains an
optional crate; applications supply network/authentication integrations.
The whole KJ library and every C++ tool/overload are outside this runtime's scope.

## Verification boundaries

Rust integration tests, selected pinned-C++ differential tests and bounded
TLA+/Rust replay are different evidence. Installed C++ 1.5.0 interoperability is
also separate from source-pinned reference comparisons. Tests do not exhaust
all malformed inputs, proxy chains, cancellation schedules, platforms or
RPC/transport/storage compositions. There is no defensible overall parity percentage.

Run the relevant selections in [Testing](Testing.md). The
[dated symbol audit and implementation records](../archive/Cpp-Parity-History.md)
preserve detailed observations and old run counts. Read those counts with their
source hashes; they are not fresh qualification of the current checkout.
