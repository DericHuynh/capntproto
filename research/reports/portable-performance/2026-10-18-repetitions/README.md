# Five-run portable benchmarks, October 18, 2026

Tracking: [portable gap #11](https://github.com/DericHuynh/capntproto/issues/11), [draft PR #12](https://github.com/DericHuynh/capntproto/pull/12). The active goal remains portable pooled p50 ≤ min(current legacy, fixed C++) for all 24 equivalent cases, with positive reads/writes and attempts ≤ C++ independently for each process capture. Legacy remains available.

**The goal remains open: 12/24 cells meet the latency target in this campaign.** These are same-host TCP results, not dedicated remote-host qualification.

## Five repetitions and descriptive statistics

All topology defaults now use five independent process repetitions. Workflow, portable loopback, general loopback and long-tail comparisons already used five; their summaries now share the new statistics. Reports retain the pooled arithmetic mean, sample standard deviation and extrema, every repetition mean, its range, and sample standard deviation across repetition means. Markdown tables show **mean ± run SD**. SD uses n−1 and is not a confidence interval. Existing percentiles and advisory gates remain available; the added statistics do not replace the active goal above.

All samples are retained, including stalls. Five runs cannot exactly balance four implementations across every order position; the runner rotates/reverses order and records it. Native timing does not overlap builds, tests or profilers. The selected comparison contains **480,000 samples** (24 cases × four variants × five repetitions × 1,000), plus **240 successful compatibility checks**. Workflows and topologies retain 100 warmups; loopback retains 10,000. A topology sample includes the complete concurrent batch and every service hop.

## Selected optimization

Capability-index validation now checks primitive list fields inline before continuing through a struct’s final pointer. Primitive lists cannot contain capabilities or descendants, so the change removes recursive validator calls while retaining complete extent checks, nesting limits, read budgets, void-list amplification and sibling error order. Far pointers, pointer lists and composite lists retain their general decoder path. No public API or unsafe exception allowance changes. The new regression test compares valid and malformed primitive siblings against an independent general-reader oracle, including repeated validation to check remaining budgets. Validation also uncovered a test-harness cancellation defect on container kernels without `/proc/<pid>/task/<tid>/children`; process-group discovery now snapshots PPID/group relationships from `/proc/<pid>/stat`, preserving nested cleanup. This harness change does not affect the measured RPC executable.

The first candidate cached segment zero at the generic validator entry. Profiles showed that the measured hot validator already uses a cached arena; that candidate increased whole-process instructions by 0.02–0.16% and was **rejected and reverted**. Its separate 480,000-sample matrix, 240 compatibility checks, full traces, failed test attempts, corrected tests and patch are retained. Its timing results are never substituted for the selected implementation. The separate regression confirmation retains another 140,000 samples; total native evidence this turn is **1,100,000 samples**.

## Matched latency

Times below are microseconds, shown as pooled mean ± sample SD of the five run means. The final ratio uses pooled p50, consistent with the active target. The previous portable control is the frozen production implementation from `41b2c697`; current legacy and portable share the selected binary. All run means/ranges, sample SD, p50/p95/p99, throughput and paired repetition ratios are in [report.json](report.json).

| Case | Legacy mean ± SD | Portable before mean ± SD | Portable mean ± SD | C++ mean ± SD | Portable p50 / faster reference | Meets target |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| unary-empty | 32.118 ± 5.070 | 35.475 ± 10.872 | 31.743 ± 4.838 | 33.311 ± 5.034 | 0.982 | yes |
| unary-small | 49.036 ± 35.764 | 32.716 ± 3.906 | 31.530 ± 5.313 | 33.315 ± 5.221 | 0.976 | yes |
| unary-1k | 33.334 ± 5.966 | 32.703 ± 5.546 | 33.180 ± 4.902 | 35.126 ± 3.052 | 0.976 | yes |
| fanout-32 | 136.140 ± 20.990 | 155.942 ± 17.277 | 152.747 ± 8.704 | 184.777 ± 1.189 | 1.136 | no |
| dependent-read | 65.379 ± 12.655 | 72.268 ± 14.042 | 72.631 ± 13.270 | 64.085 ± 13.105 | 1.086 | no |
| pipelined-read | 37.571 ± 5.540 | 52.565 ± 5.778 | 56.533 ± 16.869 | 41.243 ± 6.681 | 1.397 | no |
| write-read | 61.513 ± 12.640 | 59.788 ± 13.467 | 59.978 ± 14.252 | 61.794 ± 13.890 | 0.967 | yes |
| callback | 63.401 ± 13.425 | 65.645 ± 13.213 | 76.384 ± 13.063 | 63.770 ± 10.959 | 1.086 | no |
| bulk-64k | 76.094 ± 31.404 | 76.815 ± 32.911 | 77.591 ± 34.604 | 81.736 ± 31.874 | 0.974 | yes |
| bulk-256k | 167.729 ± 105.004 | 176.098 ± 104.414 | 222.033 ± 113.069 | 326.636 ± 77.818 | 1.226 | no |
| bulk-1m | 525.894 ± 288.814 | 621.401 ± 512.969 | 891.289 ± 325.070 | 1285.188 ± 204.509 | 1.206 | no |
| bulk-4m | 2555.922 ± 369.338 | 3121.732 ± 512.109 | 3305.105 ± 447.285 | 4863.821 ± 701.258 | 1.383 | no |
| loopback-0 | 31.619 ± 4.498 | 33.583 ± 1.100 | 31.258 ± 5.148 | 30.629 ± 5.321 | 0.980 | yes |
| loopback-64 | 29.499 ± 6.294 | 29.128 ± 6.798 | 29.141 ± 6.755 | 32.866 ± 2.862 | 0.668 | yes |
| loopback-1024 | 27.510 ± 5.604 | 28.849 ± 6.459 | 32.198 ± 5.994 | 33.384 ± 5.917 | 1.429 | no |
| loopback-65536 | 89.732 ± 32.504 | 98.795 ± 26.302 | 89.856 ± 28.979 | 90.335 ± 31.514 | 0.995 | yes |
| deep-8 | 27.092 ± 5.134 | 29.035 ± 7.266 | 27.790 ± 5.869 | 33.073 ± 10.074 | 0.978 | yes |
| deep-32 | 37.619 ± 5.702 | 36.431 ± 6.984 | 36.453 ± 7.716 | 35.317 ± 6.278 | 1.300 | no |
| concurrent-32 | 105.299 ± 7.253 | 125.664 ± 7.762 | 132.105 ± 9.391 | 148.734 ± 6.402 | 1.261 | no |
| deep-32-concurrent-32 | 265.616 ± 36.406 | 386.966 ± 36.802 | 322.868 ± 39.421 | 331.254 ± 6.354 | 1.198 | no |
| services-4-deep-16 | 146.816 ± 8.336 | 158.649 ± 12.874 | 159.444 ± 18.361 | 144.451 ± 14.100 | 1.085 | no |
| services-4-deep-16-concurrent-32 | 1166.505 ± 49.101 | 996.087 ± 33.887 | 969.436 ± 62.797 | 1167.016 ± 23.963 | 0.817 | yes |
| services-4-deep-16-clients-8 | 1171.607 ± 32.177 | 966.649 ± 75.003 | 961.919 ± 79.663 | 1096.427 ± 85.024 | 0.843 | yes |
| services-8-deep-16-clients-8 | 2457.237 ± 71.851 | 1855.666 ± 70.114 | 1701.070 ± 108.341 | 2134.864 ± 55.008 | 0.736 | yes |

The frozen portable control meets 12/24 cells against the same current references. This is the comparable before/after count; pass counts from earlier campaigns use different runs and are not causal evidence.

Run-to-run timing modes and broad spreads remain material. Every regression stays in the table. The instruction profile supports a reduction in validator work; it does not establish that every observed timing change comes from that optimization.

## Instruction profiles and read/write traces

Whole-process Callgrind uses separate diagnostic budgets (two warmups, ten retained batches), including startup and teardown. Instrumented latency never enters the native comparison.

| Case | Process | Before instructions | Selected instructions | C++ instructions | Selected / before |
| --- | --- | ---: | ---: | ---: | ---: |
| deep-32 | client | 3288282 | 3262463 | 2710141 | 0.9921 |
| deep-32 | service-0 | 3289293 | 3263605 | 2978269 | 0.9922 |
| concurrent-32 | client | 9327345 | 9311820 | 11135943 | 0.9983 |
| concurrent-32 | service-0 | 7086120 | 7070848 | 11230463 | 0.9978 |
| deep-32-concurrent-32 | client | 22396969 | 21500819 | 18739199 | 0.9600 |
| deep-32-concurrent-32 | service-0 | 25237760 | 24341592 | 23202044 | 0.9645 |

The separate five-run regression confirmation is retained without replacing the main matrix. Its mean changes versus frozen portable are −4.4% for 256 KiB, **+19.8% for 1 MiB**, −9.4% for 4 MiB, −15.9% for callback, −1.3% for pipeline, −4.5% for deep-32 and +0.04% for shallow concurrency. The 1 MiB mean regression therefore remains a gap even though its confirmation p50 is 0.8% lower. Additional profiles of the five original workflows change instructions by only +0.005–0.159%, so they do not show CPU growth proportional to the native regressions. All confirmation distributions and process profiles are retained in report.json.

Every selected case has fresh lightweight socket and full wire captures, with startup, warmup, measured calls, EOF/EAGAIN and teardown retained. Each instrument compares 64 processes independently; counters are never pooled to hide a failing capture.

| Instrument | Positive read/write passes | Read/write attempt passes | Process comparisons |
| --- | ---: | ---: | ---: |
| socket | 8 | 9 | 64 |
| wire | 6 | 6 | 64 |

Read/write parity remains unmet. This optimization reduces CPU traversal work, not physical transport publications. Startup control-message batching and repeated application-reader segment lookups remain architectural limits. The previous whole-network deferral experiment remains rejected because it masked a network-failure notification; its error/liveness contract is unchanged.

## Validation and reproduction

- runtime libraries: **passed** — 78 core + 72 futures + 319 RPC + 147 allocating wire tests (616 total).
- benchmark library: **passed** — 19 tests, including descriptive statistics and five-run identity validation.
- developer reporting tools: **passed** — 64 unit + 18 governance + 10 wiki CLI tests.
- quality reporting and process tests: **passed** — 22 unit + 2 cancellation + 2 runner tests; separate build-profile regression also passed before the cancellation fix.
- process support: **passed** — 5 verification process tests.
- Miri: **passed** — 14 selected-reader ownership/extent tests, primitive sibling validation oracle, and direct-near validation oracle (16 total).
- allocation-free feature builds: **passed** — Core no-default-features and alloc-only builds.
- strict workspace Clippy / unsafe inventory: **passed** — All targets; unsafe documentation exception inventory unchanged.
- strict benchmark Clippy: **passed** — Standalone release crate, all targets.
- formatting: **passed** — Root and standalone benchmark Rust formatting.
- Python statistics: **passed** — Two hand-calculated sample/run spread tests; syntax compilation also passed.
- source replay: **passed** — Exact final source patch reconstructs its recorded tree; post-measurement changes are reporting assertions and cancellation test infrastructure only.
- independent native evidence verification: **passed** — All 1,100,000 samples across selected, rejected and separate confirmation matrices; five repetitions, full budgets, order, means, spreads, percentiles and hashes checked.
- wire validation: **passed** — All received frames match: 48 original pairs and 228 directed topology edges per selected/rejected sweep. Unread teardown suffixes remain explicit.
- report integrity test first attempt: **failed-corrected** — Old equality assertion did not account for new means; replaced by mean-tampering and sample-order integrity checks.
- quality cancellation first attempts: **failed-corrected** — Container lacks `/proc/<pid>/task/<tid>/children`. Trace reproduced missed nested groups; PPID/process-group snapshot from `/proc/<pid>/stat` fixes discovery and cleanup.
- runner tool-path first attempt: **failed-corrected** — Pinned nextest 0.9.146 was available but absent from PATH; test rerun with frozen tool and both runner tests passed.
- segment cache candidate: **rejected** — No hot-path instruction saving (+0.02–0.16% in all six profile processes); code reverted and complete evidence preserved.
- initial cache test/build attempts: **failed-corrected-rejected** — Wrong binary name, non-Debug test unwrap, and omitted far landing-pad oracle charge corrected before candidate measurement; all failed logs retained.
- previous hosted CodeQL: **unresolved** — Commit 41b2c697 reports eight pointer-access/lifetime alerts, six critical and two high. Not dismissed by local checks; PR remains draft.
- coverage / full reference suites: **not-run-locally** — Full coverage, complete C++ correctness suites and isolated ignored-test inventory are left to hosted checks; fixed C++ benchmark fixtures were exercised locally.
- repository and wiki: **passed** — Repository policy passed; 104 project documents checked. Report links and exact source replay verified separately.

The [exact source patch](source.patch) applies to `41b2c6970d213a2f86d64a06b37f82aaa08ccfbf`. The source tree and executable SHA-256 values are recorded in report.json. The fixed C++ sources remain pinned at `0de72d8d8cec6b69edaa29de51d3bd490341f9c2`, built with clang 19; original and topology fixture binaries are unchanged. Raw samples, process logs, traces, profiles, source snapshots, comparison scripts and failed attempts are preserved locally. Hosted checks for the new commit are separate from local validation; local coverage was not run. The previous commit’s aggregate CodeQL check reports eight unresolved pointer-access/lifetime alerts (six critical, two high) in layout.rs at lines 2492–2653. These are recorded as untriaged, not dismissed by passing local tests; the PR remains draft.

Selected evidence directories: `scratch/portable35-inline-final-original20261018`, `scratch/portable35-inline-final-topology20261018`, `scratch/portable35-inline-final-diagnostics20261018`, `scratch/portable35-inline-final-topology-{socket,wire}20261018`, and `scratch/portable35-inline-topology-profile20261018`. The rejected cache uses the corresponding `portable35-final-*` and `portable35-topology-profile20261018` directories. All live under `/workspace` and are copied into the sealed archive layout.

Sealed evidence: `/workspace/capntproto/target/portable-five-runs-20261018` — 11,547 files, 719,526,107 logical bytes. SHA256SUMS SHA-256: `56c55051a03f38a0c3447f8cc342d6a703c23601f3249c40e57089eb91ba0046`. Older evidence archives were verified unchanged.
