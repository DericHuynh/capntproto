# Portable deep/concurrent comparison, October 18, 2026

Tracking: [portable gap #11](https://github.com/DericHuynh/capntproto/issues/11), [draft PR #12](https://github.com/DericHuynh/capntproto/pull/12). The active goal is portable pooled p50 ≤ min(current legacy, fixed C++) in every equivalent case, with reads/writes and attempts ≤ C++ independently for every process capture. The earlier 5% and 30% margins are not substituted for this latest target.

**The target remains unmet: 9/24 latency cells, including 4/8 deep/concurrent cells, meet the faster reference in the final local campaign.** Legacy remains available. The measurements below are same-host TCP; they do not qualify cross-host performance.

## Implementation

All eight deep/concurrent fixtures now select portable on every client, incoming server connection and downstream service connection. Readiness and result JSON expose the actual driver. The runner compares legacy, portable and C++ by default, checks all six ordered implementation pairs in alternating service chains, and accepts a frozen portable control with `--portable-baseline`. Default repetitions cover two balanced variant cycles. Payloads, depth, concurrency, validation, deadlines and sampling budgets are unchanged.

Two production changes are retained. Streaming frames now give the transport a remaining-body readiness hint bounded by the next caller slice; they restore ordinary readiness before completing the frame. Hint and reset failures remain terminal, including across cancellation. Capability-index validation iterates through the final struct pointer after recursively validating earlier siblings. This removes recursive tail calls from linked data-plus-next nodes while preserving bounds, capability, nesting and traversal-budget checks and error order. No unsafe blocks or exception allowances are added.

## Matched latency results

The final matrix retains **768,000 uninstrumented samples**: four variants × eight balanced repetitions × 1,000 samples × 24 cases. Workflows and topologies use 100 warmups; loopback uses 10,000. Current legacy and portable use the same selected binary; the frozen portable control contains topology support but unchanged production code from `c7edff6`. C++ is fixed. There are 240 successful peer-compatibility checks (192 original workflow combinations, including homogeneous controls, plus 48 mixed topology chains).

The table reports pooled p50 in microseconds. A topology sample measures the whole concurrent batch, including request construction, all service hops, response validation and destruction. Ratios use the faster current reference. All p95/p99 values, throughput, repetition medians and per-repetition candidate/control ratios remain in [report.json](report.json).

| Case | Legacy µs | Portable before µs | Portable µs | C++ µs | Portable / faster reference | Meets target |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| unary-empty | 22.654 | 22.153 | 30.386 | 31.458 | 1.341 | no |
| unary-small | 31.406 | 31.116 | 30.956 | 32.089 | 0.986 | yes |
| unary-1k | 32.990 | 31.707 | 31.658 | 33.400 | 0.960 | yes |
| fanout-32 | 133.570 | 151.157 | 151.798 | 167.671 | 1.136 | no |
| dependent-read | 69.414 | 74.001 | 74.482 | 68.753 | 1.083 | no |
| pipelined-read | 40.531 | 51.688 | 53.010 | 40.641 | 1.308 | no |
| write-read | 64.386 | 62.254 | 61.693 | 64.908 | 0.958 | yes |
| callback | 67.200 | 70.025 | 70.366 | 65.398 | 1.076 | no |
| bulk-64k | 106.170 | 104.797 | 107.492 | 108.133 | 1.012 | no |
| bulk-256k | 122.264 | 144.097 | 317.596 | 376.486 | 2.598 | no |
| bulk-1m | 344.336 | 591.061 | 372.399 | 921.152 | 1.081 | no |
| bulk-4m | 2057.484 | 2876.462 | 2816.117 | 4451.483 | 1.369 | no |
| loopback-0 | 24.056 | 30.826 | 30.877 | 32.058 | 1.284 | no |
| loopback-64 | 31.828 | 30.976 | 31.247 | 32.078 | 0.982 | yes |
| loopback-1024 | 32.268 | 21.182 | 31.457 | 22.513 | 1.397 | no |
| loopback-65536 | 50.836 | 69.263 | 48.252 | 65.619 | 0.949 | yes |
| deep-8 | 22.724 | 22.884 | 22.414 | 23.315 | 0.986 | yes |
| deep-32 | 33.330 | 31.026 | 28.653 | 27.962 | 1.025 | no |
| concurrent-32 | 100.160 | 125.658 | 125.327 | 143.465 | 1.251 | no |
| deep-32-concurrent-32 | 253.270 | 396.745 | 362.623 | 266.289 | 1.432 | no |
| services-4-deep-16 | 138.087 | 145.849 | 142.122 | 134.662 | 1.055 | no |
| services-4-deep-16-concurrent-32 | 1110.493 | 1037.533 | 996.363 | 1143.010 | 0.897 | yes |
| services-4-deep-16-clients-8 | 1129.679 | 1014.664 | 934.163 | 1091.494 | 0.856 | yes |
| services-8-deep-16-clients-8 | 2416.182 | 1839.862 | 1774.001 | 2284.840 | 0.776 | yes |

Portable deep-32 pooled p50 falls **7.6%** against its frozen control; deep-32-concurrent-32 falls **8.6%**. The concurrent microservice chains improve **4.0%, 7.9% and 3.6%**, respectively. Portable is 10.3–22.4% below the faster reference in those three service-chain cases, but remains 25.1% behind legacy on shallow concurrency and 43.2% behind on deep concurrency. These are observations from the matched local campaign, not a universal speedup claim.

Bulk-1m improves **37.0%** pooled versus frozen portable, winning seven of eight paired repetitions. The earlier balanced screen improved 48.6%, with all eight repetitions faster. Bulk-4m improves only 2.1% in the final campaign and still trails legacy by 36.9%. Pipeline pooled latency regresses 2.6%; its earlier screen regressed 10.7%. Both remain evidence, not filtered outliers.

**Timing modes materially affect pooled medians.** The final bulk-256k pooled p50 is 317.596 µs versus 144.097 µs before, a 120.4% regression; the median paired repetition ratio is 1.077. A separate 24-repetition balanced confirmation retains another 96,000 samples: portable 109.044 µs, frozen portable 115.144 µs, legacy 107.203 µs and C++ 293.230 µs. The confirmation median paired ratio is 1.007, with portable faster in 11/24 pairs. It does not replace the failed full-matrix result or establish a stable 256 KiB gain. Unchanged controls also switch modes. Unary-empty and loopback-1024 pooled regressions are retained; neither is explained away as an optimization benefit.

The initial topology baseline (144,000 samples) and two focused screens (96,000 samples each) remain separate. This campaign retains **1,200,000 native samples** in total and 48 additional baseline mixed-chain checks. Instrumented timings never enter native latency qualification.

## Read/write traces and profiles

Every original and topology case was captured separately with the lightweight socket probe and full wire tracing. Comparison includes startup, warmup, measured calls, EOF/EAGAIN and teardown. The earlier three-case captures of the exact selected binary are also included. Each instrument therefore has **70 process comparisons**: 32 original, 32 topology, and six preceding selected captures. Counters are never pooled to hide a process or repetition failure.

| Instrument | Both positive read/write counts pass | Both attempt counts pass | Captures |
| --- | ---: | ---: | ---: |
| Lightweight | 11 | 12 | 70 |
| Full wire trace | 7 | 8 | 70 |

In the paired bulk probe, 1 MiB client/server positive reads fall from **974/1,034 to 332/336** (about two-thirds fewer). At 4 MiB they fall from **2,316/2,732 to 676/659** (71–76% fewer). Writes are unchanged by the body hint. These are diagnostic process totals, not latency measurements.

Parity is still far from complete. In the final lightweight concurrent-32 capture, portable makes **15 client writes versus C++ 13**, and **14 server reads versus C++ 13**. The full deep-concurrent capture also has extra reads on both peers. The first Bootstrap Return can arrive before the first Call Return, causing portable to send a standalone bootstrap Finish; C++ batches those returns and subsequent control messages. The fixture’s deliberate invalid-payload preflight participates in this startup behavior and is retained. No startup or cleanup messages are excluded.

All received wire frames match their sending peer: 48 pairs in the final original sweep, nine in the preceding selected sweep, and 228 directed TCP edges in the final topology sweep. Unread final Abort suffixes are recorded separately. The baseline wire campaign also validates 228 topology edges and nine original pairs.

Whole-process Callgrind includes startup and teardown at fixed diagnostic budgets. Deep-concurrent portable client/server instructions fall **4.20%/3.75%**; shallow concurrent instructions rise **0.18%/0.24%**. Pipeline changes are +0.04%/+0.07%, and bulk instruction changes are below 0.1%. The large bulk timing/read improvement therefore comes mainly from readiness and read grouping, while deep validation removes CPU work.

## Architectural limits and rejected experiment

- Portable still validates outgoing and incoming capability indices and traverses shared-message readers through repeated segment/bounds operations. The paired deep profiles identify these validators and `ReaderArenaImpl<NoAllocBufferSegments<SharedBytes>>::check_offset_and_read` as substantial costs. The tail iteration helps; it does not remove ownership and checked-access overhead. Any stable reader cache must preserve immutable segment ownership, traversal charging and validity after moving the owner.
- Network lifetime/error observation and physical output scheduling are still coupled. Moving the combined network future to the deferred writer lane broke `portable::tests::network_failure_is_not_masked_by_an_already_completed_empty_disconnector` (318 passed, one failed). That experiment is fully reverted before release build/timing; the existing contract is unchanged. A useful redesign needs an independently observed failure/completion future plus a bounded, deferred publication turn, including cooperative flush and backpressure paths.
- Whole-network deferral cannot be accepted merely to reduce startup writes. Control-only progress, disconnect completion, callback progress and partial large-frame reads all need to retain their liveness guarantees. The recorded failure and patch are preserved.

## Validation and provenance

- test-libraries: passed — 77 core + 72 futures + 319 RPC library tests.
- test-wire: passed — 96 wire library tests.
- clippy: passed — Strict all-targets Clippy: core, futures, RPC.
- miri-validator: passed — branched_struct_tails_preserve_validation_order_and_limits.
- miri-stream: passed — streamed_read_hints_bound_slices_and_reset_before_completing.
- check-noalloc: passed — core no-default-features.
- check-alloc: passed — core alloc without defaults.
- rejected-network-deferral: failed-reverted — 318 passed, 1 failed; selected code passes all 319.
- initial-core-filter: zero-tests-superseded — Incorrect test filter; replaced by full core library and focused Miri runs.
- coverage: not-run-locally.
- unsafe: passed — Strict workspace all-targets Clippy with unchanged unsafe documentation inventory.
- repository: passed — Repository policy checks.
- wiki-staged: passed — 104 tracked project documents.
- format: passed — Root and benchmark Rust formatting.
- wiki-before-stage: failed-superseded — New report link was checked before new tracked files were staged; passed after staging.
- benchmark-clippy: passed — Strict all-targets Clippy for standalone benchmark crate.
- independent-evidence-verification: passed — All 1,200,000 samples, balanced order, budgets, throughput, tails, wire edges, hashes, exact patch and report links.

The four library suites total **564 passing tests**. New tests cover fragmented/Pending streaming reads, caller slice bounds, reset and hint failures, canaries, successor frames, deep branched capability trees, invalid sibling order, cycles and traversal/nesting budgets. Both new tests pass under Miri. The initial incorrect core-test filter ran zero tests and is explicitly superseded by the complete core suite and focused Miri test. Coverage was not rerun locally and its thresholds are unchanged.

Selected binary SHA-256: `ccb2441c34c48bda690e5e4bf66fa0d1ecf507cf933d52b5dcc8819c55ef79a2`. Frozen portable control: `fad68437196d076c886f2a4692a4bdfd5757676575bc7e8e0e8b9c627e25e84d`. Both source snapshots replay against `c7edff6294888a0089982839874d7e3ae81e59e0`; all selected production hashes match the compiled snapshot. The core regression test alone changed from a Vec to a fixed array after release compilation; production code is identical. The [exact reviewed source patch](source.patch) includes that no-alloc-compatible test amendment. Source trees and hashes are in [report.json](report.json).

C++ uses pinned upstream `0de72d8d8cec6b69edaa29de51d3bd490341f9c2` and the unchanged original/topology fixture binaries; their hashes are retained. No fixture, release setting, coverage floor or unsafe exception is relaxed.

The preceding c7edff6 dedicated workflow completed successfully, including its reported resource-cleanup step. It measured the preceding legacy scope and is not portable qualification. Keep the PR in draft and portable gap open until every required cell and correctness gate passes.

Immutable raw archive: `/workspace/capntproto/target/portable-topologies-20261018`; **11,731 files / 729,209,287 logical bytes**; manifest SHA-256 `9dca1debce70ead60bbd13ce2e77c03127342963b12ce95ebcecd9f267ca40c2`. All five preceding legacy archives verify unchanged.
