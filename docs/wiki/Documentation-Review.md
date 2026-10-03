# Documentation review

This migration reviewed the 72 project-owned Markdown documents present at its
start. Current guides now live in `docs/wiki/`, with Home, a sidebar and a footer
for GitHub Wiki export. Root/community/package READMEs stay where GitHub and
Cargo users expect them. Six superseded ledgers/proposals live in the archive;
they are not exported as current wiki pages.

The review corrected feature/status contradictions, retired duplicated status
ledgers, repaired destinations and separated implementation from qualification.
It did not rerun every historical test or certify every example as standalone.
Benchmark JSON/JSONL evidence remains frozen. Upstream README files, changelogs,
licenses and schema documentation in vendored dependencies retain their upstream
provenance; the two project-owned `REPROTO.md` fork notes are reviewed below.

The [machine-readable inventory](../documentation-review.json) records original
content hashes, decisions, destinations and the maintained document set. Original
paths in the table are migration identifiers, not live links. New guides and
navigation are listed after the table. See [Wiki maintenance](Wiki-Maintenance.md)
for validation, export and the live-publication prerequisite.

## File-by-file decisions

| Original document | Decision and current destination | Review result |
| --- | --- | --- |
| `.github/DESCRIPTION.md` | retained: [DESCRIPTION.md](../../.github/DESCRIPTION.md) | Retain GitHub community metadata/template in its recognized location; review links. |
| `.github/ISSUE_TEMPLATE/bug_report.md` | retained: [bug_report.md](../../.github/ISSUE_TEMPLATE/bug_report.md) | Retain GitHub community metadata/template in its recognized location; review links. |
| `.github/ISSUE_TEMPLATE/documentation.md` | retained: [documentation.md](../../.github/ISSUE_TEMPLATE/documentation.md) | Retain GitHub community metadata/template in its recognized location; review links. |
| `.github/ISSUE_TEMPLATE/feature_request.md` | retained: [feature_request.md](../../.github/ISSUE_TEMPLATE/feature_request.md) | Retain GitHub community metadata/template in its recognized location; review links. |
| `.github/ISSUE_TEMPLATE/question.md` | retained: [question.md](../../.github/ISSUE_TEMPLATE/question.md) | Retain GitHub community metadata/template in its recognized location; review links. |
| `.github/PULL_REQUEST_TEMPLATE.md` | retained: [PULL_REQUEST_TEMPLATE.md](../../.github/PULL_REQUEST_TEMPLATE.md) | Retain GitHub community metadata/template in its recognized location; review links. |
| `CHANGELOG.md` | revised: [CHANGELOG.md](../../CHANGELOG.md) | Retain concise project history and refresh guide destinations. |
| `CODE_OF_CONDUCT.md` | retained: [CODE_OF_CONDUCT.md](../../CODE_OF_CONDUCT.md) | Retain community policy and private reporting contact in GitHub-recognized location. |
| `CONTRIBUTING.md` | revised: [CONTRIBUTING.md](../../CONTRIBUTING.md) | Retain contributor workflow and add wiki link/export validation requirements. |
| `README.md` | revised: [README.md](../../README.md) | Regenerate from the updated template; make Wiki Home the primary guide entry point. |
| `SECURITY.md` | revised: [SECURITY.md](../../SECURITY.md) | Clarify plaintext TCP versus TLS 1.3; retain private reporting channel. |
| `SUPPORT.md` | revised: [SUPPORT.md](../../SUPPORT.md) | Retain support boundaries and refresh guide destinations. |
| `THIRD_PARTY_NOTICES.md` | revised: [THIRD_PARTY_NOTICES.md](../../THIRD_PARTY_NOTICES.md) | Retain license/provenance attribution; correct rustls transport description. |
| `benchmarks/concurrency/README.md` | revised: [README.md](../../benchmarks/concurrency/README.md) | Retain standalone probe setup and link to canonical research guide. |
| `crates/capnp-compat/README.md` | moved and revised: [Compatibility-Adapters.md](Compatibility-Adapters.md), [README.md](../../crates/capnp-compat/README.md) | Move detailed adapter guide to wiki; keep package landing/build README. |
| `crates/capnp-compiler/README.md` | moved and revised: [Schema-Compiler.md](Schema-Compiler.md), [README.md](../../crates/capnp-compiler/README.md) | Move detailed compiler guide to wiki; keep package landing/build README and date corpus claims. |
| `docs/ANSWER_ADOPTION.md` | moved and revised: [Third-Party-Answers.md](Third-Party-Answers.md) | Retain authenticated adoption semantics and explicit C++ interoperability boundary. |
| `docs/ARCHITECTURE.md` | moved and revised: [Architecture.md](Architecture.md) | Retain module ownership; repair guide and source references. |
| `docs/BULK_RUNTIME.md` | moved and revised: [Bulk-Transfer.md](Bulk-Transfer.md) | Retain capability-scoped bulk contracts, limits and test entry points. |
| `docs/COMPONENT_STORAGE.md` | moved and revised: [Component-Storage.md](Component-Storage.md) | Retain explicit V5 opt-in and sharing/CAS/compaction limits. |
| `docs/CONFORMANCE.md` | moved and revised: [Model-Conformance.md](Model-Conformance.md) | Distinguish historical model evidence from current replay gates; remove unavailable log links. |
| `docs/CORRECTNESS_ROADMAP.md` | rewritten; history archived: [Correctness.md](Correctness.md), [Correctness-History.md](../archive/Correctness-History.md) | Replace completed development ledger with current guarantees and remaining acceptance criteria; archive full history. |
| `docs/CPP_PARITY.md` | rewritten; history archived: [Cpp-Parity.md](Cpp-Parity.md), [Cpp-Parity-History.md](../archive/Cpp-Parity-History.md) | Replace chronological ledger with current semantic parity and qualification gaps; archive full history. |
| `docs/DURABLE_BULK.md` | moved and revised: [Durable-Bulk.md](Durable-Bulk.md) | Retain atomic value/receipt contract and recovery limits. |
| `docs/DYNAMIC_ORPHANS.md` | moved and revised: [Dynamic-Orphans.md](Dynamic-Orphans.md) | Retain API and ownership guide; correct command working-directory instruction. |
| `docs/EAE_BENCHMARKS.md` | moved and revised: [EAE-Comparison.md](EAE-Comparison.md) | Keep dated comparison; distinguish implemented worker outcomes from pending durable receipts. |
| `docs/FEATURES.md` | moved and revised: [Service-Models.md](Service-Models.md) | Clarify abstract early-data model versus production 0-RTT support; identify unavailable historical logs. |
| `docs/FORK_POLICY.md` | moved and revised: [Fork-Policy.md](Fork-Policy.md) | Correct storage format policy for default V4 and explicit V5. |
| `docs/GITHUB_SETUP.md` | moved and revised: [GitHub-Setup.md](GitHub-Setup.md) | Use actual remote identity and route documentation maintenance to wiki. |
| `docs/HANDOFF.md` | moved and revised: [Handoff-Model.md](Handoff-Model.md) | Separate TLA+ operators from Rust APIs and update conformance status. |
| `docs/IMPLEMENTATION.md` | moved and revised: [Storage-and-ORM.md](Storage-and-ORM.md) | Extract storage/ORM contracts; remove duplicated omnibus runtime/transport status. |
| `docs/JOIN.md` | moved and revised: [Capability-Join.md](Capability-Join.md) | Retain capability equality/ownership and interoperability limits. |
| `docs/LOCK_FREE_RESEARCH.md` | moved and revised: [Lock-Free-Research.md](Lock-Free-Research.md) | Retain dated measurements, concurrency hypotheses and lifecycle acceptance requirements. |
| `docs/MULTIPARTY_JOIN.md` | moved and revised: [Multiparty-Join.md](Multiparty-Join.md) | Clarify Join secret versus TLS/admission credentials. |
| `docs/NATIVE_ARBITRATION.md` | moved and revised: [Session-Arbitration.md](Session-Arbitration.md) | Remove duplicated authentication prose and preserve arbitration contract. |
| `docs/NATIVE_DEPLOYMENT.md` | moved and revised: [Discovery-and-Mobility.md](Discovery-and-Mobility.md) | Clarify native authenticated profile versus conventional two-party TLS/QUIC. |
| `docs/NATIVE_LISTENER.md` | moved and revised: [Shared-Listeners.md](Shared-Listeners.md) | Retain shared-listener ownership, peer routing and resource bounds. |
| `docs/NATIVE_PROVISIONING.md` | moved and revised: [Provisioning.md](Provisioning.md) | Retain trust and credential provisioning guidance; repair links. |
| `docs/NATIVE_RECOVERY.md` | moved and revised: [Connection-Recovery.md](Connection-Recovery.md) | Retain reconnect/fallback boundaries; repair links. |
| `docs/NATIVE_SCHEDULING.md` | moved and revised: [Transport-Scheduling.md](Transport-Scheduling.md) | Scope packet/datagram scheduling to quiche; clarify TCP distinction. |
| `docs/NATIVE_SHUTDOWN.md` | moved and revised: [Shutdown.md](Shutdown.md) | Describe quiche streams and native TCP multiplexing separately. |
| `docs/NATIVE_TRANSPORTS.md` | moved and revised: [Native-Transports.md](Native-Transports.md) | Retain quiche-only v1/v2 and native TCP transport setup. |
| `docs/ORM_HISTORY.md` | moved and revised: [Publication-History.md](Publication-History.md) | Remove obsolete v3 checkpoint wording; retain publication/history semantics. |
| `docs/PERSISTENCE.md` | moved and revised: [Persistence.md](Persistence.md) | Use typed revisions and distinguish injected I/O errors from device failure qualification. |
| `docs/PREVIEW.md` | moved and revised: [Getting-Started.md](Getting-Started.md) | Correct native build prerequisites, audit wrapper and feature relationships. |
| `docs/PROTOCOL.md` | moved and revised: [Protocol-Models.md](Protocol-Models.md) | Retain bounded model specifications; distinguish active and historical conformance evidence. |
| `docs/QUALITY.md` | moved and revised: [Quality-and-Benchmarks.md](Quality-and-Benchmarks.md) | Retain measured CI/benchmark contracts; repair guide links. |
| `docs/README.md` | moved and revised: [Home.md](Home.md), [README.md](../README.md) | Replace flat index with task-oriented Wiki Home and local entry point. |
| `docs/README.template.md` | revised: [README.template.md](../README.template.md) | Keep concise project/build overview; point to canonical wiki guides. |
| `docs/REALTIME.md` | moved and revised: [Realtime-Model.md](Realtime-Model.md) | Retain model scope; identify unbundled historical verification logs. |
| `docs/REALTIME_RUNTIME.md` | moved and revised: [Realtime-Snapshots.md](Realtime-Snapshots.md) | Retain runtime snapshot lifecycle, bounds and examples; repair links. |
| `docs/RELEASE_ACCEPTANCE.md` | moved and revised: [Release-Acceptance.md](Release-Acceptance.md) | Separate implemented mechanisms from fresh candidate qualification; remove stale release-complete count. |
| `docs/REPORTING.md` | moved and revised: [README-Reports.md](README-Reports.md) | Retain reproducible reporting/README workflow and actual repository identity. |
| `docs/REPOSITORY_LAYOUT.md` | moved and revised: [Repository-Layout.md](Repository-Layout.md) | Document canonical wiki, archive and export ownership; preserve Cargo/dependency boundaries. |
| `docs/ROADMAP.md` | rewritten; history archived: [Roadmap.md](Roadmap.md), [Roadmap-History.md](../archive/Roadmap-History.md) | Replace completed-task ledger with ordered remaining work; archive history. |
| `docs/RPC_APPLICATIONS.md` | moved and revised: [RPC-Applications.md](RPC-Applications.md) | Retain structured replies, pipeline ownership, admission and native vat ergonomics; repair links. |
| `docs/RPC_RESEARCH.md` | moved and revised: [RPC-Research.md](RPC-Research.md) | Mark first RPC implementation slices complete; retain frozen experiment and remaining budget/batch work. |
| `docs/RUNTIME_PORT.md` | rewritten; history archived: [Runtime-Status.md](Runtime-Status.md), [Runtime-Status-History.md](../archive/Runtime-Status-History.md) | Replace chronological inventory with current feature matrix and explicit limits; archive history. |
| `docs/RUST_API_DESIGN.md` | archived: [Rust-API-Design.md](../archive/Rust-API-Design.md) | Archive superseded proposal; direct current users to generator and RPC contracts. |
| `docs/RUST_GENERATOR.md` | moved and revised: [Rust-Generator.md](Rust-Generator.md) | Add structured replies and consuming call ergonomics alongside existing field API. |
| `docs/SCHEMA_COMPILER_QUALIFICATION.md` | moved and revised: [Compiler-Qualification.md](Compiler-Qualification.md) | Replace brittle current repository schema count with dated qualification scope. |
| `docs/SCHEMA_EXCHANGE.md` | moved and revised: [Schema-Exchange.md](Schema-Exchange.md) | Retain exchange validation, bounds and trust contract; repair links. |
| `docs/SCHEMA_LOADER.md` | moved and revised: [Schema-Loader.md](Schema-Loader.md) | Clarify reflection coverage/remaining limits; repair links. |
| `docs/SECURE_TRANSPORTS.md` | moved and revised: [TCP-TLS-and-QUIC.md](TCP-TLS-and-QUIC.md) | Remove stale Quinn-style 0-RTT API reference; retain TCP/TLS/mTLS and quiche v1/v2. |
| `docs/STORAGE_COMPACTION.md` | moved and revised: [Storage-Compaction.md](Storage-Compaction.md) | Retain retention, generation and crash/recovery contracts; repair links. |
| `docs/STORAGE_RESEARCH.md` | moved and revised: [Storage-Research.md](Storage-Research.md) | Keep frozen measurements and distinguish original prototype from later production worker. |
| `docs/STORAGE_RESILIENCE_RESEARCH.md` | moved and revised: [Storage-Resilience.md](Storage-Resilience.md) | Keep failure-boundary evidence and acknowledge implemented worker separately from proposed receipts. |
| `docs/STORAGE_WORKER.md` | moved and revised: [Storage-Worker.md](Storage-Worker.md) | Retain privileged opt-in worker/admission contracts; remove unrelated implementation commentary. |
| `docs/TESTING.md` | rewritten; history archived: [Testing.md](Testing.md), [Testing-History.md](../archive/Testing-History.md) | Replace chronological test log with reproducible check matrix; archive full historical ledger. |
| `research/reports/README.md` | revised: [README.md](../../research/reports/README.md) | Retain evidence provenance and explicit distinction between frozen inputs and unavailable logs. |
| `vendor/capnp/REPROTO.md` | revised: [REPROTO.md](../../vendor/capnp/REPROTO.md) | Retain local fork change notes; update guide paths and structured reply API summary. |
| `vendor/capnpc/REPROTO.md` | revised: [REPROTO.md](../../vendor/capnpc/REPROTO.md) | Retain local generator fork notes; update paths and structured-reply opt-in. |

## New maintenance pages

- [Wiki maintenance](Wiki-Maintenance.md): validation, export and publishing.
- This review and the machine-readable document inventory.
- [Sidebar](_Sidebar.md) and [footer](_Footer.md): GitHub Wiki navigation.
- [Archive index](../archive/README.md): historical records and their replacements.

Old topic files under `docs/` were removed after moving or consolidating their
content. Links to a retired detailed heading point to its labeled archive record;
ordinary usage links point to the current guide. No runtime implementation was
removed as part of this documentation migration.

## Proposal added during the migration

The [Capnt Actors API specification](<../../Capnt Actors API Specification.md>)
appeared while this migration was in progress. It explicitly describes a future,
unimplemented API. It was subsequently renamed to Capnt Actors and aligned with the current
RPC, native transport, authority and storage terminology. A subsequent specification
review added explicit ownership/read proofs, recoverable command failures, canonical
request fingerprints, capability relocation, executor boundaries, outbox retention
and control progress, with acceptance gates and an implementation sequence.
These are proposed contracts, not validated runtime behavior. It remains a separate
design input outside the supported runtime guide set; it is included in link/inventory
checks but is not one of the 72 original review records. This migration does not
validate or implement that proposal.

## ActorDB design documents

On 2026-10-02 the [ActorDB proposal](ActorDB-Proposal.md) and
[implementation checklist](ActorDB-Checklist.md) were added as future design
inputs. They cover event commits, incremental projections, progress receipts,
recovery, retention and phased acceptance. They are linked from Wiki navigation
and included in validation/export, but are not part of the 72 original migration
records. Their open checklist does not certify runtime implementation.
