# Portable observer occupancy — 2026-10-21

Portable beats C++ on both mean and pooled median in **14/24** cells; the frozen `63d3297` portable control passes **13/24** against the same C++ measurements. Against the faster legacy/C++ reference, 8/24 pass both measures. The universal speed and I/O targets remain unmet.

**A general latency improvement is not established.** Pipelined-read mean is 7.3% lower in the primary sweep but 15.1% higher in the independent confirmation. Fan-out reverses direction too. The change is retained for bounded idle progress after capacity growth; the timing tables preserve these conflicting results.

The driver now keeps invocation-observer occupancy with the observer storage. Admission, settlement, abandonment and terminal clearing maintain the count; the outer driver cannot mutate slots through the read-only view. Empty pending checks, publication/cancellation walks and maintenance proofs no longer depend on retained table capacity. Outgoing observer checks also use their existing exact live count. This stores one occupancy count, with no new allocation or unsafe code. Active observers retain the original generation, cancellation, authorization and handoff checks.

The separate worker regression fixture now reads byte-aligned transport bytes through the allocating stream reader. Its old flat-slice reader incorrectly assumed `Vec<u8>` alignment and failed 32-bit Miri. The full native probe matrix still repeats five times; Miri runs one repetition with both dispatch/output paths, budgets 1 and 64, counts 1, 2 and 16, and first/middle/last selection.

## Measurements

Each cell contains five native runs with 1,000 measured samples per run. Values are **mean ± sample SD of the five run means**, in microseconds; this spread is not a confidence interval. Pooled medians, sample SD, extrema, per-run means and throughput are retained in [report.json](report.json). Negative change is faster. Native runs rotate/reverse order, with no overlapping builds, tests or profilers; five runs do not exactly balance positions. Loopback warmup is 10,000; other cells warm up 100 operations. Concurrent cells measure the entire batch.

| Case | Legacy | Portable before | Portable | C++ | Change vs before | Change vs C++ |
|---|---:|---:|---:|---:|---:|---:|
| unary-empty | 26.96 ± 3.49 | 26.14 ± 3.99 | 25.51 ± 3.82 | 25.01 ± 1.56 | -2.4% | +2.0% |
| unary-small | 25.66 ± 2.83 | 24.38 ± 2.17 | 26.64 ± 3.62 | 25.80 ± 3.12 | +9.3% | +3.3% |
| unary-1k | 24.60 ± 1.57 | 26.33 ± 3.21 | 24.29 ± 1.48 | 27.35 ± 3.92 | -7.8% | -11.2% |
| fanout-32 | 115.71 ± 11.52 | 146.32 ± 18.12 | 155.38 ± 14.67 | 156.26 ± 9.14 | +6.2% | -0.6% |
| dependent-read | 62.75 ± 9.30 | 79.70 ± 14.22 | 77.39 ± 6.65 | 61.67 ± 8.45 | -2.9% | +25.5% |
| pipelined-read | 38.07 ± 3.10 | 60.91 ± 9.38 | 56.49 ± 2.26 | 41.64 ± 5.17 | -7.3% | +35.6% |
| write-read | 57.07 ± 9.49 | 59.08 ± 9.08 | 56.34 ± 8.56 | 63.04 ± 12.41 | -4.6% | -10.6% |
| callback | 69.45 ± 7.70 | 71.44 ± 13.02 | 68.21 ± 10.35 | 68.40 ± 9.06 | -4.5% | -0.3% |
| bulk-64k | 60.84 ± 6.28 | 71.28 ± 25.44 | 61.89 ± 2.70 | 60.09 ± 2.30 | -13.2% | +3.0% |
| bulk-256k | 114.69 ± 6.88 | 125.06 ± 27.54 | 128.54 ± 23.81 | 299.99 ± 19.79 | +2.8% | -57.2% |
| bulk-1m | 404.93 ± 17.02 | 440.90 ± 32.46 | 441.43 ± 47.54 | 1105.21 ± 43.82 | +0.1% | -60.1% |
| bulk-4m | 2116.31 ± 58.86 | 2503.13 ± 217.31 | 2352.85 ± 156.28 | 4813.49 ± 196.22 | -6.0% | -51.1% |
| loopback-0 | 29.01 ± 3.62 | 28.60 ± 4.52 | 26.40 ± 2.07 | 27.57 ± 4.08 | -7.7% | -4.2% |
| loopback-64 | 25.55 ± 3.41 | 27.22 ± 4.08 | 35.91 ± 17.91 | 27.12 ± 3.08 | +31.9% | +32.4% |
| loopback-1024 | 30.35 ± 3.44 | 28.93 ± 9.59 | 30.52 ± 6.62 | 30.25 ± 2.42 | +5.5% | +0.9% |
| loopback-65536 | 56.00 ± 2.10 | 58.50 ± 4.07 | 69.11 ± 14.64 | 60.14 ± 1.96 | +18.1% | +14.9% |
| deep-8 | 28.08 ± 4.53 | 29.29 ± 4.33 | 26.99 ± 4.09 | 28.48 ± 3.82 | -7.9% | -5.2% |
| deep-32 | 31.88 ± 5.45 | 35.95 ± 5.10 | 35.58 ± 8.46 | 36.07 ± 6.85 | -1.0% | -1.4% |
| concurrent-32 | 119.10 ± 22.48 | 130.13 ± 9.54 | 138.76 ± 15.09 | 159.08 ± 16.16 | +6.6% | -12.8% |
| deep-32-concurrent-32 | 282.92 ± 45.95 | 295.72 ± 33.89 | 308.52 ± 40.96 | 316.91 ± 20.41 | +4.3% | -2.6% |
| services-4-deep-16 | 152.03 ± 12.50 | 161.81 ± 7.16 | 168.94 ± 12.37 | 146.18 ± 16.41 | +4.4% | +15.6% |
| services-4-deep-16-concurrent-32 | 1213.08 ± 56.11 | 975.07 ± 64.05 | 976.69 ± 64.03 | 1278.51 ± 71.35 | +0.2% | -23.6% |
| services-4-deep-16-clients-8 | 1255.57 ± 22.41 | 1007.90 ± 43.18 | 987.88 ± 34.13 | 1202.84 ± 48.05 | -2.0% | -17.9% |
| services-8-deep-16-clients-8 | 2660.02 ± 142.73 | 1860.52 ± 169.67 | 1849.50 ± 46.31 | 2425.31 ± 128.99 | -0.6% | -23.7% |

## Independent timing confirmation

The original 16 cases were repeated five more times after instrumentation finished, starting with the opposite variant order. These 320,000 samples are reported separately from the primary campaign. Both sweeps retain every run; their changes are not evidence of a universal latency gain.

| Case | Portable before | Portable | C++ | Mean change vs before | Median change vs before |
|---|---:|---:|---:|---:|---:|
| unary-empty | 28.02 ± 5.34 | 30.38 ± 3.85 | 29.73 ± 7.14 | +8.4% | +39.7% |
| unary-small | 31.08 ± 4.15 | 31.66 ± 4.12 | 29.42 ± 3.37 | +1.9% | -0.7% |
| unary-1k | 27.65 ± 3.97 | 26.21 ± 4.38 | 26.59 ± 2.84 | -5.2% | -0.2% |
| fanout-32 | 158.08 ± 9.23 | 148.11 ± 20.14 | 182.72 ± 25.11 | -6.3% | -17.1% |
| dependent-read | 84.01 ± 18.59 | 81.09 ± 19.27 | 67.01 ± 9.09 | -3.5% | -6.1% |
| pipelined-read | 59.39 ± 10.67 | 68.36 ± 14.85 | 41.03 ± 6.79 | +15.1% | +44.1% |
| write-read | 60.51 ± 11.24 | 56.26 ± 5.93 | 60.43 ± 7.78 | -7.0% | -21.7% |
| callback | 74.63 ± 13.39 | 75.20 ± 11.48 | 62.78 ± 4.81 | +0.8% | -7.2% |
| bulk-64k | 63.28 ± 11.51 | 56.32 ± 2.82 | 62.30 ± 5.26 | -11.0% | -1.2% |
| bulk-256k | 142.30 ± 51.64 | 121.66 ± 8.52 | 282.26 ± 23.31 | -14.5% | +0.4% |
| bulk-1m | 442.07 ± 58.80 | 424.69 ± 26.35 | 1072.30 ± 89.11 | -3.9% | -0.4% |
| bulk-4m | 2506.48 ± 217.32 | 2546.13 ± 213.27 | 5061.75 ± 334.03 | +1.6% | +4.0% |
| loopback-0 | 26.77 ± 1.82 | 29.75 ± 5.37 | 28.42 ± 4.76 | +11.1% | +3.3% |
| loopback-64 | 28.11 ± 5.87 | 28.15 ± 4.56 | 27.58 ± 3.54 | +0.1% | +1.6% |
| loopback-1024 | 25.92 ± 1.73 | 28.25 ± 7.05 | 27.17 ± 3.63 | +9.0% | -1.0% |
| loopback-65536 | 58.41 ± 3.43 | 58.85 ± 6.44 | 60.54 ± 2.38 | +0.7% | +12.6% |

Whole-case instruction changes stay within 0.4% of the frozen control. The architectural result is an exact constant-time empty-table proof after growth; a general end-to-end latency improvement is not established. Simulated branch changes are mixed and do not establish a hardware prediction gain.

## Instruction profiles and socket activity

Callgrind profiles use separate short diagnostic workloads and include process setup, warmup and teardown. They do not supply the native latency results. Whole-case instruction totals below sum all participating processes.

| Case | Before instructions | Portable instructions | C++ instructions | Change vs before |
|---|---:|---:|---:|---:|
| unary-empty | 12,552,397 | 12,552,866 | 16,414,556 | +0.00% |
| unary-small | 12,579,965 | 12,580,195 | 16,431,837 | +0.00% |
| unary-1k | 12,791,445 | 12,793,323 | 16,810,417 | +0.01% |
| fanout-32 | 50,068,362 | 50,009,226 | 87,078,969 | -0.12% |
| dependent-read | 31,452,463 | 31,427,024 | 33,152,818 | -0.08% |
| pipelined-read | 32,647,671 | 32,640,518 | 32,357,039 | -0.02% |
| write-read | 19,583,186 | 19,582,301 | 28,407,541 | -0.00% |
| callback | 28,246,099 | 28,221,533 | 29,755,515 | -0.09% |
| bulk-64k | 10,851,570 | 10,851,261 | 10,813,778 | -0.00% |
| bulk-256k | 23,937,080 | 23,937,242 | 21,433,019 | +0.00% |
| bulk-1m | 25,949,862 | 25,949,694 | 27,568,945 | -0.00% |
| bulk-4m | 80,119,079 | 80,118,500 | 82,799,858 | -0.00% |
| loopback-0 | 12,417,604 | 12,418,081 | 16,299,810 | +0.00% |
| loopback-64 | 12,474,949 | 12,475,480 | 16,316,627 | +0.00% |
| loopback-1024 | 12,686,985 | 12,685,974 | 16,678,388 | -0.01% |
| loopback-65536 | 10,492,366 | 10,492,696 | 10,313,926 | +0.00% |
| deep-8 | 5,808,045 | 5,807,956 | 5,211,561 | -0.00% |
| deep-32 | 6,360,525 | 6,360,357 | 5,690,148 | -0.00% |
| concurrent-32 | 15,425,233 | 15,411,457 | 22,368,102 | -0.09% |
| deep-32-concurrent-32 | 38,272,179 | 38,258,101 | 41,942,919 | -0.04% |
| services-4-deep-16 | 16,899,752 | 16,899,531 | 14,711,783 | -0.00% |
| services-4-deep-16-concurrent-32 | 117,798,530 | 117,469,436 | 120,698,215 | -0.28% |
| services-4-deep-16-clients-8 | 118,362,809 | 118,221,070 | 122,719,368 | -0.12% |
| services-8-deep-16-clients-8 | 240,175,521 | 239,354,432 | 240,989,290 | -0.34% |

**socket: 42/64** process captures have both positive read/write counts and attempted read/write counts no greater than C++. Positive-only passes: 42/64; attempt-only passes: 42/64. Per-process counts are retained; startup and teardown are included.

**wire: 41/64** process captures have both positive read/write counts and attempted read/write counts no greater than C++. Positive-only passes: 41/64; attempt-only passes: 41/64. Per-process counts are retained; startup and teardown are included.

| Simulated branches, both peers | Before misses | Portable misses | C++ misses | Portable / C++ I1 misses |
|---|---:|---:|---:|---:|
| unary-empty | 117,981 | 120,879 | 148,958 | 675,395 / 448,568 |
| pipelined-read | 565,924 | 557,068 | 320,766 | 2,087,033 / 878,341 |
| callback | 403,926 | 417,425 | 293,276 | 1,928,837 / 914,655 |

All 240 mixed interoperability checks, 48 original-workflow frame checks and 304 directed topology-edge comparisons pass. Different instrumentation schedules can produce different read/write counts; the two capture modes are reported separately.

Simulated branch/cache results are retained in the report. These are Callgrind predictions, not hardware branch-miss measurements. Hardware PMU results are not claimed.

## Architecture comparison

At the pinned C++ reference, `RpcPipeline` (`vendor/capnproto/c++/src/capnp/rpc.c++:2795`) subscribes to the question response promise and maintains a per-pipeline client map; `QuestionRef` (line 2387) emits Finish at the ownership transition before question-ID reuse. Portable still scans active observer tables and visits maintenance phases. This change removes absence checks proportional to retained capacity; it does not replace active scans with question-owned notifications. That remaining design is a plausible target for pipeline and callback work, but it needs separate profiling and must preserve queued-before-publication observers, generation reuse, cancellation, and reflected-route barriers.

## Validation and provenance

- 155 alloc wire tests and 101 no-alloc wire tests passed.
- 336 RPC tests and nine TCP/Unix integration tests passed.
- All five isolated portable profiles passed: core 86, alloc 89, growing 89, std 86, unaligned + sync_reader 86.
- Core and alloc thumbv7em-none-eabi checks passed.
- Strict Clippy passed for RPC/wire across all targets; formatting and git diff whitespace checks passed.
- Three targeted 32-bit Miri tests passed on nightly-2026-08-29: worker wake/output fixture, observer abandonment/growth/reuse, and large empty retained capacity. This is not the complete Miri suite.
- Final native worker probe and release build recheck passed after the interpreter-only matrix adjustment. Source hashes match the frozen candidate.

The previous published CI run 39338429827 failed the worker fixture under 32-bit Miri and failed coverage regression thresholds. The local checks listed above do not claim full coverage qualification or a full Miri/formal rerun. Coverage thresholds have not been lowered.

Base commit: `63d329751dab9803db9b7d86ef640a301732aef5`. Source tree: `44b98edf468954bf4576b867eb1e1079fd415f07`. Candidate SHA-256: `d0b2bd2ed3dbbada8e362513db7f3bdc20ae0a7544af3e947789c0b200e36e5d`. The [source patch](source.patch), source/fixture hashes, compiler settings and fixed C++ binary hashes are retained. Rust 1.97.0 uses fat LTO and one codegen unit; the unchanged C++ controls use the pinned source `0de72d8d8cec6b69edaa29de51d3bd490341f9c2` and Clang 19 `-O3`.

Raw evidence is retained in `/workspace/capntproto/target/portable-observer-occupancy-20261021.tar.xz` (SHA-256 `7b84a5880104d103def82e1587f963b574adc3536134e13d11a474bb72b00634`). It includes the candidate executable, native samples, diagnostic traces, profiles, manifests, validation logs and reproduction scripts. Extracting the archive restores the original relative evidence directories; scope files preserve their original execution paths. Earlier sealed archives remain unchanged. The current campaign used memory-backed build and evidence storage because the workspace disk was full; disposable dependency caches were temporarily relocated and restored. Unreferenced partial Docker layers left by this session’s failed helper creation were removed after checking all layer references.
