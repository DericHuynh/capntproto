# Reports

Current quality and benchmark reports are generated under `target/quality/` and
published as GitHub Actions artifacts. See [the quality guide](../../docs/wiki/Quality-and-Benchmarks.md)
for report generation, coverage gates, and the comparison charts in each report's
README.

The [portable path fusion and retained metadata](portable-performance/2026-10-23-fused-metadata/README.md)
removes repeated capability ownership conversion and completion work, and retains
validated cleanup facts. Five-run comparisons and profiles show fewer instructions
and allocations, modest TLS gains, a QUIC latency regression, and remaining legacy
gaps. Intermediate outcomes and complete measurement evidence are retained.

The [current portable versus legacy gap diagnosis](portable-performance/2026-10-23-legacy-gap/README.md)
adds five-run native and mixed-peer comparisons, ciphertext traces, CPU profiles
and allocation stacks for the code on main. It locates remaining protocol and
ownership overhead despite portable's lower allocation volume and matching
steady socket-count growth in the traced workloads.

The [portable direct-path redesign](portable-performance/2026-10-23-direct-paths/README.md)
replaces shared pipeline drivers, eager result hooks and mutex-owned readiness
with producer completion, direct payload ownership and local readiness. Five-run
TLS/QUIC comparisons, CPU profiles and complete ciphertext traces record the
improvements and remaining legacy latency and I/O gaps.

The [five-run portable comparison](portable-performance/2026-10-18-repetitions/README.md)
adds means and spread across independent repetitions, profiles inline primitive
validation, and preserves a rejected cache experiment. It compares all 24 cases
against current legacy, a frozen portable control and fixed C++; remaining
latency and per-process read/write gaps are reported explicitly.

The JSON and JSONL files in `storage-benchmark/`, `eae-integration/`, and
`storage-next/2026-10-01/`, `storage-resilience/2026-10-01/` and
`concurrency/2026-10-01/` and `rpc-pipeline/2026-10-01/` are frozen
research evidence included in source distributions. They are not results from
the current CI run.

The [storage/RPC follow-up](../../docs/wiki/Storage-Research.md) records 36 runs across
component layouts, held snapshots, executor isolation and batch sizes, with
environment/source hashes and per-run summaries. Its raw evidence totals about
43 KB; temporary database files and binaries remain outside source control.

The [resilience follow-up](../../docs/wiki/Storage-Resilience.md) adds 27 runs
of fixed-rate load, count/byte admission, a worker pause, deadlines and lost
replies. Its environment, counters and per-run distributions total about 54 KB.
The new crash tests are separate from those measurements; neither experiment
injects actual device writeback errors.

The [lock-free follow-up](../../docs/wiki/Lock-Free-Research.md) adds 84 runs comparing
bounded queues, immutable snapshot publication and the current storage worker.
The evidence totals about 86 KB; experimental dependencies live in a standalone
benchmark crate. Synthetic queue/read gains are not production speedup claims.

The [RPC follow-up](../../docs/wiki/RPC-Research.md) adds 15 finite protocol scenarios
with real framing and a paused virtual clock. Three fresh processes reproduced
the same propagation times and wire counters. These are protocol observations,
not CPU, TCP or QUIC performance measurements.

The [portable architecture experiments](portable-performance/2026-10-17-architecture/README.md)
preserve restricted control polling, metadata revisions and borrowed synchronous
dispatch as three independent patches and a combined patch against `fe843e22`.
Source/executable hashes, replay verification and matched local profiles are
included. Raw samples and binaries remain in the workspace evidence archive;
these experiments do not qualify the dedicated 5% latency target.

The [legacy optimization campaign](legacy-performance/2026-10-17/README.md)
freezes legacy at `6e37505d701df4dbea52a92e0691b6037f7f1922` and measures receive
storage reuse, direct pipeline settlement and output batching. Its target is
2× in every RPC workflow and separate loopback payload. All trial summaries,
profiles, source hashes and rejected intermediate results remain retained.

The [legacy tracing follow-up](legacy-performance/2026-10-17-tracing/README.md)
captures every workflow and loopback payload for legacy and C++, on both peers.
It follows C++'s shared batch-header layout, bounds immediate writes while
preserving callback progress, and reuses more small segments within the same
payload-cache budget. Complete traces, profiles, rejected experiments and
repeated timing comparisons remain separate from the unchanged 2× gate.

Historical logs and model-checker output referenced by older design documents
are local archives and are not included in this repository. Model source and
verification fixtures remain under `research/baseline/`, `verification/`, and
`test-support/verification/`.

The [portable deep/concurrent comparison](portable-performance/2026-10-18-topologies/README.md)
adds portable to all eight topology fixtures and compares it with current legacy,
fixed C++ and a frozen portable control. Streaming body readiness hints and
iterative final-field validation improve selected workloads; the full 24-cell
latency target and complete read/write parity remain open. All timing modes,
regressions, rejected scheduling changes and per-process traces are retained.
