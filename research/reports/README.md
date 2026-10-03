# Reports

Current quality and benchmark reports are generated under `target/quality/` and
published as GitHub Actions artifacts. See [the quality guide](../../docs/wiki/Quality-and-Benchmarks.md)
for report generation, coverage gates, and the comparison charts in each report's
README.

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

Historical logs and model-checker output referenced by older design documents
are local archives and are not included in this repository. Model source and
verification fixtures remain under `research/baseline/`, `verification/`, and
`test-support/verification/`.
