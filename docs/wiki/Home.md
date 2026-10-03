# Capntproto Wiki

Capntproto is an experimental Rust implementation of Cap'n Proto schemas,
serialization and capability RPC, with TCP/TLS, quiche QUIC v1/v2, multiparty
capability routing and durable object storage. Package names remain `capntproto`,
`capnp`, `capnp-rpc`, `capnp-futures` and `capnpc`.

These pages describe the development source, not a published stable release.
Start with the [runtime status](Runtime-Status.md) for supported boundaries and
[release acceptance](Release-Acceptance.md) for qualification that remains open.

## Start here

| Task | Guide |
| --- | --- |
| Build a checkout or source bundle | [Getting started](Getting-Started.md) |
| Write a client or server | [RPC applications](RPC-Applications.md) |
| Prevent reply lifecycle mistakes at compile time | [Structured replies](RPC-Applications.md#structured-server-replies) |
| Configure certificates, mTLS or QUIC v2 | [TCP, TLS and QUIC](TCP-TLS-and-QUIC.md) |
| Connect authenticated vats and pass capabilities | [Native transports](Native-Transports.md), [RPC applications](RPC-Applications.md#three-party-capability-transfer) |
| Generate Rust from schemas | [Schema compiler](Schema-Compiler.md), [Rust generator](Rust-Generator.md) |
| Choose a storage layout | [Storage and ORM](Storage-and-ORM.md), [component storage](Component-Storage.md) |
| Keep blocking storage off an async executor | [Storage worker](Storage-Worker.md) |
| Run checks or contribute | [Testing](Testing.md), [contributing](../../CONTRIBUTING.md) |

## RPC and transport

- [Architecture](Architecture.md) and [C++ parity boundaries](Cpp-Parity.md).
- [Capability Join](Capability-Join.md), [multiparty Join](Multiparty-Join.md) and
  [third-party answers](Third-Party-Answers.md).
- [Shared listeners](Shared-Listeners.md), [provisioning](Provisioning.md),
  [session arbitration](Session-Arbitration.md), [connection recovery](Connection-Recovery.md).
- [Discovery and mobility](Discovery-and-Mobility.md), [transport scheduling](Transport-Scheduling.md)
  and [shutdown](Shutdown.md).

## Schemas, services and storage

- [Schema loader](Schema-Loader.md), [schema exchange](Schema-Exchange.md),
  [dynamic orphans](Dynamic-Orphans.md), [compiler qualification](Compiler-Qualification.md)
  and [compatibility adapters](Compatibility-Adapters.md).
- [Bulk transfer](Bulk-Transfer.md), [durable bulk](Durable-Bulk.md) and
  [realtime snapshots](Realtime-Snapshots.md).
- [Publication history](Publication-History.md), [persistence](Persistence.md) and
  [storage compaction](Storage-Compaction.md).

## Research and verification

- [Roadmap](Roadmap.md), [correctness work](Correctness.md),
  [quality and benchmarks](Quality-and-Benchmarks.md) and [README reports](README-Reports.md).
- [RPC research](RPC-Research.md), [storage research](Storage-Research.md),
  [storage resilience](Storage-Resilience.md), [lock-free research](Lock-Free-Research.md)
  and [EAE comparison](EAE-Comparison.md). Measurements retain their original dates and inputs.
- [Protocol models](Protocol-Models.md), [model conformance](Model-Conformance.md),
  [handoff model](Handoff-Model.md), [service models](Service-Models.md) and [realtime model](Realtime-Model.md).
  Models describe bounded scenarios; their counts are not a whole-system proof.

## Proposed ActorDB

The [event sourcing and incremental views proposal](ActorDB-Proposal.md) defines
an optional future database profile above Capnt Actors. Its
[implementation checklist](ActorDB-Checklist.md) records open phases and acceptance
gates. Both describe planned work, not supported runtime features.

## Maintain the project

[Repository layout](Repository-Layout.md) · [Fork policy](Fork-Policy.md) ·
[GitHub setup](GitHub-Setup.md) · [Wiki maintenance](Wiki-Maintenance.md) ·
[Documentation review](Documentation-Review.md) · [Historical records](../archive/README.md)

[Security](../../SECURITY.md) · [Support](../../SUPPORT.md) ·
[Code of conduct](../../CODE_OF_CONDUCT.md) · [Third-party notices](../../THIRD_PARTY_NOTICES.md)
