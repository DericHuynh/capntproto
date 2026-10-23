# RPC benchmarks

The existing `workflows.json` catalog and four loopback payloads retain their
contracts and sampling budgets. `topologies.json` adds a separate, versioned
suite compiled into both the Rust driver and the C++ reference executable.

## Performance evidence runs on CI droplets

Use the dedicated workflow for timing, tracing and profiling; this development
workspace is suitable for builds and correctness checks only:

```sh
gh workflow run performance.yml --ref codex/portable-qualification -f campaign=run-secure
```

`run-secure` uses the existing CI provisioning, dedicated-vCPU plan selection,
resource receipts and unconditional cleanup. Encrypted peers run on separate
physical cores of the client droplet. Deep service chains share those cores as
recorded in each trial. This isolates driver costs; it does not measure VPC
network latency. The other peer remains idle during this campaign. Select
`run-remote` for the existing two-droplet VPC workloads and loopback controls.

The encrypted campaign retains all 24 cases, TLS and QUIC, five repetitions with
100 warmups and 1,000 measured operations, then separate 10/100-operation syscall
traces. Post-timing Callgrind and DHAT profiles cover dependent/pipelined TLS
reads and pipelined QUIC reads for portable, legacy and supported C++ paths.
Each profile uses 50 and 500 operations with ten warmups, allowing setup costs
to be subtracted. Simulated instruction counts and cumulative allocations are
diagnostics, never native latency evidence. Artifacts include the droplet
receipt, executable hashes, CPU topology, host state, raw samples and profiles.

## Encrypted comparison (current default)

Every benchmark connection is encrypted. Capability RPC defaults to TLS 1.3 over
TCP, including every downstream microservice hop. `CAPNTPROTO_BENCH_TRANSPORT=quic`
selects the production standard QUIC v1 adapter for both legacy and portable.
QUIC uses one bidirectional stream per RPC connection, preserving the same
connection counts, ordering, payloads, concurrency, and application validation.
The independent upstream Rust fixture and C++/KJ reference use TLS. gRPC uses
HTTPS/HTTP2 with a TLS 1.3-only acceptor; WebSocket uses WSS. The existing native
UDP/TCP protocols retain their pinned mutual identity encryption and are a
separate security model, not certificate-authenticated TLS controls.

Run the complete 24-case encrypted matrix with five independent process repetitions:

```sh
python3 benchmarks/rpc/secure-runner.py --rust DRIVER --cpp CPP \
  --output target/secure-comparison --mixed --comparators --upstream UPSTREAM
```

To measure a transport change against frozen controls, pass `--rust LEGACY`,
`--portable CANDIDATE`, and `--portable-baseline PREVIOUS`. Each executable is
used on both ends and every service hop of its cell. The baseline label still
requires portable-engine metadata; binary hashes distinguish the two portable
builds. Without these overrides, legacy and portable use `--rust`.

`--transport tls|quic|both` defaults to both. `--comparators` adds gRPC/WSS and
optional independent upstream Rust to their supported loopback/workflow cells.
The runner generates a fresh private benchmark CA and seven-day localhost
certificate, verifies transport metadata on every peer, and retains raw samples,
certificate fingerprints, binary hashes, process placement, host counters,
means, sample SD, and run-mean ranges. Application setup and full handshakes finish
before warmup/timing; no application 0-RTT is enabled. These are established
connection measurements, not handshake latency measurements. Five repetitions
rotate execution order but do not completely balance every position.

**C++ QUIC is unsupported** by the pinned KJ reference. It is explicitly omitted
from the QUIC matrix. Rust QUIC comparisons are legacy versus portable on the
same production adapter; C++ comparisons use TLS versus TLS. TLS and QUIC results
remain separate, and do not reuse or pool the historical plaintext baselines.
Cryptographic implementations differ: Rust TLS uses rustls/ring, KJ uses OpenSSL,
and QUIC uses quiche/BoringSSL. All use TLS 1.3, the same CA/server identity, and
certificate plus hostname verification. These fixtures use server authentication;
they do not require client certificates. QUIC uses `capntproto-rpc/1` ALPN; HTTP2
uses `h2`; ordinary RPC TLS and WSS omit ALPN because KJ's public TLS API does not
expose it. This benchmark configuration does not relax the production RPC TLS
adapter's ALPN requirement.

For direct commands or the topology-only runner, first create and select a fixture:

```sh
python3 benchmarks/rpc/generate-certificates.py target/benchmark-certificates
export CAPNTPROTO_BENCH_CERTS="$PWD/target/benchmark-certificates"
export CAPNTPROTO_BENCH_TRANSPORT=tls
```

The standard quality bundle includes a fresh fixture beside its executables;
`CAPNTPROTO_BENCH_CERTS` overrides that directory. Regenerate expired bundle
fixtures before reusing an old build. Missing certificates, invalid names/roots, and unsupported transports fail rather
than falling back to plaintext. Fixture keys are local benchmark artifacts and
must not be deployed. Both hosts in a remote run must receive the same fixture;
`CAPNTPROTO_BENCH_SERVER_NAME` identifies the certificate name (default localhost),
independently of the dialed IP. Old frozen plaintext executables cannot serve as
controls for this encrypted campaign.

`secure-check.py --rust DRIVER --cpp CPP --upstream UPSTREAM --output NEW_DIR`
runs all supported application fixtures, mixed-engine callback/pipeline/service
chains, and wrong-hostname/untrusted-CA rejection checks. Use a separate
`secure-runner.py ... --trace --repetitions 1 --warmup 2 --iterations 10` run for
ciphertext syscall diagnostics. Counts include setup, TLS records, QUIC control
packets and UDP operations; they are not Cap'n Proto message counts. The old
plaintext frame decoder explicitly rejects encrypted captures.

## Deep and concurrent access

| Case | Nested nodes | Outstanding requests per batch | Client connections | Service processes |
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
depth is actual pointer traversal and copying, and every service hop is an encrypted RPC
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
encrypted microservice chains, not cross-host network measurements.

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
