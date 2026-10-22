# Portable structure optimization — 2026-10-22

Four source files refine the new ownership/work structures introduced by the [architecture replacements](../2026-10-22-architecture-replacements/README.md). The retained changes reduce metadata and unnecessary queue work. They do not establish a universal latency improvement or universal IO parity. Legacy remains a comparison and interoperability control.

## Retained changes

- Intrusive links store optional indices in the nonzero niche. A queued head names itself as predecessor; an unqueued link has no predecessor. This removes the separate membership flag while preserving constant-time insertion, arbitrary removal, deduplication and slot reuse. No unsafe code or allocation is added.
- Resource waits encode the resource plus one, eliminating the extra optional-requirement tag. Repeated wakes of an already runnable owner preserve its FIFO position instead of unlinking/relinking it. Resource cancellation still removes the parked wait before making the owner runnable.
- Outgoing and invocation retry paths construct a capacity snapshot only when resource waiters exist. The empty path no longer constructs a 192-byte default snapshot.

Measured 64-bit layouts (bytes):

| Structure | Before | Retained | Reduction |
|---|---:|---:|---:|
| Intrusive link | 40 | 16 | 60% |
| Queue head/tail | 32 | 16 | 50% |
| Waiting slot | 64 | 32 | 50% |
| 12-resource wait set | 432 | 224 | 48.1% |

## Rejected buffer experiments

The profiles identified repeated message-buffer searches. A rotating hint reduced TLS fanout instructions by 0.70%, but increased its simulated D1 write misses by 7.81%; adding the waiting changes raised this to 10.14%. A last-used hint retained most of that cache cost and increased conditional misses (7.76% for TLS fanout, 7.28% for deep/concurrent). Native results were mixed. Both buffer-search policies were removed. Final buffer files match the base exactly.

C++ provides a useful locality reference: `BuilderArena::allocate()` retains `segmentWithSpace` rather than restarting a segment search ([arena.c++](../../../../vendor/capnproto/c++/src/capnp/arena.c++#L226)). Its allocation/ownership model differs from the portable reusable pool; translating that hint did not produce a supported improvement here. All rejected source snapshots, binaries, native trials and profiles remain in the evidence.

## Method and controls

The source baseline is d17af34cbc64beefd72b06f94a0f1639473382d9. A fresh build of the unmodified baseline and every candidate uses the same Rust 1.97/Clang 19 toolchain, fat LTO and one codegen unit. Fresh-cache executables were smaller than the historical frozen `turn1` despite matching compiler versions, so source-change comparisons use the matched rebuild. The separate `baseline-focused` campaign retains calibration against historical `turn1`; its noisy differences are not attributed to these changes. The initial baseline rebuild hit the memory limit and was never frozen; its successful retry and later builds used one compiler job.

Native results use five process repetitions, rotating variants on pinned physical CPUs, with 100 warmups and 1,000 measured iterations per full case. The longer focused confirmation uses 1,000 warmups and 5,000 iterations. Builds, tests, tracing, profiles and compression do not overlap native timing. Means below are microseconds ± sample SD of the five run means. JSON retains all run means, their min/max, pooled percentiles including p99, and sample spread. Each operation is the fixture-defined workload, including a whole concurrent batch where applicable.

TLS and QUIC are authenticated and encrypted. C++ is compared on equivalent TLS fixtures; C++ QUIC is unsupported. Legacy uses the final executable for the full/long campaigns. Profiles simulate both peers over their complete lifecycle (10 warmups/250 iterations), including setup and shutdown; they are not native timing or hardware PMU results. Five-repeat syscall traces use 2 warmups/10 iterations and include all peer lifecycles, ciphertext, handshake, control and QUIC recovery traffic.

## Full repeated comparison

### TLS

| Reference | Lower portable means | Geometric mean latency change |
|---|---:|---:|
| portable-baseline | 10/24 | -0.23% |
| legacy | 8/24 | +9.04% |
| cpp | 24/24 | -31.49% |

| Case | Baseline | Portable | Legacy | C++ | Δ baseline |
|---|---:|---:|---:|---:|---:|
| loopback-0 | 33.91 ± 3.06 | 30.36 ± 2.78 | 30.89 ± 3.71 | 46.09 ± 8.71 | -10.47% |
| loopback-64 | 37.99 ± 8.00 | 34.77 ± 7.93 | 31.24 ± 4.17 | 50.05 ± 11.33 | -8.47% |
| loopback-1024 | 45.92 ± 10.70 | 42.75 ± 9.80 | 36.30 ± 4.68 | 59.61 ± 21.22 | -6.90% |
| loopback-65536 | 140.55 ± 24.27 | 138.51 ± 28.54 | 145.57 ± 20.52 | 187.95 ± 39.31 | -1.45% |
| unary-empty | 27.99 ± 2.65 | 29.53 ± 7.04 | 27.70 ± 5.27 | 43.04 ± 9.11 | +5.52% |
| unary-small | 30.47 ± 5.62 | 31.42 ± 5.27 | 32.92 ± 6.26 | 43.13 ± 7.76 | +3.11% |
| unary-1k | 33.67 ± 6.84 | 32.37 ± 5.44 | 32.66 ± 7.35 | 43.23 ± 7.78 | -3.85% |
| fanout-32 | 174.25 ± 13.14 | 183.78 ± 22.27 | 146.75 ± 15.15 | 326.70 ± 27.64 | +5.47% |
| dependent-read | 94.33 ± 16.66 | 82.43 ± 9.50 | 70.25 ± 14.70 | 96.29 ± 7.72 | -12.61% |
| pipelined-read | 59.79 ± 4.46 | 61.80 ± 3.99 | 45.99 ± 10.67 | 73.17 ± 19.76 | +3.36% |
| write-read | 74.05 ± 11.99 | 64.69 ± 13.26 | 68.03 ± 8.95 | 90.70 ± 11.36 | -12.64% |
| callback | 83.17 ± 10.76 | 78.97 ± 10.79 | 70.41 ± 12.44 | 86.05 ± 10.29 | -5.04% |
| bulk-64k | 122.40 ± 27.37 | 123.02 ± 28.21 | 122.80 ± 25.35 | 186.37 ± 36.92 | +0.50% |
| bulk-256k | 283.83 ± 39.45 | 315.38 ± 42.70 | 255.36 ± 18.40 | 586.67 ± 80.40 | +11.12% |
| bulk-1m | 971.84 ± 137.54 | 1164.99 ± 270.43 | 1242.10 ± 309.73 | 2194.88 ± 350.09 | +19.88% |
| bulk-4m | 4462.35 ± 260.44 | 4517.20 ± 437.27 | 4443.07 ± 373.01 | 9056.17 ± 1282.20 | +1.23% |
| deep-8 | 38.74 ± 6.80 | 39.75 ± 6.46 | 33.58 ± 5.11 | 56.49 ± 9.12 | +2.60% |
| deep-32 | 37.18 ± 3.67 | 40.29 ± 6.86 | 36.09 ± 5.81 | 51.62 ± 11.82 | +8.35% |
| concurrent-32 | 150.44 ± 14.96 | 140.53 ± 14.92 | 104.61 ± 10.15 | 334.18 ± 38.32 | -6.59% |
| deep-32-concurrent-32 | 368.48 ± 43.28 | 387.82 ± 43.03 | 330.03 ± 28.89 | 502.55 ± 51.70 | +5.25% |
| services-4-deep-16 | 249.39 ± 41.11 | 253.32 ± 49.55 | 193.05 ± 20.70 | 334.08 ± 37.36 | +1.57% |
| services-4-deep-16-concurrent-32 | 1517.68 ± 83.21 | 1535.28 ± 94.53 | 1317.76 ± 39.98 | 1943.44 ± 101.11 | +1.16% |
| services-4-deep-16-clients-8 | 1189.67 ± 96.34 | 1192.37 ± 46.40 | 1268.84 ± 39.61 | 1723.06 ± 122.17 | +0.23% |
| services-8-deep-16-clients-8 | 2424.99 ± 81.11 | 2421.77 ± 162.10 | 2794.83 ± 32.89 | 3544.56 ± 197.81 | -0.13% |

### QUIC

| Reference | Lower portable means | Geometric mean latency change |
|---|---:|---:|
| portable-baseline | 11/24 | -0.19% |
| legacy | 7/24 | +11.11% |

| Case | Baseline | Portable | Legacy | C++ | Δ baseline |
|---|---:|---:|---:|---:|---:|
| loopback-0 | 42.21 ± 9.98 | 39.82 ± 6.29 | 42.82 ± 12.32 | unsupported | -5.67% |
| loopback-64 | 45.89 ± 11.60 | 37.09 ± 1.28 | 41.52 ± 8.71 | unsupported | -19.17% |
| loopback-1024 | 43.90 ± 5.03 | 48.89 ± 12.81 | 39.02 ± 6.47 | unsupported | +11.37% |
| loopback-65536 | 133.37 ± 19.10 | 135.58 ± 25.06 | 146.15 ± 30.27 | unsupported | +1.66% |
| unary-empty | 55.70 ± 12.76 | 56.39 ± 12.22 | 44.34 ± 11.79 | unsupported | +1.24% |
| unary-small | 48.11 ± 12.71 | 42.60 ± 11.73 | 43.25 ± 10.31 | unsupported | -11.46% |
| unary-1k | 39.67 ± 0.41 | 39.60 ± 2.98 | 36.90 ± 1.52 | unsupported | -0.18% |
| fanout-32 | 206.85 ± 15.97 | 210.06 ± 38.44 | 168.79 ± 10.96 | unsupported | +1.55% |
| dependent-read | 101.31 ± 5.65 | 103.59 ± 4.97 | 71.65 ± 2.24 | unsupported | +2.26% |
| pipelined-read | 88.40 ± 13.68 | 85.44 ± 17.98 | 50.99 ± 6.22 | unsupported | -3.34% |
| write-read | 82.81 ± 16.13 | 73.41 ± 2.10 | 72.59 ± 7.93 | unsupported | -11.35% |
| callback | 119.68 ± 24.11 | 114.52 ± 39.66 | 80.83 ± 14.01 | unsupported | -4.31% |
| bulk-64k | 130.30 ± 17.98 | 130.40 ± 13.27 | 125.78 ± 20.38 | unsupported | +0.08% |
| bulk-256k | 320.93 ± 20.63 | 332.24 ± 35.96 | 349.54 ± 93.30 | unsupported | +3.52% |
| bulk-1m | 1556.44 ± 45.77 | 1694.38 ± 277.92 | 1601.74 ± 222.47 | unsupported | +8.86% |
| bulk-4m | 6270.22 ± 411.35 | 6094.36 ± 426.28 | 5879.61 ± 481.82 | unsupported | -2.80% |
| deep-8 | 50.30 ± 2.94 | 50.69 ± 2.49 | 44.89 ± 1.02 | unsupported | +0.77% |
| deep-32 | 48.29 ± 6.76 | 55.43 ± 11.78 | 45.32 ± 12.02 | unsupported | +14.78% |
| concurrent-32 | 153.85 ± 11.69 | 149.25 ± 9.20 | 116.76 ± 3.37 | unsupported | -2.99% |
| deep-32-concurrent-32 | 411.15 ± 51.10 | 425.40 ± 15.44 | 357.81 ± 48.03 | unsupported | +3.47% |
| services-4-deep-16 | 264.43 ± 59.31 | 301.61 ± 71.84 | 273.78 ± 40.57 | unsupported | +14.06% |
| services-4-deep-16-concurrent-32 | 1686.21 ± 55.06 | 1657.37 ± 69.68 | 1634.73 ± 93.52 | unsupported | -1.71% |
| services-4-deep-16-clients-8 | 1528.62 ± 53.35 | 1580.28 ± 174.46 | 1623.61 ± 66.68 | unsupported | +3.38% |
| services-8-deep-16-clients-8 | 3145.38 ± 143.30 | 3094.41 ± 79.41 | 3411.73 ± 91.53 | unsupported | -1.62% |

## Longer focused confirmation

| Transport / case | Baseline | Portable | Legacy | C++ | Δ baseline |
|---|---:|---:|---:|---:|---:|
| tls / fanout-32 | 182.87 ± 18.95 | 171.95 ± 7.66 | 135.90 ± 6.56 | 317.92 ± 22.48 | -5.97% |
| tls / dependent-read | 81.66 ± 7.30 | 82.00 ± 10.30 | 58.45 ± 6.96 | 86.91 ± 4.35 | +0.42% |
| tls / pipelined-read | 63.11 ± 11.67 | 58.86 ± 6.07 | 41.35 ± 2.77 | 64.00 ± 4.75 | -6.75% |
| tls / deep-32-concurrent-32 | 398.95 ± 47.71 | 381.21 ± 35.24 | 305.57 ± 18.54 | 491.42 ± 7.40 | -4.45% |
| quic / fanout-32 | 197.73 ± 9.67 | 200.43 ± 9.12 | 174.98 ± 11.61 | unsupported | +1.37% |
| quic / dependent-read | 116.03 ± 9.18 | 115.42 ± 11.67 | 91.74 ± 7.67 | unsupported | -0.52% |
| quic / pipelined-read | 87.80 ± 7.58 | 88.57 ± 7.61 | 49.87 ± 4.01 | unsupported | +0.88% |
| quic / deep-32-concurrent-32 | 394.12 ± 46.94 | 409.87 ± 39.43 | 368.00 ± 21.69 | unsupported | +4.00% |

## Checkpoints

Each cell below is the observed latency change versus its own matched baseline campaign; negative is lower. The five-run means/SD/ranges/p99 for every checkpoint are in [comparisons.json](comparisons.json). `cursor`, `waits`, and `reuse` include rejected buffer policies; `links` is the retained cumulative candidate.

| Transport / case | compact | cursor | waits | reuse | links |
|---|---:|---:|---:|---:|---:|
| tls / fanout-32 | -5.35% | -10.81% | -1.12% | -8.35% | +0.33% |
| tls / dependent-read | +22.18% | -0.06% | -1.54% | +3.80% | +3.59% |
| tls / pipelined-read | -3.51% | +0.66% | -10.53% | +1.06% | -2.06% |
| tls / deep-32-concurrent-32 | +3.00% | +9.21% | +4.42% | -4.19% | -1.96% |
| quic / fanout-32 | -3.76% | -5.28% | -3.39% | -1.92% | -4.96% |
| quic / dependent-read | -4.23% | -1.99% | +1.14% | -25.06% | -16.16% |
| quic / pipelined-read | +14.83% | -19.56% | +6.71% | -11.94% | -16.50% |
| quic / deep-32-concurrent-32 | -3.12% | +1.86% | -1.28% | +3.29% | -1.18% |

## Final profiles

Whole-lifecycle simulated counter changes against the matched baseline; all peer totals and staged/C++ comparisons are in [profiles.json](profiles.json). Source attribution is exclusive and includes inlining, so it is not a full component cost.

| Transport / case | Instructions | Conditional misses | D1 read misses | D1 write misses |
|---|---:|---:|---:|---:|
| tls / deep-32-concurrent-32 | -0.01% | -3.53% | -1.14% | +0.07% |
| tls / dependent-read | -0.12% | -0.78% | -3.80% | -2.95% |
| tls / fanout-32 | +0.01% | +0.37% | -3.04% | +1.26% |
| tls / pipelined-read | -0.06% | -2.73% | -2.31% | -3.41% |
| quic / deep-32-concurrent-32 | +0.09% | -0.92% | -2.84% | -2.21% |
| quic / dependent-read | -0.08% | +0.42% | +1.99% | -1.38% |
| quic / fanout-32 | -0.03% | -0.67% | +1.41% | -1.24% |
| quic / pipelined-read | -0.02% | +0.63% | -0.93% | +0.63% |

Exclusive instruction attribution to `waiting.rs` falls from 697,693 to 558,162 in TLS fanout and from 264,912 to 213,602 in TLS pipeline (about 20%). Other work and code generation changes offset that reduction in the complete lifecycle. This attribution includes inlining and is not a whole-component timing.

Instruction totals are effectively flat at this scope. The retained TLS candidate reduces simulated D1 read misses in all four focused cases, while other cache/branch metrics remain mixed. The guaranteed result is reduced metadata and redundant queue bookkeeping, not a broad CPU or latency speedup.

## Ciphertext read/write counts

A case meets the following count criterion only if none of its four mean metrics is higher: positive writes, positive reads, attempted writes and attempted reads. See [ciphertext-counts.json](ciphertext-counts.json) for every mean/SD/min/max and [io-comparison.json](io-comparison.json) for all exceptions.

| Comparison | No higher mean in all four metrics |
|---|---:|
| tls-vs-portable-baseline | 21/24 |
| tls-vs-legacy | 16/24 |
| tls-vs-cpp | 19/24 |
| quic-vs-portable-baseline | 15/24 |
| quic-vs-legacy | 4/24 |

The primary baseline exceptions include TLS four-service/eight-client positive writes of 432.2 ± 9.18 versus 426.6 ± 10.26, and eight-service/eight-client writes of 660.2 ± 15.19 versus 651.0 ± 14.46. QUIC bulk-256k writes are 231.0 ± 4.47 versus 226.8 ± 1.48; attempted reads are 705.2 ± 13.66 versus 690.2 ± 5.93. These higher observed means remain failures of the strict count criterion despite overlapping spread.

These are socket syscall counts, not RPC frame counts. QUIC packet scheduling and recovery can vary across trials. Universal IO parity with legacy/C++ is not established.

## Validation

Passed: **725 core tests**, **86 fixed-storage conversations**, **89 growing-storage conversations**, **36 targeted integrations**, and **206 encrypted compatibility/authentication checks**. Embedded checking, strict all-target Clippy for six RPC/consumer crates, and formatting pass.

Exact commands, logs, passed counts, corrected failures and exclusions are in [validation.json](validation.json). This pass reruns the affected core tests, standalone fixed/growing conversations, encrypted and pipeline/Unix/multiparty integrations, embedded checking, strict all-target Clippy for the six affected crates, formatting and encrypted compatibility/authentication checks. The previous report retains the broader root/API qualification; those checks are not newly claimed here. Coverage and complete Verus/Kani execution were not run.

## Remaining performance work

The data does not meet every earlier universal speed/IO target. Readiness/coordinator and buffer/output work remain substantial; shrinking metadata does not remove their per-call scheduling and ownership operations. Same-peer projected parameter capabilities still need transactional reflected `ReceiverAnswer` publication, including grant/reference accounting, attachments and rollback, as documented in the previous architecture report. Legacy remains available. Gap #11 and draft PR #12 remain open.

## Evidence

[evidence-index.json](evidence-index.json) records the six frozen binaries, source/patch hashes and source verification. [archive.json](archive.json) records the compressed payload and verification. Earlier sealed evidence was not changed. Native raw samples, host states, certificates, full traces, profiles, commands and validation logs are retained in the archive. Certificates are generated local test fixtures.

Archive: `target/portable-structures-20261022.tar.zst`, 212,502,219 bytes. SHA-256: `12f12127c538f8c227f1ceb072eb686a4523a84e5661d205639379e7e06adc36`. Verified payload files: 22,596.

The archive includes `REPRODUCE.md`, build settings, source overlays and complete stage executables. Final production source matches the measured `links` snapshot.
