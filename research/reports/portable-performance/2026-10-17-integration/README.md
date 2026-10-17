# Mandatory portable integration and Compio: October 17, 2026

All three architecture contracts are now mandatory production APIs: restricted
control contexts, revision-backed import metadata and borrowed generated service
contexts with explicit ownership transfer. Applications, adapters, examples,
tests and fuzz targets are migrated. The [earlier prototypes](../2026-10-17-architecture/README.md)
remain historical evidence; their decision to retain the old APIs is superseded.

Compio also runs the production portable driver successfully, without another
engine API break. Its two benchmark adapters pass all 300 mixed-peer cases, but
the measured Poll-backend implementations do not improve overall performance.
Their scheduling changes split pipeline writes that Tokio and C++ combine.
The required 5% parity remains open.

## Implemented contracts

* `Control::poll`, `received` and `shutdown` take `ControlContext`. Semantic
  operations record effects, and drop commits invalidation across early returns.
  Separate control notifications preserve unrelated application idle scans.
  Active built-in network effects still invalidate broadly.
* `Output::metadata_source` is required. A stable revision source preserves late
  descriptors and metadata changes; `None` declares an import never has metadata.
  Endpoint refresh still walks live entries and each subscribed import adds an
  allocation. This is not yet an event queue.
* Generated methods take one `FooContext<'_>` and return `Promise`. Synchronous
  methods use `complete`; suspending methods use `into_owned`. Structured replies
  use an affine context. Protected non-streaming wire dispatch can borrow under
  the existing immediate-poll permission with a supplied executor. Ordinary local
  and opaque calls defer admission; streaming calls defer until their barrier is
  installed. Cancellation, revocation, membranes and pipeline ownership remain
  on the normal dispatch path.

The [migration guide](../../../../docs/wiki/RPC-Applications.md#generated-service-dispatch)
shows the breaking signatures. The optional prototype dispatch entry and metadata
snapshot compatibility contracts are removed. The normal generator supplies the
borrowed path; no benchmark-specific method dispatcher or feature is required.

## Mandatory integration versus the frozen baseline

Each of twelve unchanged workflows has five rotating repetitions of 100 warmups
and 1,000 measurements against baseline portable, new portable, new legacy,
baseline legacy and pinned C++. These are **300,000 local TCP samples**, not the
dedicated twelve-remote-plus-four-loopback qualification. Processes are pinned to
server CPU 1 and client CPU 2. No compilation or heavy validation overlaps timing.

| Workflow | Portable µs | Legacy µs | C++ µs | Portable / faster reference |
| --- | ---: | ---: | ---: | ---: |
| unary-empty | 31.933 | 31.046 | 21.732 | 1.469× |
| unary-small | 31.156 | 20.260 | 23.075 | 1.538× |
| unary-1k | 31.596 | 20.826 | 33.530 | 1.517× |
| fanout-32 | 158.309 | 217.816 | 146.972 | 1.077× |
| dependent-read | 56.578 | 49.133 | 68.951 | 1.152× |
| pipelined-read | 43.044 | 30.255 | 31.026 | 1.423× |
| write-read | 62.942 | 41.491 | 65.958 | 1.517× |
| callback | 52.633 | 45.938 | 47.901 | 1.146× |
| bulk-64k | 52.562 | 52.037 | 65.356 | 1.010× |
| bulk-256k | 148.780 | 201.447 | 260.519 | 0.739× |
| bulk-1m | 471.705 | 715.890 | 1077.749 | 0.659× |
| bulk-4m | 3219.683 | 4664.483 | 4741.807 | 0.690× |

Four of twelve cells meet the faster-reference comparison; seven meet C++ alone.
Every p95 and repetition median is retained in [report.json](report.json).
Repetition modes vary substantially: pipeline portable medians are 42.112,
54.205, 42.182, 42.082 and 43.449 µs, while baseline includes an 86.412 µs run.
Empty-unary medians move between roughly 20 and 32 µs. Pooled changes and pass
counts do not establish causal speedups or regressions across these modes.

Whole-process Callgrind includes setup, 100 warmups and 1,000 measurements:

| Workflow / peer | Baseline instructions | Mandatory APIs | Change | Baseline / new malloc calls |
| --- | ---: | ---: | ---: | ---: |
| pipelined-read / client | 75,733,502 | 75,174,913 | -0.738% | 48,739 / 49,842 |
| pipelined-read / server | 65,177,348 | 66,250,477 | +1.646% | 34,314 / 29,913 |
| unary-empty / client | 19,058,032 | 19,048,414 | -0.050% | 4,728 / 4,731 |
| unary-empty / server | 17,313,244 | 17,189,308 | -0.716% | 4,611 / 2,410 |
| callback / client | 58,481,220 | 58,782,301 | +0.515% | 13,536 / 11,339 |
| callback / server | 58,440,586 | 58,537,470 | +0.166% | 25,526 / 26,625 |

The synchronous path removes two allocations per eligible call; pipeline server
malloc calls fall by 4,401 after including one added startup allocation. Import
metadata sources add 1,103 client allocations. The combined instruction cost
remains mixed, so reduced allocation count alone does not establish a speedup.
Current legacy pipeline totals are 54,198,105/56,366,554 instructions and C++
67,540,810/76,871,703 (client/server). These are work counts, not elapsed cycles.

All six baseline/candidate peer comparisons preserve complete socket-write bytes
and groups. Steady pipeline batching matches C++: 1,368-byte
Finish/Release/Call/Call groups and 1,216-byte paired Returns. Startup and cleanup
differences against C++ remain in the journals.

A separate 100,000-measurement CPU/wall campaign records read-to-next-write
intervals. Pipeline CPU p50 is 15,308/10,115 ns for portable, 3,676/4,677 for
legacy and 5,789/6,790 for C++ (client/server), with large repetition variation.
These instrumented intervals exclude kernel waits and add clock-probe overhead;
they are not acceptance latencies or the same scope as whole-process Callgrind.

The earlier `integrated` trial always deferred protected methods. It preserved
admission policy but prevented borrowing for the common non-cancellable methods.
Its complete 300,000 timing samples, 100,000 instrumented measurements, 24
profiles and 24 traces are retained separately. The final `admitted` candidate
reuses the established immediate-poll permission and requires an executor.
The regression test verifies when ownership is transferred; the earlier trial
is not substituted for the final result.

## Compio versus a matched Tokio host

The [new benchmark host](../../../../benchmarks/rpc-compio/README.md) pins Compio
0.19.2 and contains Tokio plus both Compio controls in one executable. It uses
the same generated services, operation code, frame/admission limits, payloads,
validation and ten-second deadlines. The only shared-fixture edit injects the
host timeout function. The independent upstream benchmark still compiles.
The minimal host's lockfile and executable differ from the previous campaign;
compare Compio against its in-executable Tokio control, not across campaigns.

* **Readiness:** Compio local executor and `PollFd`, preserving borrowed buffers
  and vectored writes. Sockets are explicitly nonblocking.
* **Bridge:** Compio local executor and buffered completion-to-futures adapters,
  which add staging copies and boxed operation futures.

Both use **Compio's Poll backend** here. Explicit io_uring construction fails
with EPERM; there is no io_uring performance claim. KJ's pinned Linux event loop
uses epoll, so a completion runtime is not automatically a closer socket model.
This host does not migrate the root package's TLS, QUIC, Unix descriptor passing
or application services.

Another 300,000 uninstrumented samples use five rotating repetitions with the
same budgets and CPU pins. Current legacy and C++ are measured alongside them.

| Workflow | Tokio µs | Compio readiness µs | Compio bridge µs | Legacy µs | C++ µs |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary-empty | 20.851 | 22.884 | 24.681 | 19.789 | 22.062 |
| unary-small | 20.701 | 22.584 | 23.825 | 19.689 | 22.548 |
| unary-1k | 21.191 | 24.221 | 24.035 | 20.250 | 22.563 |
| fanout-32 | 122.265 | 136.722 | 147.412 | 150.882 | 172.754 |
| dependent-read | 60.690 | 61.495 | 67.344 | 46.218 | 52.051 |
| pipelined-read | 53.855 | 69.302 | 62.733 | 32.348 | 42.188 |
| write-read | 48.917 | 69.723 | 52.959 | 58.437 | 66.784 |
| callback | 51.261 | 77.118 | 65.838 | 63.869 | 67.795 |
| bulk-64k | 48.973 | 53.088 | 84.225 | 51.451 | 53.398 |
| bulk-256k | 122.286 | 107.609 | 141.529 | 183.150 | 265.347 |
| bulk-1m | 491.099 | 464.685 | 524.885 | 766.080 | 990.816 |
| bulk-4m | 2996.314 | 3084.990 | 3546.811 | 4317.123 | 4817.735 |

The faster-reference comparison passes 8/12 for Tokio, 5/12 for Compio readiness,
and 6/12 for the bridge; C++-only counts are 10/12, 8/12 and 6/12 respectively.
Timing remains variable; instruction and wire evidence explain why selecting a
new runtime alone is insufficient.

| Workflow / peer | Tokio instructions | Compio readiness change | Compio bridge change |
| --- | ---: | ---: | ---: |
| pipelined-read / client | 73,112,475 | +13.173% | +23.143% |
| pipelined-read / server | 64,283,496 | +13.400% | +25.841% |
| unary-empty / client | 17,013,117 | +27.092% | +50.835% |
| unary-empty / server | 15,223,252 | +13.746% | +40.450% |
| callback / client | 56,640,777 | +12.000% | +27.583% |
| callback / server | 56,563,697 | +11.907% | +31.592% |

Pipeline malloc calls rise from 49,840/29,912 for Tokio to 56,463/36,545 for
readiness and 81,797/67,403 for the bridge (client/server). The profiles include
all setup and warmup work; they do not establish a latency percentage.

**Both Compio adapters change steady client batching:** each 1,368-byte Tokio
Finish/Release/Call/Call write becomes a 40-byte Finish followed by 1,328 bytes
of Call/Call/Release. The twelve-operation trace has 14 Tokio client writes and
26 Compio client writes including setup/cleanup. All servers still emit thirteen
paired-Return groups, twelve of them 1,216 bytes. Complete pipeline bytes differ
as ordering and protocol IDs change; empty unary and callback preserve full
write groups and bytes on both peers. All 18 syscall journals are decoded,
including sendmsg/recvmsg and unfinished/resumed syscall records.

The readiness read-to-first-write CPU p50 is 7,531/9,434 ns, compared with
11,717/8,573 for Tokio and 5,919/6,890 for C++. The smaller readiness client
interval ends at the separate Finish and excludes the following call work.
It cannot be treated as a faster full request turn. The bridge uses rustix
direct syscalls, which bypass the libc CPU probe: its empty journal is rejected
and retained. There are 80,000 complete instrumented measurements for observable
Tokio/readiness/legacy/C++ paths, plus the retained three-trial failed probe
attempt. No bridge CPU value is inferred. Whole-process profiles and kernel
syscall traces still cover the bridge.

## Architecture reassessment

The APIs now expose clearer ownership, but four costs remain:

1. Invocation ownership is distributed across boxed parameter adapters, calls,
   graph routes, wire questions, workers and pipeline interests. One admitted
   invocation owner should join parameter storage and result production while
   preserving distinct response/pipeline interest, protected execution and tail
   adoption. Borrowing removes only part of this work.
2. Metadata revisions avoid unchanged projection but still require registry walks,
   weak upgrades and per-import allocation. A generation-checked dirty-route
   journal needs bounded overflow/rescan, wakeup, late-descriptor and dynamic
   projection semantics before it can replace those scans.
3. Readiness remains per lane; broad application wakes and active multiparty
   effects still rescan unrelated work. Individual readiness tokens need fair
   rotation and an allocation-free overflow path.
4. Publication batching depends on executor wake order. Introduce an explicit,
   bounded connection turn that can combine retirement and ready commands before
   flushing, while guaranteeing control-only progress and respecting backpressure.
   A blanket extra yield does not establish that contract.

The fourth issue is directly exposed by the Compio traces and should precede a
default runtime switch. A native Compio completion transport can then use the
existing incoming-frame and owned outgoing-frame boundaries to avoid the generic
bridge. It must retain buffers through partial I/O, cancellation and drain.
No further wire/schema or no-alloc engine break is indicated by these results.
Large live-registry and sparse-readiness scaling diagnostics are still needed;
the fixed twelve-workflow fixtures do not establish those scaling properties.

## Validation and evidence

The mandatory implementation passes **307 RPC unit tests, 283 root-library tests
(four ignored), 522 integration tests across 72 targets, five focused Miri tests,
108 mixed portable/legacy/C++ cases, strict workspace/benchmark Clippy, and ARM
compile checks with and without alloc**. The integration selection is 71 full
targets plus `structured_rpc_reply_compile_contracts` from `api_contracts`.
The final suites run serially; configured root model tests use `TLA2TOOLS_JAR`.
The Compio host adds four passing adapter tests, strict Clippy and **300 mixed-peer
cases** across all five implementations. [Validation scope](validation.json)
also records resolved setup/test attempts and instrumentation limits.

Coverage is not requalified here; floors and tolerances are unchanged. The latest
dedicated performance result remains **11/16 at 86661025**,
[run 38978275111](https://github.com/DericHuynh/capntproto/actions/runs/38978275111).
[Gap #11](https://github.com/DericHuynh/capntproto/issues/11) stays open. Fresh CI
must assess this published source; no merge or successful coverage claim is made.

[source.json](source.json), [compio-source.json](compio-source.json) and
[replay.json](replay.json) bind both measured trees to base `1e667c62`, exact
source hashes, build commands, patches and binaries. The frozen trees replay
through isolated Git indexes. Result documentation is added after measurement;
all compiled source in the final Compio freeze remains identical at publication.
The earlier architecture binary predates only the host-timeout fixture injection,
whose exact transformation and unchanged schemas are verified in the report.

Build from the repository root using the commands in the source manifests. The
normal architecture benchmark needs no experimental feature. Compio selection
and reproduction commands are in its README. All runs retain full release LTO,
one codegen unit and overflow checks; no PGO or target-CPU tuning is used. The
environment is Rust 1.97.0, LLVM 22.1.6, Valgrind 3.24.0 and five AMD EPYC 9V74
virtual CPUs. Hardware performance counters are unavailable.

Raw source snapshots, binaries, scripts, samples, logs, profiles and traces are
retained in `target/portable-architecture-integration-20261017/`: **6689 files** plus `SHA256SUMS`.
The verified manifest SHA-256 is `ea869bdcc6458c76681028106613d6be73da0f4bd7e137ee94bc8c56c340dfd1`.
Earlier immutable experiment archives and reports are unchanged.
