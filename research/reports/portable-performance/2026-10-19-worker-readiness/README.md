# Portable worker readiness redesign — October 19, 2026

Tracking: [gap #11](https://github.com/DericHuynh/capntproto/issues/11), [draft PR #12](https://github.com/DericHuynh/capntproto/pull/12).

**The worker now preserves operation readiness instead of finding it by polling parked futures.** Across 720 diagnostic configurations, waking one service polls it once and polls no parked siblings, including when a broad application notification arrives at the same time. The preceding implementation needed 18–26 sibling polls with sixteen parked services at budget 64. These are work counts, not a latency speed ratio.

The universal performance target remains open. In the selected five-run TCP campaign, portable beats C++ by both mean and pooled median in **12/24** cases; the frozen previous portable achieves **12/24** against the same C++ measurements. Median-only C++ passes are **12/24**, versus **17/24** for the previous portable. Against that build, 11/24 means and 13/24 medians are lower.

## Implemented ownership and scheduling

The [worker](../../../../crates/capntproto-rpc/src/scoped/wire/server/worker.rs) retains accepted requests, results and execution stages. A separate [ready queue](../../../../crates/capntproto-rpc/src/scoped/wire/server/worker/ready.rs) owns only slot identities and notification metadata; it is not another protocol ledger. There is at most one queued key per live operation. A generation check rejects escaped wakers after slot reuse. Notifications hold weak references to the queue, so they cannot retain the driver after teardown.

Once selected, an operation advances through ready stages toward Return within the existing shared budget. It yields for a real dependency, budget exhaustion or a retryable error. Pending work registers its own waker. Retirement unlinks the key before dropping application owners. The FIFO and shared budget keep a self-waking service from starving another reply. Notifier allocations are reused only when no old observer remains. The queue uses a mutex for cross-thread-safe wakers and never calls the executor while holding that lock.

The coordinator separates application progress from protocol invalidation. Worker notifications retain operation identity even when coalesced with broad application wakes. Early pipeline notifications still reactivate dispatch and maintenance. Ingress and ControlContext explicitly report protocol changes; those changes retain a conservative fallback. Invalidation itself is constant-time. Subsequent protocol slot visits, including vacant slots, consume the worker budget. The rotating cursor survives repeated invalidation, and its span is fixed during admission growth so an old operation cannot be skipped.

**API change:** standalone Worker callers must call `protocol_changed()` after externally mutating their Driver and before polling the worker again. Spurious polls no longer recheck parked application work. Hosted execution handles this through ingress and control effects. There is no compatibility shim. The portable allocation-free core is unchanged; the queue belongs to the hosted std adapter.

## C++ comparison

C++/KJ source is pinned to `0de72d8d8cec6b69edaa29de51d3bd490341f9c2`. [TaskSet::Task](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c++/src/kj/async.c++#L265) registers its own ready event. [EventLoop::turn](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c++/src/kj/async.c++#L1828) removes the event before firing it. [Answer completion](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c++/src/capnp/rpc.c++#L3943) continues toward sendReturn directly. The redesign follows these scheduling principles while preserving Rust waker thread safety, bounded slots and generations.

The retained KJ probe executes one selected continuation and zero sibling continuations in all 60 trials. Rust future polls and KJ callbacks are different events; this is not a numerical language-speed comparison.

## Sparse readiness and validation

Widths 1/2/8/16, budgets 1/8/64, borrowed/owned output queues, first/middle/last selection, five repetitions, and with/without a coalesced broad application wake give 720 trials. Each checks exactly one Return, its question ID and payload, then terminates and verifies buffer reclamation. Duplicate selections at small widths are retained. The 360 configurations without the broad wake match the earlier audit.

| Parked services | Before sibling polls, budget 1 | Before, budget 8 | Before, budget 64 | Selected, every budget/wake mode |
| --- | ---: | ---: | ---: | ---: |
| 1 | 0 | 0 | 0 | 0 |
| 2 | 2–3 | 1–2 | 1–2 | 0 |
| 8 | 14–21 | 7–18 | 8–13 | 0 |
| 16 | 30–45 | 21–29 | 18–26 | 0 |

The selected source passes **334 RPC library tests**, **nine TCP/Unix integration tests**, strict Clippy, formatting and exact source-patch replay. Regressions cover stale/cross-thread wakes, parent replacement, self-wake fairness, protocol-scan growth/reinvalidation, cancellation of quiet methods, protected execution, early pipelines, capacity retries and teardown. The first queue iteration also passed 153 wire-library tests and 436 isolated feature-profile executions; those core/feature checks were not repeated for the subsequent hosted scheduling changes. Full local coverage, Miri and formal suites were not rerun.

The first queue-only iteration was fully measured: 8/24 mean-and-median C++ passes versus 11/24 for its frozen previous portable in that campaign. It was superseded because it requeued internal ready stages and eagerly marked protocol slots outside the budget. A continuation-only intermediate passed tests, but its release build was stopped after the latter issue was found; it has no benchmark qualification. Sources, logs and the first complete campaign remain archived.

## Five-run native comparison

Same-host TCP; five independent process runs per implementation/case, 1,000 measured samples each, 480,000 samples total. Warmup is 100, or 10,000 for loopback. Variant order rotates and reverses and is not exactly balanced with five runs and four variants. No builds, tests or instrumentation overlap native timing. Legacy and Portable explicitly select their respective drivers. Before is the frozen b352a21 implementation, whose runtime source is unchanged at base 990d6cd.

Times are microseconds: pooled mean ± sample SD across the five run means (n−1), not confidence bounds. Per-run ranges, sample spread, tails, extrema and throughput are retained in report.json and raw samples in the archive. Concurrent rows measure a complete batch; topology rows include every configured hop. A pass requires both mean and pooled median below C++.

| Case | Legacy mean ± SD | Before mean ± SD | Portable mean ± SD | C++ mean ± SD | Portable p50 / C++ | Both pass |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| unary-empty | 31.807 ± 6.557 | 32.008 ± 4.801 | 30.484 ± 3.769 | 29.583 ± 2.905 | 1.003 | no |
| unary-small | 32.934 ± 3.033 | 32.186 ± 5.013 | 32.659 ± 4.931 | 31.518 ± 4.182 | 1.030 | no |
| unary-1k | 31.345 ± 4.264 | 34.721 ± 1.490 | 31.820 ± 3.820 | 33.086 ± 2.576 | 1.014 | no |
| fanout-32 | 136.429 ± 16.062 | 146.027 ± 18.673 | 170.490 ± 6.111 | 192.561 ± 20.509 | 0.832 | yes |
| dependent-read | 75.969 ± 4.031 | 88.398 ± 15.776 | 90.670 ± 12.985 | 75.779 ± 3.217 | 1.089 | no |
| pipelined-read | 48.212 ± 6.990 | 61.904 ± 19.038 | 77.893 ± 16.057 | 46.862 ± 6.543 | 1.816 | no |
| write-read | 65.647 ± 3.115 | 57.396 ± 6.506 | 69.323 ± 5.073 | 64.378 ± 5.786 | 1.038 | no |
| callback | 68.755 ± 8.298 | 91.263 ± 14.461 | 89.756 ± 11.931 | 66.490 ± 7.660 | 1.384 | no |
| bulk-64k | 63.752 ± 3.996 | 60.514 ± 2.912 | 60.833 ± 4.323 | 63.900 ± 4.001 | 1.125 | no |
| bulk-256k | 120.822 ± 7.896 | 132.922 ± 12.761 | 136.977 ± 28.917 | 374.339 ± 65.983 | 0.341 | yes |
| bulk-1m | 429.566 ± 12.742 | 457.437 ± 32.617 | 455.130 ± 39.586 | 1136.090 ± 91.429 | 0.378 | yes |
| bulk-4m | 2558.053 ± 174.258 | 2764.835 ± 206.943 | 2799.444 ± 245.933 | 5626.541 ± 214.889 | 0.478 | yes |
| loopback-0 | 29.356 ± 4.303 | 27.578 ± 4.099 | 30.620 ± 5.179 | 30.895 ± 6.096 | 0.755 | yes |
| loopback-64 | 29.635 ± 7.888 | 27.439 ± 4.758 | 32.252 ± 4.732 | 30.594 ± 5.353 | 1.022 | no |
| loopback-1024 | 32.115 ± 3.968 | 33.543 ± 4.178 | 33.057 ± 7.070 | 30.910 ± 2.885 | 1.465 | no |
| loopback-65536 | 63.334 ± 3.222 | 64.611 ± 5.430 | 58.583 ± 4.713 | 62.665 ± 2.946 | 0.934 | yes |
| deep-8 | 29.331 ± 2.052 | 32.086 ± 7.592 | 28.240 ± 2.268 | 31.769 ± 4.361 | 0.976 | yes |
| deep-32 | 34.826 ± 6.049 | 38.166 ± 4.241 | 36.151 ± 7.106 | 36.941 ± 7.330 | 0.986 | yes |
| concurrent-32 | 122.471 ± 13.573 | 151.847 ± 18.214 | 154.408 ± 15.800 | 196.584 ± 24.453 | 0.727 | yes |
| deep-32-concurrent-32 | 328.254 ± 30.882 | 324.439 ± 48.401 | 364.859 ± 26.496 | 351.989 ± 33.216 | 1.020 | no |
| services-4-deep-16 | 202.111 ± 52.289 | 229.044 ± 57.381 | 224.778 ± 46.214 | 181.446 ± 31.257 | 1.276 | no |
| services-4-deep-16-concurrent-32 | 1343.901 ± 103.987 | 1288.620 ± 123.361 | 1198.473 ± 152.022 | 1369.165 ± 87.835 | 0.857 | yes |
| services-4-deep-16-clients-8 | 1386.958 ± 105.363 | 1322.588 ± 220.891 | 1226.906 ± 97.004 | 1290.770 ± 109.848 | 0.942 | yes |
| services-8-deep-16-clients-8 | 3207.298 ± 637.431 | 2665.327 ± 472.785 | 2763.617 ± 713.857 | 2878.981 ± 149.935 | 0.830 | yes |

These measurements do not establish a universal or statistically bounded latency improvement. Sparse-readiness work conservation is independently demonstrated; existing TCP fixtures do not deliberately keep N−1 services parked while releasing one controlled completion. A network fixture for that pattern remains useful before attributing an end-to-end speedup to the readiness redesign.

## Instruction, branch and I/O diagnostics

Every fixture and intermediate service has an instruction profile for all four implementations: 256 process captures. 18/64 portable process captures use fewer instructions than the frozen previous portable. These separate diagnostic runs never enter native timing statistics.

The following are whole-process Callgrind branch/cache simulations, including startup, warmup and teardown. Hardware branch counters returned EPERM. Cache configuration is 32 KiB eight-way L1 I/D, 64-byte lines, and 256 MiB direct-mapped last-level cache. Function and line branch totals are verified against process totals; binary layout can change predictor aliasing.

| Case/process | Before branch misses | Portable branch misses | C++ branch misses | Instruction change vs before | Miss change vs before |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary-empty/client | 64,368 | 63,617 | 71,793 | +0.04% | -1.17% |
| unary-empty/server | 59,007 | 62,015 | 77,160 | +1.38% | +5.10% |
| pipelined-read/client | 322,108 | 326,636 | 150,202 | +0.11% | +1.41% |
| pipelined-read/server | 247,957 | 257,833 | 170,561 | +1.13% | +3.98% |
| callback/client | 198,820 | 208,462 | 148,212 | +0.68% | +4.85% |
| callback/server | 219,620 | 228,838 | 145,058 | +0.40% | +4.20% |

| Instrument | Positive read/write passes | Attempt passes | Both pass | Process comparisons |
| --- | ---: | ---: | ---: | ---: |
| socket | 43 | 43 | 43 | 64 |
| wire | 40 | 40 | 40 | 64 |

Each pass requires both reads and writes individually no greater than C++ for the same process. Attempts include nonpositive syscalls. All 240 mixed-implementation checks, 48 original received-frame checks and 304 directed topology-edge checks pass. Frame integrity does not imply read/write parity. In both capture modes, pipeline, write/read, callback and fanout satisfy read/write-count parity with C++ on both peers. Their remaining latency gaps therefore cannot be explained simply by extra socket calls in those captures; ownership, dispatch, framing and scheduling work still matter.

## Remaining architecture work

Hosted ownership already prevents arbitrary concurrent mutation of the consumed coordinator; public fields alone do not force broad hosted scans. The remaining conservative protocol path should be replaced by explicit affected-answer keys as ingress/control producers are converted. Outgoing Calls and foreign promises still use scans, and local progress still reactivates coarse coordinator lanes.

Synchronous dispatch still chooses its executor path based on whether the worker has only one live operation. Ready turns make a unified dispatch/completion path worth testing next, with protected execution and streaming policy preserved; it is not a measured speedup in this change. Next, bind completion directly to the authoritative question/exchange owner instead of the separate reply-ID index and optional WireCall route fields. Preserve pre-question invocations and canceled-question tombstones. Then record Finish, Release, retirement and pipeline obligations at their owner transitions instead of rediscovering them through eighteen maintenance phases. Finally, select one explicit transport input/output contract at connection setup. These remain design work, not measured speedups.

The preceding commit’s [CI run](https://github.com/DericHuynh/capntproto/actions/runs/39201542510) failed the coverage job; its [dedicated benchmark job](https://github.com/DericHuynh/capntproto/actions/runs/39201536594) succeeded. Those results do not validate this candidate or establish universal performance. Coverage floors were not changed. The performance gap and draft PR remain open; legacy remains a reference.

## Reproduction and provenance

Apply source.patch to the base commit and use the frozen source hashes in report.json. Run the sparse regression with `cargo test --locked -p capntproto-rpc --lib single_service_wake_never_polls_parked_siblings -- --nocapture`. The archive retains scripts, fixture hashes, exact source patches, binaries, every raw native sample, complete I/O profiles, branch rankings and validation logs. The first iteration’s raw Callgrind files and retained test executables are losslessly gzip-compressed; compression.json maps original paths and hashes to their compressed files. Restore those paths in a fresh workspace before rerunning the corresponding analysis scripts. A release link failed with SIGBUS under tight disk space; unchanged source was retried after compression. Rebuildable compiler caches were reclaimed without modifying earlier sealed evidence.

- Source base: `990d6cdedeb400c6c2095f160d40165720333f44`; selected source tree: `7a0f214371e95457ff2fd21f17ce8948c914a146`.
- Patch SHA-256: `9b2017cbf9462f006559609618736a162c8837a0fc751e1ece02c86245a48ab7`.
- Selected executable SHA-256: `217ee23a2eba8fd56d65772bf899d9525efa7d28fea91c30491c72e0732aa021`.
- Frozen previous portable SHA-256: `b42fa1aed5643695d3bd31897850418cec34ee76f37883e0abfe0bc03198322c`.
- Archive: `/workspace/capntproto/target/portable-worker-readiness-20261019`; 12,982 files; 759,194,785 logical bytes.
- SHA256SUMS SHA-256: `e639732c94af2564e3a118c4ef8f05de722f5550abc7e6caeae328c816d13f06`.
