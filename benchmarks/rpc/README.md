# RPC benchmarks

The existing `workflows.json` catalog and four loopback payloads retain their
contracts and sampling budgets. `topologies.json` adds a separate, versioned
suite compiled into both the Rust driver and the C++ reference executable.

## Deep and concurrent access

| Case | Nested nodes | Outstanding requests per batch | Client TCP connections | Service processes |
| --- | ---: | ---: | ---: | ---: |
| deep-8 | 8 | 1 | 1 | 1 |
| deep-32 | 32 | 1 | 1 | 1 |
| concurrent-32 | 1 | 32 | 1 | 1 |
| deep-32-concurrent-32 | 32 | 32 | 1 | 1 |
| services-4-deep-16 | 16 | 1 | 1 | 4 |
| services-4-deep-16-concurrent-32 | 16 | 32 | 1 | 4 |
| services-4-deep-16-clients-8 | 16 | 32 | 8 | 4 |
| services-8-deep-16-clients-8 | 16 | 32 | 8 | 8 |

Each node has a sequence-dependent value, 32 payload bytes, and a pointer to the
next node. Every service traverses and validates every node and byte. Intermediate
services copy the nested request into a downstream RPC, await its result, add
their checksum and visit count, and copy its nested response back upstream. The
client validates the returned chain, checksum, and total visits. Consequently,
depth is actual pointer traversal and copying, and every service hop is a TCP RPC
to another OS process. A setup RPC verifies the chain length before timing.

Each service shares one persistent downstream connection across its incoming
clients. The eight-connection cases distribute the 32 outstanding requests over
eight independently established connections. All calls are submitted together;
the next batch begins after every response has been validated and released.
This measures closed-loop batch completion and throughput. It does not measure
individual request percentiles or an open-loop saturation workload.

Request construction, traversal, forwarding, response validation/destruction,
and a ten-second batch deadline are inside the timed operation. Connection setup,
bootstrap resolution and topology discovery are outside timing. Rust and C++
perform the same application work; no payload or validation is skipped. The
topology suite compares portable Tokio, legacy Tokio and C++/KJ. The Rust
implementations use the same release binary by default; each client and every
service process explicitly selects its driver, including downstream connections.

## Running the suite

Build the normal benchmark bundle (the quality runner includes `topology.c++`
when compiling the C++ reference), or supply equivalent release binaries:

```sh
python3 benchmarks/rpc/topology-runner.py \
  --rust target/quality/benchmark/bundle/driver \
  --cpp target/quality/benchmark/bundle/capnp-reference \
  --output target/topology-latency --mixed
```

Use the actual bundle paths produced by your build. Defaults are five repetitions,
100 warmups and 1,000 retained batch samples per cell. Variant order rotates and
reverses across cycles. Five runs do not exactly balance every variant position;
the complete order is retained. `--mixed`
also checks every ordered pair of portable, legacy and C++ in alternating service
chains, outside the latency comparison. `--cases` accepts comma-separated catalog
names. `--portable DRIVER` supplies a separate portable candidate while keeping
the legacy reference fixed with `--rust DRIVER`.

To isolate a portable optimization from run-to-run shifts, add
`--portable-baseline FROZEN_PORTABLE`. All four variants still run five times.
`--baseline FROZEN_LEGACY` remains available for a legacy control. At least five
repetitions are required for a qualifying budget. Mixed-chain checks use the three current
implementations; frozen controls run the same homogeneous fixtures.

The runner selects distinct physical cores for the client and available service
workers. Longer chains cycle over the available service cores; the exact placement
is retained. On a five-core host, four-service chains use one core per process,
while eight-service chains intentionally share service cores. These are same-host
TCP microservice chains, not cross-host network measurements.

Every output directory must be new. It retains raw latency samples, per-process
logs, server readiness/chain checks, CPU placement, binary and fixture hashes,
CPU quota/throttling and steal counters around each process lifecycle,
and every trial's p50/p95/p99 and batch/request/service-call throughput. The generated
README adds mean ± sample standard deviation of the repetition means, plus their
minimum and maximum. This measures between-run spread, not a confidence interval.
JSON also retains the pooled arithmetic mean, sample standard deviation and
extrema of all latency samples, and every repetition's mean. The workflow,
portable-loopback and general loopback reports use the same definitions and five
repetitions. All samples remain included; percentiles and acceptance gates are
preserved. Host counters include setup and teardown; they do not modify or filter samples. The
portable latency goal is `portable p50 <= min(legacy p50, C++ p50)` in every
cell; passing some cells does not qualify the suite. Syscall count parity is
assessed separately using diagnostic captures.

## Tracing and profiling

Use separate directories for diagnostic captures:

```sh
python3 benchmarks/rpc/topology-runner.py --rust DRIVER --cpp CPP \
  --output target/topology-socket --instrument socket --repetitions 1
python3 benchmarks/rpc/topology-runner.py --rust DRIVER --cpp CPP \
  --output target/topology-wire --instrument wire --repetitions 1
python3 benchmarks/rpc/analyze-topology.py target/topology-wire
python3 benchmarks/rpc/topology-runner.py --rust DRIVER --cpp CPP \
  --output target/topology-cpu --instrument callgrind --repetitions 1
```

Diagnostic defaults are two warmups and ten measurements. The socket mode retains
the lightweight probe's complete event journal and positive/nonpositive counts
for the client and every service. Wire mode retains full-payload, timestamped
strace journals compressed with gzip. Callgrind records each process separately.
All modes include startup, warmup and shutdown. Instrumented timings are marked
diagnostic and cannot pass the latency gate. Full tracing can perturb socket
fragmentation; compare it with the lightweight probe rather than interpreting
one capture as a universal syscall count.

The wire analyzer reassembles each TCP connection independently, checks framing,
and compares every received frame byte-for-byte with its peer's transmitted
frame. It requires the expected number of connections and reports per-process
read/write counts, protocol message counts, and any unread teardown frames.
It pairs sockets between adjacent services using a unique match of both complete
byte streams, so it also works when strace exposes only socket inodes. Missing
or ambiguous peers and unconsumed calls or responses fail validation.
