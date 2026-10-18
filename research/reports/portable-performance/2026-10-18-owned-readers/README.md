# Portable owning-reader cache, October 18, 2026

Tracking: [gap #11](https://github.com/DericHuynh/capntproto/issues/11), [draft PR #12](https://github.com/DericHuynh/capntproto/pull/12). The active target remains portable pooled p50 ≤ min(current legacy, fixed C++) in all 24 equivalent cases, with positive reads/writes and attempts ≤ C++ separately for every process capture. Legacy remains available.

**13/24 cases meet the latency target; the frozen previous portable meets 9/24 against these same current references.** Read/write parity is evaluated separately below. These are local same-host TCP measurements, not remote-host qualification.

## Architecture and implementation

Application reads previously returned through a generic segment provider on every near-pointer bounds check. C++ stores a direct segment descriptor (`vendor/capnproto/c++/src/capnp/arena.h`, `SegmentReader::checkOffset/checkObject`); legacy buffered input also caches its first/second segment ranges in `Frame`. The portable admission validator already had a short-lived cache, but that cache could not escape into a response hook.

`CheckedPointerReader::into_cached()` now moves the reader arena into an immutable Rc owner before caching segment zero. `CachedPointerReader` retains that stable owner while application views borrow it. No payload bytes are copied. The existing checked offset/extent operations, far-segment fallback, traversal limiter and nesting limits remain shared. This additive API is local to a thread, like Rc. Existing movable checked readers keep their original behavior; the new owning cache explicitly retains the segment-zero extent captured at conversion.

The cache is connected to both `PlainParams`/`PlainResults` in the wire crate and the general admitted payload wrapper in the hosted RPC crate. Constructing a plain result cache happens before committing its pending question transition. An initial wrapper-only candidate missed the plain paths and increased whole-process instructions by 0.00–0.03%; it was superseded before native timing. Its source, binary, profiles and failed test attempts remain available.

The owning cache costs one additional Rc allocation per retained payload. The five-run matrix includes shallow calls and all regressions, so deep traversal gains do not stand in for a universal speedup. No unsafe exception allowance was added.

### Remaining architectural issues

- **Receive buffers still pay builder initialization costs.** `Coordinator::drive` reserves a cleared buffer with `Messages::reserve_exact` before a streamed frame overwrites it. Fresh 1 MiB server Callgrind attributes 688,458 instructions (10.84% of 6,349,688) to memory clearing from `reserve_length`; total memset instructions are 1,384,996 versus legacy’s 711,027. A dedicated receive reservation should keep partial bytes private, permit sealing only after complete framing, and preserve cancellation/discard behavior. Ordinary builder reservations must retain zero-initialization. This is a separate optimization, not implemented in this change.
- **Portable eagerly walks payload graphs during admission.** `owned.rs` and `owned/question.rs` validate every capability index before exposing admitted parameters/results, then permit application reads with unlimited traversal budgets. Pinned C++ `rpc.c++` (call admission around line 3893 and result delivery around 4081) receives the capability table and retains the content reader; `ReaderCapabilityTable::extractCap` checks an index when used. Removing the extra portable graph walk would change the admission/error-timing contract and requires bounded lazy application readers plus capability-slot ownership checks. The cache preserves the current contract; this remains a larger architectural opportunity.
- **Physical write scheduling remains coupled to failure and completion observation.** The earlier whole-network deferral experiment masked a network-failure notification and remains rejected. Improving startup/control-message batching requires a bounded writer-publication phase while the lifecycle/failure observer continues to run immediately, including cooperative flush and backpressure. This cache does not change transport publication.
- **Plain and general admission have separate ownership paths.** Both now use the same core reader cache, but future changes must exercise both paths; changing only the hosted wrapper misses the common capability-free path.
- **Hosted pointer alerts remain unresolved.** The previous commit reports 12 CodeQL alerts (8 critical, 4 high), including four in primitive-list validation. Local Miri success does not dismiss them. The draft PR remains open for triage.

## Five-run comparison

Each cell has five independent process runs, 1,000 retained samples per run, and unchanged fixture/warmup budgets (100 for workflows/topologies, 10,000 for loopback). All 480,000 samples and 240 mixed compatibility checks are retained. Native timing is sequential and separate from compilation, tests and instrumentation. Order rotates/reverses and is recorded; five runs cannot exactly balance four variants across every position.

Times are microseconds: pooled mean ± sample SD across the five run means, not a confidence interval. Per-sample SD, run means/ranges, p50/p95/p99, rates and paired comparisons are in [report.json](report.json). The before column is the frozen `f516e150` portable binary; current legacy and portable share the selected executable.

| Case | Legacy mean ± SD | Portable before mean ± SD | Portable mean ± SD | C++ mean ± SD | Portable p50 / faster reference | Meets target |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| unary-empty | 27.819 ± 5.263 | 30.058 ± 4.884 | 30.703 ± 4.728 | 32.584 ± 4.759 | 1.388 | no |
| unary-small | 27.703 ± 5.041 | 26.446 ± 5.572 | 26.718 ± 5.669 | 30.791 ± 5.757 | 0.946 | yes |
| unary-1k | 33.043 ± 6.443 | 30.652 ± 5.549 | 29.514 ± 6.372 | 31.049 ± 5.407 | 0.960 | yes |
| fanout-32 | 120.875 ± 21.841 | 152.607 ± 22.023 | 145.837 ± 22.598 | 165.408 ± 22.493 | 1.252 | no |
| dependent-read | 68.668 ± 13.158 | 76.739 ± 17.600 | 79.485 ± 12.166 | 65.179 ± 11.930 | 1.088 | no |
| pipelined-read | 38.544 ± 5.483 | 51.009 ± 5.083 | 59.632 ± 12.767 | 39.539 ± 6.498 | 1.628 | no |
| write-read | 48.092 ± 2.013 | 46.506 ± 3.406 | 47.511 ± 3.817 | 51.986 ± 10.498 | 0.960 | yes |
| callback | 59.431 ± 8.468 | 60.415 ± 8.476 | 61.803 ± 8.844 | 58.726 ± 10.771 | 1.104 | no |
| bulk-64k | 58.644 ± 6.035 | 80.336 ± 30.116 | 73.480 ± 24.986 | 83.886 ± 31.939 | 1.145 | no |
| bulk-256k | 246.463 ± 128.523 | 200.704 ± 108.383 | 170.563 ± 123.825 | 325.244 ± 53.824 | 0.332 | yes |
| bulk-1m | 430.444 ± 85.049 | 645.846 ± 310.103 | 374.377 ± 25.202 | 1084.022 ± 232.028 | 0.874 | yes |
| bulk-4m | 2410.088 ± 632.066 | 3192.767 ± 596.859 | 3344.911 ± 840.937 | 4747.805 ± 395.790 | 1.386 | no |
| loopback-0 | 27.515 ± 6.854 | 31.720 ± 6.020 | 27.919 ± 7.966 | 31.313 ± 4.848 | 0.968 | yes |
| loopback-64 | 30.608 ± 6.899 | 31.430 ± 5.540 | 27.177 ± 5.726 | 31.317 ± 5.591 | 0.674 | yes |
| loopback-1024 | 26.959 ± 5.113 | 29.079 ± 6.086 | 27.492 ± 6.878 | 29.264 ± 6.892 | 0.972 | yes |
| loopback-65536 | 77.209 ± 31.254 | 89.383 ± 35.128 | 89.065 ± 33.182 | 70.472 ± 30.972 | 2.018 | no |
| deep-8 | 32.931 ± 5.395 | 34.112 ± 2.597 | 33.507 ± 4.552 | 34.793 ± 5.245 | 0.981 | yes |
| deep-32 | 36.273 ± 6.647 | 37.637 ± 2.177 | 33.430 ± 7.030 | 35.577 ± 4.755 | 0.917 | yes |
| concurrent-32 | 106.454 ± 6.481 | 128.918 ± 8.415 | 130.721 ± 7.559 | 157.725 ± 26.597 | 1.256 | no |
| deep-32-concurrent-32 | 260.349 ± 27.810 | 346.516 ± 25.959 | 346.171 ± 4.880 | 318.555 ± 30.599 | 1.269 | no |
| services-4-deep-16 | 162.987 ± 7.954 | 161.401 ± 11.696 | 161.983 ± 11.401 | 143.586 ± 13.731 | 1.090 | no |
| services-4-deep-16-concurrent-32 | 1140.552 ± 35.542 | 1031.569 ± 15.696 | 970.467 ± 60.375 | 1196.992 ± 54.098 | 0.845 | yes |
| services-4-deep-16-clients-8 | 1141.702 ± 24.019 | 977.528 ± 39.823 | 985.325 ± 27.061 | 1110.451 ± 51.602 | 0.883 | yes |
| services-8-deep-16-clients-8 | 2499.191 ± 32.687 | 1763.976 ± 47.818 | 1813.937 ± 116.739 | 2286.482 ± 98.304 | 0.731 | yes |

Broad run spreads limit causal interpretation of individual timing changes. Pass counts describe this campaign; they do not establish a universal latency improvement.

## Separate confirmation

The main matrix above remains unchanged. A separate five-run follow-up retains another 100,000 samples, concentrating on the pipeline regression, bulk transfers and deep concurrency. Its statistics are recorded separately; these runs are not pooled with or substituted for the primary matrix.

| Case | Before mean ± SD | Portable mean ± SD | Mean change | p50 change |
| --- | ---: | ---: | ---: | ---: |
| pipelined-read | 54.243 ± 13.605 | 47.342 ± 3.954 | -12.7% | -2.3% |
| bulk-1m | 382.103 ± 49.496 | 388.911 ± 50.152 | +1.8% | +1.1% |
| bulk-4m | 3048.112 ± 305.312 | 3024.312 ± 330.260 | -0.8% | -1.1% |
| deep-32-concurrent-32 | 308.244 ± 20.525 | 282.683 ± 7.010 | -8.3% | -6.1% |
| services-4-deep-16-concurrent-32 | 973.324 ± 44.681 | 914.318 ± 43.943 | -6.1% | -9.6% |

The pipeline regression and the large 1 MiB gain did not repeat; both sweeps remain visible. Deep-concurrent latency was nearly unchanged in the primary matrix, then 8.3% lower in confirmation. The four-service concurrent mean was 5.9% lower in the primary matrix and 6.1% lower in confirmation. Instruction savings support reduced traversal work, not a uniform wall-clock speedup.


## Profiles and physical IO

Callgrind totals include complete process startup, fixture setup, diagnostic warmup, measured operations and teardown. These short diagnostic budgets are identical between variants; instrumented latency never enters the native comparison.

| Case | Process | Legacy instructions | Before | Portable | C++ | Portable / before |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| deep-32 | client | 3123153 | 3262463 | 3224030 | 2710141 | 0.9882 |
| deep-32 | service-0 | 3126373 | 3263605 | 3185951 | 2978269 | 0.9762 |
| concurrent-32 | client | 6855293 | 9311837 | 9340174 | 11135943 | 1.0030 |
| concurrent-32 | service-0 | 6438960 | 7070848 | 7058341 | 11230463 | 0.9982 |
| deep-32-concurrent-32 | client | 16209220 | 21500819 | 20282918 | 18739199 | 0.9434 |
| deep-32-concurrent-32 | service-0 | 20162832 | 24341592 | 21853060 | 23202044 | 0.8978 |
| bulk-1m | client | 19376353 | 20912468 | 20909421 | 21563187 | 0.9999 |
| bulk-1m | server | 5926536 | 6354286 | 6350433 | 6004353 | 0.9994 |
| bulk-4m | client | 67142518 | 71101025 | 71105170 | 71321315 | 1.0001 |
| bulk-4m | server | 11178581 | 14306315 | 14307451 | 11475358 | 1.0001 |
| fanout-32 | client | 23124786 | 31821483 | 32032817 | 43868715 | 1.0066 |
| fanout-32 | server | 19821721 | 21861007 | 21985857 | 43208554 | 1.0057 |
| pipelined-read | client | 10464706 | 17123322 | 17157435 | 15173808 | 1.0020 |
| pipelined-read | server | 13072499 | 15156988 | 15211553 | 17181545 | 1.0036 |

Every selected benchmark has fresh lightweight socket and full wire captures. Counts include startup, warmup, measurements, EOF/EAGAIN and teardown. Each process is compared independently; counts are never pooled to hide a failing capture. Received frame matching is checked separately from call-count parity.

| Instrument | Positive read/write passes | Attempt passes | Process comparisons |
| --- | ---: | ---: | ---: |
| socket | 8 | 11 | 64 |
| wire | 6 | 7 | 64 |

## Validation and provenance

- Core/RPC/allocating wire libraries: **passed** — 78 + 319 + 147 tests; 544 total.
- Selected-reader ownership, bounds and budget regressions: **passed** — 16 native tests and all 16 under Miri, including two new owning-cache regressions.
- Allocation-free configurations: **passed** — Core no-default-features and alloc-only; wire no-default-features.
- Strict workspace Clippy and unsafe documentation gate: **passed** — All targets. Exception counts unchanged; reviewed hashes refreshed for message.rs and arena.rs.
- Formatting and source replay: **passed** — Root format and exact source patch replay; frozen binary source files remain identical.
- First new nesting test: **failed-corrected** — Asserted the list nesting error for a struct; corrected to the existing struct error. All regression tests then passed.
- First Clippy invocation: **failed-corrected** — Build-cache cleanup removed a proc-macro dependency during Clippy. Rebuilt and reran; subsequent cleanup occurred only after builds finished.
- Unsafe inventory hash check: **failed-corrected** — Source edits changed two recorded hashes, with identical diagnostic counts. Reviewed and refreshed only those hashes; gate then passed.
- Native evidence and wire matching: **passed** — 580,000 native samples independently verified, including five-run means/spreads/percentiles, budgets, order and executable hashes; 240 compatibility checks; 48 original wire pairs and 228 directed topology edges match received frames.
- Repository, wiki and workflow policy: **passed** — Repository check, 104 project documents and workflow dependency checks pass; report-relative links checked separately.
- Full coverage and full C++ correctness suites: **not-run-locally** — Targeted ownership/protocol checks and fixed C++ benchmark compatibility exercised; no coverage claim.

The [exact source patch](source.patch) applies to `f516e1509f3cf724722e4a93f768c252f317ed90` and reconstructs source tree `fbcc8d3ac6dac267c9e88231040784d30aa35478`. Frozen binary hashes, raw profiles, per-process counters, statistics, incomplete-candidate evidence and validation logs are recorded in report.json and the evidence archive. The fixed C++ sources remain pinned at `0de72d8d8cec6b69edaa29de51d3bd490341f9c2` with clang 19. Fresh hosted checks after publication are separate from these local checks.

Sealed evidence: `/workspace/capntproto/target/portable-owned-readers-20261018` — 6,349 files, 636,882,620 logical bytes; SHA256SUMS SHA-256 `f717e6f9dfeefd401027a19d27ea287079d41db99d1ca833e3e189311297bb10`. Older archives verified unchanged.
