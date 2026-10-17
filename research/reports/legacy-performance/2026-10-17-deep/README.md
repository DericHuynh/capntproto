# Deep and concurrent RPC performance, October 17, 2026

Tracking: [gap #15](https://github.com/DericHuynh/capntproto/issues/15), [draft PR #12](https://github.com/DericHuynh/capntproto/pull/12). This continues the [earlier tracing work](../2026-10-17-tracing/README.md).

**The 30% latency target is not met.** The selected local campaigns observe 6/24 pooled-p50 passes. The separate frozen-legacy 2× target observes 1/16. Several small-message results switch timing modes across repetitions, including the unchanged controls; isolated passes do not establish stable qualification. All raw samples, p95/p99, throughput and repetition medians are retained.

## Added workload coverage

The [versioned topology suite](../../../../benchmarks/rpc/README.md) adds depth 8/32, width 32, four/eight independent service processes and one/eight client TCP connections. Every service validates every nested value and payload byte, copies the request downstream, awaits it, and copies the validated nested response upstream. The client checks checksums, visit counts and returned data. Every hop is a TCP RPC to another OS process; downstream connections are shared across concurrent incoming calls.

Rust and C++ perform the same application work. Connection setup, bootstrap resolution and topology discovery are outside measurement. Construction, traversal, copying, validation, response destruction and a ten-second deadline are inside each timed batch. The metric is closed-loop batch completion, not individual-request latency or open-loop saturation. Four-service cases use separate physical cores; eight services share the four available service cores. These are same-host measurements.

All eight new cells pass homogeneous and both alternating Rust/C++ chain checks. The selected runs retain **320,000 uninstrumented samples** (256,000 original-cell samples across four controls, plus 64,000 new-cell samples across Rust/C++) and **208 mixed-peer checks** (192 original workflows plus 16 chains). Earlier trials remain retained.

## Measured topology results

Four repetitions retain 100 warmups and 1,000 measurements per cell. Order rotates/reverses with equal positions for Rust and C++. No compilation, tests or profiling overlap latency campaigns.

| Case | Rust p50 µs | C++ p50 µs | Rust/C++ | Meets 0.70 |
| --- | ---: | ---: | ---: | --- |
| deep-8 | 22.324 | 23.015 | 0.970 | no |
| deep-32 | 27.091 | 27.571 | 0.983 | no |
| concurrent-32 | 85.200 | 129.465 | 0.658 | yes |
| deep-32-concurrent-32 | 237.912 | 275.985 | 0.862 | no |
| services-4-deep-16 | 143.626 | 134.123 | 1.071 | no |
| services-4-deep-16-concurrent-32 | 1150.765 | 1148.642 | 1.002 | no |
| services-4-deep-16-clients-8 | 1160.094 | 1078.091 | 1.076 | no |
| services-8-deep-16-clients-8 | 2479.615 | 2310.720 | 1.073 | no |

## Changes guided by tracing

- Defer ordinary output until ready RPC work has run, following KJ end-of-turn batching. Separate ready sets and activity notification preserve new work; a 32-turn bound prevents writer starvation. Tokio supplies the deferred-wake hook.

- Match C++’s 64-KiB input buffer and bootstrap size hint; use Linux’s 1,024-iovec limit with 16 stack entries for small batches. The input buffer costs 56 KiB more per connection than the preceding 8-KiB default. Other platforms retain 16 iovecs.

- Retire replaced capability routes outside mutable table borrows and settle bootstrap pipelines directly, allowing Finish to join pending output. Consume completed physical-close observers; poll shutdown observers only after notification.

- Expose dedicated Tokio TCP read/write halves. The framer supplies exact missing-body lengths; Linux uses a bounded receive low-water hint for large incomplete bodies, restores ordinary readiness on completion and restores the original option on drop. TCP receive autotuning remains enabled. The fixed-buffer experiment was rejected. TLS/QUIC keep their adapters. The public TCP half-type change is intentional for this unreleased API.

- Fuse relative-target, complete-extent and traversal-budget validation for near pointers during deep copy. Far landing pads, nesting limits, amplification checks, capability handling and canonicalization retain their existing contracts. New tests exercise exact budgets and escaped/missing targets.

The changes remove measured work; they do not constitute a complete KJ implementation. Linux socket-option wrappers and the fused copy checks contain documented unsafe operations. The source-bound unsafe inventory updates only the reviewed layout file hash; exception counts and coverage thresholds are unchanged.

| Four-service, eight-client process | Before instructions | Candidate | C++ | Reduction |
| --- | ---: | ---: | ---: | ---: |
| client.callgrind | 13,174,590 | 13,076,701 | 16,004,701 | 0.74% |
| service-0.callgrind | 32,255,444 | 29,968,870 | 30,822,841 | 7.09% |
| service-1.callgrind | 30,415,278 | 28,329,026 | 29,538,954 | 6.86% |
| service-2.callgrind | 30,415,272 | 28,379,648 | 29,505,271 | 6.69% |
| service-3.callgrind | 14,681,133 | 13,645,643 | 16,886,820 | 7.05% |

Whole-process Callgrind uses two warmups and ten new-topology batches and includes startup/shutdown. Both pre-change and candidate runs cover all 64 topology processes. The original 16 cells retain 64 additional candidate/C++ profiles. Allocation/free, recursive construction/copying and scheduling remain material costs. Instruction reductions are not interchangeable with wall-clock reductions.

## Socket and wire accounting

The new suite passes **32/32 process rows** for positive reads/writes and total attempts in both lightweight and full tracing. The original suite passes **30/32 positive rows and 31/32 attempt rows** under the lightweight probe, and **32/32 positive rows and 32/32 attempt rows** under full tracing. Count parity therefore remains open. Startup, warmup, EOF, EAGAIN and teardown are included.

The lightweight misses are bulk-256k server (224 positive reads versus 223) and pipelined-read server (112 positive writes versus 111, with one extra read attempt). Full tracing perturbs delivery and cannot erase those misses. Original probe budgets remain 10/100 and wire budgets 2/10; new-topology diagnostics use 2/10. These are separate captures, not universal syscall guarantees.

New wire analysis reassembles **152 directed TCP edges** and uniquely pairs full bidirectional payloads between adjacent service processes, even when strace exposes only socket inodes. Every received frame matches its sender byte-for-byte. Unread teardown is explicitly recorded on 76 directed edges; unconsumed calls/results are rejected. A corrupted segment-table negative control is rejected. Original traces reassemble all 32 peer pairs and record their unread 88-byte shutdown Aborts.

## Original latency cells

Four variants rotate over four repetitions, each occupying every position once. Original fixtures and validation are unchanged: 100 warmups/1,000 samples for workflows, 10,000/1,000 for loopback. The frozen controls are legacy `6e37505d` and preceding `c71b3b9`; C++ remains pinned to `0de72d8d`.

| Case | Frozen legacy µs | Preceding µs | Candidate µs | C++ µs | Rust/C++ |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary-empty | 29.835 | 30.205 | 31.097 | 31.597 | 0.984 |
| unary-small | 29.674 | 29.054 | 21.222 | 31.778 | 0.668 |
| unary-1k | 20.271 | 20.661 | 21.782 | 22.393 | 0.973 |
| fanout-32 | 135.763 | 109.655 | 96.826 | 136.185 | 0.711 |
| dependent-read | 46.820 | 44.968 | 67.712 | 47.922 | 1.413 |
| pipelined-read | 28.773 | 27.281 | 31.137 | 30.967 | 1.005 |
| write-read | 39.599 | 60.431 | 64.307 | 43.926 | 1.464 |
| callback | 62.804 | 61.582 | 66.449 | 66.149 | 1.005 |
| bulk-64k | 105.348 | 103.725 | 106.701 | 100.251 | 1.064 |
| bulk-256k | 312.960 | 274.351 | 112.368 | 274.356 | 0.410 |
| bulk-1m | 674.561 | 379.738 | 374.926 | 960.043 | 0.391 |
| bulk-4m | 3702.168 | 2770.527 | 2460.728 | 4322.363 | 0.569 |
| loopback-0 | 30.025 | 19.759 | 21.403 | 21.602 | 0.991 |
| loopback-64 | 29.484 | 29.545 | 31.918 | 31.417 | 1.016 |
| loopback-1024 | 20.761 | 20.331 | 21.863 | 32.319 | 0.676 |
| loopback-65536 | 104.477 | 104.907 | 106.961 | 79.239 | 1.350 |

For example, candidate 64-KiB loopback repetition medians are 108.9, 109.6, 50.4 and 50.1 µs; C++ gives 111.6, 52.1, 111.0 and 53.0 µs. The pooled result and all repetitions remain visible. Earlier minimum/observer campaigns are retained rather than substituted into unsuccessful cells.

## Correctness and provenance

- `core`: passed, 137 tests.
- `rpc-framing`: passed, 388 tests.
- `tokio-adapters`: passed, 93 tests.
- `result_pipeline`: passed, 11 tests.
- `answer_adoption`: passed, 34 tests.
- `tail_transfer`: passed, 5 tests.
- `rpc_ownership`: passed, 5 tests.
- `rpc_admission`: passed, 5 tests.
- `rpc`: passed, 3 tests.
- `membrane`: passed, 7 tests.
- `membrane_join`: passed, 8 tests.
- `membrane_copy`: passed, 8 tests.
- `tcp_rpc`: passed, 1 tests.
- `native_pipeline_migration`: passed, 5 tests.
- `native_rpc`: passed, 7 tests.
- `scoped_vat`: passed, 6 tests.
- `flow_control`: passed, 15 tests.
- `twoparty_facade`: passed, 9 tests.
- `secure_rpc`: passed, 7 tests.
- `upstream_interop`: passed, 2 tests.
- `unsafe`: passed.
- `miri-copy`: passed, 30 tests.
- `miri-observer`: passed, 2 tests.
- `topology-contract`: passed, 1 tests.

The shared upstream-interoperability fixture now adapts our Context API and published upstream Params/Results signatures while keeping one behavioral implementation. This fixes the compile failure visible in the preceding hosted Cargo tests/coverage job. Full hosted coverage for this change is not claimed.

Selected executable SHA-256: `19f03c5739c189f38a9bd7add0137eaecb87360c46481eac9f246068764f547b`. Compiled source snapshot: `c85c7bc77caebcc1e1dca17df633a7e00ab26b44`; selected review snapshot: `3dfabef35685e54d7c4fe1e3ff1c52ad052b8b41`. Only the Python trace analyzer and its README changed after the binary was built. The selected [patch](source.patch) replays against `c71b3b9f9ae0e3ac84b7c559a271ad102b944fb9`. The report records all 15 candidate snapshots and exact C++ source/library hashes.

Raw archive: `target/legacy-counts-20261017/`, **9,277 files / 2,562,270,133 bytes**. `SHA256SUMS` SHA-256: `62f988c232b0a17e71da165dfd74ee4433c292958aca052df1a257f06c290385`. Both preceding legacy archives verify unchanged. Independent verification replays all source patches, checks binary/fixture identities, and recomputes percentiles and throughput from raw samples.

Run the checked-in topology runner and analyzer with fresh output directories. Original-cell campaign, tracing, comparison and verification scripts are retained in the raw archive. Failed experiments, interrupted builds, and the initial inode-parser rejection remain there too. The earlier `legacy_counts_report20261017.py` is an unexecuted obsolete draft; this report is generated by `legacy30_report20261017.py`. [Machine-readable results](report.json) retain the full comparisons and evidence locations.
