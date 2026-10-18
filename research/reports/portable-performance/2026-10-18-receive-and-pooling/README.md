# Portable receive ownership, pooling and writer experiments — October 18, 2026

Tracking: [gap #11](https://github.com/DericHuynh/capntproto/issues/11), [draft PR #12](https://github.com/DericHuynh/capntproto/pull/12). Portable must beat the faster current legacy/fixed C++ reference in every equivalent case. The existing numerical gate is pooled p50 ≤ min(legacy, C++), with positive reads/writes and attempts ≤ C++ independently for every process capture. Means, spreads and tails remain necessary evidence. Legacy remains available.

**The goal is not met: 8/24 primary cells pass the p50 gate, versus 14/24 for frozen d8ed2001 portable against the same current references.** Read/write parity also remains unmet. The selected changes reduce measured CPU work; the native results do not demonstrate an across-the-board speedup. Separate confirmation does not replace the primary matrix.

## Selected implementation

- `Messages::receive()` gives streamed input an exclusive `Receiving` owner without clearing already initialized storage. Only the committed prefix is exposed to decoding; cancellation, EOF and invalid progress cannot seal stale bytes. Ordinary builder reservations still clear storage. `FrameCursor` shares the framing state machine with borrowed `FrameReceiver`, including size, segment-table, traversal and core-feature alignment rules.
- **API change:** `owned::tasks::receive` and `receive_until` take `Receiving` instead of `Writing`; callers acquire it with `Messages::receive`. Every in-repository caller was migrated. Returned receive state retains ownership until finish/discard. No unsafe code was added.
- Immediate reply-owner lookup uses a sparse integer-key hash map and one entry lookup on retirement. Only locally generated outgoing question IDs enter it; the full branded/generational key still authorizes delivery. Peer-supplied IDs never index an allocation.
- The portable segment cache defaults to at most 64 entries, matching legacy, instead of 16. Its existing cached-word limit is unchanged (at most 16,384 words / 128 KiB); handle metadata can grow to 64 entries. The fan-out client profile falls from 898 heap-segment allocations to 34, versus legacy’s 38.
- The hosted Miri test inventory expected 45 selected tests but actually contained 47. The expected ownership count is corrected from 31 to 33; no test was filtered and no coverage threshold was changed.

The isolated receive candidate reduced 1 MiB server instructions by 10.82% and 4 MiB server instructions by 19.23%. An alignment-policy issue found during review was fixed before the hash/cache candidates or native qualification: wire framing must follow the core crate’s unified feature policy. The hash experiment then reduced fan-out client instructions by 2.82%. The selected combination reduces fan-out client instructions by 11.51% and 4 MiB server instructions by 19.13% against d8ed2001. Whole-process profile budgets and all intermediate artifacts are retained.

## Architectural findings

Portable still performs a complete capability-index walk before exposing retained payloads, then uses unlimited application traversal budgets. C++ retains the content reader and checks capability indexes when accessed (`rpc.c++` call/result admission and `capability.c++` extraction). In the frozen before deep-concurrent client, outbound and inbound validation account for about 3.21 million of 20.28 million instructions. Removing either walk safely requires finite shared traversal-budget ownership in retained readers, on-access capability checks, and an explicit change to malformed-field error timing. Simply deleting validation while leaving unlimited readers would be incorrect. Both plain and general payload ownership paths must change together.

C++ batches output with KJ `evalLast()` (`vendor/capnproto/c++/src/capnp/rpc-twoparty.c++`, around lines 176–202). Two new experiments separated the physical writer from the ordinary lifecycle/failure observer; both preserved immediate-error and shutdown tests. The first used the transport runtime yield; the second used the task set’s ordinary wake. They are evaluated below. A separate writer can improve startup/control batching, but scheduling and receipt ownership add work. Generic bounded writers must still finish a pending physical flush before further writes; changing that contract needs an explicit adapter capability and backpressure tests.

Previous hosted coverage and CodeQL checks remain unresolved. The Miri inventory repair fixes its observed pre-test failure, not a claim that the complete hosted interpretation matrix passed.

## Primary five-run comparison

Each cell uses five independent process runs and 1,000 retained samples per run. Warmups remain 100, or 10,000 for loopback. All 480,000 samples and 240 mixed compatibility checks are retained. Native timing ran sequentially, separately from builds, tests and instrumentation. Variant order rotates/reverses; five repetitions cannot exactly balance four variant positions.

Times below are microseconds: pooled arithmetic mean ± sample standard deviation of the five run means (n−1), not a confidence interval. Per-sample SD, run means/ranges, p50/p95/p99 and throughput are in [report.json](report.json). Before means frozen d8ed2001 portable; current legacy and portable use the same selected executable.

| Case | Legacy mean ± SD | Before mean ± SD | Portable mean ± SD | C++ mean ± SD | Portable p50 / faster | Pass |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| unary-empty | 25.887 ± 3.439 | 25.241 ± 1.533 | 24.938 ± 2.090 | 28.774 ± 5.147 | 0.971 | yes |
| unary-small | 25.089 ± 2.131 | 23.962 ± 1.160 | 26.509 ± 4.552 | 25.904 ± 1.841 | 0.962 | yes |
| unary-1k | 26.836 ± 4.199 | 25.888 ± 4.167 | 24.809 ± 2.030 | 28.347 ± 3.950 | 0.952 | yes |
| fanout-32 | 120.085 ± 17.857 | 152.142 ± 19.830 | 136.268 ± 9.548 | 181.167 ± 21.316 | 1.167 | no |
| dependent-read | 68.175 ± 10.719 | 80.712 ± 20.863 | 75.234 ± 17.176 | 63.687 ± 11.737 | 1.249 | no |
| pipelined-read | 44.271 ± 11.732 | 59.096 ± 18.341 | 59.598 ± 18.103 | 41.937 ± 8.499 | 1.505 | no |
| write-read | 59.390 ± 7.245 | 55.939 ± 12.871 | 56.177 ± 8.137 | 58.243 ± 11.536 | 1.005 | no |
| callback | 62.031 ± 8.545 | 72.257 ± 14.353 | 72.680 ± 14.818 | 58.692 ± 8.785 | 1.516 | no |
| bulk-64k | 71.757 ± 25.366 | 69.548 ± 26.427 | 69.161 ± 25.392 | 73.676 ± 26.321 | 1.054 | no |
| bulk-256k | 160.814 ± 103.263 | 164.386 ± 104.831 | 226.116 ± 121.538 | 355.976 ± 43.006 | 1.378 | no |
| bulk-1m | 405.294 ± 21.707 | 403.127 ± 27.525 | 407.238 ± 19.286 | 1075.741 ± 124.922 | 1.029 | no |
| bulk-4m | 2351.259 ± 206.396 | 2805.761 ± 90.591 | 2571.532 ± 164.868 | 5093.295 ± 408.203 | 1.105 | no |
| loopback-0 | 30.059 ± 4.346 | 24.960 ± 5.122 | 29.857 ± 6.679 | 28.603 ± 4.231 | 1.410 | no |
| loopback-64 | 28.009 ± 5.792 | 27.774 ± 6.245 | 27.584 ± 4.267 | 31.610 ± 6.300 | 1.002 | no |
| loopback-1024 | 28.341 ± 4.090 | 29.502 ± 7.457 | 28.647 ± 5.422 | 31.075 ± 8.787 | 1.007 | no |
| loopback-65536 | 59.833 ± 3.406 | 55.924 ± 3.415 | 56.512 ± 4.066 | 63.168 ± 4.126 | 0.954 | yes |
| deep-8 | 28.321 ± 4.957 | 28.895 ± 6.512 | 31.641 ± 5.165 | 29.606 ± 3.710 | 1.478 | no |
| deep-32 | 35.032 ± 5.795 | 30.971 ± 2.265 | 32.524 ± 4.340 | 33.169 ± 2.753 | 0.956 | yes |
| concurrent-32 | 99.843 ± 13.273 | 133.218 ± 17.513 | 127.324 ± 11.356 | 148.892 ± 9.364 | 1.288 | no |
| deep-32-concurrent-32 | 275.788 ± 33.271 | 327.004 ± 30.774 | 318.771 ± 12.597 | 332.958 ± 31.859 | 1.101 | no |
| services-4-deep-16 | 171.627 ± 13.795 | 172.921 ± 11.339 | 197.071 ± 11.197 | 159.865 ± 11.589 | 1.128 | no |
| services-4-deep-16-concurrent-32 | 1263.280 ± 80.694 | 1042.999 ± 67.791 | 1060.441 ± 65.085 | 1308.831 ± 86.615 | 0.865 | yes |
| services-4-deep-16-clients-8 | 1366.484 ± 307.380 | 1072.100 ± 104.079 | 1074.767 ± 80.462 | 1224.849 ± 231.232 | 0.903 | yes |
| services-8-deep-16-clients-8 | 2704.501 ± 193.508 | 2258.640 ± 573.686 | 2027.178 ± 162.051 | 2479.426 ± 189.861 | 0.819 | yes |

## Independent confirmation

A separate five-run sweep retains another 240,000 samples across the fan-out gain, shallow/deep regressions, pipelining, callbacks and bulk transfers. It is not pooled with or substituted for the primary matrix.

| Case | Before mean ± SD | Portable mean ± SD | Mean change | p50 change |
| --- | ---: | ---: | ---: | ---: |
| fanout-32 | 150.932 ± 26.512 | 142.841 ± 25.483 | -5.4% | -5.6% |
| callback | 61.179 ± 11.705 | 71.039 ± 12.810 | +16.1% | +28.5% |
| pipelined-read | 65.333 ± 19.354 | 54.282 ± 10.433 | -16.9% | -5.1% |
| bulk-64k | 58.476 ± 3.852 | 59.672 ± 4.100 | +2.0% | +13.4% |
| bulk-256k | 211.104 ± 143.458 | 126.733 ± 9.577 | -40.0% | -4.6% |
| bulk-1m | 408.091 ± 81.930 | 440.383 ± 23.961 | +7.9% | +17.1% |
| bulk-4m | 3006.495 ± 198.061 | 2480.318 ± 69.589 | -17.5% | -16.6% |
| loopback-0 | 27.788 ± 5.427 | 27.004 ± 5.745 | -2.8% | -2.5% |
| loopback-1024 | 32.291 ± 7.105 | 29.440 ± 6.051 | -8.8% | -1.2% |
| deep-8 | 30.716 ± 6.255 | 31.249 ± 6.254 | +1.7% | -1.1% |
| deep-32-concurrent-32 | 316.674 ± 48.402 | 314.073 ± 28.056 | -0.8% | +7.8% |
| services-4-deep-16 | 170.506 ± 10.381 | 166.942 ± 19.462 | -2.1% | -2.1% |

Fan-out mean latency improved 10.4% in the primary sweep and 5.4% in confirmation; 4 MiB improved 8.4% and 17.5%. Callback p50 regressed in both sweeps (about 28%); its mean was nearly unchanged in the primary sweep and 16.1% higher in confirmation. The 1 MiB confirmation mean was 7.9% higher. Callback and 64 KiB whole-process instruction counts changed by less than 0.04%; those profiles do not explain or dismiss the timing regressions. The 256 KiB, loopback-0 and deep-8 primary regressions did not repeat at the same magnitude. All results remain part of the assessment.


## Writer experiments

Each experiment retains six cases × four variants × five runs × 1,000 samples (120,000 samples). Its before control is the selected receive/hash/cache binary, not d8ed2001. All failures/regressions remain visible.

| Candidate | Case | Mean change vs selected | p50 change vs selected |
| --- | --- | ---: | ---: |
| deferred | unary-empty | +18.1% | +11.6% |
| deferred | fanout-32 | -6.9% | -20.9% |
| deferred | pipelined-read | +7.2% | +7.4% |
| deferred | callback | +9.2% | -9.3% |
| deferred | deep-32-concurrent-32 | -4.5% | -3.9% |
| deferred | services-4-deep-16 | +17.3% | +15.1% |
| wake | unary-empty | +11.7% | +7.7% |
| wake | fanout-32 | +9.7% | +9.2% |
| wake | pipelined-read | -1.0% | -19.0% |
| wake | callback | +26.6% | +19.3% |
| wake | deep-32-concurrent-32 | -0.8% | -1.0% |
| wake | services-4-deep-16 | +18.0% | +19.0% |

Both deferred-writer candidates are rejected and their code is removed from the selected tree. Runtime yield reduced unary/pipeline positive writes to C++ parity in short captures, but added an empty event-loop poll per output turn, 23.90–25.25% unary instructions, and 18.06% unary mean latency. Ordinary wake removed the empty polls and lost the syscall improvement, while still increasing unary instructions 14.40–15.96%, unary mean latency 11.67%, callback mean latency 26.61%, and four-service mean latency 17.99%. The experiments preserve error/flush/shutdown correctness but do not improve the overall performance target. The remaining architectural requirement is coordinated application/protocol/output scheduling with physical receipt ownership; deferring a second task alone is insufficient.

## Profiles and physical IO

Profiles include process startup, warmup, measured work and teardown. Instrumented latencies never enter native results. The complete instruction totals are in report.json. The selected combination reduced concurrent-32 client instructions by 7.61% and deep-32-concurrent-32 client instructions by 3.84%; single deep-32 was unchanged.

Every selected case has fresh lightweight socket journals and full wire traces. Positive reads/writes and attempts are compared independently for every process capture, never pooled across captures. All received frames match; this does not imply syscall parity.

| Instrument | Positive read/write passes | Attempt passes | Processes |
| --- | ---: | ---: | ---: |
| socket | 8 | 14 | 64 |
| wire | 6 | 7 | 64 |

## Validation and provenance

- Core, engine, wire and RPC libraries: **passed** — 79 + 200 + 148 + 319 tests in the receive candidate (746 total). The final hash/alignment candidate also passed 319 RPC tests, the three wire receive tests in both default and unified core-unaligned configurations, and five core framing tests. The selected cache candidate passed 319 RPC tests and segment-pool ownership tests.
- Standalone portable verification: **passed** — All 86 library tests, plus the receiver lifetime compile-fail doctest.
- Miri: **passed-targeted** — Eight receive/storage/framing tests interpreted on the receive candidate. The full 188-execution hosted matrix was not run locally. Actual selected ownership test inventory built and enumerated: 47 tests, including 33 ownership tests.
- Allocation-free configurations: **passed** — Core, engine and wire no-default-features checks.
- Clippy and unsafe documentation: **passed** — Strict workspace all-targets Clippy and unsafe documentation gate passed on receive candidate; strict affected-crate Clippy passed after alignment/hash changes, and strict RPC Clippy after cache change. No unsafe source or exception allowance added.
- Deferred writer experiments: **passed-tests-rejected-performance** — Both variants passed all 320 RPC tests, including a new separate writer/lifecycle test and the existing immediate-failure regression. Their implementation is not in the selected source; both patches, binaries, profiles and five-run results are retained.
- Initial Clippy failure: **failed-corrected** — items_after_test_module in new receiving.rs; moved tests after implementation, then reran strict checks without a lint exception.
- Native samples and compatibility: **passed** — 480,000 primary samples, 240,000 separate confirmation samples and 120,000 samples for each of two scheduling experiments. Five independent runs per cell, unchanged fixture budgets, all samples retained. Primary statistics, fixture/binary hashes and order verified. All 240 mixed compatibility checks passed.
- Wire correctness: **passed** — 48 original-suite frame pairs and 228 directed topology TCP edges matched all received frames. Physical read/write parity remains unmet and is reported per process, separately for each instrument.
- Formatting, source replay and repository policy: **passed** — Formatting and diff checks; selected source patch reconstructs its recorded tree. Repository, 104 wiki documents and workflow dependency checks passed.
- Full coverage and full C++ correctness suites: **not-run-locally** — Fixed C++ benchmark interoperability was checked. No full coverage, full C++ suite or green hosted-CI claim.

The [source patch](source.patch) applies to `d8ed2001ee68d4e5bb0b36dc1e67e2dbe15aa259` and reconstructs `21dbdb37bba9f02a8d682f4c2d34e9a39abdac03`. Fixed C++ remains pinned at `0de72d8d8cec6b69edaa29de51d3bd490341f9c2`, built with clang 19. Frozen executable hashes and individual candidate source trees are recorded in report.json. Hosted checks for the new commit are separate from these local checks.

Evidence archive: `/workspace/capntproto/target/portable-receive-pooling-20261018`; 8,627 files, 993,538,666 logical bytes. SHA256SUMS SHA-256: `89e5f9da349e9faa1252ccfc6327e89cee8340a49ca49d005d1436a75b97aeeb`. Prior archives verified unchanged.
