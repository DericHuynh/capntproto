# Concurrency probes

Standalone research crate for bounded queues, immutable shared reads and the
current storage worker. Experimental dependencies stay outside the production
workspace. See [the research report](../../docs/wiki/Lock-Free-Research.md) for
methodology, limitations, results and reproduction commands.

The binary accepts `BASE KIND VARIANT THREADS BATCH` and writes one JSON result.
Use [the runner](../../dev/src/probes.rs) for the shuffled matrix and
environment capture. All probes assert their logical results; timing is never a
test pass/fail condition. These probes are not replacement worker implementations.
