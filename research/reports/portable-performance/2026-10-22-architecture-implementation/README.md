# Portable architecture implementation and qualification

Five architecture changes are retained. The sixth was implemented three ways, benchmarked and reverted because it increased QUIC packet activity. The broader scan and queue replacements remain open; this is not a completed scan-free redesign.

The full cumulative five-run comparison observes **0.55% lower TLS** and **2.61% lower QUIC** geometric-mean latency than the original portable, with lower means in 14/24 cases on each transport. Portable has a lower observed mean than C++ in 23/24 TLS cases; pipeline is the exception. Legacy remains faster in most cases. These small aggregates and substantial spread do not establish a universal or reliable general speedup.

The longer four-case confirmation uses five runs, 1,000 warmups and 5,000 measured operations per cell. TLS dependent-read improves 10.9%; TLS fanout, pipeline and deep/concurrent are 1.4%, 2.2% and 1.5% slower. The four QUIC means improve 1.9–9.3%. Means, sample SD, ranges and p99 remain available for assessment.

Base: `788103bc106df1c21e2ff17b5030b6a0870803fb`. These changes deliberately break the prior hosted transport API. The detailed architecture review remains a historical assessment of that base.

## Replacements implemented

1. Application completion tokens now travel through queued invocation publication and the authoritative exchange. Return carries the token to the hosted owner, validated against the complete question generation. Removed `ReplyOwners`, the ready-task application-slot search, and duplicated question identity in application entries. Per-question weak observer counts replace membership scans at result fences. The first version was revised after profiles and the fanout benchmark; neither version is claimed as a universal speedup.
2. Finish and Release now use intrusive queues owned by their authoritative ledgers. Release revival unlinks exactly its obligation; rejected publication preserves its head and ownership. Release maintenance visits one pending obligation, independent of import-table capacity. Sparse-slot, revival, retry, late-Return and generation tests cover these transitions. Other maintenance classes retain their existing scheduler.
3. Portable hosting requires `Connection::take_frame_io()`: framed input, ordered output and independent writer-failure observation are acquired once. Removed optional portable negotiation, message-future fallbacks, per-frame receipt tracking and the second output-driver poll. One output stream owns deferred flushing and shutdown. Native frame halves retain connection lifetime explicitly. Unix framing preserves FD ownership across prefetched frames and reports the original write error. Queue windows borrow immutable parts rather than cloning them. The standalone core's transactional FIFO still hands frames to the transport FIFO.
4. Hook lookup uses one full-generational-key index, with alias chains for graph keys that converge, reusable slots and a dense refresh set. Settled imports subscribe to metadata updates instead of being revisited on each collection. Endpoint epochs disarm old subscriptions without allocated subscription leases. The index uses a private inexpensive hasher only for locally generated keys; equality still checks the entire generation. Foreign promises share the worker's thread-safe readiness queue and receive individual wakers; unrelated protocol activity no longer polls their futures. A bounded weak-key retirement sweep remains.
5. `Connection::control_mode()` is an immutable construction-time contract. Ordinary two-party connections use the direct TwoParty handler, omit network-control state and reject unsupported Join/Accept through the normal bounded output path. Network transports retain introductions and handoffs. Bootstrap-only delegation is separate from two-party rejection policy; the authenticated multiparty integration caught and verified that distinction. Transport ownership survives idle transitions and shutdown.
6. **Rejected after measurement:** QUIC turn integration. The first version ended coalescing at stream flush; a second required a parked byte reader; a third added a separate coordinator `finish_turn()` hint after framed input, ready work and accepted output drained. All three increased QUIC fanout positive writes/reads from 35/87 to 38/118, and deep/concurrent from 35/110 to 41/144 in their paired traces. The deferred packet turn provides coalescing beyond what those local completion conditions certify. The third version passed the expanded 140-frame burst and transport-boundary tests, showing that correct local turn accounting was insufficient to preserve packet behavior. Retained source restores step5 exactly; its existing deferred yield, pacing, recovery, packet budget and shared-endpoint ownership remain. Every experiment, source snapshot, binary and repeated measurement is retained.

## API and correctness

The frame split, stream shutdown and control-mode methods are required. The rejected turn-hint API is not part of the retained code. Repository transports and fixtures are migrated; no fallback preserves the previous portable transport API. Legacy RPC still uses its separate message-oriented APIs because it remains a benchmark and interoperability control. Metadata revision polling was removed from the metadata API.

No TLS verification, ALPN, QUIC pacing, flow control, output ordering or protocol retirement is disabled. Completion, queue acceptance and local flushing remain separate events. Bilateral portable Join was already unsupported and remains a gap; the network-mode multiparty tests that were supported still pass.

Validation includes RPC, engine and wire unit suites; standalone portable verification tests; native RPC, pipeline migration, FD, Unix and multiparty integration; secure lifecycle tests; strict all-target Clippy; formatting; and the allocation-free wire build. Model checking and coverage were not requalified. Two TLC-backed root tests require an unavailable model-checker setup and are excluded explicitly, rather than reported as passing.

## Remaining architecture work

These stages remove specific discovery and adapter mechanisms; they do not complete every larger replacement proposed in the review. Used-pipeline completion still scans, public projections still use the generic queued-client bridge, and pre-publication observer scheduling lacks exact per-owner lists. Only Finish/Release cleanup producers use precise queues; the other maintenance phases and mutable standalone-core interfaces still require conservative invalidation. Dynamic/opaque hook projections and foreign abandonment retain bounded discovery. The core and transport still have separate ordered output FIFOs.

Those are structural gaps, not compatibility requirements. Replacing them requires moving their producers and lifetime transitions together, then deleting the corresponding scanner. Adding an eager delivery bypass while leaving the old discovery path active would repeat the earlier rejected design. The measured instruction reductions here are modest; no 5%, 30%, or universal C++ speed target is established by them.

## Repeated measurements

The full campaigns below use five process repetitions, 100 warmups and 1,000 measured operations per case/variant. Both TLS and QUIC cover 24 cases; TLS has an equivalent C++ control, QUIC has none. Variant order rotates and CPUs are pinned to separate physical cores. Timing runs do not overlap builds, tests, profiling or archive compression. Profiles and traces are separate diagnostic runs. No traced or simulated elapsed time is used as native latency.

Each stage uses the immediately preceding frozen portable candidate as control (step1b compares to the original baseline). The first five stages use frozen legacy from secure52. Step6 and its revisions use freshly built legacy to keep the shared transport comparison current. Step6c tested moving the hint into the portable coordinator. All step6 variants were rejected. The retained campaign uses current step5 legacy/portable binaries and the original secure52 portable baseline. The table reports geometric mean latency changes over 24 case ratios; negative means lower latency. These aggregates do not express statistical significance or universal performance.

| Candidate | Portable control | TLS latency change | QUIC latency change |
| --- | --- | ---: | ---: |
| step1 | secure52 | +2.30% | +0.70% |
| step1b | secure52 | -0.03% | -0.25% |
| step2 | step1b | +0.21% | -1.42% |
| step3 | step2 | -0.84% | +0.18% |
| step4 | step3 | -2.57% | +0.82% |
| step4b | step4 | -0.19% | -0.09% |
| step5 | step4b | +0.63% | +1.34% |
| step6 | step5 | -2.53% | -1.13% |

Step6 changed the QUIC path; its TLS timing difference is not evidence of a causal TLS improvement.

The complete per-case mean, sample SD of the five run means, range and p99 are in [comparisons.json](comparisons.json). Raw samples, manifests, failed candidates, source snapshots, logs and profiles are retained in the evidence archive. Step1a was revised to step1b; step4a was revised to step4b after profiles identified extra hashing/allocation work. Step6 and step6b were superseded after full traces exposed premature packet flushing during buffered RPC bursts. Step6c used a separate coordinator completion hint and still increased packet counts. All three QUIC turn-integration experiments were rejected; retained code exactly matches step5. Larger individual regressions are not dismissed merely because the aggregate is near zero.

## Retained legacy, portable and C++ against the original portable

Values are microseconds: mean ± sample SD of run means. Lower is better. Range and p99 remain in the JSON. The portable control is secure52 in the retained campaign and longer confirmation; the rejected QUIC experiments compare with step5. The focused rejected-candidate confirmations use five runs, 500 warmups and 2,000 measured operations per cell.

### TLS

| Case | Legacy | Portable | C++ | Portable control | Change |
| --- | ---: | ---: | ---: | ---: | ---: |
| loopback-0 | 35.37 ± 5.62 | 34.84 ± 5.62 | 40.12 ± 5.23 | 31.93 ± 8.70 | +9.1% |
| loopback-64 | 35.54 ± 3.00 | 35.03 ± 5.99 | 48.06 ± 8.75 | 35.19 ± 5.75 | -0.5% |
| loopback-1024 | 32.99 ± 5.03 | 34.51 ± 9.32 | 49.66 ± 9.52 | 34.88 ± 6.01 | -1.1% |
| loopback-65536 | 121.33 ± 17.47 | 122.43 ± 21.39 | 175.69 ± 19.46 | 116.98 ± 7.21 | +4.7% |
| unary-empty | 30.83 ± 2.97 | 31.49 ± 4.85 | 46.21 ± 9.08 | 32.88 ± 6.34 | -4.2% |
| unary-small | 33.72 ± 4.91 | 37.05 ± 5.48 | 47.92 ± 8.72 | 31.50 ± 5.63 | +17.6% |
| unary-1k | 35.96 ± 5.56 | 41.56 ± 21.54 | 42.28 ± 4.54 | 34.65 ± 7.27 | +19.9% |
| fanout-32 | 156.18 ± 31.23 | 177.89 ± 25.06 | 343.36 ± 31.45 | 177.65 ± 39.91 | +0.1% |
| dependent-read | 78.66 ± 12.34 | 91.90 ± 18.90 | 103.78 ± 21.26 | 106.64 ± 17.51 | -13.8% |
| pipelined-read | 42.15 ± 6.22 | 74.72 ± 18.70 | 73.61 ± 13.27 | 72.53 ± 14.35 | +3.0% |
| write-read | 59.25 ± 6.00 | 63.35 ± 10.81 | 106.04 ± 16.20 | 71.05 ± 15.74 | -10.8% |
| callback | 61.37 ± 3.68 | 85.34 ± 9.96 | 106.57 ± 25.05 | 83.48 ± 13.31 | +2.2% |
| bulk-64k | 116.41 ± 12.98 | 121.60 ± 15.48 | 180.78 ± 14.08 | 137.18 ± 4.41 | -11.4% |
| bulk-256k | 272.12 ± 16.25 | 302.69 ± 51.99 | 616.09 ± 27.43 | 280.99 ± 4.59 | +7.7% |
| bulk-1m | 986.59 ± 24.11 | 1017.66 ± 40.48 | 2314.75 ± 128.91 | 1120.02 ± 134.25 | -9.1% |
| bulk-4m | 4798.60 ± 313.23 | 4828.21 ± 222.23 | 9655.45 ± 524.04 | 4802.55 ± 284.54 | +0.5% |
| deep-8 | 37.44 ± 6.05 | 36.19 ± 7.56 | 47.90 ± 8.15 | 40.75 ± 6.23 | -11.2% |
| deep-32 | 37.72 ± 5.87 | 40.86 ± 8.54 | 45.43 ± 2.80 | 36.87 ± 0.72 | +10.8% |
| concurrent-32 | 109.36 ± 11.10 | 139.39 ± 13.51 | 343.56 ± 26.51 | 144.81 ± 19.77 | -3.7% |
| deep-32-concurrent-32 | 336.94 ± 26.52 | 365.93 ± 36.66 | 501.31 ± 37.10 | 366.79 ± 39.14 | -0.2% |
| services-4-deep-16 | 191.46 ± 15.53 | 214.07 ± 10.13 | 303.49 ± 19.73 | 230.21 ± 32.64 | -7.0% |
| services-4-deep-16-concurrent-32 | 1357.56 ± 98.79 | 1134.51 ± 118.79 | 2062.22 ± 199.35 | 1159.54 ± 45.25 | -2.2% |
| services-4-deep-16-clients-8 | 1358.26 ± 29.44 | 1147.98 ± 65.26 | 1835.82 ± 149.97 | 1162.31 ± 43.35 | -1.2% |
| services-8-deep-16-clients-8 | 2935.69 ± 103.97 | 2112.00 ± 102.05 | 3734.84 ± 95.81 | 2198.81 ± 159.33 | -3.9% |

### QUIC

| Case | Legacy | Portable | C++ | Portable control | Change |
| --- | ---: | ---: | ---: | ---: | ---: |
| loopback-0 | 43.80 ± 5.82 | 42.97 ± 8.74 | N/A | 45.59 ± 10.68 | -5.8% |
| loopback-64 | 32.70 ± 1.41 | 45.84 ± 14.13 | N/A | 50.71 ± 9.63 | -9.6% |
| loopback-1024 | 41.22 ± 7.82 | 38.70 ± 6.18 | N/A | 47.79 ± 11.89 | -19.0% |
| loopback-65536 | 134.67 ± 17.36 | 141.16 ± 21.42 | N/A | 129.84 ± 12.69 | +8.7% |
| unary-empty | 49.33 ± 7.80 | 51.19 ± 8.43 | N/A | 49.25 ± 6.02 | +3.9% |
| unary-small | 39.60 ± 9.70 | 46.06 ± 12.16 | N/A | 41.94 ± 12.14 | +9.8% |
| unary-1k | 39.26 ± 6.43 | 43.53 ± 11.28 | N/A | 43.16 ± 8.54 | +0.9% |
| fanout-32 | 168.59 ± 15.31 | 207.96 ± 21.50 | N/A | 202.00 ± 16.45 | +2.9% |
| dependent-read | 82.00 ± 10.56 | 100.90 ± 7.37 | N/A | 105.97 ± 9.07 | -4.8% |
| pipelined-read | 54.79 ± 9.92 | 86.58 ± 19.13 | N/A | 96.79 ± 26.60 | -10.5% |
| write-read | 90.46 ± 10.43 | 97.62 ± 11.86 | N/A | 91.75 ± 9.11 | +6.4% |
| callback | 78.99 ± 9.85 | 100.80 ± 17.18 | N/A | 111.55 ± 17.11 | -9.6% |
| bulk-64k | 147.45 ± 46.24 | 146.26 ± 30.93 | N/A | 143.95 ± 26.19 | +1.6% |
| bulk-256k | 325.54 ± 57.54 | 402.10 ± 127.09 | N/A | 367.29 ± 86.34 | +9.5% |
| bulk-1m | 1792.92 ± 106.20 | 1647.99 ± 96.86 | N/A | 1753.29 ± 250.50 | -6.0% |
| bulk-4m | 6037.80 ± 310.47 | 6462.33 ± 270.32 | N/A | 6188.94 ± 113.15 | +4.4% |
| deep-8 | 44.47 ± 12.38 | 53.57 ± 13.80 | N/A | 48.07 ± 12.40 | +11.5% |
| deep-32 | 51.97 ± 10.38 | 52.17 ± 12.96 | N/A | 54.37 ± 12.38 | -4.0% |
| concurrent-32 | 128.26 ± 16.05 | 151.89 ± 10.01 | N/A | 159.07 ± 19.80 | -4.5% |
| deep-32-concurrent-32 | 372.37 ± 52.93 | 363.99 ± 26.79 | N/A | 406.23 ± 37.66 | -10.4% |
| services-4-deep-16 | 345.99 ± 33.28 | 295.78 ± 23.19 | N/A | 333.80 ± 36.93 | -11.4% |
| services-4-deep-16-concurrent-32 | 1781.31 ± 154.63 | 1934.69 ± 145.11 | N/A | 2164.94 ± 266.49 | -10.6% |
| services-4-deep-16-clients-8 | 1738.60 ± 120.68 | 1712.20 ± 221.19 | N/A | 1857.50 ± 279.26 | -7.8% |
| services-8-deep-16-clients-8 | 3625.06 ± 117.76 | 3535.91 ± 169.27 | N/A | 3542.15 ± 118.73 | -0.2% |

## Superseded step6 confirmation

Values are microseconds: mean ± sample SD of run means. Lower is better. Range and p99 remain in the JSON. The portable control is secure52 in the retained campaign and longer confirmation; the rejected QUIC experiments compare with step5. The focused rejected-candidate confirmations use five runs, 500 warmups and 2,000 measured operations per cell.

### QUIC

| Case | Legacy | Portable | C++ | Portable control | Change |
| --- | ---: | ---: | ---: | ---: | ---: |
| pipelined-read | 77.08 ± 13.89 | 118.90 ± 20.87 | N/A | 122.41 ± 15.41 | -2.9% |
| write-read | 97.83 ± 15.68 | 93.84 ± 16.78 | N/A | 96.22 ± 7.03 | -2.5% |
| bulk-1m | 1715.29 ± 104.06 | 1679.70 ± 176.73 | N/A | 1695.35 ± 58.34 | -0.9% |
| services-8-deep-16-clients-8 | 3848.07 ± 171.81 | 3766.97 ± 88.60 | N/A | 3564.38 ± 127.39 | +5.7% |

## Superseded guarded-flush confirmation

Values are microseconds: mean ± sample SD of run means. Lower is better. Range and p99 remain in the JSON. The portable control is secure52 in the retained campaign and longer confirmation; the rejected QUIC experiments compare with step5. The focused rejected-candidate confirmations use five runs, 500 warmups and 2,000 measured operations per cell.

### QUIC

| Case | Legacy | Portable | C++ | Portable control | Change |
| --- | ---: | ---: | ---: | ---: | ---: |
| pipelined-read | 61.15 ± 10.96 | 88.80 ± 11.80 | N/A | 117.36 ± 26.34 | -24.3% |
| write-read | 82.11 ± 13.06 | 91.86 ± 10.42 | N/A | 87.49 ± 10.68 | +5.0% |
| bulk-1m | 1758.81 ± 146.85 | 1721.04 ± 92.06 | N/A | 1795.27 ± 126.45 | -4.1% |
| services-8-deep-16-clients-8 | 3692.47 ± 132.68 | 3693.63 ± 177.69 | N/A | 3536.60 ± 178.14 | +4.4% |

## Rejected coordinator-hint confirmation

Values are microseconds: mean ± sample SD of run means. Lower is better. Range and p99 remain in the JSON. The portable control is secure52 in the retained campaign and longer confirmation; the rejected QUIC experiments compare with step5. The focused rejected-candidate confirmations use five runs, 500 warmups and 2,000 measured operations per cell.

### QUIC

| Case | Legacy | Portable | C++ | Portable control | Change |
| --- | ---: | ---: | ---: | ---: | ---: |
| pipelined-read | 66.33 ± 15.67 | 99.32 ± 8.61 | N/A | 109.13 ± 13.42 | -9.0% |
| write-read | 94.83 ± 12.49 | 94.74 ± 9.79 | N/A | 115.32 ± 11.97 | -17.8% |
| bulk-1m | 1590.34 ± 96.56 | 1596.67 ± 162.92 | N/A | 1597.89 ± 126.59 | -0.1% |
| services-8-deep-16-clients-8 | 3746.95 ± 197.26 | 4055.32 ± 262.00 | N/A | 3560.67 ± 78.93 | +13.9% |

## Longer comparison against the original portable

Values are microseconds: mean ± sample SD of run means. Lower is better. Range and p99 remain in the JSON. The portable control is secure52 in the retained campaign and longer confirmation; the rejected QUIC experiments compare with step5. The focused rejected-candidate confirmations use five runs, 500 warmups and 2,000 measured operations per cell.

### TLS

| Case | Legacy | Portable | C++ | Portable control | Change |
| --- | ---: | ---: | ---: | ---: | ---: |
| fanout-32 | 149.42 ± 6.40 | 181.00 ± 19.25 | 350.93 ± 29.96 | 178.51 ± 11.69 | +1.4% |
| dependent-read | 69.56 ± 8.47 | 77.41 ± 12.52 | 97.71 ± 10.78 | 86.86 ± 10.64 | -10.9% |
| pipelined-read | 47.15 ± 7.81 | 68.18 ± 13.94 | 65.69 ± 6.22 | 66.70 ± 4.21 | +2.2% |
| deep-32-concurrent-32 | 334.33 ± 31.13 | 379.64 ± 8.60 | 534.04 ± 23.42 | 374.10 ± 20.78 | +1.5% |

### QUIC

| Case | Legacy | Portable | C++ | Portable control | Change |
| --- | ---: | ---: | ---: | ---: | ---: |
| fanout-32 | 186.51 ± 14.38 | 211.26 ± 7.96 | N/A | 232.87 ± 23.76 | -9.3% |
| dependent-read | 95.07 ± 8.13 | 122.28 ± 7.93 | N/A | 130.03 ± 7.73 | -6.0% |
| pipelined-read | 59.78 ± 7.73 | 89.37 ± 15.60 | N/A | 94.16 ± 12.64 | -5.1% |
| deep-32-concurrent-32 | 366.51 ± 25.75 | 395.55 ± 24.05 | N/A | 403.04 ± 28.73 | -1.9% |

## Retained CPU work, TLS

Both-peer Callgrind totals for 10 warmups and 1,000 measured operations. The control is the original secure52 portable binary. These are simulated counters, not hardware PMU results or native latency.

| Case | Original instructions | Retained instructions | Change | Original conditional misses | Retained conditional misses |
| --- | ---: | ---: | ---: | ---: | ---: |
| dependent-read | 166,204,724 | 163,133,142 | -1.85% | 1,778,082 | 1,756,223 |
| pipelined-read | 162,560,974 | 160,479,708 | -1.28% | 2,161,013 | 2,126,736 |
| fanout-32 | 1,414,476,918 | 1,399,900,236 | -1.03% | 4,989,405 | 5,277,395 |

Instruction work falls modestly, but fanout simulated conditional misses remain higher. The earlier five-run longer step1b fanout check regressed TLS mean latency by 12.4%; the cumulative retained campaign and focused confirmation are the evidence for the final code, not a multiplication of stage ratios.

## Diagnostic boundaries

[profiles.json](profiles.json) contains both-peer Callgrind instruction and simulated branch counters, using 10 warmups and 1,000 measured operations. They are not PMU measurements or a latency estimate. [socket-counts.json](socket-counts.json) records positive and attempted IO separately. Traces cover the whole encrypted peer lifecycle, including handshake, shutdown, ACK/control packets and would-block attempts; they are not RPC frame counts. C++ QUIC is N/A.

Full retained traces have no increase in any of positive/attempted writes/reads in **22/24 TLS** and **19/24 QUIC** cases versus the original portable. The corresponding counts versus current legacy are **19/24 TLS** and **1/24 QUIC**; versus C++ they are **19/24 TLS**, with QUIC unsupported. Universal IO parity is not achieved.

The focused retained counts are TLS dependent 54/53, pipeline 30/29 and fanout 30/173; QUIC dependent 59/63, pipeline 35/39 and fanout 35/87 (successful writes/reads, both peers). Fanout reads still exceed C++ TLS, which records 128/127.

Five additional trace repetitions check the bulk and multi-service exceptions. Below are mean ± sample SD of successful lifecycle syscall counts, not RPC frames or native timing. The single full trace had +37/+65 TLS multi-service writes/reads versus the old portable; the repeated means are nearly equal. QUIC multi-service counts remain higher on average with substantial spread.

| Transport / case | Retained writes | Original writes | Retained reads | Original reads |
| --- | ---: | ---: | ---: | ---: |
| quic / bulk-1m | 850.8 ± 13.3 | 847.6 ± 23.5 | 2033.2 ± 15.6 | 2023.0 ± 26.5 |
| quic / services-8-deep-16-clients-8 | 1177.6 ± 76.8 | 1144.4 ± 36.4 | 1392.6 ± 61.2 | 1361.4 ± 33.8 |
| tls / bulk-1m | 432.0 ± 0.0 | 432.0 ± 0.0 | 1758.4 ± 61.9 | 1742.2 ± 22.1 |
| tls / services-8-deep-16-clients-8 | 1209.4 ± 16.5 | 1207.6 ± 20.4 | 1944.2 ± 27.6 | 1941.2 ± 23.7 |

## Reproduction and retained evidence

The source snapshots and frozen binary hashes are listed in [evidence-index.json](evidence-index.json). See [archive.json](archive.json) and the archived reconstruction instructions for every candidate, including the superseded variants. The existing secure49–52 evidence is read-only and unchanged.
