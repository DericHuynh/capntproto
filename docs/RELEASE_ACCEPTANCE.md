# 0.x release acceptance

Updated 2026-09-20. The target is a **Linux x86-64 developer preview**, distributed
as a coordinated source bundle. It remains experimental transport/storage, not a
complete C++ replacement or a production security qualification. Optional roadmap
features do not block this scope.

## Main blockers and evidence

The earlier audit reproduced lost committed data after a single-bit length change,
a broken external dependency graph and failed registry packaging. Its
[original evidence](../reports/release-review/2026-09-20/review.json) is preserved
as a historical baseline. The old corruption reproducer demonstrates the old bug;
it is not a passing check for the new private format.

| Status / ID | Resolution and acceptance evidence |
|---|---|
| [x] R1 — Downstream dependency graph | Explicit paths coordinate core, RPC, generator and newly vendored `capnp-futures`. The external consumer generates the field API, pipelines a returned capability across two RPC systems and commits/reopens storage. Eight feature combinations and an optimized run pass without a patch table. `cargo test --test tooling external_consumer_feature_matrix -- --exact` records the resolved graph. |
| [x] R2 — Release artifact and provenance | Source distribution selected; registry publication disabled. License, notices, changelog, allowlist, per-file hashes and deterministic archive builder added. Cargo's reproducible archive round-trip and fresh extraction/consumer validation pass. The qualified artifact is `dist/reproto-0.1.0-source.tar.gz`; its adjacent JSON records the checksum, size and file count. |
| [x] R3 — Corrupt framing | RPROTO04 / RPENTRY2 independently checks record framing before trusting length. All 1,920 single-bit header changes across entry, publication and batch frames fail without rewriting. Genuine torn-tail recovery remains covered. Versions 1–3 are rejected without migration. |
| [x] R4 — Recovery barriers | Every successful open syncs the validated data and selected directory before returning. Either failure returns no serving Store. `StorageRecovery.tla`: 734 states / 1,082 concrete trace prefixes / three rejected mutations, including uncertain append/rename, recovery failure and a second crash. |
| [x] R5 — I/O and crash paths | Test-only seams exercise short writes, EINTR, WriteZero, partial ENOSPC/EIO, append/file-sync/directory-sync/rename errors and actual child exit. Atomic two-object publication/receipt, quarantine, writer locks, second recovery and revocation survival are checked. Physical power loss is modeled under an explicit fsync contract. |
| [x] R6 — Clean stable qualification | Rust 1.97.0 pinned and declared minimum. `cargo test --locked --test release isolated_release_qualification -- --ignored --exact` passed on the extracted source with fresh build outputs: 559 default-suite tests passed, zero failed, six explicitly ignored. The gate also ran isolated crash checks, optimized tests, C++ comparisons, TLC, downstream feature combinations and standalone crates. Clippy passed; workspace doctests completed with four existing ignored examples. Evidence is in `target/release-qualification/cargo-qualification.json` and its three Cargo logs. |
| [ ] R7 — Operational contract / publication contact | [Preview guide](PREVIEW.md) covers LocalSet ownership, provisioning, limits, ambiguity, reconnect, revocation, retention and offline backup/restore. Synchronous storage latency and custom Noise are explicit. Security/dependency boundaries are documented. A private reporting contact is intentionally unset; the distributor must choose one before public publication. It does not block local artifact validation. |

The [Cargo test guide](TESTING.md) defines the current acceptance commands;
`target/release-qualification/cargo-qualification.json` records the isolated run's executable
input hashes. The older `reports/reproto` inventory and machine report are
historical evidence. A previously passing report is not current evidence until
its hashes match.
Bounded trace replay, process-exit tests and simulated durable images do not
certify arbitrary executor schedules or hardware/filesystem behavior.

## EAE decision

[Measured comparison and feasibility probe](EAE_BENCHMARKS.md): retain Store as
the runtime default. EAE can reduce redo for small changes in dense, large entries,
but fresh snapshots copy the arena and sparse checkpoints write unused capacity.
The private probe uses the actual vendored generator/core and rejects live
capability pointers before commit. Durable history, allocation/growth and full
ORM/RPC behavior are not implemented by that probe. Backend replacement remains
optional and requires demonstrated contract equivalence.

## Required for production claims or a broader release

These qualify existing behavior; they do not require another wave of optional
protocol features.

| Status / ID | Qualification | Completion evidence |
|---|---|---|
| [ ] P1 — Custom transport security | Review Noise IK/IKpsk2 identity, prologue, transport-parameter and packet-key bindings, PSK lifecycle, replay rejection and resource exhaustion. Pinned BLAKE3 Snow features are verified; that does not review the custom quiche binding. | Recorded specialist review, resolved findings and targeted regressions. A cryptographic theorem is not required to ship; an experimental label alone is insufficient for a production security claim. |
| [ ] P2 — Hostile inputs and lifecycle stress | Finite traces and hand-picked malformed cases leave gaps in wire/schema/transport/storage parsers and long-lived capability-table/task behavior. Upstream quiche fuzzing does not establish coverage of the custom Noise profile. | Reproducible fuzz targets/corpora, sanitizer or appropriate memory checks, and bounded-memory soak tests across cancellation, disconnect, reconnect, revocation, malformed input, loss/reordering and stalled consumers. Fix failures; “all possible schedules” is not an acceptance criterion. |
| [ ] P3 — Advertised peer interoperability | Bilateral RPC/SCM_RIGHTS interop uses installed C++ 1.5.0. Pinned development-source reference checks are separate. General Join/third-party-answer cannot be qualified against a C++ dispatcher lacking those paths. | Name supported peer/version/feature combinations and test those peers. Extend runtime interop to the pinned development version before advertising it. Rust multiparty evidence can support a Rust-only extension; do not claim unsupported C++ paths. |
| [ ] P4 — Deployment performance and storage operations | No integrated workload baseline/soak gate establishes queue/CPU/memory limits, storage-induced executor stalls, loss recovery or restart/compaction cost on deployment filesystems. | Measure latency distributions, memory/table/task counts, recovery time and amplification under stated loads. Isolate blocking storage where required by the latency contract; test backup restoration and faults on supported filesystems/devices. EAE container microbenchmarks do not qualify Capn't Proto. |

A production release can remain Linux-only, single-writer and explicitly
provisioned. It does not need public discovery, NAT traversal, a daemon, a query
engine or an unlimited set of transports.

## Optional for the first release

| Backlog item | Why it can wait / when it becomes required |
|---|---|
| [Complete C++ runtime/API parity](CPP_PARITY.md) | Schema-free aggregate views, general two-party conveniences and platform/adapters remain; required for a **complete replacement** claim, not an accurately scoped 0.x runtime |
| Broader active-pipeline migration qualification | Native Noise implements Join fencing; arbitrary proxy/policy/scheduling combinations still need broader qualification |
| Non-Linux descriptor transport | Required when another OS enters the support matrix |
| Broader discovery/NAT deployment and automatic peer-key retirement | Capability discovery/renewal, STUN rendezvous/mapping refresh and CID/path migration exist; full ICE/TURN, deployment automation and background key lifecycle are optional extensions |
| Native generic specializations, derives, Serde and logical comparison | API conveniences beyond existing generator contracts |
| EAE mutable arena backend | A new strategy with missing ORM/builder integration; not a prerequisite for whole-entry storage |
| Multiwriter, cross-file transactions, indexes and replicated realms | Database/distribution extensions; atomic single-store batches already exist |
| General capability-graph reconstruction | Application factories and explicit SturdyRefs define the supported persistence boundary |
| Realtime grant migration, clock synchronization, hard realtime and reserved bandwidth | New guarantees beyond logical deadline/drop semantics |
| Authenticated 0-RTT application execution | Deliberately rejected; needs a replay contract if added |
| TLS/standard QUIC interoperability | Deliberately outside the custom Noise profile |
| At-rest encryption, hostile-writer rollback protection and erasure of every historical copy | Additional threat models; trusted-local-storage assumptions must remain explicit |
| Unbounded refinement/crypto/power-loss proofs and all-schedule C++ equivalence | Research goals; bounded checks state their limits without making these mandatory release checkboxes |

Do not defer a discovered correctness bug under “optional proof.” Concrete
failures in supported behavior remain blockers regardless of model coverage.
Follow R1–R7 first; retain the [roadmap](ROADMAP.md) as a feature/research backlog.
