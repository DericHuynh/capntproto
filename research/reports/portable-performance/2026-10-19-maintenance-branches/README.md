# Portable maintenance branch simplification — October 19, 2026

Tracking: [gap #11](https://github.com/DericHuynh/capntproto/issues/11), [draft PR #12](https://github.com/DericHuynh/capntproto/pull/12). Legacy remains a measured reference.

**The universal C++ target remains unmet.** Portable beats C++ by both mean and pooled median in 15/24 cases in this local campaign; the frozen parent achieves 17/24 against the same C++ measurements. These are descriptive comparisons, not confidence bounds.

## Selected implementation

Maintenance walks consecutive empty phases directly in their fixed order, stopping before the first active phase. It evaluates each live idle proof only when that phase is reached. The walk replaces the repeated indirect phase dispatch and keeps empty phases outside the large ownership-bearing transition handler. An outlined helper shares cursor/budget accounting across those phases. The combined dispatch/helper code is 4,251 bytes versus 6,177 for the unfactored walk and 3,160 before this change; the ownership-bearing handler remains approximately 35 KB.

There is no cached idle mask; every actual poll or callback is followed by fresh live checks. Phase order, cursor advancement on resource errors, per-phase budget charges, retained-capacity skipping, zero-budget behavior and the eight-owner retirement limit remain unchanged. Existing regressions cover a Pending callback exposing new cleanup in the same poll, newly retired owners after idle, retained capacity, and bounded retirement. No unsafe code or compatibility adapter is added.

Three alternatives were rejected; an unfactored direct walk was superseded by shared accounting. Fusing the occupancy/transition dispatch entered a roughly 35 KB handler for empty phases: pipeline branch misses fell 2.5–2.8%, but instructions rose 1.1–2.2% and instruction-cache misses increased. An eager mask of all phases unnecessarily scanned tables before reaching them: pipeline instructions rose 1.2–2.0%, with focused mean/median regressions of 7.7%/8.4%. A narrower cheap-mask variant with direct empty continuation still added pipeline instructions and I1 misses, with focused mean/median regressions of 15.5%/5.4%. Their exact sources, binaries, five-run timings and profiles are retained. The unfactored walk reduced instructions but raised instruction-cache misses more than the selected implementation. See experiments.json.

## Branch and cache diagnostics

Hardware branch counters are unavailable (perf_event_open returned EPERM). The following counts come from separate whole-process Callgrind runs, including startup, warmup and teardown. The simulator uses 32 KiB eight-way L1 instruction/data caches with 64-byte lines, and a 256 MiB direct-mapped last-level cache. These are diagnostic events, not measured hardware miss rates or native latency. Both conditional and indirect misses are counted.

| Case/process | Before misses | Portable misses | C++ misses | Miss change | Instruction change | I1 miss change |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| unary-empty/client | 62,928 | 64,345 | 71,793 | +2.25% | -0.01% | -0.34% |
| unary-empty/server | 54,013 | 59,013 | 77,160 | +9.26% | -0.00% | +0.54% |
| pipelined-read/client | 330,881 | 322,099 | 150,202 | -2.65% | -0.12% | +1.22% |
| pipelined-read/server | 248,001 | 247,957 | 170,561 | -0.02% | -0.34% | +2.46% |
| callback/client | 198,090 | 198,819 | 148,212 | +0.37% | -0.22% | +0.52% |
| callback/server | 216,879 | 219,620 | 145,058 | +1.26% | -0.24% | +0.14% |

### Dispatch costs including the helper

Self costs in poll_maintenance_budget_with plus the outlined idle accounting helper; ownership-bearing maintenance_step and other callees are excluded consistently. This avoids crediting the caller for work merely moved into a helper.

| Case/process | Before branch misses | Portable branch misses | Change | Instruction change | Before / portable indirect branches |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary-empty/client | 149 | 123 | -17.45% | -4.96% | 64 / 0 |
| unary-empty/server | 20 | 12 | -40.00% | -5.41% | 3 / 0 |
| pipelined-read/client | 10,054 | 7,826 | -22.16% | -0.36% | 4,894 / 0 |
| pipelined-read/server | 15,928 | 15,891 | -0.23% | -2.26% | 8,565 / 0 |
| callback/client | 7,535 | 7,152 | -5.08% | -3.43% | 4,221 / 0 |
| callback/server | 6,865 | 5,961 | -13.17% | -5.22% | 3,953 / 0 |

### Repeated CPU diagnostics

Five additional instrumented process runs rotate all five variants through each position once. Values below are the mean of five per-run read-to-next-write thread-CPU medians, plus their sample SD, in microseconds. Gaps use the same descriptor/thread and include intervening application/protocol work and probe overhead, including bootstrap, warmup and teardown. They are not isolated reader/writer timings and never enter the native speed gate. Spread is large, including synchronized slow runs in unchanged controls; these are descriptive diagnostic improvements, not confidence bounds.

| Case/process | Before mean ± SD | Portable mean ± SD | C++ mean ± SD | Portable / before |
| --- | ---: | ---: | ---: | ---: |
| dependent-read/client | 7.632 ± 3.336 | 7.558 ± 3.347 | 3.466 ± 0.362 | 0.990 |
| dependent-read/server | 7.071 ± 3.625 | 7.052 ± 3.320 | 4.463 ± 0.189 | 0.997 |
| pipelined-read/client | 16.835 ± 7.635 | 16.113 ± 7.458 | 5.963 ± 0.121 | 0.957 |
| pipelined-read/server | 12.631 ± 7.686 | 12.252 ± 6.942 | 6.774 ± 0.167 | 0.970 |
| callback/client | 9.260 ± 4.206 | 9.005 ± 3.941 | 4.803 ± 1.730 | 0.972 |
| callback/server | 7.706 ± 3.727 | 7.365 ± 3.618 | 4.313 ± 1.633 | 0.956 |

All branch events in the source-attributed profiles are checked against their process totals. Full function/line rankings, conditional and indirect counts, and all cache events are archived. Binary layout also changes predictor aliasing, so unrelated per-function miss changes are not evidence of changed algorithms.

## Five-run native comparison

Same-host TCP, five independent process runs per cell, 1,000 retained samples each. Warmup is 100, or 10,000 for loopback. Variant order rotates and reverses; five repetitions do not exactly balance four positions. Builds, tests and instrumented runs never overlap native timing. Before is the frozen portable executable at ab76901; Legacy and Portable explicitly select their respective drivers in the candidate binary.

Times are microseconds: pooled mean ± sample SD across five run means (n−1), not a confidence interval. Raw samples, per-run means/ranges, per-sample SD, extrema, tails and throughput are retained. A pass requires both mean and pooled median strictly below C++. Fanout and concurrent rows measure a complete batch; service-topology rows include all configured hops. Batch and logical-request throughput are retained separately.

| Case | Legacy mean ± SD | Before mean ± SD | Portable mean ± SD | C++ mean ± SD | Portable p50 / C++ | Both pass |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| unary-empty | 27.798 ± 4.970 | 29.519 ± 5.957 | 29.014 ± 5.710 | 31.618 ± 4.370 | 0.688 | yes |
| unary-small | 26.626 ± 4.341 | 30.881 ± 5.949 | 30.550 ± 4.279 | 30.473 ± 4.980 | 0.827 | no |
| unary-1k | 25.728 ± 3.669 | 24.699 ± 3.045 | 26.802 ± 4.584 | 25.435 ± 0.726 | 0.989 | no |
| fanout-32 | 119.130 ± 11.889 | 143.270 ± 16.903 | 135.451 ± 8.542 | 163.631 ± 20.284 | 0.835 | yes |
| dependent-read | 59.804 ± 9.395 | 73.061 ± 10.312 | 74.539 ± 13.887 | 63.211 ± 12.004 | 1.180 | no |
| pipelined-read | 34.355 ± 1.197 | 56.018 ± 9.329 | 51.092 ± 2.562 | 36.483 ± 5.740 | 1.384 | no |
| write-read | 50.437 ± 2.602 | 54.047 ± 10.598 | 55.058 ± 8.107 | 56.130 ± 10.218 | 0.968 | yes |
| callback | 60.383 ± 9.649 | 68.943 ± 13.859 | 65.601 ± 5.707 | 57.223 ± 7.617 | 1.182 | no |
| bulk-64k | 58.348 ± 3.809 | 59.475 ± 3.500 | 57.945 ± 6.246 | 62.982 ± 3.984 | 0.811 | yes |
| bulk-256k | 111.356 ± 2.666 | 125.744 ± 9.809 | 116.968 ± 12.248 | 327.221 ± 43.882 | 0.353 | yes |
| bulk-1m | 388.480 ± 14.820 | 403.396 ± 19.570 | 451.647 ± 52.779 | 1074.823 ± 171.991 | 0.400 | yes |
| bulk-4m | 2208.790 ± 67.470 | 2466.793 ± 208.192 | 2343.758 ± 154.349 | 5038.437 ± 181.848 | 0.475 | yes |
| loopback-0 | 26.340 ± 4.895 | 26.157 ± 5.223 | 26.979 ± 4.680 | 28.585 ± 6.678 | 0.984 | yes |
| loopback-64 | 32.167 ± 5.208 | 26.001 ± 5.403 | 30.173 ± 6.676 | 30.585 ± 6.249 | 0.994 | yes |
| loopback-1024 | 27.515 ± 5.064 | 25.690 ± 4.002 | 23.087 ± 0.529 | 31.256 ± 7.181 | 0.666 | yes |
| loopback-65536 | 68.090 ± 26.901 | 57.140 ± 2.415 | 57.942 ± 4.871 | 58.201 ± 2.195 | 0.944 | yes |
| deep-8 | 28.570 ± 5.536 | 25.835 ± 1.243 | 28.231 ± 5.370 | 28.080 ± 3.392 | 0.989 | no |
| deep-32 | 30.476 ± 1.476 | 35.652 ± 7.885 | 31.823 ± 1.865 | 31.844 ± 5.617 | 0.980 | yes |
| concurrent-32 | 110.100 ± 19.039 | 133.814 ± 13.199 | 144.557 ± 8.176 | 163.030 ± 23.383 | 1.000 | no |
| deep-32-concurrent-32 | 279.185 ± 31.203 | 296.163 ± 25.421 | 303.500 ± 27.644 | 319.619 ± 34.667 | 0.893 | yes |
| services-4-deep-16 | 166.915 ± 18.113 | 204.770 ± 42.654 | 167.548 ± 12.510 | 154.010 ± 18.368 | 1.086 | no |
| services-4-deep-16-concurrent-32 | 1219.259 ± 33.967 | 1101.513 ± 29.276 | 1142.721 ± 124.102 | 1265.107 ± 42.187 | 0.895 | yes |
| services-4-deep-16-clients-8 | 1232.820 ± 49.815 | 1142.718 ± 102.001 | 1174.536 ± 29.698 | 1123.716 ± 27.753 | 1.001 | no |
| services-8-deep-16-clients-8 | 2618.953 ± 75.971 | 2183.569 ± 98.510 | 2252.717 ± 261.949 | 2450.999 ± 112.815 | 0.883 | yes |

Against the frozen parent, 11/24 means and 13/24 pooled medians improve. The selected change is retained for its measured reduction in maintenance dispatch work; the end-to-end results do not establish a speedup, and its mean-and-median C++ pass count is lower than the parent in this campaign. Run spread remains material, especially in small calls and 64 KiB transfers. This campaign does not establish a universal or statistically bounded latency improvement.

## All-fixture traces

Instruction, socket and complete-wire traces cover every fixture and every process, including intermediate services. Instrumented timings are excluded from the native table. report.json retains all 64 process comparisons and all four instruction-profile variants.

| Instrument | Positive read/write passes | Attempt passes | Processes |
| --- | ---: | ---: | ---: |
| socket | 42 | 42 | 64 |
| wire | 40 | 40 | 64 |

A count pass requires both reads and writes individually no greater than C++ for that process. Attempts include nonpositive syscalls. All 240 mixed-implementation checks, 48 original received-frame checks and 304 directed topology-edge checks pass. Frame integrity does not imply read/write parity.

## Architecture and validation

The larger bottleneck remains repeated scheduling through hosted hooks, six coordinator lanes and eighteen maintenance phases. Driver exposes mutable runtime and ledger fields, so persistent ready-work tracking requires mutation notifications at those boundaries; assuming no external changes would be incorrect. C++ KJ EventLoop::turn removes one ready event and fires it; its RPC Return path finds and fulfills the owning question directly. Portable still needs generation-checked ready-work queues and more cohesive question/completion ownership to remove broad scans. Capability identity, retained reader limits, cancellation, release ordering and bounded storage remain required; bypassing them is not an optimization.

This increment passes 481 RPC/wire library tests, 436 isolated feature-profile executions, nine TCP/Unix integrations, strict Clippy, formatting, exact source replay, and repository/workflow/wiki checks. Full local coverage, Miri and formal suites were not rerun.

The frozen parent’s [hosted dedicated benchmark run](https://github.com/DericHuynh/capntproto/actions/runs/39173500903) passed, but its separate two-host portable/C++ mean-and-median comparison passes only 3/12 workflows. Its [CI run](https://github.com/DericHuynh/capntproto/actions/runs/39173507619) passed platform, safety and formal jobs and failed coverage regression gates. These parent results do not qualify this candidate. Coverage remains unresolved; its floors were not weakened. The complete parent benchmark artifact is retained as a ZIP.

## Provenance

- Source base: `ab76901a88a8cf477f57a30156c6738879c6c7bc`; source tree: `e514b8852cb9c940219dc0be747d1f1dd108117b`.
- Exact patch SHA-256: `1b83b60dc79f9d3e7a4998e72909041472231a4ed830ab65a89b76328e2396c2`.
- Selected executable SHA-256: `b42fa1aed5643695d3bd31897850418cec34ee76f37883e0abfe0bc03198322c`.
- Frozen parent SHA-256: `c2dedb86c31d122555f838f53aaa871454a4a8f089dff8710a361e1a147f31b2`.
- C++ source pin: `0de72d8d8cec6b69edaa29de51d3bd490341f9c2`; original/topology binary hashes are in report.json.
- Archive: `/workspace/capntproto/target/portable-maintenance-branches-20261019`; 14,065 files, 1,140,049,000 logical bytes.
- SHA256SUMS SHA-256: `e142a68d39787eacceb5f620c5bba959e96c2d9100d62fa0d9ca3d97f312755c`. Fourteen prior archives verify unchanged.

The archive retains orchestration and analysis scripts, fixture hashes, exact source patches, frozen binaries, every native sample, all diagnostics, rejected experiments, validation logs and hosted-parent evidence. Rebuildable compiler caches were reclaimed for disk space; sealed evidence was preserved.
