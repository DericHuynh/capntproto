# Portable runtime and bounded ownership architecture — October 18, 2026

Tracking: [gap #11](https://github.com/DericHuynh/capntproto/issues/11), [draft PR #12](https://github.com/DericHuynh/capntproto/pull/12). This is a breaking architecture change with no adapters for the removed portable APIs. Legacy remains a comparison implementation because the performance gate is still open.

**The overall performance goal is not met: 8/24 selected pooled-p50 cells match or beat the faster of current legacy and C++; 3/24 are at least 5% faster.** Frozen pre-refactor portable passes 10/24 against these same current references. The independent per-process IO gates are reported below.

## Breaking architecture changes

- `portable::RpcSystem` no longer implements `Future`. A bounded `portable::Runtime` owns related systems, and `run(application)` drives application continuations, protocol progress and physical output. `run_with` and `run` cover single-system execution. Application completion is immediate; graceful shutdown requires awaiting disconnectors. Each source has independent readiness and all turns share a bounded fairness allowance.
- Live limits and trace settings move to a weak `Control` handle. The old setters are removed. `VatNetwork::take_output_driver` explicitly transfers physical-writer ownership; TCP and Unix implement the split. In-memory and externally driven authenticated transports explicitly return no writer.
- Retained readers pin their original arena, traversal budget and remaining nesting depth. Reopening and cloning cannot reset limits. The old bytes-plus-options retention APIs and unlimited admitted-reader mode are removed. Fixed storage shares an explicit read budget and retains its bounded copy path.
- Inbound admission validates envelopes and capability descriptors transactionally. Application content is checked when accessed, with capability lookup restricted to the exact admitted descriptor table. Unused malformed fields need not be traversed; access still fails under the original limits.
- Question generations bind explicitly to exchange owners. Peer IDs neither allocate storage nor rely on slot/ID coincidence. Ownership is cleared on exact-generation transfer or release; stale owners fail closed.
- Owning transport queues may admit frames during pending physical flushes. They retain bytes, attachments and credits and remain bounded to 32 outstanding frames. Borrowed sinks preserve their flush barrier. Calls attempt transactional admission; blocked publication is distinct from an inapplicable direct path. Only pending output can justify waiting for capacity, so impossible requests retain an error path.
- Physical output runs between buffered protocol turns, after ready application continuations. This allows a service to process the next input while an earlier request travels downstream. Physical completion, failure, idle and shutdown fences remain separate from queue acceptance.

All maintained call sites, tests and benchmark hosts use the new APIs. See [Architecture](../../../../docs/wiki/Architecture.md) for the contracts.

## Experiments and remaining costs

The initial runtime was rejected after its full matrix exposed large-message and service-chain regressions. Polling only ready sources recovered the large-message path. Separating queue admission from flush completion fixed two architectural barriers, and a regression test reproduced pipeline work blocked behind an earlier write. Removing that barrier exposed temporary buffer exhaustion being treated as failure; publication now distinguishes it and retries only behind a real completion fence. All failed commands, test iterations, source snapshots and executable hashes are retained.

In the focused four-service/eight-connection test, admission-only portable had a 1.424 ms median versus C++ at 1.087 ms. Output between protocol turns reduced portable to 1.095 ms versus C++ at 1.119 ms; the old portable binary remained faster at 1.016 ms. The admission-only socket and full-wire captures matched or beat C++ read/write counts for every process in that case. Those focused captures are intermediate evidence; the selected full matrix and counters below determine qualification.

Small-call timings are sometimes bimodal, including the controls. The preceding phase candidate had callback p50 of 93.16 µs in its full sweep despite 57.08 µs in its focused sweep. Neither result is discarded. Five-run means, spreads, tails and raw samples remain available; whole-process instruction counts and instrumented latency do not replace native timing.

Capability workflows and forwarding still require optimization. The architecture removes the known ownership and scheduling barriers; it does not establish a global speed or syscall win. The selected instruction profiles below identify the remaining work separately from native latency.

## Five-run native comparison

All cells use five independent process runs, 1,000 measured samples per run, and unchanged fixture validation. Warmups are 100, or 10,000 for loopback. Native campaigns are serial and uninstrumented; variant order rotates/reverses, which does not exactly balance four positions across five runs. Current legacy and portable use the same selected executable with explicit driver selection. Before is frozen checkpoint b9f92327.

Times are µs: pooled arithmetic mean ± sample SD of the five run means (n−1), not a confidence interval. Per-sample SD, extrema, all run means/ranges, p50/p95/p99 and throughput are in [report.json](report.json).

| Case | Legacy mean ± SD | Before mean ± SD | Portable mean ± SD | C++ mean ± SD | Portable p50 / faster | Pass |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| unary-empty | 29.341 ± 5.010 | 25.266 ± 2.324 | 28.897 ± 6.083 | 25.280 ± 1.087 | 1.014 | no |
| unary-small | 29.120 ± 4.884 | 31.171 ± 4.999 | 29.356 ± 4.101 | 30.987 ± 3.805 | 1.083 | no |
| unary-1k | 27.345 ± 4.022 | 30.095 ± 4.236 | 34.942 ± 2.048 | 25.828 ± 1.281 | 1.580 | no |
| fanout-32 | 118.087 ± 17.141 | 128.139 ± 4.846 | 136.381 ± 12.608 | 179.791 ± 21.975 | 1.172 | no |
| dependent-read | 64.141 ± 11.772 | 66.687 ± 11.371 | 73.858 ± 16.342 | 58.964 ± 10.351 | 1.209 | no |
| pipelined-read | 43.886 ± 8.222 | 61.826 ± 21.447 | 54.719 ± 4.305 | 42.204 ± 7.152 | 1.082 | no |
| write-read | 62.307 ± 14.420 | 60.011 ± 9.784 | 57.767 ± 13.419 | 57.886 ± 12.027 | 0.983 | yes |
| callback | 60.246 ± 12.412 | 64.123 ± 13.861 | 77.494 ± 15.025 | 64.479 ± 12.610 | 1.373 | no |
| bulk-64k | 56.961 ± 3.370 | 70.623 ± 24.201 | 53.683 ± 1.559 | 74.417 ± 23.978 | 0.968 | yes |
| bulk-256k | 168.231 ± 103.812 | 107.049 ± 4.643 | 161.813 ± 106.537 | 301.005 ± 15.398 | 0.864 | yes |
| bulk-1m | 403.479 ± 18.886 | 437.147 ± 70.381 | 409.172 ± 28.416 | 1111.749 ± 88.711 | 1.066 | no |
| bulk-4m | 2465.092 ± 117.310 | 2601.175 ± 207.428 | 2763.305 ± 155.779 | 4870.482 ± 170.183 | 1.088 | no |
| loopback-0 | 30.238 ± 6.135 | 27.263 ± 5.301 | 29.128 ± 5.743 | 28.687 ± 4.967 | 0.993 | yes |
| loopback-64 | 25.021 ± 1.588 | 26.643 ± 3.994 | 23.966 ± 1.070 | 26.591 ± 4.645 | 0.998 | yes |
| loopback-1024 | 27.492 ± 5.798 | 27.411 ± 4.407 | 28.902 ± 5.750 | 27.251 ± 4.545 | 1.000 | no |
| loopback-65536 | 59.635 ± 5.239 | 59.543 ± 3.808 | 58.912 ± 4.876 | 63.893 ± 4.281 | 1.036 | no |
| deep-8 | 25.687 ± 2.890 | 33.472 ± 12.591 | 29.625 ± 4.783 | 29.551 ± 5.012 | 1.020 | no |
| deep-32 | 33.331 ± 4.595 | 29.398 ± 2.854 | 35.221 ± 7.687 | 33.231 ± 6.829 | 1.030 | no |
| concurrent-32 | 97.591 ± 9.163 | 126.545 ± 11.507 | 126.470 ± 14.944 | 150.131 ± 11.609 | 1.276 | no |
| deep-32-concurrent-32 | 290.164 ± 28.852 | 311.358 ± 27.842 | 288.743 ± 34.605 | 300.393 ± 34.068 | 1.038 | no |
| services-4-deep-16 | 175.419 ± 16.826 | 188.885 ± 41.201 | 190.868 ± 19.019 | 169.045 ± 32.640 | 1.129 | no |
| services-4-deep-16-concurrent-32 | 1187.924 ± 25.237 | 1036.728 ± 48.512 | 1076.119 ± 38.467 | 1222.371 ± 46.848 | 0.920 | yes |
| services-4-deep-16-clients-8 | 1202.384 ± 48.665 | 998.676 ± 41.926 | 1146.205 ± 119.086 | 1099.623 ± 28.610 | 0.992 | yes |
| services-8-deep-16-clients-8 | 2549.197 ± 76.494 | 1864.028 ± 93.703 | 2059.800 ± 60.999 | 2451.929 ± 211.998 | 0.843 | yes |

## Profiles and physical IO

Callgrind totals include process startup, warmup, work and teardown. These serial diagnostic captures never contribute timing samples.

| Case | Process | Legacy instructions | Before | Portable | C++ | Change vs before |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| unary-small | client | 6,451,393 | 6,135,359 | 6,489,252 | 8,077,614 | +5.77% |
| unary-small | server | 5,910,623 | 5,576,467 | 6,008,331 | 8,353,364 | +7.74% |
| fanout-32 | client | 23,126,928 | 27,766,441 | 27,690,123 | 43,869,190 | -0.27% |
| fanout-32 | server | 19,820,147 | 21,979,650 | 21,759,213 | 43,209,026 | -1.00% |
| pipelined-read | client | 10,470,284 | 17,149,659 | 17,656,352 | 15,174,261 | +2.95% |
| pipelined-read | server | 13,070,952 | 15,206,803 | 15,705,905 | 17,182,017 | +3.28% |
| callback | client | 10,902,012 | 13,890,319 | 14,595,703 | 15,302,494 | +5.08% |
| callback | server | 11,970,556 | 13,937,073 | 14,248,183 | 14,452,286 | +2.23% |
| bulk-4m | client | 67,024,515 | 68,580,110 | 68,566,846 | 71,323,324 | -0.02% |
| bulk-4m | server | 11,041,283 | 11,549,398 | 11,563,271 | 11,476,041 | +0.12% |
| concurrent-32 | client | 6,852,608 | 8,502,229 | 8,433,234 | 11,136,401 | -0.81% |
| concurrent-32 | service-0 | 6,438,542 | 7,057,392 | 6,977,837 | 11,230,916 | -1.13% |
| deep-32-concurrent-32 | client | 16,178,423 | 19,376,615 | 17,847,638 | 18,739,652 | -7.89% |
| deep-32-concurrent-32 | service-0 | 20,162,434 | 21,852,111 | 20,284,467 | 23,202,497 | -7.17% |
| services-4-deep-16-clients-8 | client | 12,810,735 | 15,767,010 | 15,315,363 | 15,993,770 | -2.86% |
| services-4-deep-16-clients-8 | service-0 | 28,760,928 | 32,008,621 | 30,374,662 | 30,827,999 | -5.10% |
| services-4-deep-16-clients-8 | service-1 | 27,004,058 | 41,277,365 | 38,982,116 | 29,512,514 | -5.56% |
| services-4-deep-16-clients-8 | service-2 | 27,056,690 | 38,159,436 | 37,742,214 | 29,524,625 | -1.09% |
| services-4-deep-16-clients-8 | service-3 | 13,015,040 | 14,146,509 | 13,471,828 | 16,854,642 | -4.77% |

Every selected case has separate socket-probe and full-wire captures. Each pass requires both reads and writes ≤ C++ for that process; attempts also include nonpositive calls. Frame integrity and syscall parity are separate checks.

| Instrument | Positive read/write passes | Attempt passes | Processes |
| --- | ---: | ---: | ---: |
| socket | 39 | 39 | 64 |
| wire | 35 | 37 | 64 |

## Validation and provenance

- Selected unit and integration suites: **passed** — 328 RPC, 202 allocating engine and 151 allocating wire unit tests; 69 root integration tests including TCP, Unix resources, policy, cancellation, multiparty joins, answer adoption and TLC replays. RPC doctests: 3 passed, 4 ignored.
- Core reader and ownership validation: **passed** — Core library and integration suites passed during the reader redesign. All 192 strict-provenance Miri executions passed across alloc/unaligned and Stacked/Tree Borrows. The tested core, wire and question-ownership files are unchanged in the selected source; later runtime/output changes received native unit and integration coverage.
- Formal contracts and negative controls: **passed** — Pinned Verus verified 155 production/source contracts and 139 temporal-library obligations. All 44 deliberate faults were rejected, including the new live-exchange-owner reuse fault. The earlier install/download/checkout attempts are retained; tools and the proof library were installed from checksum-verified/pinned sources.
- Build and static checks: **passed** — Strict Clippy on all targets of core, engine, wire and RPC; root tests/examples compile; no-default-features wire and core sync_reader configurations; formatting for the workspace and both maintained benchmark hosts. Earlier maintained compio and upstream Rust fixture compile checks passed.
- Native and diagnostic evidence: **passed** — 480,000 selected native samples, five runs per cell, full fixture budgets and all 240 mixed checks verified. All 48 original frame pairs and 304 directed topology edges match received frames. Native timing is separate from builds, tests and instrumentation.
- Repository, documentation, workflows and source replay: **passed** — Repository and wiki checks and workflow checks passed. The exact selected source patch reconstructs its recorded tree. Literal source.patch whitespace is preserved; source/document whitespace checks exclude its unchanged diff context.
- Full coverage and full independent C++ correctness suite: **not-run-locally** — These remain separate from affected tests and benchmark interoperability. The parent commit CI failed in its tests-and-coverage job; its dedicated performance and CodeQL workflows succeeded. New-head hosted checks are not claimed here.

The [source patch](source.patch) applies to `b9f923271aa3ca5a17f4f40a43505e0dea7f0961` and reconstructs `39aca8d3d52ceb61d9c82d8bcf2b55611ac46b61`. Selected executable SHA-256: `71dfcc187c3cab8fda22b59e9e6e6397de96482fd79bc6d814d0d9f537cfc309`. C++ remains pinned to `0de72d8d8cec6b69edaa29de51d3bd490341f9c2`. The [verification-only delta](verification.patch) updates the mutation plan after the frozen build; no compiled sources or fixtures changed. Source-bound intermediate binaries and complete experiments are retained in report.json and the archive. Hosted results for the new commit are separate from local validation.

Evidence archive: `/workspace/capntproto/target/portable-runtime-ownership-20261018`; 17,749 files, 1,440,007,929 logical bytes. SHA256SUMS SHA-256: `339b4f980c8d5633e3574d7086ed40a29218a914ed0ea3baa5e2955d442a89d7`. Earlier archives verified unchanged.
