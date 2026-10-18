# Portable payload construction and bounded batching — October 18, 2026

Tracking: [gap #11](https://github.com/DericHuynh/capntproto/issues/11), [draft PR #12](https://github.com/DericHuynh/capntproto/pull/12). This continues the new portable architecture; the existing legacy implementation remains a benchmark reference.

**The global performance gate remains open: 5/24 pooled-p50 cells match or beat the faster current legacy/C++ reference; 1/24 are at least 5% faster.** Frozen portable from 75ab5e0 passes 9/24 against these same references. Means, spread, tails, throughput and raw samples are retained separately.

## Selected changes

- Construct a capability-bearing Call through one payload builder, filling both content and descriptors without reopening the mutable envelope. This follows the retained call/params builder structure in C++ rpc.c++ (`RpcRequest` construction and `setupSend`). Existing size, capability-index, descriptor, publication and rollback checks remain in place.
- Permit at most 64 outstanding owned transport frames instead of 32, leaving room for control frames alongside an application burst. This raises the frame/receipt overhead bound; the message-pool byte budget and application admission credits are unchanged. Physical completion, failure, idle and shutdown fences remain separate from queue acceptance.
- Fix the Kani mutation anchor for question cleanup after explicit exchange ownership was introduced. The Finish-acceptance negative control still preserves the live-owner guard.

Separate focused fanout captures confirm the extra startup write disappears: the portable client changes from 112 to 111 positive socket writes, matching C++ at 111. The independent full-wire capture changes from 14 to 13 writes, matching C++ at 13. These captures use different diagnostic budgets and are not combined with native timings; raw summaries are included in report.json.

## Rejected scheduling experiment and remaining architecture work

An eight-round protocol burst reduced focused microservice reads/writes to approximately C++ levels, but increased pooled median latency by 36.5–64.9% against the frozen portable control in the same five-run campaign. It delayed downstream progress while accumulating work. That scheduler change is rejected; the selected runtime retains output between protocol turns. All measurements and the rejected binary/source snapshot are retained.

The isolated payload experiment reduced callback-client Callgrind instructions from 14,594,751 to 14,449,821 (0.993%). Other profiled peers were essentially unchanged. Native small-call results are bimodal, including the controls; the adverse unary-1k results are retained and no latency gain is inferred from instruction counts alone.

The unresolved architectural tradeoff is batching within a connection while preserving overlap across a service chain. A global drain-before-write policy reduces syscalls at a substantial latency cost in these fixtures. Per-connection scheduling and capability publication/maintenance remain optimization targets; this increment does not claim global syscall or speed parity.

## Five-run native comparison

Every cell has five independent process runs and 1,000 measured samples per run. Warmups remain 100, or 10,000 for loopback. Runs are serial and uninstrumented, separate from builds and tests. Variant order rotates/reverses, which does not exactly balance four positions over five runs. Legacy and portable use the selected executable with explicit driver selection; Before is the frozen 75ab5e0 executable.

Times are µs: pooled arithmetic mean ± sample SD across the five run means (n−1), not a confidence interval. [report.json](report.json) also contains per-sample SD, extrema, run ranges, p50/p95/p99 and throughput.

| Case | Legacy mean ± SD | Before mean ± SD | Portable mean ± SD | C++ mean ± SD | Portable p50 / faster | Pass |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| unary-empty | 28.013 ± 6.004 | 27.958 ± 6.749 | 28.195 ± 6.589 | 24.634 ± 1.078 | 1.011 | no |
| unary-small | 29.076 ± 5.885 | 29.393 ± 7.394 | 30.566 ± 5.221 | 29.346 ± 6.245 | 1.366 | no |
| unary-1k | 26.611 ± 5.015 | 28.455 ± 7.164 | 30.691 ± 5.694 | 29.629 ± 6.309 | 1.085 | no |
| fanout-32 | 121.320 ± 23.990 | 144.473 ± 18.334 | 148.118 ± 18.360 | 174.059 ± 15.942 | 1.461 | no |
| dependent-read | 62.341 ± 14.109 | 73.405 ± 10.643 | 83.119 ± 21.224 | 61.931 ± 10.662 | 1.625 | no |
| pipelined-read | 39.256 ± 12.248 | 60.390 ± 20.991 | 60.365 ± 21.951 | 38.855 ± 5.070 | 1.512 | no |
| write-read | 56.324 ± 11.162 | 65.640 ± 11.416 | 54.293 ± 12.403 | 59.505 ± 12.858 | 0.989 | yes |
| callback | 60.585 ± 9.763 | 70.375 ± 12.393 | 79.760 ± 15.869 | 62.535 ± 13.396 | 1.629 | no |
| bulk-64k | 68.741 ± 26.710 | 57.424 ± 6.459 | 68.962 ± 27.399 | 61.749 ± 3.785 | 1.002 | no |
| bulk-256k | 195.180 ± 108.306 | 151.937 ± 47.639 | 189.151 ± 97.366 | 350.958 ± 59.167 | 1.112 | no |
| bulk-1m | 421.656 ± 60.555 | 501.631 ± 104.858 | 447.523 ± 64.450 | 1148.258 ± 163.450 | 1.033 | no |
| bulk-4m | 2492.196 ± 232.746 | 2389.329 ± 119.485 | 2495.630 ± 86.519 | 4942.580 ± 294.294 | 1.033 | no |
| loopback-0 | 29.612 ± 8.435 | 28.642 ± 6.040 | 28.063 ± 5.857 | 29.257 ± 5.862 | 1.032 | no |
| loopback-64 | 30.840 ± 5.249 | 25.820 ± 5.141 | 30.228 ± 6.789 | 33.193 ± 4.910 | 1.010 | no |
| loopback-1024 | 28.728 ± 5.929 | 33.181 ± 4.431 | 31.703 ± 5.947 | 32.699 ± 5.246 | 1.444 | no |
| loopback-65536 | 56.178 ± 5.124 | 79.077 ± 29.980 | 75.026 ± 25.337 | 72.477 ± 28.012 | 1.220 | no |
| deep-8 | 30.142 ± 5.423 | 29.863 ± 6.311 | 32.701 ± 5.483 | 31.399 ± 5.325 | 1.356 | no |
| deep-32 | 36.631 ± 7.113 | 32.983 ± 5.512 | 34.354 ± 7.242 | 33.242 ± 5.126 | 0.998 | yes |
| concurrent-32 | 102.391 ± 10.371 | 126.112 ± 14.881 | 135.123 ± 16.824 | 154.932 ± 9.722 | 1.318 | no |
| deep-32-concurrent-32 | 269.222 ± 31.158 | 286.701 ± 29.722 | 304.463 ± 31.329 | 292.039 ± 29.860 | 1.210 | no |
| services-4-deep-16 | 164.452 ± 17.557 | 191.440 ± 33.207 | 181.309 ± 17.551 | 171.498 ± 59.400 | 1.153 | no |
| services-4-deep-16-concurrent-32 | 1170.121 ± 22.118 | 1125.613 ± 72.460 | 1109.220 ± 108.390 | 1212.909 ± 59.475 | 0.955 | yes |
| services-4-deep-16-clients-8 | 1199.123 ± 45.663 | 1072.648 ± 49.572 | 1088.935 ± 35.321 | 1115.969 ± 59.789 | 0.967 | yes |
| services-8-deep-16-clients-8 | 2553.532 ± 94.942 | 2078.636 ± 50.170 | 2152.842 ± 207.193 | 2376.488 ± 126.467 | 0.872 | yes |

## Confirmation of inconsistent latency results

One additional five-run native campaign was specified after the full sweep, using the same binaries, fixtures and budgets. Its 160,000 samples are retained in report.json and the archive. It does not replace the full qualification sweep above. Several pooled-median regressions reversed, but dependent-read and callback remained slower. Those regressions are unresolved; the instruction and startup-write savings do not establish a native latency win.

| Case | Portable mean ± run SD (µs) | Before mean ± run SD (µs) | Portable p50 / before | Portable mean / before | Median paired run-p50 ratio |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary-small | 32.485 ± 7.056 | 34.143 ± 7.418 | 0.996 | 0.951 | 1.005 |
| dependent-read | 86.191 ± 19.835 | 78.567 ± 10.341 | 1.342 | 1.097 | 1.317 |
| pipelined-read | 54.570 ± 6.439 | 54.837 ± 5.478 | 0.996 | 0.995 | 0.997 |
| callback | 84.178 ± 9.153 | 73.137 ± 12.222 | 1.168 | 1.151 | 1.011 |
| fanout-32 | 157.970 ± 20.330 | 156.833 ± 20.413 | 1.004 | 1.007 | 0.999 |
| loopback-64 | 28.030 ± 6.728 | 30.698 ± 6.548 | 0.698 | 0.913 | 1.019 |
| deep-8 | 31.284 ± 5.890 | 33.586 ± 5.379 | 0.704 | 0.931 | 0.993 |
| deep-32-concurrent-32 | 290.275 ± 16.513 | 318.212 ± 22.921 | 0.852 | 0.912 | 0.896 |

## Profiles and IO

Callgrind totals include process startup, warmup, work and teardown. These separate diagnostic runs never contribute native timing samples.

| Case | Process | Legacy instructions | Before | Portable | C++ | Change vs before |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| unary-small | client | 6,451,358 | 6,489,192 | 6,489,204 | 8,077,572 | +0.00% |
| unary-small | server | 5,937,459 | 6,008,271 | 6,008,283 | 8,353,364 | +0.00% |
| unary-1k | client | 6,622,385 | 6,617,663 | 6,617,614 | 8,274,952 | -0.00% |
| unary-1k | server | 6,066,782 | 6,091,854 | 6,091,842 | 8,534,930 | -0.00% |
| fanout-32 | client | 23,126,940 | 27,688,950 | 27,666,577 | 43,869,190 | -0.08% |
| fanout-32 | server | 19,820,159 | 21,759,213 | 21,754,272 | 43,209,026 | -0.02% |
| pipelined-read | client | 10,469,823 | 17,656,460 | 17,656,435 | 15,174,285 | -0.00% |
| pipelined-read | server | 13,072,052 | 15,705,905 | 15,705,917 | 17,182,017 | +0.00% |
| callback | client | 10,901,870 | 14,596,499 | 14,449,602 | 15,302,494 | -1.01% |
| callback | server | 11,970,466 | 14,248,276 | 14,248,204 | 14,452,286 | -0.00% |
| bulk-4m | client | 67,096,019 | 68,565,154 | 68,567,250 | 71,333,947 | +0.00% |
| bulk-4m | server | 11,150,812 | 11,548,783 | 11,550,314 | 11,483,500 | +0.01% |
| concurrent-32 | client | 6,852,620 | 8,433,234 | 8,309,813 | 11,136,401 | -1.46% |
| concurrent-32 | service-0 | 6,438,554 | 6,977,837 | 6,974,922 | 11,230,916 | -0.04% |
| deep-32-concurrent-32 | client | 16,178,489 | 17,848,794 | 17,850,162 | 18,739,652 | +0.01% |
| deep-32-concurrent-32 | service-0 | 20,162,446 | 20,284,467 | 20,281,518 | 23,202,497 | -0.01% |
| services-4-deep-16-clients-8 | client | 12,810,743 | 15,269,813 | 15,341,635 | 15,987,708 | +0.47% |
| services-4-deep-16-clients-8 | service-0 | 28,760,955 | 30,400,017 | 30,462,270 | 30,827,081 | +0.20% |
| services-4-deep-16-clients-8 | service-1 | 27,004,466 | 39,276,958 | 39,227,873 | 29,516,994 | -0.12% |
| services-4-deep-16-clients-8 | service-2 | 27,004,075 | 37,445,301 | 37,097,695 | 29,477,435 | -0.93% |
| services-4-deep-16-clients-8 | service-3 | 13,047,430 | 13,428,198 | 13,472,344 | 16,850,059 | +0.33% |

Each IO pass requires reads and writes ≤ C++ independently for that process. Attempt counts include nonpositive syscalls. Socket and full-wire captures are separate observations; frame integrity does not establish count parity.

| Instrument | Positive read/write passes | Attempt passes | Processes |
| --- | ---: | ---: | ---: |
| socket | 42 | 43 | 64 |
| wire | 40 | 41 | 64 |

## Validation and provenance

- Affected unit and integration tests: **passed** — 328 RPC and 151 allocating wire tests; 9 TCP/Unix transport integration tests with no default root features; 92 development-tool tests. The queue64 candidate received the RPC and transport checks; wire production source is unchanged from the earlier payload/burst8 tests.
- Static and configuration checks: **passed** — Strict Clippy for RPC and wire on all targets, no-default-features wire compilation, alloc/unaligned/sync_reader wire compilation, and workspace formatting.
- Benchmark integrity: **passed** — Five runs per cell, 480,000 native samples, 240 mixed implementation checks, unchanged fixtures and budgets, exact executable hashes, recomputed statistics, and separately captured socket/wire/profile evidence. All 48 original frame pairs and 304 directed topology edges match received frames.
- Repository and source provenance: **passed** — Repository, wiki and workflow checks; source patch replays to the recorded source tree. All candidates and failed experiments are retained.
- Kani mutation-plan regression: **passed** — Corrected the stale reuse-before-finish-acceptance anchor without removing the owner guard or changing the expected failure. The development-tool suite checks all production mutation anchors. This fixes the observed parent CI failure before coverage.
- Full formal, Miri, coverage and independent C++ suites: **not-rerun** — No unsafe reader, engine ownership, or formal production model changes in this increment. Earlier qualification remains in the runtime-ownership report; it is not a fresh run for this candidate. Full coverage and independent C++ correctness remain separate from the affected tests and mixed benchmark checks.

The [source patch](source.patch) applies to `75ab5e006d3fcd76808a20b0eb079422ad88e575` and reconstructs `43e699225a0ee42c1ac948a948d5c51249dd4738`. Selected executable SHA-256: `1945a38d8dda62624e354431d4249d1bea1619136d286ce08cf0e4d329f6671d`. C++ remains pinned to `0de72d8d8cec6b69edaa29de51d3bd490341f9c2`. All intermediate source snapshots and executable hashes are in report.json and the local evidence archive. Hosted new-head checks are reported separately.

Evidence archive: `/workspace/capntproto/target/portable-payload-batching-20261018`; 10,628 files and 759,682,059 logical bytes. SHA256SUMS SHA-256: `e9becf8d78bb01013f229c5a8a4f5f700d0b524c484727073728f682857b080c`. Prior archives verified unchanged.
