# Portable ready dispatch and admission — October 19, 2026

Tracking: [gap #11](https://github.com/DericHuynh/capntproto/issues/11), [draft PR #12](https://github.com/DericHuynh/capntproto/pull/12).

**Portable beats C++ by both mean and pooled median in 13/24 cases in this five-run campaign.** The frozen e18a2e9 portable achieves 13/24 against the same C++ measurements. The universal target remains open. All 480,000 native samples are retained; none of the instrumented timings enter this comparison.

## Selected changes

The worker now uses the same immediate dispatch/completion path whether or not unrelated services are parked. After its first execution poll returns Pending, it waits for an operation wake instead of immediately polling that future again. Dispatch still reports route/cleanup progress, and every execution/completion step retains the cooperative budget charge.

Descriptor-free calls to an existing hosted target create a new answer without resolving or canceling older calls. When the control policy certifies that property and performs no protocol mutation, admission preserves parked worker readiness. A fully idle connection can admit the new call without rebuilding unrelated lane scans. Protocol resolution, cancellation, capability-bearing input, and control effects retain conservative invalidation.

Output capacity recovery now resumes the existing ready queue. Operations blocked on publication already retain their ready keys; flushing bytes does not make a sleeping service future ready. This removes a second broad invalidation that the real ingress regression exposed. There is no new public API change and no fixture change in this iteration.

## C++ references and rejected experiments

C++/KJ remains pinned to `0de72d8d8cec6b69edaa29de51d3bd490341f9c2`. [TaskSet::Task](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c++/src/kj/async.c++#L265) owns an explicit ready event, and [answer completion](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c++/src/capnp/rpc.c++#L3943) continues toward sending its Return. These support conserving operation readiness, rather than inferring readiness from unrelated protocol traffic.

The [C++ two-party writer](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c++/src/capnp/rpc-twoparty.c++#L175) uses evalLast to collect related messages before writing. A tested Rust runtime variant drained ready connections before flushing even while the top-level listener slept. In the four-service concurrent socket capture, service 0 fell from 123 writes to 27, versus C++ 28, and reads reached parity. However, the three concurrent service-chain native means regressed 40–57% against the frozen portable. That variant was reverted. Its instruction growth was much smaller than its latency regression; loss of overlap between hops is a hypothesis, not an established causal measurement.

A separate protected-execution variant made the executor the sole future owner and used a local completion channel instead of a shared future. It passed the RPC tests, but callback client instructions increased 3.97% while server instructions fell 1.24% against the batching variant. It was reverted. Returning unused worker quantum also failed to demonstrate a benefit and was reverted. An unqualified pipeline-guard change was discarded after inspection showed that Driver::take_output still waits for outgoing observers; it was neither compiled nor benchmarked. All measured candidates retain their binaries, source patches, raw results and decisions.

## Validation

The selected source passes **336 RPC library tests**, **nine TCP/Unix integration tests**, strict Clippy, formatting and source-patch replay. One regression covers widths 1/2/4 and budgets 1/2/4/8/64, checking first suspension and a selected wake. The other admits and returns an independent call while a sibling is parked, across borrowed/owned output, budgets 1/8/64, idle/busy maintenance, and a one-slot output queue. It checks the reply ID and payload, then termination and buffer reclamation. Both reproduced excess polls before their fixes. Existing cancellation, protected execution, early pipeline, stale wake, fairness and teardown tests remain passing. Full coverage, Miri and formal suites were not rerun.

## Five-run native comparison

Same-host TCP, five independent process runs per implementation/case, 1,000 measured samples each. Warmup is 100, or 10,000 for loopback. Variant order rotates/reverses and is not exactly balanced with five runs and four variants. Native timings ran without builds, tests or instrumentation. Legacy and Portable explicitly select different drivers in the selected executable; Before is frozen e18a2e9. Rust retains the existing fat-LTO, one-codegen-unit build configuration and checked overflow.

Times are microseconds: pooled arithmetic mean ± sample SD across the five run means (n−1), not confidence intervals. Per-run means/ranges, sample spread, tails, extrema and throughput are in report.json. Concurrent cases time a complete batch and service cases include every configured hop. A pass requires both mean and pooled median below C++.

| Case | Legacy mean ± SD | Before mean ± SD | Portable mean ± SD | C++ mean ± SD | Portable p50 / C++ | Both pass |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| unary-empty | 29.223 ± 9.747 | 29.797 ± 7.683 | 30.217 ± 10.258 | 29.502 ± 7.362 | 0.992 | no |
| unary-small | 29.815 ± 6.464 | 32.653 ± 5.883 | 32.327 ± 7.568 | 26.606 ± 2.033 | 1.447 | no |
| unary-1k | 30.207 ± 6.326 | 33.877 ± 5.389 | 34.419 ± 6.489 | 33.912 ± 8.152 | 1.023 | no |
| fanout-32 | 126.907 ± 24.058 | 150.844 ± 21.423 | 143.020 ± 17.022 | 179.741 ± 25.447 | 0.738 | yes |
| dependent-read | 68.208 ± 14.685 | 82.256 ± 16.464 | 86.780 ± 11.485 | 71.734 ± 11.843 | 1.129 | no |
| pipelined-read | 43.228 ± 8.546 | 66.660 ± 16.159 | 61.902 ± 9.398 | 40.417 ± 7.107 | 1.642 | no |
| write-read | 64.229 ± 10.825 | 60.770 ± 11.936 | 58.769 ± 9.430 | 65.335 ± 10.766 | 0.706 | yes |
| callback | 66.569 ± 14.955 | 77.638 ± 14.630 | 75.117 ± 11.518 | 67.707 ± 7.276 | 0.888 | no |
| bulk-64k | 70.088 ± 26.669 | 83.212 ± 36.225 | 77.256 ± 31.544 | 70.848 ± 26.558 | 1.135 | no |
| bulk-256k | 117.657 ± 4.841 | 133.588 ± 22.772 | 111.747 ± 6.425 | 305.260 ± 55.869 | 0.368 | yes |
| bulk-1m | 420.020 ± 39.285 | 404.826 ± 12.578 | 407.257 ± 7.176 | 1150.189 ± 162.106 | 0.365 | yes |
| bulk-4m | 2322.703 ± 90.693 | 2512.932 ± 109.399 | 2628.793 ± 238.645 | 5111.630 ± 250.481 | 0.498 | yes |
| loopback-0 | 26.252 ± 2.791 | 33.779 ± 5.095 | 29.233 ± 3.456 | 35.032 ± 7.331 | 0.689 | yes |
| loopback-64 | 28.045 ± 5.475 | 29.509 ± 4.582 | 30.332 ± 4.776 | 32.647 ± 4.639 | 0.698 | yes |
| loopback-1024 | 28.380 ± 6.479 | 29.598 ± 6.445 | 31.687 ± 6.019 | 29.431 ± 5.667 | 1.466 | no |
| loopback-65536 | 93.444 ± 35.838 | 86.289 ± 32.653 | 57.063 ± 5.285 | 59.936 ± 3.121 | 0.941 | yes |
| deep-8 | 27.833 ± 3.793 | 26.370 ± 2.341 | 27.906 ± 3.321 | 28.097 ± 4.739 | 0.996 | yes |
| deep-32 | 33.048 ± 5.797 | 31.009 ± 2.626 | 36.530 ± 6.845 | 32.723 ± 3.764 | 1.048 | no |
| concurrent-32 | 102.358 ± 14.738 | 126.466 ± 7.389 | 128.495 ± 10.228 | 147.564 ± 7.902 | 0.851 | yes |
| deep-32-concurrent-32 | 248.751 ± 26.708 | 287.593 ± 27.342 | 300.230 ± 24.271 | 293.230 ± 26.031 | 0.980 | no |
| services-4-deep-16 | 170.388 ± 21.657 | 182.927 ± 8.169 | 173.890 ± 5.658 | 150.300 ± 7.855 | 1.201 | no |
| services-4-deep-16-concurrent-32 | 1173.542 ± 34.866 | 996.255 ± 20.674 | 936.697 ± 48.190 | 1198.004 ± 7.146 | 0.769 | yes |
| services-4-deep-16-clients-8 | 1148.601 ± 20.186 | 1055.142 ± 14.484 | 905.955 ± 23.583 | 1084.952 ± 17.471 | 0.822 | yes |
| services-8-deep-16-clients-8 | 2503.118 ± 37.582 | 1940.238 ± 53.247 | 1814.324 ± 82.632 | 2340.246 ± 101.122 | 0.747 | yes |

These results are descriptive measurements from this host. Five repetitions and variable scheduling do not establish a universal or statistically bounded speedup. The regression tests independently establish that specific unnecessary future polls were removed.

## Instructions, simulated branches and socket calls

All 24 fixtures and every intermediate service have instruction profiles for all four implementations: 256 process captures. 43/64 portable captures use fewer whole-process instructions than Before. Profiles include startup, warmup and teardown; they are not per-request CPU timings. Across all processes in the three concurrent service chains, instructions fell approximately 11.0%, 7.4%, and 9.8%. Pipeline instructions were nearly unchanged, so its native timing improvement cannot be confidently attributed to this patch.

A separate gprofng native sampling attempt reported timer-period changes and collected too few samples for credible function rankings. Those rankings and all instrumented latencies are excluded; raw experiments and the rejection record are retained. Hardware branch counters remain unavailable: perf_event_open returned EPERM. The following are Callgrind simulations, using the retained script’s 32 KiB eight-way L1 I/D caches and 256 MiB direct-mapped last-level cache, with 64-byte lines. Binary layout can affect predictor aliasing.

| Case/process | Before branch misses | Portable branch misses | C++ branch misses | Instruction change vs before | Miss change vs before |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary-empty/client | 63,609 | 62,141 | 71,796 | +0.00% | -2.31% |
| unary-empty/server | 62,019 | 59,151 | 77,162 | -0.11% | -4.62% |
| pipelined-read/client | 326,411 | 325,296 | 150,203 | +0.01% | -0.34% |
| pipelined-read/server | 257,832 | 249,578 | 170,563 | +0.05% | -3.20% |
| callback/client | 208,467 | 204,829 | 148,216 | -0.04% | -1.75% |
| callback/server | 228,842 | 219,062 | 145,060 | -0.72% | -4.27% |

| Instrument | Positive read/write passes | Attempt passes | Both pass | Process comparisons |
| --- | ---: | ---: | ---: | ---: |
| socket | 42 | 42 | 42 | 64 |
| wire | 40 | 42 | 40 | 64 |

Each pass requires reads and writes individually no greater than C++ for the same process, retaining complete lifecycle traffic. Attempts also include nonpositive syscalls. All 240 mixed-implementation checks, 48 original received-frame checks and 304 directed topology-edge checks pass. Frame integrity does not imply socket-count parity.

## Remaining architecture work

Outgoing completion still spans WireCall, a reply-ID index, question/exchange storage and outgoing pipeline observers. A used pipeline cannot simply take its parent result before those observers settle. Bind observer settlement to the authoritative question owner before removing the general scan; relaxing the guard alone does not remove the dependency. Maintenance still discovers obligations through eighteen phases, and local progress still reactivates coarse coordinator lanes. Record obligations at the ownership transitions to reduce that repeated dispatch work.

The runtime batching experiment also shows that matching KJ’s write count alone is insufficient. Portable must reduce the work between transport phases while preserving useful overlap across connections. Deep forwarding profiles still spend substantial instructions traversing/validating and copying payloads. Any ownership redesign there must preserve capability-table bounds, traversal limits, retained-reader lifetimes and cancellation. These are unresolved architecture gaps, not measured gains in the selected patch. Legacy remains available as a reference; the performance issue and PR stay open.

## Reproduction and provenance

Apply source.patch to the base below. The evidence archive contains every raw native sample, complete socket/wire and instruction captures, candidate patches/binaries, build/test logs, analysis scripts and decision records. Source replay and binary/fixture hashes are verified. The runtime compiler cache was moved to a container-local tmpfs to work within disk limits; earlier sealed archives were not changed.

- Base: `e18a2e99f8c9366318795808115532de499603a8`; selected source tree: `028b535aa15eba478ebd5807e9940799f8de5829`.
- Source patch SHA-256: `fc28af6d315bf0a33d6faad5ad6a98987309f6b1821407b30d00f084d9514642`.
- Selected binary SHA-256: `45aba38004eedcbab94946b2e1fe8a4a0d4efae135ea42006c118b090c5efb13`.
- Archive: `/workspace/capntproto/target/portable-ready-dispatch-20261019`; 14,073 files, 1,118,090,946 logical bytes.
- Archive SHA256SUMS SHA-256: `610ee959ff5fde6bbbda8fdd5213d255e1ad635593380bb3d344bb91897fc64d`.
