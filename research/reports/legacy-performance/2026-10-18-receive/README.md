# Owning receive buffers and bounded readiness, October 18, 2026

Tracking: [gap #15](https://github.com/DericHuynh/capntproto/issues/15), [draft PR #12](https://github.com/DericHuynh/capntproto/pull/12). Continues the [layout report](../2026-10-18-layout/README.md).

**The requested target remains unmet.** The final local campaign observes **5/24** pooled-p50 cells at Rust/C++ ≤ 0.70. Individual read/write count misses remain. All repeated captures, including failed comparisons, are retained. The separate frozen-legacy 2× target remains open and was not rerun.

## Retained implementation

Released owning message allocations can replace an empty receive buffer. A complete large frame occupying the whole exclusive buffer then transfers its allocation to the returned message while restoring the cached buffer. This removes the repeated small prefix read/copy without borrowing storage from a live reader. Partial input, classifier errors, caller scratch, successor frames, cancellation and independent reader lifetimes remain checked. Known incomplete buffered frames now provide exact remaining-body hints and reset readiness before exposure.

TCP requests larger than 1 MiB drain immediately like KJ. Requests above 64 KiB and no larger than 1 MiB retain Tokio’s short-read readiness clearing, so a stale readiness flag does not bypass the framer’s next exact-body hint. Partial large reads still notify the deferred RPC scheduler. Linux SO_RCVLOWAT remains capped at 1 MiB, preserves receive autotuning and restores the original option on drop. No unsafe code or API break is added by the selected change.

With the default RPC buffer configuration, the unchanged 8 MiB bound applies to each automatically cached allocation. The receive buffer can now retain a large allocation too: asynchronous return of independently retained messages can leave two large allocations in owning-mode receive/cache storage, up to 16 MiB excluding live readers. Small-only connections retain their ordinary buffer size.

## Latency and profiling

The final comparison retains **656,000 uninstrumented samples**: 528,000 from selected Rust, previous Rust and C++ plus 128,000 from the rejected allocator candidate. Original cases have eight balanced repetitions across four variants; the eight topology cases have six repetitions across three variants. Each variant occupies each position twice. There are **208 mixed-peer checks**. Workflows/topologies retain 100 warmups and 1,000 samples; loopback retains 10,000 warmups and 1,000 samples. Fixtures, validation, deadlines and fixed C++ binaries are unchanged. All tails, throughput, repetition medians and rejected-candidate results are in [report.json](report.json).

Against frozen Rust, 4 MiB pooled p50 improves **15.4%** and is faster in **all eight repetitions**. The 1 MiB pooled result improves **8.3%**. The 256 KiB result is **13.6% slower**, while still exceeding the requested C++ margin. Small-message and loopback modes vary in unchanged controls: the apparent loopback-65536 target pass also occurs in frozen Rust and is not a gain attributable to this change.

| Case | Previous Rust µs | Selected Rust µs | C++ µs | Rust/C++ | ≤0.70 |
| --- | ---: | ---: | ---: | ---: | --- |
| unary-empty | 21.512 | 21.593 | 21.772 | 0.992 | no |
| unary-small | 21.402 | 21.282 | 21.933 | 0.970 | no |
| unary-1k | 22.334 | 21.883 | 27.442 | 0.797 | no |
| fanout-32 | 126.141 | 128.473 | 161.473 | 0.796 | no |
| dependent-read | 48.343 | 48.783 | 48.122 | 1.014 | no |
| pipelined-read | 31.437 | 31.508 | 30.836 | 1.022 | no |
| write-read | 43.045 | 43.275 | 44.207 | 0.979 | no |
| callback | 47.281 | 49.655 | 45.409 | 1.094 | no |
| bulk-64k | 59.219 | 51.668 | 53.601 | 0.964 | no |
| bulk-256k | 94.012 | 106.771 | 250.967 | 0.425 | yes |
| bulk-1m | 391.123 | 358.690 | 923.078 | 0.389 | yes |
| bulk-4m | 2396.916 | 2028.162 | 4492.439 | 0.451 | yes |
| loopback-0 | 21.743 | 21.573 | 21.552 | 1.001 | no |
| loopback-64 | 30.906 | 31.918 | 31.968 | 0.998 | no |
| loopback-1024 | 32.269 | 31.357 | 32.719 | 0.958 | no |
| loopback-65536 | 50.457 | 50.516 | 102.394 | 0.493 | yes |
| deep-8 | 22.704 | 22.724 | 23.055 | 0.986 | no |
| deep-32 | 27.070 | 27.151 | 27.601 | 0.984 | no |
| concurrent-32 | 85.928 | 86.559 | 131.507 | 0.658 | yes |
| deep-32-concurrent-32 | 217.326 | 221.472 | 258.978 | 0.855 | no |
| services-4-deep-16 | 133.500 | 127.891 | 119.850 | 1.067 | no |
| services-4-deep-16-concurrent-32 | 1089.915 | 1087.370 | 1103.284 | 0.986 | no |
| services-4-deep-16-clients-8 | 1090.382 | 1103.966 | 1077.035 | 1.025 | no |
| services-8-deep-16-clients-8 | 2429.314 | 2412.726 | 2276.545 | 1.060 | no |

The selected 256 KiB repetition medians are 103,006; 329,028; 102,805; 105,299; 117,377; 336,235; 99,400; and 101,522 ns. The frozen Rust control includes a 367,223 ns repetition. All repetitions contribute to the reported pooled result. The earlier all-drain candidate had two 1 MiB repetitions above 1.3 ms; these remain in the preceding-candidate evidence.

Callgrind shows selected 1 MiB client/server instructions falling about **3.2%/10.8%** against previous Rust; 4 MiB falls about **0.9%/3.8%**. Unary/fanout instruction counts rise roughly **0.35–0.46%**. Allocator counts remain largely unchanged. These include process setup and fixed diagnostic budgets; they do not qualify native latency.

## Complete count evidence

| Instrument | Positive reads/writes pass | All attempts pass | Captures |
| --- | ---: | ---: | ---: |
| Lightweight | 184 | 184 | 184 |
| Full wire trace | 182 | 183 | 184 |

These totals cover **64 distinct process cases**: 32 original captures, 128 topology captures (four separate repetitions), plus 24 prior bulk captures of the exact selected binary. Every process and repetition is compared separately with its paired fixed C++ control. Startup, warmup, EOF/EAGAIN, teardown and unread cleanup are included. Each original full sweep validates 48 peer pairs; each topology sweep validates 912 directed TCP edges; the four selected bulk screens add 24 peer pairs. Every received frame must match its sender.

Before the final sweep, one selected-binary 4 MiB client made **14 writes versus C++ 13**: its first Bootstrap+Call write split into 4,157,547 and 36,965 bytes. A different repetition’s 4 MiB server made **49 positive reads versus C++ 42**, although its 54 attempts were below C++’s 57. These are distinct failures; the second is a read miss, not another partial write. All final-sweep failures and detailed per-process counters are retained in report.json.

The earlier all-drain candidate first passed 160/160 captures under each instrument, but three supplementary bulk repetitions exposed two 256 KiB server read misses and one extra 4 MiB server write. Across all its captures it passes **176/178 lightweight** and **177/178 full-trace** rows. The extra write was a partial first response (4,161,536 + 32,872 bytes), not teardown; both peers sent the same final Abort. The initial clean sweep cannot replace the failures.

## Experiments and architecture assessment

- Removing a consumed activity notification from writer deferral did not reduce unary reactor polls and added writes; reverted.
- Immediate reads alone did not fix owning-buffer behavior; borrowed-buffer promotion targeted a different API from the legacy owning path; reverted.
- The early owning-buffer prototype passed its limited bulk screen but had slower 1 MiB timing. It did not yet preserve caller-scratch selection for exact large frames; the selected implementation disables allocation transfer when caller scratch is present.
- Combining ownership reuse with immediate retry at every large size improved 4 MiB but fragmented 256 KiB requests. The selected bounded policy fixes those repeated lightweight 256 KiB misses while preserving the 4 MiB gain.
- The zeroed-allocation candidate changed compiled malloc-plus-memset to calloc, but yielded no useful 4 MiB timing improvement and had worse results in several pooled cells. It also included a small empty-cache guard and shared readiness constant, so this comparison does not isolate the allocator alone. Its read/write misses persisted. The whole candidate, including its two unsafe blocks, test additions and inventory hash change, is reverted.

The remaining architectural gap is end-to-end scheduling and transport behavior, not simply allocator count. The selected instruction reductions do not close small-call latency. Large-frame ownership now supports direct allocation transfer, but kernel fragmentation and partial writes still vary with peer drain timing. Future work must retain the complete process lifecycle in count comparisons and stable native repetitions; increasing buffer limits or omitting first-message events would not establish this target.

## Validation and provenance

- libraries: passed.
- core-docs: passed.
- core-noalloc: passed.
- core-alloc: passed.
- miri-framing: passed.
- tcp-unit: passed.
- tcp-integration: passed.
- root-tests: passed.
- interop: passed.
- schema-fixtures: failed (missing nextest; superseded by passing schema-fixtures-nextest).
- schema-fixtures-nextest: passed.
- format: passed.
- strict-workspace-clippy-and-unsafe-inventory: passed.
- repository: passed.
- documents-104: passed.
- report-links-counts-and-exact-patch: passed.

The earlier combined receive candidate passed 287 root tests (four existing controls ignored), 71 framing tests and 319 RPC tests, plus both new ownership tests under Miri. Final selected-code results are listed above. New tests cover overlapping retained readers, exact allocation reuse, fragmented/canceled preparation, small successors, readiness reset errors and caller-scratch canaries. An initial new test-fixture offset mistake and an invalid no-default-features native integration command are retained with their corrected reruns. The selected schema filter also exercises embed bindings. Its initial three failures were caused by missing cargo-nextest; pinned 0.9.146 was installed with its release digest verified before rerunning. No coverage thresholds or unsafe exceptions change.

Preceding-commit hosted coverage finished with **2,323 passed, two failed and 20 skipped**. Both failures came from generated downstream interface fixtures implementing the old async server signature. This change migrates the ordinary and generic fixtures to CallContext/Promise while preserving inheritance, nested generics, streaming and pipelining assertions. The downloaded artifact hash verifies; local focused reruns do not substitute for fresh hosted coverage. All other preceding platform, documentation, safety and proof jobs passed.

Selected binary SHA-256: `8c545136647d79228f97655ba02254ee4281b277e489e0ab08e93dad18da0e72`. Compiled snapshot: `e3afd82ca237047a8595e44663c7ab81ba5c4095`; review snapshot with the downstream fixture fixes: `5d276210bc9c53c154610deaf805b9e492d4ea0b`; base: `ce60a8a5e0099bde3fc1388c026d635bff20ecfe`. Production file hashes match the compiled snapshot exactly. The [exact reviewed source patch](source.patch) includes the implementation and fixture migrations. Eight snapshots replay, and independent verification recomputes quantiles/throughput, checks frozen hashes, budgets, balanced order, every repetition and wire-frame correspondence.

Immutable raw archive: `/workspace/capntproto/target/legacy-receive-20261018`; **16,314 files / 2,215,123,517 logical bytes**; manifest SHA-256 `f483a775c4a8a610b59cae4bd85c9821372c70a16e85187c0e230d7aa874d504`. All four previous legacy archives verify unchanged.

The preceding ce60a8a commit completed [dedicated run 39084584326](https://github.com/DericHuynh/capntproto/actions/runs/39084584326), including verified resource cleanup. It achieved **0/12** remote workflow passes at the 0.70 ratio. Unary-empty was 103.241 µs versus C++ 105.419 µs, fanout 442.675 versus 495.307 µs, and 4 MiB 12.747 versus 13.347 ms. Both artifact hashes verify. Those are separate preceding-commit results and do not qualify the selected candidate. Keep the PR in draft and the performance gap open.
