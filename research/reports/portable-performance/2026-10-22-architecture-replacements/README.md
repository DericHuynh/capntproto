# Portable architecture replacements — 2026-10-22

This completes the six larger replacements from the [architecture review](../2026-10-22-architecture-review/README.md), building on the earlier [partial implementation](../2026-10-22-architecture-implementation/README.md). The former discovery paths are removed from normal progress. This is an architectural completion, not a claim that portable meets every performance target. Legacy remains a benchmark and interoperability control.

The implementation is **not performance-qualified**: the full repeated campaign has observed geometric-mean latency increases of **4.96% for TLS and 4.11% for QUIC** versus the pre-replacement portable. The longer confirmation retains fanout and deep/concurrent regressions. These changes complete the requested ownership and API replacements; they do not meet the review’s additional no-regression performance criterion or the earlier universal speed targets.

Base commit: `52de546a350a30ff1e6878eab32a33c3964a0b93`. The retained executable is `turn1`, SHA-256 `5639eca2ef60182b48471fefa104360aa93ddebb51f5967f3dcba9c0b4d5d021`. Source and binary manifests preserve every measured stage. No compatibility layer preserves the previous portable mutation API.

## Replacements

1. **Operations and projections.** A branded durable operation ID survives queued admission, publication, completion and cancellation. Explicit routes replace combinations of optional state. Questions and invocations own their observer lists and completion bindings; a Return visits that owner's observers. Hosted Calls use exact ready queues. Public projections reference the operation and normalized path directly, memoize equivalent paths and acquire their scoped endpoint without the former generic queued-client bridge. Foreign/local hooks retain their necessary adapter. Pre-publication operations, wire questions and canceled application interests retain distinct lifetimes.
2. **Readiness, resources and cleanup.** Authoritative transitions enqueue exact dispatch, observer, retirement, cancellation, Finish and Release obligations. Sixteen actual work classes replace the 18-phase maintenance scan, idle proofs and invalidation API. A fair work-class cursor remains; it selects nonempty queues rather than searching capacity-sized owner stores. Blocked owners wait for their relevant resource threshold or producer event. Budget-one fairness, rollback, smaller ready demands behind larger blocked demands and generation reuse are tested. Standalone Driver internals are private; controlled capability scopes and ledger transactions emit the required notifications. Caller-supplied fixed storage remains allocation-free.
3. **Hosted framing and output.** Required framed input/output and a prepublication reservation feed one physical FIFO. The hosted engine-to-transport FIFO handoff is removed. Queue acceptance, local flush completion and peer completion remain separate. The runtime batches ready application and protocol work for at most 32 rounds before polling output; output-only wakes do not prolong progress. Coordinator call-shape lookahead, parameter-size traversal, cleanup preferences and overlapping deferred-flush policy are removed. Storage-backed standalone byte IO retains an explicit bounded input-turn boundary.
4. **Capability changes.** Full-generational hook bindings, graph subscriptions and explicit metadata sources replace refresh-vector and weak-retirement scans. Foreign futures, abandonment, answer projections and protocol-driven worker changes notify their exact owner. Retained original hooks preserve membranes, late FD metadata, introductions and reentrant cleanup. Incoming and outgoing observer registries have affine ownership so an independent registry cannot reinterpret another registry's slot indices.
5. **Network control.** The immutable TwoParty/Network contract from the previous implementation remains. Ordinary two-party turns omit multiparty state; supported introductions, handoffs and network-mode policies keep their full handler. This stage needed no second implementation.
6. **QUIC output turns.** The physical writer ends an output turn through stream flush. A deduplicated transport notification records pending work; raw stream writes, input, recovery and flow control retain independent progress. The bounded executor-turn coalescing fence remains: prior attempts to remove it increased packet counts. This replacement removes redundant notifications and gives output an explicit contract; it does not claim that the fence or QUIC lifecycle overhead disappeared.

No universal “scan-free” claim is made: bounded class selection, slot allocation, shutdown traversal, actual affected-owner lists and user payload traversal still perform necessary iteration. Engine, wire and hosted ownership boundaries remain.

## Qualification and interpretation

Every meaningful implementation checkpoint has a frozen binary, source snapshot and five-repeat focused comparison. Native campaigns use separate physical CPU cores, rotating variant order and authenticated TLS 1.3 or QUIC v1. They run without concurrent builds, tests, tracing, profiling or archive compression. Each full cell has 100 warmups and 1,000 measured operations; the longer focused confirmation has 1,000 warmups and 5,000 operations. Means below are microseconds, with sample SD of the five run means. JSON retains individual run means, min/max and pooled p50/p95/p99.

Per-stage controls are the preceding frozen portable candidate and the prior frozen legacy. The final full/long comparison uses current legacy and portable from the same retained executable, with the pre-replacement portable (`secure53/step5`) as the cumulative control. C++ uses pinned revision `0de72d8d8cec6b69edaa29de51d3bd490341f9c2` with the equivalent encrypted fixtures. **C++ QUIC is unsupported**; there is no plaintext or TCP substitute in QUIC tables. Dependent read waits for the returned capability; pipelined read sends through its unresolved parent. Multi-service cases encrypt every connection.

The `batch1` checkpoint exposed a real batching regression: five-repeat TLS fanout/deep traces rose from 30 to 90 successful lifecycle writes. The retained `turn1` runtime restored 30 writes in both cases. Those failed checkpoint measurements remain in the evidence. A stale `calls1-invalid-stale` binary is explicitly excluded: it was identical to `worker1` after a failed build and is not evidence for the Calls replacement.

## Remaining gaps

- Portable is not universally faster than legacy or C++, and the prior percentage targets remain open in [gap #11](https://github.com/DericHuynh/capntproto/issues/11).
- Same-peer projected parameter capabilities retain their authoritative route but still publish through an export as SenderHosted or SenderPromise. C++ can emit ReceiverAnswer directly. Integrating the existing reflected-publication transactions must also migrate grant/reference accounting, attachment ownership and rollback; rewriting only a descriptor would be incorrect. Packet impact has not been attributed to this gap.
- QUIC retains additional lifecycle writes/reads in the focused cases. TLS fanout reads exceed C++ despite fewer writes. Ciphertext syscall counts are not Cap'n Proto message counts.
- Bilateral portable Join remains unsupported. Supported network-mode integrations remain covered.
- Coverage and the complete formal verification suite are not requalified. TLC-backed pipeline/tail tests ran with Java and the configured tools jar; local Verus and Kani tools were absent. No success is claimed for checks that did not run.

## Final repeated comparison

The cumulative observed geometric-mean latency changes versus the pre-replacement portable are **+4.96% TLS** and **+4.11% QUIC**. Lower means occur in 6/24 TLS and 8/24 QUIC cases. Portable has a lower mean than C++ in 23/24 TLS cases, and than current legacy in 8/24 TLS and 7/24 QUIC cases. These descriptive aggregates do not establish statistical significance or a universal speedup.

### TLS

| Case | Legacy | Portable | C++ | Portable control | Change |
| --- | ---: | ---: | ---: | ---: | ---: |
| loopback-0 | 27.75 ± 2.60 | 27.25 ± 0.97 | 37.56 ± 1.91 | 26.48 ± 1.82 | +2.9% |
| loopback-64 | 28.00 ± 1.52 | 27.72 ± 1.45 | 37.63 ± 3.23 | 29.91 ± 4.99 | -7.3% |
| loopback-1024 | 30.72 ± 5.37 | 29.70 ± 1.21 | 40.30 ± 7.04 | 29.63 ± 3.79 | +0.3% |
| loopback-65536 | 145.11 ± 25.30 | 141.28 ± 31.90 | 217.07 ± 50.22 | 154.27 ± 19.93 | -8.4% |
| unary-empty | 28.15 ± 2.94 | 30.96 ± 6.93 | 43.07 ± 9.35 | 30.56 ± 4.64 | +1.3% |
| unary-small | 36.34 ± 5.62 | 43.59 ± 1.86 | 52.16 ± 4.34 | 41.13 ± 1.31 | +6.0% |
| unary-1k | 38.93 ± 4.07 | 41.00 ± 3.09 | 51.73 ± 10.28 | 36.23 ± 7.37 | +13.2% |
| fanout-32 | 159.89 ± 26.52 | 207.34 ± 31.72 | 367.77 ± 70.22 | 169.62 ± 30.01 | +22.2% |
| dependent-read | 80.77 ± 5.18 | 122.62 ± 10.38 | 136.88 ± 2.74 | 118.98 ± 9.19 | +3.1% |
| pipelined-read | 61.63 ± 8.68 | 103.34 ± 10.87 | 90.48 ± 6.92 | 90.13 ± 14.97 | +14.7% |
| write-read | 74.00 ± 3.70 | 79.85 ± 12.95 | 122.88 ± 18.38 | 80.42 ± 3.09 | -0.7% |
| callback | 83.70 ± 8.51 | 115.25 ± 10.90 | 115.85 ± 7.98 | 105.34 ± 8.66 | +9.4% |
| bulk-64k | 140.54 ± 9.61 | 143.86 ± 14.29 | 189.89 ± 8.89 | 138.45 ± 10.30 | +3.9% |
| bulk-256k | 310.15 ± 24.08 | 314.47 ± 17.12 | 636.56 ± 17.85 | 328.78 ± 3.24 | -4.4% |
| bulk-1m | 963.33 ± 15.59 | 941.45 ± 36.08 | 2168.45 ± 254.02 | 1097.56 ± 201.41 | -14.2% |
| bulk-4m | 5074.07 ± 431.74 | 4501.15 ± 191.65 | 9697.08 ± 224.31 | 4801.95 ± 398.77 | -6.3% |
| deep-8 | 35.64 ± 6.21 | 38.83 ± 6.11 | 56.15 ± 8.94 | 35.22 ± 6.31 | +10.2% |
| deep-32 | 43.44 ± 6.69 | 46.11 ± 8.79 | 63.33 ± 7.07 | 45.78 ± 4.96 | +0.7% |
| concurrent-32 | 111.29 ± 6.58 | 143.30 ± 9.14 | 361.51 ± 22.75 | 141.44 ± 5.80 | +1.3% |
| deep-32-concurrent-32 | 334.26 ± 54.42 | 372.44 ± 48.95 | 516.79 ± 65.77 | 361.58 ± 41.07 | +3.0% |
| services-4-deep-16 | 186.27 ± 16.95 | 219.30 ± 29.43 | 242.56 ± 22.55 | 204.31 ± 39.36 | +7.3% |
| services-4-deep-16-concurrent-32 | 1289.71 ± 32.08 | 1479.64 ± 32.02 | 1953.83 ± 30.73 | 1066.75 ± 62.61 | +38.7% |
| services-4-deep-16-clients-8 | 1372.31 ± 130.30 | 1245.68 ± 117.39 | 1724.99 ± 95.62 | 1087.36 ± 85.13 | +14.6% |
| services-8-deep-16-clients-8 | 3138.78 ± 153.59 | 2635.53 ± 108.94 | 3908.14 ± 209.42 | 2174.10 ± 95.17 | +21.2% |

### QUIC

| Case | Legacy | Portable | C++ | Portable control | Change |
| --- | ---: | ---: | ---: | ---: | ---: |
| loopback-0 | 49.25 ± 10.91 | 54.96 ± 8.33 | N/A | 61.25 ± 6.45 | -10.3% |
| loopback-64 | 43.54 ± 11.08 | 41.18 ± 4.19 | N/A | 45.17 ± 12.25 | -8.8% |
| loopback-1024 | 39.22 ± 7.65 | 43.83 ± 12.80 | N/A | 43.03 ± 10.96 | +1.9% |
| loopback-65536 | 127.77 ± 14.88 | 126.62 ± 15.17 | N/A | 133.90 ± 12.45 | -5.4% |
| unary-empty | 42.43 ± 11.36 | 41.77 ± 5.21 | N/A | 40.51 ± 7.56 | +3.1% |
| unary-small | 41.12 ± 9.04 | 44.22 ± 8.61 | N/A | 46.63 ± 12.00 | -5.2% |
| unary-1k | 50.57 ± 5.73 | 59.89 ± 14.78 | N/A | 51.23 ± 11.38 | +16.9% |
| fanout-32 | 190.43 ± 26.75 | 222.12 ± 8.11 | N/A | 200.97 ± 8.93 | +10.5% |
| dependent-read | 86.61 ± 13.74 | 121.06 ± 21.40 | N/A | 103.82 ± 2.91 | +16.6% |
| pipelined-read | 58.35 ± 14.28 | 109.56 ± 23.30 | N/A | 94.01 ± 10.19 | +16.5% |
| write-read | 71.24 ± 1.56 | 90.90 ± 16.57 | N/A | 79.51 ± 17.13 | +14.3% |
| callback | 82.17 ± 7.92 | 111.33 ± 15.67 | N/A | 100.93 ± 9.19 | +10.3% |
| bulk-64k | 116.76 ± 15.99 | 130.73 ± 25.47 | N/A | 127.01 ± 35.00 | +2.9% |
| bulk-256k | 323.39 ± 14.20 | 316.10 ± 6.24 | N/A | 321.87 ± 18.13 | -1.8% |
| bulk-1m | 1457.36 ± 224.30 | 1581.54 ± 213.56 | N/A | 1532.65 ± 129.33 | +3.2% |
| bulk-4m | 6213.46 ± 258.73 | 6414.85 ± 124.02 | N/A | 6220.93 ± 206.24 | +3.1% |
| deep-8 | 39.18 ± 3.34 | 60.65 ± 13.12 | N/A | 53.34 ± 10.21 | +13.7% |
| deep-32 | 66.39 ± 8.70 | 59.65 ± 19.28 | N/A | 59.52 ± 22.34 | +0.2% |
| concurrent-32 | 145.11 ± 14.08 | 176.88 ± 19.57 | N/A | 167.02 ± 28.68 | +5.9% |
| deep-32-concurrent-32 | 399.88 ± 21.93 | 449.38 ± 26.76 | N/A | 458.76 ± 49.72 | -2.0% |
| services-4-deep-16 | 359.00 ± 30.14 | 397.59 ± 50.91 | N/A | 329.13 ± 24.47 | +20.8% |
| services-4-deep-16-concurrent-32 | 1730.10 ± 35.86 | 2007.60 ± 337.53 | N/A | 2013.88 ± 168.86 | -0.3% |
| services-4-deep-16-clients-8 | 1634.32 ± 79.33 | 1624.34 ± 186.34 | N/A | 1553.46 ± 45.84 | +4.6% |
| services-8-deep-16-clients-8 | 3461.39 ± 78.73 | 3192.37 ± 167.62 | N/A | 3315.19 ± 66.86 | -3.7% |

## Longer focused confirmation

Five repetitions; 1,000 warmups and 5,000 measured operations per cell. Negative change means lower latency than the cumulative portable control.

### TLS

| Case | Legacy | Portable | C++ | Portable control | Change |
| --- | ---: | ---: | ---: | ---: | ---: |
| fanout-32 | 143.06 ± 6.79 | 184.17 ± 23.73 | 337.00 ± 29.78 | 169.39 ± 17.69 | +8.7% |
| dependent-read | 70.66 ± 11.78 | 87.02 ± 7.32 | 101.01 ± 8.73 | 86.03 ± 9.82 | +1.1% |
| pipelined-read | 45.41 ± 5.34 | 66.44 ± 5.12 | 68.58 ± 10.64 | 68.54 ± 10.85 | -3.1% |
| deep-32-concurrent-32 | 345.30 ± 29.14 | 397.49 ± 27.78 | 482.98 ± 11.96 | 350.31 ± 15.61 | +13.5% |

### QUIC

| Case | Legacy | Portable | C++ | Portable control | Change |
| --- | ---: | ---: | ---: | ---: | ---: |
| fanout-32 | 185.25 ± 23.07 | 206.81 ± 11.34 | N/A | 193.37 ± 11.29 | +6.9% |
| dependent-read | 94.18 ± 5.61 | 115.17 ± 12.50 | N/A | 124.34 ± 7.84 | -7.4% |
| pipelined-read | 63.53 ± 8.52 | 92.25 ± 11.24 | N/A | 94.71 ± 6.48 | -2.6% |
| deep-32-concurrent-32 | 384.38 ± 22.40 | 437.13 ± 20.10 | N/A | 411.41 ± 34.20 | +6.3% |

## Per-stage evidence

The table summarizes four-case geometric-mean latency changes against each stage’s paired portable control. It is not a sequence of multiplicative gains. Complete per-case means, SD, ranges and p99 are in [comparisons.json](comparisons.json); binary and source identifiers are in [evidence-index.json](evidence-index.json).

| Stage | TLS change | QUIC change |
| --- | ---: | ---: |
| operations1 | +8.92% | -4.47% |
| queues1 | +0.33% | +7.02% |
| tail1 | +1.86% | -5.38% |
| output1 | +1.26% | +4.84% |
| output2 | -7.87% | -3.36% |
| events1 | +0.84% | +10.90% |
| hooks1 | +1.50% | -4.02% |
| incoming1 | +3.50% | +5.16% |
| cancel1 | +2.25% | -7.69% |
| worker1 | +2.27% | +8.05% |
| calls1 | +0.55% | -1.93% |
| maintenance1 | +4.22% | +0.69% |
| projection1 | -1.03% | +3.89% |
| producer1 | +0.13% | -0.44% |
| quicturn1 | -3.18% | +3.81% |
| access1 | +3.19% | +1.63% |
| resource1 | -3.78% | +8.01% |
| resource2 | +2.57% | +7.85% |
| batch1 | -2.69% | -0.54% |
| turn1 | -2.04% | +3.40% |

## Ciphertext IO diagnostics

Five traced repetitions, two warmups and ten measured operations. Counts sum all participating peer processes for the whole lifecycle, including handshake, shutdown, TLS records and QUIC control/recovery traffic. They are neither RPC message counts nor timing qualification. Full counts, attempts and spread are in [ciphertext-counts.json](ciphertext-counts.json).

| Transport | Focused case | Legacy writes/reads | Portable writes/reads | C++ writes/reads | Prior portable writes/reads |
| --- | --- | ---: | ---: | ---: | ---: |
| tls | fanout-32 | 30/173 | 30/173 | 128/127 | 30/173 |
| tls | dependent-read | 54/53 | 54/53 | 56/55 | 54/53 |
| tls | pipelined-read | 30/29 | 30/29 | 32/30.8 | 30/29 |
| tls | deep-32-concurrent-32 | 30/197 | 30/197 | 200/199 | 30/197 |
| quic | fanout-32 | 32.4/84.4 | 35.6/92.8 | N/A | 35/87 |
| quic | dependent-read | 56.4/60.2 | 58.6/63 | N/A | 59/63 |
| quic | pipelined-read | 32.8/36.6 | 34.8/38.8 | N/A | 34.8/39 |
| quic | deep-32-concurrent-32 | 32.8/107.8 | 35/110 | N/A | 36/116.6 |

Against the prior portable, no higher mean in any of the four read/write metrics is observed in **24/24 TLS** and **14/24 QUIC** cases. Against current legacy this holds in **21/24 TLS** and **1/24 QUIC**; against C++ it holds in **19/24 TLS**. QUIC fanout includes one 38-write/116-read trace among four 35/87 traces, while its baseline is 35/87 in all five; the eight-service/client case has substantial packet-count spread. These exceptions remain visible and do not establish parity.

Across all 24 cases, [io-comparison.json](io-comparison.json) records every exception to having no higher mean in any of positive/attempted writes/reads. Variable ciphertext counts retain their five-run spread. No universal parity claim is made.

## CPU work

Callgrind 3.24.0, cache and branch simulation, both peers summed, ten warmups and 1,000 operations. Includes setup, handshake and shutdown; these are simulated counters, not hardware PMU measurements or native timings. Raw profiles and commands are archived; [profiles.json](profiles.json) carries every counter and source capture hash.

| Transport | Case | Baseline instructions | Final instructions | Change | Baseline conditional misses | Final conditional misses |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| tls | fanout-32 | 1,399,900,236 | 1,472,255,244 | +5.17% | 5,277,395 | 5,703,460 |
| tls | dependent-read | 163,133,142 | 171,770,430 | +5.29% | 1,756,223 | 1,808,505 |
| tls | pipelined-read | 160,479,708 | 165,706,869 | +3.26% | 2,126,736 | 2,140,375 |
| quic | fanout-32 | 1,486,852,055 | 1,553,134,364 | +4.46% | 6,951,202 | 7,875,509 |
| quic | dependent-read | 244,647,998 | 246,036,158 | +0.57% | 3,451,993 | 3,348,383 |
| quic | pipelined-read | 205,371,292 | 206,738,298 | +0.67% | 3,201,216 | 3,136,115 |
| tls | deep-32-concurrent-32 | 3,727,035,196 | 3,798,456,718 | +1.92% | 5,758,979 | 6,562,478 |

The deeper TLS profile increases simulated instructions by **1.92%**, conditional misses by **13.95%**, D1 read misses by **12.08%**, and D1 write misses by **13.57%**. Its longer native mean increases **13.47%**. The counters show that the regression is not adequately described as measurement noise alone; they do not isolate every cause of the wall-time difference.

Exclusive source attribution confirms reductions in Calls, awaiting/queued adapters, maintenance, and the old message queue. In fanout, Calls falls by 7.45 million simulated instructions, but message backing rises by 10.22 million, coordinator work by 8.03 million, readiness handling by 5.86 million and scheduler handling by 4.45 million. Pipeline removes 1.30 million maintenance instructions while introducing about 1.03 million in resource waiting and 0.86 million in graph events. These are whole-lifecycle source-file attributions, with inlined work sometimes attributed to callers or dependencies, rather than independent component totals. [profile-attribution.json](profile-attribution.json) retains all parsed counts, hashes and residual checks.

The remaining common-path performance work is reducing the bookkeeping and buffer/output work introduced by exact ownership, without restoring capacity scans or weakening resource/cancellation guarantees. No universal allocation, copied-byte or RPC-frame reduction is asserted; the measured totals here are native latency, simulated CPU/cache counters, exact-readiness diagnostics and ciphertext syscalls.


Dedicated producer1/quicturn1 before/after QUIC profiles and five-repeat traces isolate the transport notification change from later resource and batching work. Intermediate profiles preserve regressions rather than attributing the cumulative result to one mechanism.

## Validation

- Final RPC/engine/wire unit suites: 348 + 215 + 159 = **722 passed**.
- Standalone fixed storage: **86 passed**; growing: **89 passed**; alloc: **89 passed**; std + unaligned + sync_reader: **86 passed**. Embedded `thumbv7em-none-eabi` allocation-free check passed (three unused-fixture warnings).
- Compile-time API contracts: **35 passed**, including rejection of raw Driver-field mutation and mutable runtime escape.
- Encrypted integration: **22 passed**, including sparse parent/pipeline readiness, large bursts, flow control, shared endpoints, stalled writers and shutdown. Pipeline migration 5, Unix 3, scoped vat 6, and tail transfer 5 passed. TLC replay tests are included.
- Strict Clippy passed for all six affected crates, all their targets, with QUIC enabled; formatting passed. Whole-workspace Clippy was blocked by uncached dependencies and network download timeouts; it is not reported as passed.
- Root suite: **293 passed, 4 intentional negative controls ignored**. Encrypted compatibility/authentication: **206 checks passed**. Corrected failed attempts and complete scope are recorded in [validation.json](validation.json).

## Reproduction and archive

[archive.json](archive.json) identifies the local evidence archive and checksum. It contains native raw samples, five-repeat traces, profiles, failed-stage logs, frozen source snapshots/patches, binary manifests and reconstructable compressed binaries. The original secure49–53 evidence remains sealed and unchanged. Four post-freeze files differ only in test-fixture lint/format cleanup; [evidence-index.json](evidence-index.json) records the exact hashes and supplemental source copies. Production code matches the frozen executable.

Apply each stage patch to the base commit and overlay its `source-STAGE/` files, including newly added modules. Restore a binary with `gzip -dc bundle/STAGE.gz > STAGE`, mark it executable and verify the uncompressed manifest hash. Cumulative baseline and C++ controls are identified in each campaign scope and use the preserved earlier archives. The archived `final-campaigns.sh` records exact measurement commands.
