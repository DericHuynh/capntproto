# Portable direct paths — 2026-10-23

This redesign removes the shared pipeline driver and eager result-hook path, and changes readiness ownership so ordinary local operations do not lock the entire queue. It follows legacy's direct completion model. Native results and simulated CPU reductions are reported separately below; the remaining legacy gap is still open.

TLS: full-suite geometric mean latency changes -5.15% versus the original portable baseline; the longer focused comparison changes -3.31%. Portable remains +23.48% above legacy in the focused comparison.

QUIC: full-suite geometric mean latency changes -1.79% versus the original portable baseline; the longer focused comparison changes -6.15%. Portable remains +29.13% above legacy in the focused comparison.

## Architecture changes

1. **The producer completes the pipeline.** Public response and pipeline interests retain one completion state. Return, local completion, failure and shutdown publish an unresolved pipeline before waking the response. The former shared response future, separate result cell, and recursive polling of the parent's driver are removed. Projected children subscribe directly to endpoint binding; this preserves wakeup and failure delivery when nobody polls the parent response.
2. **Pipelined results take the direct payload path.** Prepared requests keep a lightweight result owner and attach their existing wire pipeline at admission. General `ResultsHook` objects are materialized only when local/custom hook code needs them. A received shared payload goes directly into response ownership. A used pipeline can now attempt completion through its known question owner; the protocol's observer fence still defers it when ordering requires that.
3. **Readiness belongs to the local driver.** The FIFO and slot generations live in local state. Asynchronous wakers use a bounded, coalesced mailbox. A poll guard records self-wakes: successful completion discards them, while suspension republishes them. Outgoing calls start parked, avoiding insert-then-remove queue work. The old internal queue API is removed and every consumer migrated. Waking, entering/leaving a poll and retiring an owner share one atomic transition, and insertion synchronizes in-flight mailbox entries before generation reuse.

The mailbox still uses a mutex, including asynchronous wakes originating on the same thread. Installation synchronizes with that mailbox. This is not a claim of lock-free execution. Local queue operations avoid that lock when no notifications are pending. No compatibility shim was added; generated public clients continue to work naturally. No unsafe code is introduced.

Relevant code: [response ownership](../../../../crates/capntproto-rpc/src/scoped/response.rs), [wire payload and projections](../../../../crates/capntproto-rpc/src/scoped/wire.rs), [direct Return lookup](../../../../crates/capntproto-rpc/src/scoped/wire/calls.rs), and [readiness](../../../../crates/capntproto-rpc/src/scoped/wire/ready.rs).

Legacy's `QuestionRef::fulfill_result()` settles pipeline state at receipt, then sends the response. C++ `handleReturn()` looks up the question and fulfills its owner with an owned response ([legacy](../../../../crates/capntproto-rpc/src/rpc.rs), [C++](../../../../vendor/capnproto/c++/src/capnp/rpc.c++)). These are the reference paths for this change. The portable engine retains its resource-admission, authorization and pipeline-ordering checks.

## Method

Baseline: `acc0f74f9a6c19da3e45f0d7248dcdf55fe977d0`. A fresh build is byte-identical to the prior frozen portable executable. Every candidate uses Rust 1.97, Clang 19, fat LTO, one codegen unit, release debug level 1 and overflow checks. Benchmarks use unchanged encrypted fixtures, pinned physical CPUs and rotating variant order. Builds, tests, native timing, tracing, profiling and compression run serially.

All native comparisons have five process repetitions. Full qualification uses 100 warmups/1,000 measured iterations; the longer focused campaign uses 1,000/5,000. Values are microseconds ± sample SD of the five run means. Concurrent batches count as whole fixture operations. JSON retains run means, spread, ranges and percentiles. C++ supports the equivalent TLS fixtures; C++ QUIC is unsupported.

Callgrind simulates both peers over their complete lifecycles, with 10 warmups/250 iterations, including handshakes and shutdown. These counters are not hardware PMU results or native timing. Five-repeat socket traces use 2 warmups/10 iterations and likewise include ciphertext, handshake, control and recovery traffic.

## Full repeated comparison

### TLS

| Reference | Lower portable means | Geometric mean latency change |
|---|---:|---:|
| portable-baseline | 16/24 | -5.15% |
| legacy | 7/24 | +6.84% |
| cpp | 24/24 | -31.68% |

| Case | Baseline | Portable | Legacy | C++ | Δ baseline |
|---|---:|---:|---:|---:|---:|
| loopback-0 | 37.23 ± 15.34 | 34.66 ± 6.87 | 28.75 ± 7.55 | 45.55 ± 7.76 | -6.92% |
| loopback-64 | 37.52 ± 5.79 | 37.70 ± 4.67 | 36.85 ± 3.51 | 46.87 ± 7.12 | +0.48% |
| loopback-1024 | 40.53 ± 3.29 | 38.03 ± 5.92 | 37.47 ± 4.49 | 60.20 ± 2.69 | -6.16% |
| loopback-65536 | 133.76 ± 17.64 | 141.86 ± 33.17 | 122.26 ± 16.87 | 170.68 ± 21.70 | +6.05% |
| unary-empty | 38.72 ± 4.24 | 36.96 ± 6.61 | 36.96 ± 2.48 | 52.75 ± 4.59 | -4.55% |
| unary-small | 33.01 ± 6.64 | 30.40 ± 6.15 | 33.50 ± 4.24 | 48.87 ± 8.32 | -7.90% |
| unary-1k | 53.79 ± 41.50 | 38.94 ± 10.06 | 37.50 ± 5.40 | 45.43 ± 8.70 | -27.61% |
| fanout-32 | 199.13 ± 29.41 | 183.97 ± 29.26 | 154.81 ± 18.23 | 389.07 ± 76.69 | -7.61% |
| dependent-read | 97.43 ± 22.96 | 97.84 ± 13.31 | 72.43 ± 14.32 | 114.32 ± 11.26 | +0.41% |
| pipelined-read | 73.35 ± 21.60 | 70.29 ± 19.67 | 52.22 ± 11.47 | 76.12 ± 16.12 | -4.17% |
| write-read | 73.48 ± 9.05 | 72.31 ± 7.70 | 66.45 ± 5.94 | 95.21 ± 12.19 | -1.59% |
| callback | 93.61 ± 10.79 | 85.98 ± 21.28 | 65.36 ± 11.52 | 91.69 ± 16.23 | -8.15% |
| bulk-64k | 118.62 ± 17.09 | 121.41 ± 13.66 | 124.29 ± 16.08 | 155.97 ± 3.94 | +2.35% |
| bulk-256k | 355.87 ± 70.58 | 273.71 ± 35.97 | 283.20 ± 43.28 | 679.26 ± 171.50 | -23.09% |
| bulk-1m | 915.31 ± 21.01 | 961.01 ± 53.25 | 1100.97 ± 169.41 | 2096.02 ± 119.38 | +4.99% |
| bulk-4m | 4902.61 ± 194.11 | 4872.98 ± 257.32 | 4708.85 ± 250.13 | 9558.04 ± 392.83 | -0.60% |
| deep-8 | 39.37 ± 6.05 | 38.18 ± 7.54 | 36.58 ± 6.32 | 50.49 ± 8.29 | -3.03% |
| deep-32 | 38.36 ± 1.98 | 38.03 ± 3.32 | 38.51 ± 3.43 | 46.74 ± 2.66 | -0.87% |
| concurrent-32 | 143.63 ± 10.07 | 145.32 ± 23.63 | 106.37 ± 2.62 | 386.93 ± 51.11 | +1.18% |
| deep-32-concurrent-32 | 383.43 ± 34.07 | 368.11 ± 19.72 | 360.04 ± 37.35 | 471.60 ± 16.71 | -4.00% |
| services-4-deep-16 | 402.13 ± 51.12 | 327.42 ± 63.63 | 294.70 ± 42.74 | 427.10 ± 48.03 | -18.58% |
| services-4-deep-16-concurrent-32 | 1483.01 ± 29.45 | 1566.70 ± 72.72 | 1429.31 ± 186.65 | 2040.38 ± 132.49 | +5.64% |
| services-4-deep-16-clients-8 | 1459.58 ± 307.45 | 1292.99 ± 83.89 | 1485.07 ± 134.05 | 1984.81 ± 243.57 | -11.41% |
| services-8-deep-16-clients-8 | 2538.04 ± 68.01 | 2564.69 ± 143.90 | 2889.76 ± 81.48 | 3884.04 ± 327.70 | +1.05% |

### QUIC

| Reference | Lower portable means | Geometric mean latency change |
|---|---:|---:|
| portable-baseline | 16/24 | -1.79% |
| legacy | 5/24 | +8.39% |

| Case | Baseline | Portable | Legacy | C++ | Δ baseline |
|---|---:|---:|---:|---:|---:|
| loopback-0 | 52.17 ± 11.05 | 48.35 ± 13.76 | 45.66 ± 11.48 | unsupported | -7.32% |
| loopback-64 | 47.21 ± 12.75 | 46.27 ± 12.12 | 36.59 ± 4.55 | unsupported | -2.01% |
| loopback-1024 | 49.03 ± 11.09 | 49.93 ± 10.64 | 48.41 ± 9.36 | unsupported | +1.84% |
| loopback-65536 | 144.73 ± 24.12 | 159.43 ± 23.67 | 153.15 ± 11.05 | unsupported | +10.15% |
| unary-empty | 47.36 ± 11.28 | 39.53 ± 2.37 | 49.69 ± 10.60 | unsupported | -16.53% |
| unary-small | 49.38 ± 13.93 | 62.52 ± 19.01 | 43.95 ± 12.21 | unsupported | +26.62% |
| unary-1k | 58.59 ± 9.47 | 56.12 ± 11.27 | 50.70 ± 9.89 | unsupported | -4.22% |
| fanout-32 | 225.13 ± 29.74 | 226.96 ± 36.29 | 206.59 ± 24.02 | unsupported | +0.81% |
| dependent-read | 128.19 ± 19.58 | 121.97 ± 17.30 | 116.35 ± 20.92 | unsupported | -4.85% |
| pipelined-read | 89.41 ± 14.96 | 84.19 ± 5.14 | 61.65 ± 13.90 | unsupported | -5.85% |
| write-read | 86.58 ± 15.49 | 79.55 ± 6.96 | 74.80 ± 13.59 | unsupported | -8.13% |
| callback | 133.83 ± 28.01 | 130.00 ± 15.16 | 95.48 ± 17.85 | unsupported | -2.86% |
| bulk-64k | 145.37 ± 21.47 | 142.13 ± 24.93 | 124.01 ± 7.60 | unsupported | -2.23% |
| bulk-256k | 379.32 ± 19.17 | 383.09 ± 37.31 | 352.98 ± 36.65 | unsupported | +0.99% |
| bulk-1m | 1603.47 ± 84.38 | 1569.86 ± 82.20 | 1577.58 ± 123.93 | unsupported | -2.10% |
| bulk-4m | 6382.49 ± 276.15 | 6244.64 ± 445.99 | 6181.59 ± 102.49 | unsupported | -2.16% |
| deep-8 | 59.35 ± 12.84 | 46.09 ± 13.82 | 41.57 ± 7.06 | unsupported | -22.34% |
| deep-32 | 57.58 ± 16.14 | 59.03 ± 14.62 | 51.47 ± 11.14 | unsupported | +2.52% |
| concurrent-32 | 180.05 ± 22.50 | 191.65 ± 27.79 | 163.06 ± 27.01 | unsupported | +6.44% |
| deep-32-concurrent-32 | 411.81 ± 34.49 | 395.12 ± 29.55 | 372.10 ± 46.11 | unsupported | -4.05% |
| services-4-deep-16 | 277.14 ± 50.32 | 298.75 ± 56.28 | 328.03 ± 25.36 | unsupported | +7.80% |
| services-4-deep-16-concurrent-32 | 1805.96 ± 74.90 | 1798.95 ± 55.93 | 1625.32 ± 43.30 | unsupported | -0.39% |
| services-4-deep-16-clients-8 | 1698.57 ± 173.91 | 1634.16 ± 157.04 | 1666.38 ± 70.28 | unsupported | -3.79% |
| services-8-deep-16-clients-8 | 3420.57 ± 338.06 | 3359.24 ± 277.37 | 3904.65 ± 445.76 | unsupported | -1.79% |

## Longer focused confirmation

### TLS
| Case | Baseline | Portable | Legacy | C++ | Δ baseline |
|---|---:|---:|---:|---:|---:|
| fanout-32 | 186.85 ± 16.66 | 193.48 ± 9.33 | 145.33 ± 12.49 | 349.58 ± 19.79 | +3.55% |
| dependent-read | 83.47 ± 11.96 | 78.29 ± 11.06 | 69.27 ± 11.23 | 100.21 ± 4.56 | -6.21% |
| pipelined-read | 68.46 ± 4.60 | 65.06 ± 5.46 | 47.41 ± 7.10 | 70.57 ± 6.87 | -4.96% |
| deep-32-concurrent-32 | 382.75 ± 20.94 | 362.40 ± 12.27 | 321.89 ± 14.67 | 506.98 ± 26.52 | -5.32% |

### QUIC
| Case | Baseline | Portable | Legacy | C++ | Δ baseline |
|---|---:|---:|---:|---:|---:|
| fanout-32 | 230.98 ± 10.18 | 213.33 ± 15.11 | 189.52 ± 9.77 | unsupported | -7.64% |
| dependent-read | 126.36 ± 13.45 | 125.27 ± 10.72 | 93.21 ± 13.91 | unsupported | -0.86% |
| pipelined-read | 105.58 ± 11.41 | 98.04 ± 13.74 | 57.12 ± 9.17 | unsupported | -7.14% |
| deep-32-concurrent-32 | 460.75 ± 24.89 | 420.38 ± 27.55 | 392.52 ± 27.53 | unsupported | -8.76% |

## Per-step checkpoints

Each checkpoint follows its source replacement and uses five repetitions of 100 warmups/1,000 iterations. `completion` compares with the original baseline; `payload` compares with `completion`; `ready` compares with `payload`. The intermediate `ready` prototype was replaced after concurrency review by the final unified-state implementation. The final column uses the longer cumulative comparison against the original baseline. Percentages are observed latency changes; negative means lower latency. Mixed results and overlapping spreads prevent attributing every change in latency to the source change. Full means and spreads are in [comparisons.json](comparisons.json).

| Transport / case | completion | payload | ready prototype | final, longer |
|---|---:|---:|---:|---:|
| tls / fanout-32 | +3.60% | -6.58% | -1.15% | +3.55% |
| tls / dependent-read | -2.50% | -13.06% | -1.65% | -6.21% |
| tls / pipelined-read | +10.29% | +12.35% | +2.06% | -4.96% |
| tls / deep-32-concurrent-32 | +5.69% | +1.60% | -5.05% | -5.32% |
| quic / fanout-32 | +1.06% | +3.77% | +6.25% | -7.64% |
| quic / dependent-read | -5.31% | -18.38% | +5.65% | -0.86% |
| quic / pipelined-read | -7.66% | -6.35% | -21.53% | -7.14% |
| quic / deep-32-concurrent-32 | -4.65% | +3.75% | +2.07% | -8.76% |

## CPU profiles

Whole-lifecycle simulated changes from the original portable baseline:

| Transport / case | Instructions | Conditional misses | D1 read misses | D1 write misses |
|---|---:|---:|---:|---:|
| tls / fanout-32 | -2.66% | -10.13% | -4.49% | -4.28% |
| tls / pipelined-read | -2.09% | -5.74% | +0.30% | -0.75% |
| quic / pipelined-read | -1.42% | -3.79% | -1.15% | +0.45% |

[profiles.json](profiles.json) contains complete totals for intermediate and final portable, legacy and C++ profiles. [profile-attribution.json](profile-attribution.json) records exclusive source attribution, including inlining; it is not a complete component cost or an estimate of removable overhead. TLS fanout and pipeline, plus QUIC pipeline, are the final CPU-profile scope. The TLS legacy profiles use the intermediate `payload` executable; legacy source is unchanged. Native final comparisons and the QUIC legacy profile use `direct`.

The fanout profile attributes 6,237,279 instructions to coordinator `work.rs` before the change and 3,350,276 afterward. That supports reduced coordinator revisits. The tradeoff is visible in pipeline readiness attribution, which rises from 199,463 to 254,380 instructions even though the whole pipeline run falls by 2.09%. Local bookkeeping is not uniformly cheaper; the benefit comes from removing work across the completion path.

## Ciphertext reads and writes

A case meets this strict criterion only when none of its four mean counts is higher: positive writes, positive reads, write attempts and read attempts. All means/SD/min/max are in [ciphertext-counts.json](ciphertext-counts.json), with exceptions in [io-comparison.json](io-comparison.json).

| Comparison | No higher mean in all four metrics |
|---|---:|
| tls-vs-portable-baseline | 20/24 |
| tls-vs-legacy | 20/24 |
| tls-vs-cpp | 19/24 |
| quic-vs-portable-baseline | 15/24 |
| quic-vs-legacy | 1/24 |

Focused positive write/read counts (mean ± SD, whole peer lifecycle):

| Transport / case | Baseline | Portable | Legacy | C++ |
|---|---:|---:|---:|---:|
| tls / fanout-32 | 30.0 ± 0.0 / 173.0 ± 0.0 | 30.0 ± 0.0 / 173.0 ± 0.0 | 30.0 ± 0.0 / 173.0 ± 0.0 | 128.0 ± 0.0 / 126.8 ± 0.4 |
| tls / pipelined-read | 30.0 ± 0.0 / 29.0 ± 0.0 | 30.0 ± 0.0 / 29.0 ± 0.0 | 30.0 ± 0.0 / 29.0 ± 0.0 | 32.0 ± 0.0 / 31.0 ± 0.0 |
| quic / fanout-32 | 35.0 ± 0.0 / 87.0 ± 0.0 | 35.0 ± 0.0 / 87.0 ± 0.0 | 32.6 ± 0.5 / 84.6 ± 0.5 | unsupported |
| quic / pipelined-read | 35.0 ± 0.0 / 39.0 ± 0.0 | 35.0 ± 0.0 / 39.0 ± 0.0 | 32.6 ± 0.5 / 36.6 ± 0.5 | unsupported |

These are socket syscall counts, not RPC frame counts. QUIC counts vary with packet scheduling and recovery. Universal IO parity is not established by overlapping spread.

## Validation

Passed: **733 core tests**, **86 fixed-storage conversations**, **89 growing-storage conversations**, **36 targeted integrations**, and **206 encrypted interoperability/authentication checks**. Embedded checking, strict all-target Clippy for six RPC/consumer crates and formatting pass.

New lifecycle tests cover producer-driven unpolled pipelines, pipeline-only interest, final-interest cancellation, shutdown and early publication. Readiness tests cover batching, stale generations, synchronous self-wake elimination, wakeup during poll entry, and wakeup racing poll exit. Existing pipeline-ordering, capacity, cancellation, fairness and cross-thread wake tests also pass. [validation.json](validation.json) lists exact scope and corrected prototype failures. Coverage, whole root/API suites, complete formal proof runs and hardware PMU profiling were not rerun.

## Remaining architectural cost

Portable still crosses protocol admission, wire-call ownership and hosted response ownership. Removing shared futures and queue detours reduces some of that work; it does not collapse those layers into legacy's question-to-fulfiller path. These results must not be read as universal superiority over legacy or C++. The next larger opportunity is to fuse more ordinary question completion with its hosted owner while preserving bounded resource release and pipeline ordering. Deleting those obligations would change correctness, independently of API compatibility.

## Evidence

[evidence-index.json](evidence-index.json) verifies the five frozen stage binaries, patches and complete source manifests. Final production source matches `direct`. Earlier sealed evidence remains unchanged. The archive contains raw trials, ciphertext traces, profiles, source overlays, commands, toolchain/build settings and validation logs. Certificates are generated local test fixtures. [archive.json](archive.json) records the archive hash and full file-inventory verification.
