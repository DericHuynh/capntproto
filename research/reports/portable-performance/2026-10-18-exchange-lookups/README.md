# Portable exchange lookup and allocation scans — October 18, 2026

Tracking: [gap #11](https://github.com/DericHuynh/capntproto/issues/11), [draft PR #12](https://github.com/DericHuynh/capntproto/pull/12). The gate remains pooled p50 ≤ min(current legacy, fixed C++), with positive reads/writes and attempts ≤ C++ independently for every process capture. Means, spreads and tails are retained. Legacy remains available.

**The overall goal is not met: 11/24 primary p50 cells pass, versus 11/24 for frozen b69520de portable against the same references.** The candidate reduces concurrent-client instruction work; it does not establish an across-the-board latency or physical-IO win.

## Implementation

- Pending exchanges try an existing slot at the question ID first, then verify the stored ID. Divergent slot/question lifetimes fall back to the ordinary scan. Bounds checks precede lookup; peer IDs never cause allocation. The original question ledger, authorization, cancellation and generation checks still authorize delivery.
- Propagation checks first establish whether any observer exists. With observers, the exact response/observer ordering fence is unchanged. With none, scanning every outstanding exchange cannot find an obligation.
- Hosted calls track their live count and lowest free slot. Failed admission leaves the position intact; successful insertion advances it, and retirement restores the earliest hole. Bounded scheduling order and cancellation fairness are unchanged.
- A new regression relocates two live exchanges so each ID hint points to the other owner, checks an extreme unknown ID, and completes both plain and general return paths. The existing burst/cancellation/reuse regression also verifies live and free-slot accounting.

## Architecture compared with C++

C++ `ExportTable::find()` directly indexes ordinary locally allocated IDs (`vendor/capnproto/c++/src/capnp/rpc.c++`, around lines 217–244); `handleReturn()` uses that question owner directly (around line 4061). Portable retains separate question, exchange and hosted-call stores. This change removes common-case searches, but the fallback remains necessary because their lifetimes can diverge. A complete redesign should make the exchange owner reachable from the exact generational question, with explicit fixed-storage and adopted-ID behavior, instead of relying on coincident slot positions.

The larger deep-access issue remains eager graph validation followed by unlimited retained application readers. In the frozen before profile, inbound and outbound validation together account for about 3.21 million of 19.50 million deep-concurrent client instructions. C++ retains the payload and validates fields as they are accessed. Matching that design requires moving a finite traversal budget and the selected pointer’s remaining nesting limit into every retained plain/general payload owner, preserving capability receipts, and explicitly changing malformed-field error timing. `Reader::try_select_pointer()` currently creates a fresh budget; the hosted callers request unlimited limits. Deleting the eager walks under that ownership contract would be incorrect.

Physical-write batching still differs from KJ `evalLast()`. The prior receive/pooling report preserves both rejected separate-writer experiments. This change does not alter write scheduling or the pending-flush contract, and no syscall-parity claim follows from fewer lookup instructions.

## Five-run native comparison

Each cell contains five independent process runs with 1,000 samples each; warmups are 100, or 10,000 for loopback. All 480,000 primary samples and 240 mixed compatibility checks are retained. Native runs are sequential and separate from builds, tests and instrumentation. Variant order rotates/reverses; five runs do not exactly balance four positions.

Times are microseconds: pooled mean ± sample SD of the five run means (n−1), not a confidence interval. Per-sample SD, extrema, run means/ranges, p50/p95/p99 and throughput are in [report.json](report.json). Before is the published receive/pooling binary; current legacy and portable select different drivers in the same candidate executable.

| Case | Legacy mean ± SD | Before mean ± SD | Portable mean ± SD | C++ mean ± SD | Portable p50 / faster | Pass |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| unary-empty | 27.221 ± 2.684 | 25.166 ± 2.861 | 27.244 ± 4.739 | 28.333 ± 5.889 | 0.975 | yes |
| unary-small | 28.901 ± 6.320 | 27.492 ± 3.696 | 25.469 ± 4.414 | 29.407 ± 4.873 | 0.942 | yes |
| unary-1k | 29.411 ± 4.245 | 30.096 ± 4.855 | 29.620 ± 4.175 | 29.055 ± 3.937 | 1.401 | no |
| fanout-32 | 126.048 ± 19.159 | 139.329 ± 7.010 | 147.577 ± 15.903 | 158.695 ± 16.102 | 1.241 | no |
| dependent-read | 63.543 ± 8.956 | 68.852 ± 14.101 | 61.509 ± 4.335 | 62.927 ± 10.629 | 1.070 | no |
| pipelined-read | 40.262 ± 6.268 | 59.042 ± 11.530 | 61.367 ± 16.229 | 40.607 ± 6.857 | 1.453 | no |
| write-read | 57.438 ± 10.692 | 57.154 ± 11.608 | 52.914 ± 4.023 | 64.968 ± 9.136 | 0.917 | yes |
| callback | 60.650 ± 10.055 | 77.872 ± 10.122 | 74.056 ± 15.963 | 58.190 ± 9.680 | 1.536 | no |
| bulk-64k | 58.395 ± 3.622 | 63.493 ± 7.797 | 70.327 ± 28.900 | 65.073 ± 5.139 | 0.981 | yes |
| bulk-256k | 119.475 ± 17.071 | 159.172 ± 81.222 | 124.058 ± 25.636 | 327.179 ± 55.355 | 1.170 | no |
| bulk-1m | 419.940 ± 26.735 | 439.904 ± 52.761 | 425.238 ± 18.590 | 1016.210 ± 65.415 | 1.045 | no |
| bulk-4m | 2292.422 ± 75.877 | 2407.224 ± 38.139 | 2531.346 ± 141.615 | 5210.630 ± 369.941 | 1.090 | no |
| loopback-0 | 29.155 ± 5.312 | 23.779 ± 1.814 | 25.510 ± 5.480 | 24.510 ± 1.426 | 0.953 | yes |
| loopback-64 | 27.628 ± 3.780 | 27.293 ± 6.222 | 28.414 ± 5.201 | 30.375 ± 6.459 | 1.001 | no |
| loopback-1024 | 26.292 ± 4.992 | 23.243 ± 1.316 | 26.578 ± 4.723 | 32.318 ± 5.117 | 0.969 | yes |
| loopback-65536 | 57.401 ± 3.880 | 55.449 ± 3.615 | 65.045 ± 21.576 | 64.735 ± 4.231 | 0.979 | yes |
| deep-8 | 32.489 ± 5.858 | 28.778 ± 4.314 | 31.737 ± 6.473 | 31.216 ± 6.200 | 0.962 | yes |
| deep-32 | 32.556 ± 6.193 | 32.492 ± 6.885 | 36.184 ± 6.710 | 33.152 ± 5.547 | 1.484 | no |
| concurrent-32 | 102.521 ± 13.844 | 140.019 ± 27.872 | 121.539 ± 6.163 | 153.023 ± 9.433 | 1.236 | no |
| deep-32-concurrent-32 | 264.504 ± 41.524 | 340.077 ± 54.738 | 313.797 ± 42.414 | 304.320 ± 24.855 | 1.208 | no |
| services-4-deep-16 | 170.426 ± 18.593 | 172.933 ± 40.336 | 162.822 ± 15.168 | 151.939 ± 10.508 | 1.003 | no |
| services-4-deep-16-concurrent-32 | 1208.633 ± 49.475 | 1007.512 ± 76.927 | 1031.613 ± 55.133 | 1229.973 ± 52.530 | 0.853 | yes |
| services-4-deep-16-clients-8 | 1231.854 ± 84.909 | 1050.375 ± 126.696 | 1050.813 ± 65.258 | 1136.988 ± 27.653 | 0.915 | yes |
| services-8-deep-16-clients-8 | 2585.555 ± 66.274 | 1927.999 ± 117.798 | 1949.984 ± 194.873 | 2353.097 ± 75.556 | 0.808 | yes |

## Separate confirmation

This second five-run sweep is retained separately and does not replace the primary matrix.

| Case | Before mean ± SD | Portable mean ± SD | Mean change | p50 change |
| --- | ---: | ---: | ---: | ---: |
| unary-empty | 25.879 ± 4.585 | 25.967 ± 4.029 | +0.3% | -0.3% |
| fanout-32 | 138.856 ± 14.798 | 132.666 ± 10.402 | -4.5% | -2.0% |
| callback | 69.950 ± 16.828 | 75.533 ± 18.932 | +8.0% | +67.6% |
| bulk-64k | 60.845 ± 5.182 | 59.748 ± 1.892 | -1.8% | +1.9% |
| bulk-4m | 2742.440 ± 212.069 | 2456.917 ± 133.122 | -10.4% | -8.1% |
| loopback-1024 | 26.148 ± 4.379 | 24.110 ± 1.176 | -7.8% | -0.5% |
| loopback-65536 | 68.696 ± 23.209 | 54.726 ± 2.672 | -20.3% | -1.8% |
| deep-8 | 27.589 ± 4.736 | 29.619 ± 4.224 | +7.4% | +3.8% |
| deep-32 | 32.696 ± 6.260 | 33.976 ± 4.107 | +3.9% | +2.0% |
| concurrent-32 | 130.483 ± 9.314 | 122.599 ± 6.378 | -6.0% | -2.3% |
| deep-32-concurrent-32 | 324.164 ± 38.320 | 294.309 ± 17.226 | -9.2% | -5.0% |
| services-4-deep-16 | 176.169 ± 7.685 | 177.907 ± 18.523 | +1.0% | -0.7% |

Concurrent-32 mean latency improved 13.2% in the primary matrix and 6.0% in confirmation; deep-32-concurrent-32 improved 7.7% and 9.2%. Fan-out changed from 5.9% slower to 4.5% faster, and several bulk/loopback signs also reversed. Deep-8 remained slower in both sweeps (10.3% and 7.4% mean); deep-32 was 11.4% and 3.9% slower. Callback changed from a 4.9% mean improvement to an 8.0% regression, with a 67.6% confirmation p50 increase. All remain visible; instruction reductions do not explain away these timing regressions. The selected changes are retained for their measured CPU-work reduction and bounded lookup behavior, without claiming uniform latency improvement.

Follow-up deep-8/deep-32 profiles change whole-process instructions by less than 0.02%. That result leaves the observed shallow timing regressions unexplained; it is not evidence that they can be discarded.


## Instruction profiles and physical IO

Selected Callgrind profiles ran in isolation and retain whole-process startup, warmup, measured work and teardown. Instrumented latency never enters native results. Small instruction changes can include startup/polling variation.

| Case | Process | Legacy instructions | Before | Portable | C++ | Change vs before |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| unary-small | client | 6,446,489 | 6,127,754 | 6,135,356 | 8,077,572 | +0.12% |
| unary-small | server | 5,939,669 | 5,577,560 | 5,576,464 | 8,353,364 | -0.02% |
| fanout-32 | client | 23,125,434 | 28,346,411 | 27,766,441 | 43,869,190 | -2.05% |
| fanout-32 | server | 19,822,332 | 21,986,118 | 21,979,650 | 43,209,026 | -0.03% |
| pipelined-read | client | 10,465,501 | 17,164,456 | 17,149,786 | 15,174,261 | -0.09% |
| pipelined-read | server | 13,073,690 | 15,213,033 | 15,206,833 | 17,182,017 | -0.04% |
| callback | client | 10,896,542 | 13,926,846 | 13,888,360 | 15,302,494 | -0.28% |
| callback | server | 11,972,630 | 13,935,085 | 13,927,297 | 14,452,286 | -0.06% |
| concurrent-32 | client | 6,855,927 | 8,629,449 | 8,502,229 | 11,136,401 | -1.47% |
| concurrent-32 | service-0 | 6,439,594 | 7,058,635 | 7,057,392 | 11,230,916 | -0.02% |
| deep-32-concurrent-32 | client | 16,209,828 | 19,503,711 | 19,376,615 | 18,739,652 | -0.65% |
| deep-32-concurrent-32 | service-0 | 20,163,466 | 21,853,352 | 21,852,111 | 23,202,497 | -0.01% |
| deep-8 | client | 2,905,582 | 2,957,962 | 2,958,492 | 2,523,738 | +0.02% |
| deep-8 | service-0 | 2,793,655 | 2,827,896 | 2,828,146 | 2,686,085 | +0.01% |
| deep-32 | client | 3,123,265 | 3,224,201 | 3,224,299 | 2,710,197 | +0.00% |
| deep-32 | service-0 | 3,126,522 | 3,185,753 | 3,185,997 | 2,978,269 | +0.01% |

Every selected case has separate socket-probe and full-wire captures. All 48 original-suite frame pairs and all 304 directed topology edges match received frames. Counts below are independent per-process comparisons; matching frames does not imply matching syscalls.

| Instrument | Positive read/write passes | Attempt passes | Processes |
| --- | ---: | ---: | ---: |
| socket | 8 | 11 | 64 |
| wire | 5 | 7 | 64 |

## Validation and provenance

- Repository, documentation and workflow policy: **passed** — Repository artifact checks, 104 project documents and workflow triggers/dependencies passed.
- Primary samples and wire integrity: **passed** — All 480,000 samples revalidated against five-run budgets, raw statistics, variant order and executable/fixture hashes. All 240 mixed checks, 48 original frame pairs and 304 directed topology edges passed. Initial summary-helper failure used a stale binary pathname; fixed without changing any raw evidence.
- Affected library suites: **passed** — 150 wire and 319 RPC tests. The relocated-owner regression also passed after replacing a redundant closure in its test code; production source is unchanged from the complete suite runs.
- Strict Clippy: **passed** — Both affected crates, all targets, -D warnings. Initial redundant-closure failure retained and repaired without a lint exception.
- Allocation-free configuration: **passed** — Wire crate with no default features. The new lookup hint allocates no storage, including for u32::MAX IDs.
- Formatting and source replay: **passed** — cargo fmt --all --check and source/document whitespace checks (literal source.patch context excluded); the selected source patch reconstructs its recorded tree.
- Workspace unsafe documentation audit: **interrupted** — An initial invocation lacked Cargo on PATH. The corrected invocation omitted low-footprint Cargo settings and exhausted disk space while rebuilding the workspace. Stopped its process tree and removed only rebuildable target/debug. The first profiles are retained but excluded; all selected profiles reran in isolation. No unsafe source or allowance was added.
- Full coverage, Miri and full C++ correctness suite: **not-run-locally** — Benchmark interoperability and affected library tests are separate from these full suites. Hosted CodeQL remains failed on the parent commit with 12 alerts: 8 critical and 4 high.

The [source patch](source.patch) applies to `b69520de21a834ffe11cc50e348151307f97066a` and reconstructs `7970cc71ed3f15abfcd567f9ea2d6a4b8cd952ff`. C++ remains pinned to `0de72d8d8cec6b69edaa29de51d3bd490341f9c2`. Binary/source hashes, raw evidence locations and failed attempts are retained. Hosted checks for the published commit are separate from local validation.

Evidence archive: `/workspace/capntproto/target/portable-exchange-lookups-20261018`; 7,122 files and 528,792,057 logical bytes. SHA256SUMS SHA-256: `329bf87aba6f120a5159c3a219a6213842bb234ff794a02abbf21fda82e9d649`. Earlier archives verified unchanged.
