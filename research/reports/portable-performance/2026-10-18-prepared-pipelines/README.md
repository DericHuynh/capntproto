# Portable prepared pipelines and live occupancy — October 18, 2026

Tracking: [gap #11](https://github.com/DericHuynh/capntproto/issues/11), [draft PR #12](https://github.com/DericHuynh/capntproto/pull/12). Legacy remains a reference implementation.

**The global gate remains open: 4/24 cases match or beat the faster legacy/C++ reference by both mean and pooled median.** The separate totals are 6/24 for mean and 7/24 for median. No claim of universal speed or I/O parity is made.

## Selected implementation

- Unresolved wire pipelines now construct parameters in a prepared Call envelope. Publication copies the already-validated envelope and appends the generated promised-answer target, preserving relative pointers and any second payload segment. This removes a second payload graph construction. Paths over 16 fields, unsupported layouts, insufficient storage and changed reader limits retain the general validated path; no unsafe code or unchecked capability ownership is introduced.
- Server workers track live entries and the first free slot. An idle worker and repeated reuse of a free slot no longer scan from slot zero behind a long-running request. Existing dispatch fairness and cancellation semantics remain.
- Pipeline and exported-promise bindings track live observer occupancy. Empty retained capacity no longer forces repeated scans. Answer/export ledgers reuse their explicitly installed lifetime observations to prove emptiness; unobserved tables keep their bounded scan. Forgotten batches and retained owners still require cleanup. Scan budgets and transition ordering are unchanged.
- Repair the parent CI failures: align the immediate-return regression with lazy payload capability validation, while retaining malformed-envelope rejection and cancellation cleanup; refresh three reviewed unsafe-inventory file hashes without changing diagnostic counts.

The C++ comparison uses the pinned `rpc.c++` implementation: `ExportTable::empty()` uses slot/free-ID sizes, `ImportTable::empty()` uses presence bits, and `RpcRequest::setupSend()` retains its Call/params builders while admitting the question. The portable changes apply those principles to the existing explicit ownership and bounded-storage contracts.

## Experiments and selection

Each full campaign below contains 480,000 native samples and 240 mixed implementation checks. Separate campaigns are retained in full rather than choosing the best sample for each case. The projected campaign compares against the frozen parent; subsequent campaigns compare against the frozen projected candidate.

| Candidate | Median passes / 24 | Mean passes / 24 | Both / 24 | Disposition |
| --- | ---: | ---: | ---: | --- |
| projected | 9 | 9 | 8 | Prepared pipeline base; retained |
| readfirst | 11 | 6 | 6 | Receive-first scheduling; rejected |
| occupancy | 7 | 6 | 4 | Selected live-occupancy increment |

The prepared pipeline change reduced pipeline-client total Callgrind instructions by approximately 1.57% against the parent. Message-layout self instructions fell from 860,304 to 670,232 (22.1%). These counts include startup, warmup and teardown and do not establish an equivalent latency percentage.

Receive-first scheduling failed to remove the separate 40-byte bootstrap Finish or materially reduce instruction counts. The initial ungated form also failed the pending-reader/write-failure regression; the gated version passed all 329 RPC tests but still lacked the intended I/O benefit. Both sources and the gated binary remain in the archive; neither scheduler change is selected.

Earlier focused readiness and promised-target experiments are retained in focused-experiments.json. Narrower worker wakeups regressed the callback mean; the first promised-target implementation did not reach the unresolved pipeline path until prepared-envelope integration was added. No focused result replaces a full campaign.

## Five-run comparison for the selected candidate

Same-host TCP; five independent process runs and 1,000 measured samples per cell. Warmups are 100, or 10,000 for loopback. Runs are serial, uninstrumented and separate from compilation, tests and profiles. Order rotates/reverses; five runs do not exactly balance four variant positions. Before is the frozen projected candidate. Legacy and Portable explicitly select different drivers in the same selected binary.

Times are µs: pooled arithmetic mean ± sample standard deviation across the five run means (n−1), not a confidence interval. report.json retains per-sample SD, run means/ranges, extrema, p50/p95/p99 and throughput. The pass column requires both mean and median to be no slower than their independently faster legacy/C++ reference.

| Case | Legacy mean ± SD | Before mean ± SD | Portable mean ± SD | C++ mean ± SD | Portable p50 / faster | Both pass |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| unary-empty | 29.390 ± 7.425 | 30.014 ± 8.645 | 29.252 ± 6.629 | 29.194 ± 6.944 | 1.015 | no |
| unary-small | 28.506 ± 5.314 | 28.301 ± 4.725 | 25.298 ± 2.611 | 31.571 ± 7.170 | 0.893 | yes |
| unary-1k | 30.452 ± 7.255 | 27.468 ± 5.083 | 28.058 ± 5.207 | 27.989 ± 5.298 | 0.978 | no |
| fanout-32 | 115.879 ± 16.810 | 146.962 ± 19.290 | 143.824 ± 18.260 | 176.672 ± 31.922 | 1.233 | no |
| dependent-read | 59.514 ± 10.319 | 78.943 ± 17.418 | 66.542 ± 3.998 | 61.582 ± 10.940 | 1.175 | no |
| pipelined-read | 38.356 ± 5.418 | 52.040 ± 5.048 | 50.845 ± 5.563 | 41.451 ± 7.224 | 1.411 | no |
| write-read | 51.161 ± 4.104 | 57.404 ± 11.729 | 54.557 ± 8.249 | 57.667 ± 7.602 | 0.998 | no |
| callback | 60.509 ± 8.612 | 80.789 ± 14.115 | 76.410 ± 11.496 | 54.720 ± 5.420 | 1.554 | no |
| bulk-64k | 62.064 ± 8.366 | 72.561 ± 25.829 | 72.556 ± 25.668 | 80.237 ± 27.847 | 1.184 | no |
| bulk-256k | 114.692 ± 7.322 | 157.861 ± 58.803 | 124.367 ± 27.585 | 335.825 ± 49.191 | 1.063 | no |
| bulk-1m | 408.093 ± 21.920 | 421.530 ± 6.977 | 416.124 ± 10.556 | 1047.112 ± 97.275 | 1.069 | no |
| bulk-4m | 2213.245 ± 44.584 | 2543.714 ± 265.147 | 2478.193 ± 268.905 | 4864.234 ± 321.660 | 1.104 | no |
| loopback-0 | 29.069 ± 6.924 | 31.227 ± 6.351 | 32.613 ± 6.834 | 30.097 ± 6.120 | 1.009 | no |
| loopback-64 | 30.426 ± 6.772 | 30.275 ± 6.812 | 29.797 ± 7.207 | 30.178 ± 6.568 | 1.008 | no |
| loopback-1024 | 32.208 ± 4.881 | 29.505 ± 6.410 | 29.070 ± 7.021 | 28.334 ± 5.631 | 0.976 | no |
| loopback-65536 | 56.912 ± 3.870 | 67.022 ± 27.435 | 80.910 ± 33.829 | 73.305 ± 31.080 | 1.202 | no |
| deep-8 | 25.666 ± 2.106 | 25.699 ± 0.969 | 25.611 ± 1.486 | 26.092 ± 2.317 | 1.008 | no |
| deep-32 | 28.251 ± 0.733 | 35.849 ± 8.133 | 30.443 ± 2.622 | 34.338 ± 5.427 | 1.039 | no |
| concurrent-32 | 100.862 ± 10.729 | 120.876 ± 4.532 | 116.425 ± 1.149 | 141.110 ± 5.195 | 1.242 | no |
| deep-32-concurrent-32 | 246.460 ± 44.242 | 265.602 ± 8.246 | 260.339 ± 5.781 | 284.475 ± 24.754 | 1.115 | no |
| services-4-deep-16 | 151.874 ± 15.431 | 186.012 ± 34.441 | 161.250 ± 13.686 | 154.438 ± 16.512 | 1.038 | no |
| services-4-deep-16-concurrent-32 | 1206.473 ± 125.329 | 1093.938 ± 59.948 | 1024.305 ± 40.950 | 1208.670 ± 61.589 | 0.883 | yes |
| services-4-deep-16-clients-8 | 1183.671 ± 74.600 | 1053.886 ± 31.668 | 1114.306 ± 125.243 | 1142.959 ± 66.668 | 0.931 | yes |
| services-8-deep-16-clients-8 | 2637.355 ± 190.069 | 2054.493 ± 26.181 | 2115.617 ± 124.026 | 2423.647 ± 240.324 | 0.870 | yes |

Against the frozen candidate in this same campaign, 19/24 means and 17/24 medians improve, but the frozen candidate also passes 7/24 median targets against these references. The 64 KiB loopback mean regresses by 20.7%, and the four/eight-service multi-client means regress by 5.7%/3.0%. Instruction savings from live occupancy are generally under 0.2% in the original profiles. These observations support removing the empty-table scans, not attributing every native timing change to that optimization or claiming a broad speed win.

## Separate instruction and I/O diagnostics

Callgrind totals cover the complete diagnostic process, including startup, warmup and teardown. Instrumented latencies are excluded from qualification.

| Case | Process | Legacy instructions | Before | Portable | C++ | Change vs before |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| unary-small | client | 6,451,707 | 6,494,784 | 6,492,784 | 8,077,586 | -0.03% |
| unary-small | server | 5,939,605 | 6,012,974 | 6,007,544 | 8,353,364 | -0.09% |
| unary-1k | client | 6,622,759 | 6,623,226 | 6,620,724 | 8,274,700 | -0.04% |
| unary-1k | server | 6,067,179 | 6,096,101 | 6,090,658 | 8,534,930 | -0.09% |
| fanout-32 | client | 23,127,266 | 27,681,739 | 27,667,256 | 43,869,190 | -0.05% |
| fanout-32 | server | 19,820,508 | 21,789,767 | 21,759,720 | 43,209,026 | -0.14% |
| pipelined-read | client | 10,470,418 | 17,378,305 | 17,366,741 | 15,174,261 | -0.07% |
| pipelined-read | server | 13,071,854 | 15,661,671 | 15,631,685 | 17,182,017 | -0.19% |
| callback | client | 10,900,148 | 14,447,777 | 14,441,530 | 15,302,494 | -0.04% |
| callback | server | 11,970,815 | 14,210,962 | 14,193,391 | 14,452,286 | -0.12% |
| bulk-4m | client | 67,024,867 | 68,571,762 | 68,570,218 | 71,322,855 | -0.00% |
| bulk-4m | server | 11,065,247 | 11,543,598 | 11,547,967 | 11,478,689 | +0.04% |
| concurrent-32 | client | 6,852,960 | 8,315,004 | 8,310,292 | 11,136,401 | -0.06% |
| concurrent-32 | service-0 | 6,438,894 | 6,984,485 | 6,976,413 | 11,230,916 | -0.12% |
| deep-32-concurrent-32 | client | 16,178,903 | 17,855,325 | 17,850,617 | 18,739,652 | -0.03% |
| deep-32-concurrent-32 | service-0 | 20,162,786 | 20,291,083 | 20,283,011 | 23,202,497 | -0.04% |
| services-4-deep-16-clients-8 | client | 12,811,069 | 15,232,479 | 15,218,502 | 15,977,109 | -0.09% |
| services-4-deep-16-clients-8 | service-0 | 28,761,280 | 30,405,272 | 30,386,633 | 30,827,081 | -0.06% |
| services-4-deep-16-clients-8 | service-1 | 27,004,443 | 38,775,550 | 38,851,326 | 29,516,994 | +0.20% |
| services-4-deep-16-clients-8 | service-2 | 27,004,448 | 37,212,163 | 36,962,079 | 29,523,929 | -0.67% |
| services-4-deep-16-clients-8 | service-3 | 13,047,770 | 13,476,471 | 13,459,360 | 17,019,475 | -0.13% |

Each I/O pass requires reads and writes individually ≤ C++ for the same process and fixture. Attempts include nonpositive syscalls. Socket journals and full-wire traces are separate observations, with their own documented diagnostic budgets.

| Instrument | Positive read/write passes | Attempt passes | Processes |
| --- | ---: | ---: | ---: |
| socket | 45 | 45 | 64 |
| wire | 40 | 40 | 64 |

All 48 original frame-pair checks and 304 directed topology-edge checks pass for each full campaign. Frame integrity does not imply count parity. Full per-process counts, including failures, are in each campaign JSON.

## Remaining architecture issues

The coordinator still traverses multiple ownership/maintenance phases when real observers are live. Capability-bearing publication and asynchronous hook admission remain more involved than C++ direct request setup. Live occupancy removes empty work, but does not replace the scheduler with readiness-driven queues.

Microservice forwarding still has a batching/overlap tradeoff. The earlier global protocol-burst experiment reduced syscalls while worsening latency substantially; it remains rejected. The receive-first experiment also left the large-message bootstrap split unchanged. More aggressive batching or altered read readiness needs its own backpressure, fairness and full-duplex validation before adoption.

Small-call native timings vary materially, including the unchanged controls. Median pass counts alone are not sufficient evidence of improvement. The dedicated parent-commit run separately found dependent-read, pipelined-read and callback above the old 5% threshold; it is evidence about the parent, not this candidate.

## Validation and provenance

- Affected libraries: 328 RPC, 203 engine and 153 wire tests pass. The new tests exercise large retained tables, canceled observers, failed resolution, slot reuse, discarded unexposed observers, output retry, deferred answer reclamation and forgotten export batches.
- Isolated portable configurations: all five pass (86 default, 89 alloc, 89 growing, 86 std, 86 unaligned/sync_reader test executions). The fixed-storage cases remain allocation checked.
- Strict Clippy on all targets of the three affected libraries passes. Formatting, repository, workflow and wiki checks pass. The unsafe inventory check passed after the three source-hash repairs; the later occupancy change adds no unsafe code.
- Nine scoped TCP/Unix integration tests pass. Five additional facade tests pass serially with TLC configured, including the pinned C++ descriptor trace comparison. The first parallel facade invocation failed from a missing TLC environment setting and a raw-descriptor liveness assertion; the latter did not reproduce serially. That failed log is retained, and the parallel failure is not claimed resolved.
- Every full benchmark campaign independently verifies executable/fixture hashes, five runs, unchanged budgets, mixed-peer operation, all descriptive statistics and ordering. Native timing never overlaps builds, tests or instrumented runs.
- Coverage, full formal suites and Miri were not rerun for this increment. The facade TLC/C++ check is not a substitute for those suites. Hosted checks for the new commit are tracked separately.

The parent hosted CI run [39158440084](https://github.com/DericHuynh/capntproto/actions/runs/39158440084) failed on the stale immediate-return assertion and unsafe inventory hashes. Both have local corrective evidence. Parent performance run [39158437313](https://github.com/DericHuynh/capntproto/actions/runs/39158437313) succeeded; its downloaded report and provenance are archived separately and never mixed into current timings.

The [source patch](source.patch) applies to `6d9afa78c57fdaaa3893159accdf82532487dc01` and reconstructs `e6ee29e82b557dd8d4fb4afb0f80d86c97e9a1a3`. Selected executable SHA-256: `523047a57c1bcb20bfcf973e856b1f5b7016da9f64ec88b0ee3d0edcab4c5656`. C++ source remains pinned to `0de72d8d8cec6b69edaa29de51d3bd490341f9c2`. All tested candidates, rejected sources, raw samples, socket journals, wire captures and profiles are retained.

Evidence archive: `/workspace/capntproto/target/portable-prepared-pipelines-20261018`; 22,268 files and 1,364,991,154 logical bytes. SHA256SUMS SHA-256: `db0f26147ab5283df4f38b6d081ce8ce45c6c0ba17552b198a4eae82c5d6dcad`. Prior evidence archives verified unchanged.
