# Portable maintenance budgets and publication — October 18, 2026

Tracking: [gap #11](https://github.com/DericHuynh/capntproto/issues/11), [draft PR #12](https://github.com/DericHuynh/capntproto/pull/12). Legacy remains a measured reference.

**The requested universal C++ win is not established.** In this local campaign, portable beats C++ by both mean and pooled median in 14/24 cases (14/24 means; 16/24 medians). These are descriptive comparisons, not confidence bounds. The older faster-of-legacy/C++ comparison passes 9/24 cases by both metrics.

## Implementation and experiments

- Charge an idle maintenance phase for its live proof once, instead of charging every retained slot. Physical scan coverage and the rotating cursor still include all slots; a real ownership transition still costs a step. Zero budgets, the eight-owner retirement burst limit, blocked output and cleanup ordering remain enforced. This removes budget exhaustion caused solely by retained capacity.
- Report every committed immediate Call as `Published { plain }`, including calls carrying capabilities. The scheduler stops redundant preparation after publication. Only `plain: true` allows the existing no-grant fast path; capability-bearing publication keeps ordinary invalidation and cleanup. This intentionally changes the public progress enum without a compatibility alias.
- Reject the direct-Return observer-settlement experiment: pipeline-client instructions increased 2.60%, with no pipeline median benefit. Its sources, executable, five-run timings and diagnostics remain in the evidence archive.
- The publication-only experiment reduced callback-client instructions by 1.09%, but did not establish a latency improvement. Its mixed native results and small instruction regressions are retained in focused-experiments.json. The full campaign below evaluates the combined implementation against the frozen parent.

## Five-run comparison

Same-host TCP, five independent process runs with 1,000 measured samples per cell; warmup 100, or 10,000 for loopback. Native runs are serial and separate from builds, tests and instrumentation. Order rotates and reverses; five repetitions do not exactly balance four positions. Before is the frozen parent executable from commit d1b8675. Legacy and Portable explicitly select their own drivers in the candidate executable.

Times are microseconds: pooled arithmetic mean ± sample SD across the five run means (n−1), not a confidence interval. Raw samples, run means, run ranges, per-sample SD, p50/p95/p99, extrema and throughput are retained. A pass requires both mean and pooled median strictly below C++ in this campaign.

| Case | Legacy mean ± SD | Before mean ± SD | Portable mean ± SD | C++ mean ± SD | Portable p50 / C++ | Both pass |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| unary-empty | 29.130 ± 4.533 | 27.829 ± 4.729 | 33.762 ± 3.602 | 26.533 ± 4.436 | 1.554 | no |
| unary-small | 25.036 ± 4.646 | 28.100 ± 6.471 | 26.302 ± 4.886 | 25.988 ± 4.244 | 0.974 | no |
| unary-1k | 31.752 ± 5.518 | 34.461 ± 8.599 | 31.002 ± 8.379 | 32.509 ± 6.104 | 0.789 | yes |
| fanout-32 | 109.755 ± 14.690 | 151.878 ± 18.444 | 141.870 ± 17.113 | 164.017 ± 18.553 | 0.850 | yes |
| dependent-read | 54.365 ± 3.772 | 63.898 ± 6.094 | 68.706 ± 15.157 | 52.302 ± 0.442 | 1.198 | no |
| pipelined-read | 37.640 ± 7.165 | 52.431 ± 8.854 | 49.481 ± 6.330 | 36.560 ± 6.408 | 1.400 | no |
| write-read | 57.558 ± 10.973 | 58.591 ± 11.389 | 57.504 ± 11.221 | 59.216 ± 11.809 | 0.717 | yes |
| callback | 68.954 ± 12.074 | 73.763 ± 8.027 | 78.200 ± 11.182 | 67.140 ± 10.555 | 1.123 | no |
| bulk-64k | 92.443 ± 34.855 | 92.274 ± 31.804 | 96.816 ± 30.411 | 93.878 ± 32.103 | 0.987 | no |
| bulk-256k | 249.180 ± 127.545 | 229.299 ± 135.729 | 186.849 ± 110.856 | 336.328 ± 59.725 | 0.368 | yes |
| bulk-1m | 720.480 ± 424.809 | 400.910 ± 14.398 | 407.180 ± 41.205 | 1174.773 ± 264.913 | 0.379 | yes |
| bulk-4m | 2289.437 ± 218.147 | 2497.911 ± 305.212 | 2360.090 ± 275.810 | 4745.587 ± 338.643 | 0.487 | yes |
| loopback-0 | 22.800 ± 0.546 | 24.071 ± 1.179 | 23.772 ± 0.531 | 23.399 ± 1.625 | 1.000 | no |
| loopback-64 | 27.119 ± 6.564 | 27.531 ± 5.868 | 26.398 ± 5.388 | 28.504 ± 6.228 | 0.965 | yes |
| loopback-1024 | 31.945 ± 5.514 | 29.803 ± 6.628 | 28.331 ± 6.608 | 28.882 ± 5.539 | 0.984 | yes |
| loopback-65536 | 77.994 ± 33.959 | 90.402 ± 32.656 | 94.618 ± 28.481 | 81.857 ± 32.735 | 2.000 | no |
| deep-8 | 25.848 ± 1.988 | 24.889 ± 0.620 | 28.937 ± 5.520 | 26.309 ± 2.562 | 1.005 | no |
| deep-32 | 28.283 ± 0.797 | 29.021 ± 1.049 | 29.770 ± 1.599 | 32.002 ± 4.517 | 0.976 | yes |
| concurrent-32 | 94.157 ± 7.676 | 117.702 ± 6.105 | 119.529 ± 9.094 | 138.314 ± 2.733 | 0.841 | yes |
| deep-32-concurrent-32 | 247.665 ± 29.619 | 284.559 ± 41.019 | 267.811 ± 22.106 | 302.342 ± 32.503 | 0.847 | yes |
| services-4-deep-16 | 149.359 ± 16.619 | 158.803 ± 19.952 | 161.019 ± 13.944 | 146.695 ± 22.636 | 1.065 | no |
| services-4-deep-16-concurrent-32 | 1145.579 ± 24.781 | 1002.183 ± 41.390 | 1001.572 ± 34.041 | 1154.828 ± 21.194 | 0.860 | yes |
| services-4-deep-16-clients-8 | 1130.844 ± 28.047 | 1010.433 ± 36.657 | 1006.928 ± 36.356 | 1053.566 ± 35.650 | 0.931 | yes |
| services-8-deep-16-clients-8 | 2493.202 ± 79.846 | 1937.051 ± 97.253 | 1973.642 ± 46.048 | 2192.635 ± 75.415 | 0.853 | yes |

Against the frozen parent in this same campaign, 13/24 means and 13/24 medians improve; that parent beats C++ by both metrics in 12/24 cases. Regressions remain, including unary-empty mean +21.3%, deep-8 +16.3%, dependent-read +7.5% and callback +6.0%. Small-call and 64 KiB timings vary materially across unchanged controls. The instruction reductions below are much smaller than these timing swings; the campaign does not establish a broad causal latency gain.

## Instruction and I/O traces

Callgrind, socket journals and full-wire traces cover all 24 fixtures and every process in each topology, including intermediate services. Diagnostic budgets are fixed separately from native timing. Whole-process instruction totals include startup, warmup and teardown; they are not per-RPC latency estimates. report.json retains all 64 process comparisons, with the legacy and frozen-parent instruction counts as well as C++.

| Instrument | Positive read/write passes | Attempt passes | Processes |
| --- | ---: | ---: | ---: |
| socket | 42 | 43 | 64 |
| wire | 40 | 40 | 64 |

Each pass requires reads and writes individually no greater than C++ for the equivalent process. Attempts include nonpositive syscalls. All 48 original received-frame checks and 304 directed topology-edge checks pass. Frame integrity does not imply count parity.

| Case | Process | Before instructions | Portable instructions | C++ instructions | Change vs before |
| --- | --- | ---: | ---: | ---: | ---: |
| dependent-read | client | 16,500,745 | 16,462,706 | 15,021,592 | -0.23% |
| dependent-read | server | 15,134,303 | 15,112,147 | 18,130,465 | -0.15% |
| pipelined-read | client | 17,366,768 | 17,346,690 | 15,174,261 | -0.12% |
| pipelined-read | server | 15,631,685 | 15,586,570 | 17,182,017 | -0.29% |
| callback | client | 14,440,744 | 14,260,564 | 15,302,494 | -1.25% |
| callback | server | 14,193,310 | 14,176,557 | 14,452,286 | -0.12% |
| concurrent-32 | client | 8,310,292 | 8,316,781 | 11,136,401 | +0.08% |
| concurrent-32 | service-0 | 6,976,413 | 6,976,411 | 11,230,916 | -0.00% |
| services-4-deep-16-clients-8 | client | 15,223,431 | 15,218,804 | 15,992,909 | -0.03% |
| services-4-deep-16-clients-8 | service-0 | 30,279,745 | 30,227,357 | 30,830,936 | -0.17% |
| services-4-deep-16-clients-8 | service-1 | 38,594,291 | 38,469,279 | 29,516,994 | -0.32% |
| services-4-deep-16-clients-8 | service-2 | 37,027,399 | 36,995,784 | 29,500,780 | -0.09% |
| services-4-deep-16-clients-8 | service-3 | 13,475,554 | 13,468,211 | 16,879,678 | -0.05% |

The combined candidate reduces callback-client instructions by 1.25%, pipeline-client/server by 0.12%/0.29%, and dependent-read client/server by 0.23%/0.15%. These are modest savings. The pipeline socket diagnostic has 111 reads and 111 writes for each client, but portable read-to-next-write CPU median is 12.88 µs versus C++ 6.01 µs. This diagnostic includes instrument overhead and intervening work; it is not an isolated decoder benchmark.

Additional cache/branch simulation targets unary, pipeline and callback for every implementation and both peers. The modeled L1 instruction/data caches are 32 KiB, 64-byte lines, eight-way; the modeled last-level cache is 256 MiB, direct-mapped. These are simulator events, not hardware measurements. Complete counters and the model are retained in report.json.

| Case/process | Portable I1 misses | C++ I1 misses | Portable branch misses | C++ branch misses |
| --- | ---: | ---: | ---: | ---: |
| unary-empty/client | 338,031 | 221,822 | 63,035 | 71,793 |
| unary-empty/server | 321,474 | 226,747 | 54,008 | 77,160 |
| pipelined-read/client | 1,071,791 | 383,034 | 330,881 | 150,202 |
| pipelined-read/server | 946,318 | 495,308 | 248,001 | 170,561 |
| callback/client | 937,880 | 476,195 | 198,092 | 148,212 |
| callback/server | 945,932 | 438,468 | 216,879 | 145,058 |

Pipeline-client simulated instruction-cache misses are 2.80× C++ and branch misses 2.20×; callback-client instruction-cache misses are 1.97× despite fewer executed instructions than C++. The largest portable contributors include Coordinator::drive, poll_active_work, admission, maintenance and call polling. The simulator supports investigating the code footprint and branch-heavy scheduling layers; it does not establish hardware miss rates or promise a corresponding speedup.

## Architecture assessment

C++ RpcRequest::setupSend() retains the request builders, writes descriptors and directly admits the question. handleReturn() addresses the exact question and fulfills its QuestionRef. KJ EventLoop::turn() fires one queued ready event. The portable path still layers public hook admission, hosted call state, a six-lane coordinator and an eighteen-phase ownership scanner. Removing empty scans helps, but this layering remains visible when capability observers and forwarding are active.

A full simplification should make ownership transitions enqueue generation-checked ready work, so Return, cancellation, publication and retirement each schedule the exact dependent object. That requires a coherent public-client/driver lifetime and identity design: the existing static hooks and borrowed generative driver cannot safely become direct pointers. Pipeline hook memoization must continue preserving capability identity and embargo behavior. The rejected direct-Return patch shows that adding a shortcut on top of the existing scanner is insufficient.

Forwarding also needs readiness-driven output batching while preserving full-duplex progress. Earlier global burst and receive-first experiments failed their latency or I/O objectives and remain rejected. Neither fewer syscalls nor fewer instructions alone establishes the requested universal latency win.

## Dedicated parent results

The dedicated run is evidence for parent commit d1b8675, not this candidate. [Run 39166617880](https://github.com/DericHuynh/capntproto/actions/runs/39166617880) collected measurements and uploaded its report, then failed its legacy loopback gate: legacy/C++ median at zero bytes was 1.3143× against a 1.15× limit. The artifact ZIP hash is recorded in remote-parent.json; every raw artifact file is retained inside the archived ZIP.

On the two-host workload comparison, parent portable beat C++ by both mean and median in only 2/12 cases. Pipeline median was 14.4% slower, dependent-read 6.8% slower, callback 4.3% slower. The larger remote payload cases also trailed C++; same-host bulk wins must not be represented as remote wins.

## Validation and evidence

[Parent CI 39166626747](https://github.com/DericHuynh/capntproto/actions/runs/39166626747) passed workspace tests and doctests, platforms, Miri, unsafe controls and formal checks. It failed the coverage regression gate: for example, combined coordinator branch coverage changed from 310/410 to 485/656, alongside other listed file regressions. The complete job log and job conclusions are retained. This increment does not resolve that coverage failure.

328 RPC and 153 wire library tests pass, along with all 436 isolated portable feature-profile test executions and nine TCP/Unix integration tests. Strict Clippy for both affected libraries and formatting pass. The first wire run failed the old vacant-slot cursor assertion; the revised assertion verifies the intended one-proof phase skip and preserves the eight-owner retirement bound. That failed run is retained. No unsafe code is added. Full coverage, Miri and formal suites were not rerun for this increment. Repository, workflow and wiki checks are recorded separately at publication.

Source base: `d1b8675cb96338017001aceca6b328995eacdbc3`; reconstructed source tree: `46ce50d445683ed31db267cfb4ebb7dba9d81822`. The [source patch](source.patch) reconstructs that tree. Selected executable SHA-256: `c2dedb86c31d122555f838f53aaa871454a4a8f089dff8710a361e1a147f31b2`. C++ source remains pinned to `0de72d8d8cec6b69edaa29de51d3bd490341f9c2`.

Evidence archive: `/workspace/capntproto/target/portable-maintenance-budget-20261018`; 9,781 files, 923,189,773 logical bytes. SHA256SUMS SHA-256: `27f3ed9329a8b45e26c77fc668641b156d5ae595ff8c4cfc42c3ce44a7c0a507`. Earlier archives verified unchanged. The archive includes rejected candidates and failed validation logs; it does not select the best timing sample per case.
