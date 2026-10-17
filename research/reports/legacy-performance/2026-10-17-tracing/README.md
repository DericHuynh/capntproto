# Legacy RPC tracing and batching, October 17, 2026

Tracking: [legacy gap #15](https://github.com/DericHuynh/capntproto/issues/15). This extends the [earlier campaign](../2026-10-17/README.md).

**The 2× goal remains unmet.** The final local campaign has 0/16 observed pooled-p50 passes against frozen legacy and lower p50 than C++ in 15/16 cells. These are local observations, not remote qualification or proof of a stable speedup.

## Implementation and C++ correspondence

Legacy now gives ordinary Calls and Returns separate immediate output allowances between network turns, then batches the remaining burst through the existing FIFO. An input wait replenishes the allowance as well as an output-task poll: a fully inline write does not necessarily wake that task. Buffered incoming messages do not replenish them. Separate Call and Return allowances let a client reply to an incoming callback without waiting behind its initiating outgoing Call. Pipeline-parent and Finish deferral, explicit stream flushes, error propagation and physical-flush ownership remain intact. The first budget experiment omitted input replenishment; a regression test now covers both input-stream variants. A second test checks independent immediate Call/Return writes, followed by FIFO batching and retained flush guards.

The released-segment cache now accepts 64 segments instead of 16, within the same 16,384-word (128 KiB) payload budget. This avoids repeatedly allocating the excess small segments in a burst. Descriptor storage can grow by up to 768 bytes. Live-message storage is separate from this cache bound.

The general multi-message serializer follows C++ `writeMessagesImpl` in `vendor/capnproto/c++/src/capnp/serialize-async.c++`: size one shared segment-table array and one scatter/gather array, fill their slices, and retain them until the write completes. It replaces a table Vec and async cursor writes per message. Small-batch and already-framed paths remain specialized; empty segments, padding, partial writes, Pending and write failures retain the same wire encoding. No unsafe code or further public API break is added.

C++ `rpc-twoparty.c++` uses `kj::evalLast()` for whole-turn batching. The Rust allowance is a runtime-independent compromise, not an implementation of KJ scheduling. Tokio remains the host.

## Complete tracing and profiling

All 16 cases were captured for current legacy, the final implementation and C++, on both peers. The final diagnostic pass runs these three implementations together case by case, avoiding CPU-timing comparisons between distant host conditions. Across these and the retained experiments there are 268 full-payload syscall journals, the same number of socket wall/thread-CPU journals, and the same number of Callgrind profiles. Raw gzip traces retain every byte, including 4 MiB payloads. Reassembly joins unfinished/resumed syscalls and validates frame boundaries and hashes across both peers. Unread 88-byte Abort frames emitted after client shutdown are explicitly retained and identified; every received frame matches the sender. No payload truncation is silently accepted.

Socket timing uses 10 warmups and 100 iterations; full-payload traces use 2 and 10. Whole-process Callgrind uses 20/200 for ordinary cases, 5/50 for fanout, and 1/10 for large payloads. Startup and teardown are retained. These shorter diagnostic budgets are matched across implementations and do not change qualification budgets. Read-to-write intervals describe IO groups, not individual RPCs in fanout; workload-normalized CPU windows are also retained. Instrumented latency is never used for the performance gate.

| Profile / peer | Current instructions | Final instructions | C++ instructions | Change from current |
| --- | ---: | ---: | ---: | ---: |
| unary-empty / client | 5,691,550 | 5,691,472 | 8,065,889 | -0.00% |
| unary-empty / server | 5,268,527 | 5,268,179 | 8,348,462 | -0.01% |
| fanout-32 / client | 25,230,394 | 23,139,085 | 43,869,443 | -8.29% |
| fanout-32 / server | 20,980,740 | 20,219,697 | 43,209,283 | -3.63% |
| pipelined-read / client | 10,731,838 | 10,433,992 | 15,174,532 | -2.78% |
| pipelined-read / server | 13,312,966 | 13,306,559 | 17,182,274 | -0.05% |
| callback / client | 9,429,282 | 9,428,629 | 15,302,751 | -0.01% |
| callback / server | 11,054,353 | 11,054,101 | 14,452,543 | -0.00% |

| Traced case / peer | Current reads / writes | Final reads / writes | C++ reads / writes |
| --- | ---: | ---: | ---: |
| unary-empty / client | 14 / 15 | 14 / 15 | 13 / 13 |
| unary-empty / server | 15 / 14 | 15 / 14 | 13 / 14 |
| fanout-32 / client | 111 / 387 | 86 / 27 | 13 / 13 |
| fanout-32 / server | 94 / 386 | 75 / 38 | 13 / 14 |
| pipelined-read / client | 14 / 26 | 14 / 26 | 13 / 13 |
| pipelined-read / server | 21 / 14 | 24 / 14 | 13 / 14 |
| callback / client | 26 / 27 | 26 / 27 | 25 / 25 |
| callback / server | 27 / 26 | 27 / 26 | 25 / 26 |

These counts include setup and shutdown. Packet coalescing and full-payload strace overhead affect read groups. Socket CPU journals separately capture syscall entry/exit and read-to-write gaps; their instrumentation also adds work per syscall.

| Socket-window CPU per workflow iteration, µs | Current | Final | C++ |
| --- | ---: | ---: | ---: |
| fanout-32 / client | 175.424 | 80.409 | 78.004 |
| fanout-32 / server | 163.969 | 73.469 | 78.012 |
| callback / client | 29.976 | 29.302 | 29.121 |
| callback / server | 28.530 | 31.517 | 27.983 |
| bulk-1m / client | 266.269 | 322.575 | 286.508 |
| bulk-1m / server | 247.897 | 248.474 | 639.298 |

CPU windows include setup, warmup, teardown and probe overhead and are normalized by warmups plus iterations. They are diagnostics, not isolated application CPU or latency qualification. Fanout is normalized per 32-call workflow, not per packet or individual RPC.

All 16 instruction/allocation comparisons and socket timings are in `report.json`; the raw per-function profiles and complete message timelines are in the archive. The shared-table step removes 1,760 client malloc calls and 220 reallocations in the fanout profile relative to the preceding burst build; pipeline-client counts fall by 885 mallocs and 221 reallocations. These whole-process deltas do not imply equal end-to-end latency reductions.

## Uninstrumented final comparison

Baseline is frozen legacy at `6e37505d`; current is the published `cc784423` implementation. Five variants rotate and alternate order over five repetitions, including the preceding shared-table build. Each workflow retains 100 warmups and 1,000 samples with server/client CPUs 1/2. Separate loopback trials retain 10,000 warmups, 1,000 samples and discovered physical-core placement. The intermediate shared-table variant, p95/p99, throughput and every repetition median remain in `report.json`. No build, profiler or test process overlaps these latency campaigns.

| Case | Frozen legacy µs | Current µs | Final µs | C++ µs | Before/final | 2× |
| --- | ---: | ---: | ---: | ---: | ---: | :---: |
| unary-empty | 22.032 | 22.193 | 22.153 | 24.006 | 0.995× | miss |
| unary-small | 21.722 | 21.682 | 21.873 | 24.325 | 0.993× | miss |
| unary-1k | 30.926 | 30.936 | 29.864 | 32.218 | 1.036× | miss |
| fanout-32 | 169.870 | 151.523 | 150.832 | 136.982 | 1.126× | miss |
| dependent-read | 56.493 | 66.037 | 66.588 | 66.869 | 0.848× | miss |
| pipelined-read | 40.560 | 38.647 | 39.779 | 41.401 | 1.020× | miss |
| write-read | 61.470 | 61.501 | 61.651 | 65.546 | 0.997× | miss |
| callback | 62.902 | 64.526 | 62.873 | 66.167 | 1.000× | miss |
| bulk-64k | 55.822 | 53.068 | 53.659 | 108.189 | 1.040× | miss |
| bulk-256k | 174.407 | 121.469 | 102.250 | 285.632 | 1.706× | miss |
| bulk-1m | 697.459 | 388.233 | 432.498 | 1010.159 | 1.613× | miss |
| bulk-4m | 4475.196 | 2774.793 | 2878.935 | 4796.783 | 1.554× | miss |
| loopback-0 | 20.250 | 20.100 | 19.950 | 22.123 | 1.015× | miss |
| loopback-64 | 30.485 | 30.465 | 19.969 | 32.197 | 1.527× | miss |
| loopback-1024 | 30.015 | 20.771 | 31.266 | 33.379 | 0.960× | miss |
| loopback-65536 | 48.562 | 50.604 | 48.923 | 53.790 | 0.993× | miss |

## Repetition and rejected experiments

The cleanup-only yield did not reduce pipeline writes and was rejected. Full deferral reduced fanout writes but raised small-message instructions and exposed per-message batch allocations. The initial budget also exposed a sequential-call scheduling penalty; input-wait replenishment removes it. All six source patches, frozen binaries, diagnostic results and failed compilation logs remain retained. The first input-regression test had a closure argument type error; the corrected test passes natively and under Miri.

A twenty-repetition follow-up of the shared-table build retains eight variable cases. After separating Call and Return allowances, a fresh ten-repetition follow-up covers callbacks, pipelines, fanout, empty and 1-KiB calls, 1-KiB loopback and 1/4-MiB payloads with frozen legacy, current, final and C++. It preserves timing modes and does not replace a failed full-campaign cell. Earlier runs reversed every odd repetition, which couples variant and position with an even variant count. The final follow-up reverses after complete variant cycles: its first eight repetitions balance all four variants over positions, followed by two additional rotations. Both runner revisions and all earlier observations are retained. Paired ratios below compare corresponding repetition medians; the median of those ratios can differ from the ratio of pooled medians.

| Case | Current pooled µs | Final pooled µs | C++ pooled µs | Median paired current/final |
| --- | ---: | ---: | ---: | ---: |
| unary-empty | 19.970 | 20.000 | 21.602 | 0.998× |
| unary-1k | 23.394 | 20.440 | 24.616 | 1.003× |
| fanout-32 | 217.921 | 136.801 | 143.472 | 1.317× |
| pipelined-read | 28.502 | 28.101 | 41.060 | 1.000× |
| callback | 43.625 | 61.300 | 47.340 | 0.998× |
| loopback-1024 | 20.420 | 25.487 | 32.839 | 0.992× |
| bulk-1m | 408.452 | 414.932 | 1048.746 | 1.005× |
| bulk-4m | 2781.012 | 2721.094 | 4774.211 | 0.980× |

Timing uncertainty remains material: the full final campaign is slower than C++ in fanout, while the final follow-up reverses that ordering and is slower than C++ in callbacks. Callback instruction counts are effectively unchanged from current legacy, yet pooled wall-time comparisons disagree. These results do not establish stable latency improvement on every workload. The follow-up and every earlier campaign remain separate: **2,480,000 uninstrumented samples** and **792 mixed-peer workflow cells** are independently checked. All prescribed warmups are run; slow samples and failed cells remain retained. Fixtures, coverage floors and performance thresholds are unchanged.

## Remaining architecture costs

The transport still cannot reproduce KJ end-of-turn scheduling exactly. Fanout retains more write groups than C++, and pipeline cleanup still produces separate writes. Small-call user-space instructions are already below C++ but round trips remain dominated by costs not halved by these changes. Further work needs evidence on wake scheduling, batching across application/driver boundaries, and allocation/copy costs; instruction counts alone cannot establish that all sixteen end-to-end latencies can be halved.

## Validation and reproduction

**313 RPC unit tests, 67 framing/serialization tests, 99 integration tests and 22 focused Miri tests pass.** Strict Clippy and the workspace unsafe documentation gate pass. New tests cover burst FIFO, network wakes, retained flush guards, input-wait replenishment, mixed segment tables beyond the iovec limit, partial writes and failures. Integration includes pipelines, adopted answers, ownership, admission, membranes, TCP and native migration, with existing TLC/C++ replay. This is not a claim that full workspace coverage passes.

The [machine-readable report](report.json) contains source/executable identities, all campaign summaries, diagnostic budgets and independent verification. The [final source patch](source.patch) replays against `cc78442385dec0108c90e5b801c22926dd0064d6`. All six patches replay to their recorded trees; final source hashes and unchanged fixture hashes are checked against the measured binary.

Raw evidence is under `target/legacy-tracing-20261017/`. The archive includes the runner, analyzer, independent verifier, freeze helper, all samples/profiles/traces/logs and all frozen control/candidate binaries. Old archives remain unchanged.

Archive: **11,999 files / 1,157,141,082 bytes**. `SHA256SUMS` SHA-256: `37d6ce42364c4b43254636b134a47cc292e3f4930069f005d4fed6c2f1efee28`.

Reproduce in the existing `capntproto-perf` environment with the archived `legacy_trace2_diagnostics20261017.py` and `legacy_trace2_campaign20261017.py`, using fresh output paths. Preserve CPU placement and the frozen controls. Diagnostic output must remain separate from uninstrumented campaigns. The dedicated workflow compares implementations at one head; it does not contain the frozen before binary and cannot independently close gap #15.
