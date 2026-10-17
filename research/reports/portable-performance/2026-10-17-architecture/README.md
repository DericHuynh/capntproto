# Portable architecture experiments: October 17, 2026

All three proposed changes were implemented and profiled separately, then
combined. Borrowed synchronous service dispatch gives the clearest instruction
and allocation reduction. Metadata subscriptions and restricted control polling
have mixed costs, and their combination offsets part of that reduction. These
experiments do not establish the required 5% latency parity.

The four replayable patches are preserved here. They are experimental source,
not changes enabled in the production runtime. The runtime baseline remains
`fe843e229890d8f1a58557c8a47d02f4c1358cce`. Each patch applies independently to
that commit; `combined.patch` already includes all three changes.

## What was tested

| Prototype | Implemented boundary | Limits |
| --- | --- | --- |
| [Control context](control.patch) | `Control::poll_scoped` receives an operation context. Bootstrap publication records scheduling effects; quiet two-party network work uses a separate control waker. | Active handoffs and custom controls retain conservative compatibility transactions. Receive/shutdown callbacks remain on their existing interfaces. This is a restricted polling prototype, not a complete replacement of `Control`. |
| [Metadata revisions](metadata.patch) | Settled imports subscribe to an owned revision source. Unchanged sources avoid graph inspection and metadata projection; later descriptors and failures remain observable. | The registry still walks entries and upgrades weak owners. Each subscribed import allocates shared state. Pending promises and local/custom metadata retain their existing refresh paths. Switching projection policy invalidates the subscription. |
| [Borrowed dispatch](native.patch) | Synchronous services borrow the original parameter and results hooks. Dispatch avoids a boxed results hook and generated async method future. | Parameters still have an owned adapter. The fast path requires the existing immediate dispatch boundary; suspended, streaming, shortened, revocable and opaque paths keep owned dispatch. This is not a complete borrowed frame/service ABI or a replacement for the endpoint registry. |
| [Combined](combined.patch) | All three patches together. | Tests interactions rather than assuming individual gains add together. |

The native fixture adds hand-written synchronous implementations of echo, open,
cell read/write and callback notification behind the `borrowed-native` benchmark
feature. Callback invocation still awaits the original generated service path.
Client validation, payloads, method operations, response lifetimes, schemas and
iteration budgets remain the same. The fixture source changes are included in
the patch and source manifest. Legacy and C++ timing references are frozen
executables, so their implementations are unaffected by these changes.

## Instruction and allocation results

Whole-process Callgrind instruction changes relative to the same frozen baseline;
100 warmup operations and 1,000 measured operations. Negative is less work.
These counts include setup, warmup and cleanup and are not latency percentages.

| Prototype | Pipeline client | Pipeline server | Empty unary server | Callback client | Callback server |
| --- | ---: | ---: | ---: | ---: | ---: |
| Borrowed dispatch | -0.002% | -1.581% | -2.910% | -0.842% | +0.170% |
| Metadata revisions | -0.517% | +0.359% | +0.016% | -1.110% | +0.535% |
| Control context | -0.492% | +1.377% | +0.009% | +1.670% | -0.693% |
| Combined | -1.004% | +0.153% | -2.890% | -0.288% | +0.009% |

The fresh baseline pipeline totals are 75,732,867 client and 65,177,073 server
instructions. Borrowed dispatch removes two allocations per eligible service
call: pipeline server `malloc` calls fall from 34,314 to 29,912, and empty unary
server calls from 4,611 to 2,409. Totals include startup operations. Metadata
subscriptions add 1,102 pipeline client allocations, from 48,739 to 49,841.
The control context adds one startup allocation per peer.

## Timing and wire evidence

Each isolated experiment and the combination used five rotating repetitions
against baseline portable, candidate portable, frozen legacy and frozen C++.
Server/client processes were pinned to CPUs 1/2. Each run used 100 warmups and
1,000 measurements. No build or other heavy validation ran during timing.

| Prototype | Pipeline baseline p50 | Candidate p50 | Change | Candidate / faster reference |
| --- | ---: | ---: | ---: | ---: |
| Borrowed dispatch | 42.172 µs | 40.830 µs | -3.182% | 1.450× |
| Metadata revisions | 42.673 µs | 47.751 µs | +11.899% | 1.681× |
| Control context | 41.897 µs | 41.371 µs | -1.255% | 1.440× |
| Combined | 43.384 µs | 42.468 µs | -2.113% | 1.501× |

These are separate paired local TCP diagnostics. Timing modes vary substantially
between repetitions, including approximately doubled medians in some baseline,
candidate and reference runs. Every sample and repetition is retained. The
instruction results support modest, path-specific savings; these latency
observations do not establish a general speedup or a dedicated qualification.

Separate instrumented runs retain both thread CPU and wall time from the most
recent successful read to the next successful write on the same thread/socket.
For borrowed dispatch, pipeline server CPU p50 is 8,122 ns versus 8,913 ns
baseline; the combined result is 7,962 ns versus its paired 8,302 ns baseline.
The probe adds two CPU-clock calls per intercepted operation. Instrumented
latencies are excluded from the uninstrumented comparison.

Across all four prototypes, **24 of 24 baseline/candidate peer comparisons have
identical complete socket-write bytes and write groups**, including startup and
cleanup. There are 72 decoded syscall journals covering pipeline, callback and
empty unary, with baseline, candidate and C++ on both peers. The gains therefore
come from local work, while the traced batching and wire payloads stay the same.

The combined executable also ran all twelve workflow fixtures over local TCP,
with the same five repetitions and budgets. Nine meet the local faster-reference
comparison; dependent reads, pipelined reads and callbacks still miss. This is
a separate diagnostic run, so its numbers are not pooled with the four-workflow
experiment above or substituted for the dedicated remote/loopback campaign.

| Workflow | Combined p50 | Combined / faster reference |
| --- | ---: | ---: |
| Empty unary | 20.279 µs | 1.019× |
| 64-byte unary | 20.210 µs | 1.020× |
| 1-KiB unary | 20.480 µs | 1.024× |
| Fanout 32 | 122.075 µs | 0.892× |
| Dependent read | 54.760 µs | **1.213×** |
| Pipelined read | 41.591 µs | **1.464×** |
| Write/read | 40.590 µs | 1.013× |
| Callback | 54.090 µs | **1.324×** |
| 64-KiB bulk | 49.609 µs | 0.987× |
| 256-KiB bulk | 119.096 µs | 0.655× |
| 1-MiB bulk | 412.699 µs | 0.648× |
| 4-MiB bulk | 2,957.660 µs | 0.684× |

All **560,000 uninstrumented samples** are retained: 320,000 from the four
paired prototype campaigns and 240,000 from the full workflow run. An additional
320,000 instrumented measurements and their CPU/wall journals are retained
separately. [The machine-readable report](report.json) includes every repetition
median, p95, instruction count, allocation count, executable hash and comparison.

Whole-process pipeline instructions for the reference executables are
54,770,959/55,913,195 for legacy and 67,540,555/76,871,432 for C++ (client/server).
The combination still executes 74,972,561/65,276,691. Instructions count work,
not elapsed cycles: they are diagnostic rather than the faster-reference latency
acceptance criterion.

## Validation and architectural decision

Each isolated prototype passed RPC all-target Clippy, its complete RPC unit suite,
92 integration tests, and 72 mixed portable/legacy/C++ workflow cases. Unit counts
are 303 borrowed, 303 metadata and 304 control. The combined source passed **306
RPC unit tests, 92 integration tests, 72 mixed-peer cases, four focused Miri tests,
and the alloc-enabled `thumbv7em-none-eabi` compile check**. Individual prototypes
also passed their focused Miri tests (one borrowed, one metadata, two control).

New regressions cover retained native result capabilities and owned fallback,
late metadata and projection changes, independent control wakeups and cooperative
yield. The control experiment required an explicit yield when a handler wakes
itself without making application work ready; otherwise that path could spin
without consuming an application quantum.

No mandatory integration break is justified by these measurements. The measured
parts can be introduced through optional contracts, but the current context and
per-import subscription designs do not supply a broad performance gain. Preserve
them as experiments. Continue native ownership work only with a larger reduction
in parameter adaptation and execution ownership, including concurrent dispatch,
cancellation, early pipelines and policy wrappers. This prototype still pays
those remaining ownership costs.

The baseline pipeline profile spreads cost across allocation, checked layout
access, coordinator work, maintenance and memory copying. Removing a single
metadata projection or results allocation is too small to explain the remaining
gap. A connection-owned change journal is a future alternative to per-import
subscription allocations; it has not been measured here. No result calls for a
wire/schema change or weakening generational capability authority.

This is local experimental validation, not a full workspace coverage run or the
dedicated twelve-remote-plus-four-loopback performance gate. Coverage floors and
tolerances are unchanged. [Gap #11](https://github.com/DericHuynh/capntproto/issues/11)
remains open; its latest dedicated result is still 11/16 at `86661025`.

## Reproduction and retained evidence

Build from the frozen base in a separate checkout, apply exactly one patch, and
use the existing workflow commands. For example, from the checkout root:

```sh
git apply /path/to/2026-10-17-architecture/native.patch
cargo test --locked -p capntproto-rpc --lib
CARGO_INCREMENTAL=0 cargo build --locked --release \
  --manifest-path benchmarks/rpc/Cargo.toml --features borrowed-native
CAPNTPROTO_BENCH_PORTABLE=1 taskset -c 1 \
  benchmarks/rpc/target/release/capntproto-rpc-bench \
  workflow-server 127.0.0.1:49110
```

In a second terminal, run the client with the same driver selection:

```sh
CAPNTPROTO_BENCH_PORTABLE=1 taskset -c 2 \
  benchmarks/rpc/target/release/capntproto-rpc-bench \
  workflow-measure 127.0.0.1:49110 pipelined-read 100 1000
```

Use `borrowed-native` for native and combined; omit it for metadata/control.
Repeat with separately frozen reference binaries, rotating execution order.
Callgrind profiles wrap both processes with `--tool=callgrind
--collect-atstart=yes`; the checked-in wire-timing probe supplies the separate
instrumented diagnostics. This campaign used Rust 1.97.0, LLVM 22.1.6,
Valgrind 3.24.0 and AMD EPYC 9V74 virtual CPUs. It did not use PGO or target-CPU
tuning. Benchmark release settings and overflow checks were retained.

The `*-source.json` files bind source hashes, patch hashes, feature arguments and
executable hashes to the base commit. [Replay verification](replay.json) rebuilds
each tree in an isolated Git index and checks every changed file against its
frozen source. Full raw evidence is retained in the workspace under
`target/portable-architecture-three-20261017/`, with a SHA-256 manifest.
