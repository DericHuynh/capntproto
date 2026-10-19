# Portable maintenance budget simplification — October 19, 2026

Tracking: [gap #11](https://github.com/DericHuynh/capntproto/issues/11), [draft PR #12](https://github.com/DericHuynh/capntproto/pull/12). Legacy remains a measured reference.

**The universal C++ target remains unmet.** Portable beats C++ by both mean and pooled median in 13/24 cases in this campaign; frozen 1a5dc9f achieves 8/24 against the same measurements. The earlier campaign also had 13/24 passes for its then-current portable. These changing counts are descriptive same-host results, not evidence of a universal or statistically bounded speedup.

## Implementation and decision

Maintenance previously charged a scheduling step for each empty phase. With a one-step allowance, a ready cleanup owner could require several polls separated by empty phases. The driver now walks those empty phases in a bounded cycle before polling the next owner. Every actual owner poll still costs one step; a complete idle proof costs one step. The rotating cursor, physical-slot accounting, zero-budget behavior, capacity checks, and eight-owner retirement burst remain intact. Large tables without a bounded idle certificate retain individual slot visits. There is no cached idle mask, allocation, new queue, or unsafe code.

Each live proof is evaluated only when its phase is reached. The walk stops before callbacks, and later proofs see the state after those callbacks. The public function signatures do not change; budget semantics intentionally permit more progress through empty phases. External runtime mutations still require invalidating a partial maintenance scan.

The change is retained for a reproduced scheduling delay and reduced maintenance instruction work. End-to-end latency improvement is not established: the focused five-run pipeline mean/median improved 12.7%/5.1%, but the independent full campaign regressed 3.8%/3.0% on that case. Unchanged unary and bulk paths also show material variation. All runs, including regressions, remain in the evidence.

## C++ comparison and remaining architecture

C++/KJ is pinned to `0de72d8d8cec6b69edaa29de51d3bd490341f9c2`. [ImportClient destruction](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c++/src/capnp/rpc.c++#L1193) sends Release from the owning transition; [QuestionRef destruction](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c++/src/capnp/rpc.c++#L2397) sends Finish before permitting question-ID reuse. [TaskSet::Task](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c++/src/kj/async.c++#L265) attaches a ready event to its owned promise.

Portable still discovers obligations through eighteen maintenance phases and coarse coordinator lanes. This patch reduces the cost of reaching an operation; it does not replace those scans with owner-recorded obligations. Mutable public runtime/ledger access must participate in invalidation before persistent readiness can safely replace scanning. Outgoing completion also still spans WireCall, question/exchange storage and pipeline observers: parent results must remain owned until their observers settle. Those are the next architecture bottlenecks.

## Five-run native comparison

All 24 fixtures run five times per implementation, with 1,000 measured samples per run: 480,000 samples total. Warmup is 100, or 10,000 for loopback. Variant order rotates and reverses; five repetitions do not exactly balance four positions. Native timing ran without builds, tests or profilers. Before is frozen 1a5dc9f; Legacy and Portable explicitly select different drivers in the candidate executable. Rust retains fat LTO, one codegen unit, debug information and checked overflow. Fixtures are unchanged.

Times are microseconds: pooled arithmetic mean ± sample SD across the five run means (n−1), not confidence intervals. report.json retains run means/ranges, sample SD, tails, extrema, throughput and every comparison. Concurrent cases measure a complete batch; service cases include all configured hops. A pass requires mean and pooled median both below C++.

| Case | Legacy mean ± SD | Before mean ± SD | Portable mean ± SD | C++ mean ± SD | Portable p50 / C++ | Both pass |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| unary-empty | 31.165 ± 6.139 | 29.397 ± 5.269 | 35.643 ± 8.170 | 28.375 ± 4.957 | 1.395 | no |
| unary-small | 29.029 ± 4.318 | 29.941 ± 7.064 | 28.238 ± 3.549 | 28.157 ± 3.593 | 0.979 | no |
| unary-1k | 31.028 ± 4.584 | 37.833 ± 19.798 | 28.445 ± 5.091 | 29.187 ± 5.124 | 0.939 | yes |
| fanout-32 | 112.555 ± 14.735 | 161.145 ± 31.789 | 144.927 ± 21.434 | 163.064 ± 16.112 | 0.936 | yes |
| dependent-read | 56.341 ± 5.182 | 72.449 ± 11.871 | 77.424 ± 18.405 | 57.106 ± 3.267 | 1.221 | no |
| pipelined-read | 37.266 ± 7.727 | 59.718 ± 21.130 | 61.991 ± 19.858 | 36.655 ± 4.779 | 1.482 | no |
| write-read | 55.265 ± 9.313 | 51.928 ± 2.563 | 56.049 ± 10.957 | 55.944 ± 7.848 | 0.978 | no |
| callback | 59.643 ± 9.537 | 75.209 ± 11.228 | 78.084 ± 15.500 | 60.477 ± 8.428 | 1.557 | no |
| bulk-64k | 85.445 ± 33.419 | 78.230 ± 28.494 | 70.795 ± 28.405 | 75.882 ± 25.026 | 0.814 | yes |
| bulk-256k | 195.593 ± 113.959 | 122.137 ± 12.475 | 170.074 ± 90.682 | 337.246 ± 37.346 | 0.419 | yes |
| bulk-1m | 431.962 ± 40.444 | 505.045 ± 157.375 | 441.851 ± 44.441 | 1144.824 ± 154.282 | 0.394 | yes |
| bulk-4m | 2438.289 ± 171.446 | 2639.336 ± 281.496 | 2688.255 ± 196.620 | 5177.818 ± 307.870 | 0.516 | yes |
| loopback-0 | 24.567 ± 2.091 | 31.043 ± 5.973 | 27.235 ± 4.887 | 26.402 ± 4.599 | 1.005 | no |
| loopback-64 | 27.654 ± 5.204 | 26.684 ± 5.613 | 27.041 ± 5.584 | 28.410 ± 7.444 | 0.997 | yes |
| loopback-1024 | 27.085 ± 4.006 | 32.277 ± 6.218 | 27.294 ± 4.856 | 31.594 ± 5.523 | 0.670 | yes |
| loopback-65536 | 77.923 ± 27.504 | 64.275 ± 7.913 | 71.833 ± 26.428 | 60.734 ± 4.117 | 1.151 | no |
| deep-8 | 29.672 ± 4.542 | 30.556 ± 5.353 | 29.484 ± 1.618 | 28.906 ± 4.582 | 0.994 | no |
| deep-32 | 38.101 ± 7.370 | 38.069 ± 5.477 | 39.036 ± 5.569 | 35.278 ± 7.160 | 1.103 | no |
| concurrent-32 | 102.933 ± 8.913 | 131.375 ± 7.800 | 136.802 ± 18.143 | 157.114 ± 16.079 | 0.846 | yes |
| deep-32-concurrent-32 | 281.659 ± 41.992 | 313.289 ± 42.647 | 305.029 ± 47.662 | 312.011 ± 10.763 | 0.962 | yes |
| services-4-deep-16 | 180.883 ± 46.495 | 203.249 ± 51.338 | 216.558 ± 95.646 | 166.303 ± 44.810 | 1.173 | no |
| services-4-deep-16-concurrent-32 | 1174.825 ± 37.694 | 982.874 ± 68.759 | 932.569 ± 57.019 | 1247.357 ± 56.368 | 0.767 | yes |
| services-4-deep-16-clients-8 | 1209.778 ± 30.241 | 1012.271 ± 87.486 | 1019.971 ± 79.019 | 1155.705 ± 48.530 | 0.865 | yes |
| services-8-deep-16-clients-8 | 2798.168 ± 227.608 | 1898.138 ± 217.306 | 1985.544 ± 243.968 | 2507.668 ± 263.479 | 0.750 | yes |

## Instruction and branch diagnostics

Across both pipeline peers, whole-process instructions fell 1.25%; dependent-read fell 0.65% and callback 0.61%. Concurrent service-chain totals stayed within 0.1% of the baseline. These work reductions do not establish an end-to-end latency gain. Whole-process instruction captures cover all 24 fixtures and every intermediate service, for all four implementations: 256 process captures. They include startup, warmup and teardown and are separate from native timing. Focused self-cost accounting includes both poll_maintenance_budget_with and advance_idle_phase, so moving work to the helper cannot appear as a reduction. It excludes the ownership-bearing transition handler and its callees.

| Case/process | Before maintenance instructions | Portable maintenance instructions | Change |
| --- | ---: | ---: | ---: |
| dependent-read/client | 506,675 | 442,099 | -12.75% |
| dependent-read/server | 325,664 | 268,058 | -17.69% |
| pipelined-read/client | 370,237 | 317,480 | -14.25% |
| pipelined-read/server | 597,401 | 509,512 | -14.71% |
| callback/client | 277,633 | 221,053 | -20.38% |
| callback/server | 238,994 | 209,132 | -12.49% |

Hardware PMU counters remain unavailable in this environment. The following are Callgrind simulations, not hardware misses: 32 KiB eight-way L1 I/D caches, 256 MiB direct-mapped last-level cache, 64-byte lines. Binary layout can affect simulated predictor aliasing. No rejected gprofng sampling rankings from earlier work are used.

| Case/process | Before branch misses | Portable branch misses | C++ branch misses | Instruction change vs before |
| --- | ---: | ---: | ---: | ---: |
| unary-empty/client | 62,071 | 58,530 | 71,796 | -0.00% |
| unary-empty/server | 59,150 | 59,553 | 77,162 | -0.00% |
| pipelined-read/client | 325,364 | 331,061 | 150,203 | -0.99% |
| pipelined-read/server | 249,581 | 234,876 | 170,563 | -1.54% |
| callback/client | 204,839 | 193,834 | 148,216 | -1.00% |
| callback/server | 219,062 | 209,739 | 145,060 | -0.21% |

## Read/write counts and integrity

Each pass requires reads and writes independently no greater than C++ for the same process. Attempts include nonpositive syscalls. Complete lifecycle traffic is retained. All 240 mixed-implementation checks, 48 original received-frame checks and 304 directed topology-edge checks pass. Frame integrity does not imply I/O-count parity.

| Instrument | Positive read/write passes | Attempt passes | Both pass | Process comparisons |
| --- | ---: | ---: | ---: | ---: |
| socket | 42 | 42 | 42 | 64 |
| wire | 41 | 41 | 41 | 64 |

The all-process I/O target remains unmet. Concurrent service-chain batching and bootstrap lifecycle traffic remain gaps; the earlier drain-before-flush experiment is still rejected because its count improvement came with a substantial latency regression.

## Validation and provenance

The selected source passes **154 allocating wire tests**, **101 no-allocation wire tests**, **336 RPC library tests**, and **nine TCP/Unix integration tests**, plus strict Clippy for the RPC and wire crates, and workspace formatting. The isolated portability matrix passes core (86), alloc (89), growing (89), std (86), and unaligned/sync_reader (86) test executions. Bare-metal thumbv7em-none-eabi checks pass for both core and alloc wire profiles. Full coverage, Miri and formal suites were not rerun.

The growth fixture now retains its observer for the first Pending poll, then drops it before later bindings grow. This preserves its intended mid-scan growth precondition without relying on empty-phase budget consumption. The other adjusted test explicitly invalidates a partial scan after externally releasing a graph observer; its later deferred-answer/export idle-certificate checks still omit invalidation as before. The benchmark executable was rechecked after this verification-only fixture adjustment and remains byte-identical.

The regression fails on the old implementation at starting phase 0 with a one-step budget. It now checks all eighteen starting phases, the returned owner and exhausted allowance, cursor position and teardown. Existing tests still cover pending callbacks exposing cleanup, fixed/growing capacity, owner-by-owner retirement, cancellation and reclamation.

All executable, fixture and source hashes are verified. Source replay reconstructs the selected source tree. Earlier sealed archives and their hardlinked sources were not modified. The evidence archive retains every raw native sample, socket/wire capture, instruction profile, source patch, test/build log and analysis script.

- Base: `1a5dc9faedb9a97d6d284a24b0882b04daeb9310`; selected source tree: `edfb9293c04452f19e76bea59386765b70edf5c0`.
- Source patch SHA-256: `6aa9952328a08e944cad9c71537a2b5bfb13ecd5bd4a11171c2f6eb826cba498`.
- Selected binary SHA-256: `772304a8a3e5184043ae44eea35c9c21fa39ad1e77cc70556f6d8b9b0ac004c7`.
- Archive: `/workspace/capntproto/target/portable-maintenance-walk-20261019`; 6,972 files, 481,558,086 logical bytes.
- Archive SHA256SUMS SHA-256: `b889a1c37bd4e4785abd0fcdcc3d00ef66a35e4e4447b72099432355c6b1be57`.
