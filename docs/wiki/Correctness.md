# Correctness and resilience work

Prefer public types that make invalid construction and ownership transitions
unrepresentable, then use runtime validation for untrusted input and distributed
state. [RPC applications](RPC-Applications.md#structured-server-replies) documents
the latest consuming reply API. [Roadmap](Roadmap.md) owns feature priorities.

## Existing foundations

Checked identities/configuration, distinct object/revision/generation types,
opaque reservations, immutable snapshots/cursors and owned task lifecycles cover
several API boundaries. Generated field operations and structured replies add
borrowing and consuming transitions. Compile-contract tests include positive
controls and check exact diagnostic categories for rejected usage.

Runtime evidence combines integration tests, selected independent C++ comparisons,
TLA+ bounded graphs with named fault controls, generated histories, local schedule
exploration, Miri, fuzz/sanitizer controls and scoped implementation mutations.
Those techniques cover different failure classes. Model mutations are not Rust
mutation coverage, and a state count is not a correctness percentage.

## Remaining work and acceptance

| Area | Next work | Evidence required |
| --- | --- | --- |
| API construction | Audit remaining raw IDs, public phase fields and mixed-state builders; preserve useful incremental APIs | Valid downstream programs plus compiler rejection of concrete misuses |
| Error/outcome contracts | Distinguish rejection, cancellation-before-start and uncertain execution | Races against start/commit, lost replies and no unsafe implicit retries |
| RPC resources | Byte/principal/local-unresolved budgets and bounded batches | Account for preparation, active writes, retained replies and control progress |
| Hostile inputs | Extend framing/schema/transport/storage fuzzing and minimized corpora | Bounded CI runs, reproducible failures, native/sanitizer checks |
| Unsafe ownership | Audit reader arenas, mappings, FFI and descriptor lifetime | Miri/negative controls where supported; platform-specific native checks |
| Fault composition | Combine partial I/O, disconnect, cancellation, expiry, resource exhaustion and storage failure | Observable cleanup, retained authority and truthful outcomes |
| Soak and performance | Long-lived capability/table/task/FD/buffer bounds and latency distributions | Fixed offered load, rejection rates, source/environment identity and memory growth |
| Simulation | Extend arbitration, handoff, provisioning and rendezvous through production IO boundaries | Recorded/minimized schedules; clearly state uncontrolled TLS entropy |
| Concurrency | Extend local task scheduling; apply Loom selectively to custom atomics | Checked lifecycle/credit invariants, not blanket lock-free claims |
| Model correspondence | More pending/cyclic RPC histories and cross-layer handoff/recovery | Explicit observed invariants, fairness, bounds and graph/corpus provenance |
| Native admission security | Review identity/context bindings, replay, malformed participants, key retirement and resource abuse | Independent review plus resolved findings and regressions |
| Durability | Cold recovery, writeback errors, backup restore and retained snapshot behavior | Disposable faulted filesystems and an external acknowledgement oracle |
| Platforms | Native macOS descriptor execution and advertised deployment targets | Results from those platforms; cross-compilation is insufficient |
| Dependencies | Maintain advisory/source/license policy and fork provenance; consider reviewed dependency audits | Reproducible source pins, SBOM/advisory reports and reviewed exceptions |

## Limits and tools

Native transport uses standard TLS 1.3 over TCP or quiche QUIC v1/v2. Application
0-RTT remains disabled. Retired custom-handshake tests and deterministic crypto
fixtures do not qualify the TLS implementation. TLA+ ideal-crypto assumptions
need separate implementation evidence.

Selective code-level proof tools can be evaluated for a specific critical
algorithm with stated language/trusted-code limits. They do not replace runtime
checks or require an unbounded proof before fixing a concrete bug. API semver
checks become relevant when a stable compatibility policy is declared.

[Testing](Testing.md), [quality](Quality-and-Benchmarks.md), [model conformance](Model-Conformance.md)
and [release acceptance](Release-Acceptance.md) provide the current commands and
boundaries. The [historical correctness ledger](../archive/Correctness-History.md)
retains old per-change counts and proposals; it is not a current transport guide.
