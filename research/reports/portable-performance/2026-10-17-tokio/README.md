# Tokio selection and portable metadata refresh

Tokio is the selected production host and the runtime for continuing portable optimization. Production already used Tokio; the comparison harness now also excludes Compio from its default dependency graph. Reproducing Compio requires `--features compio-experiment`. An explicit Compio selector in a default build fails with instructions to enable that feature. The historical comparison and its fixtures remain available.

The portable metadata refresh checks a settled import’s revision before upgrading its weak endpoint. Unchanged imports avoid retaining and dropping the endpoint. Changed metadata and pending promises still retain it through publication; endpoint release remains independently journaled. This reduces reference-count work, but retains the registry scan and per-import metadata allocation.

## Measurement

The before binary is the frozen Tokio control from `625fe56c`, SHA-256 `6c8cab0a3e814817b7283e799027799b34b94711d291f876b57eafb0812df838`. The after binary is `653ed43814019b1251dbd7b8af30b47da18d0e12a14996e69576f79fe4793046`. Both run Tokio’s current-thread runtime and the identical production portable engine fixtures. The measured after binary enables the experimental feature only to retain the before binary’s host composition; neither measured peer executes Compio. The separate default Tokio-only executable was also built and checked.

Whole-process Callgrind profiles include startup, 100 warmups and 1,000 measured operations. Allocator counts are unchanged in all six comparisons.

| Workflow | Peer | Before instructions | After instructions | Change |
| --- | --- | ---: | ---: | ---: |
| pipelined-read | client | 73,115,182 | 72,851,524 | -0.361% |
| pipelined-read | server | 64,283,170 | 64,280,982 | -0.003% |
| unary-empty | client | 17,012,721 | 17,010,666 | -0.012% |
| unary-empty | server | 15,222,848 | 15,220,936 | -0.013% |
| callback | client | 56,639,865 | 56,369,500 | -0.477% |
| callback | server | 56,563,203 | 56,528,940 | -0.061% |

All six before/after peer comparisons have identical complete write bytes and write groups. The 12 syscall journals include startup and cleanup, with every successful write decoded and accounted for. The pipeline retains the previously observed C++ grouping: 1,368-byte Finish/Release/Call/Call writes and paired 1,216-byte Returns. No yield, batching policy, admission limit, deadline or fixture changed.

Latency uses CPUs 1/2, 100 warmups and 1,000 samples per trial, rotating before/after/legacy/C++ order. The first five repetitions retain 60,000 samples. Their pooled after latencies are worse for unary and callback despite the small instruction savings. An additional balanced 20 repetitions retain another 240,000 samples; all results are preserved separately in [report.json](report.json). Their pooled p50 values follow.

| Workflow | Before µs | After µs | Legacy µs | C++ µs | After / faster reference |
| --- | ---: | ---: | ---: | ---: | ---: |
| pipelined-read | 43.614 | 44.015 | 29.173 | 32.378 | 1.509× |
| unary-empty | 21.772 | 23.380 | 20.070 | 22.283 | 1.165× |
| callback | 73.499 | 72.346 | 43.875 | 46.679 | 1.649× |

The repeated run does not establish a latency improvement: pipeline and unary pooled p50 remain higher, while callback is lower. Unary repetition medians span approximately 20–33 µs in both binaries; pipeline and callback also change timing modes. Keep the measured instruction reduction distinct from latency. None of these three repeated local cells meets the faster-reference 5% target. This is not a complete dedicated 12-remote/four-loopback campaign, and gap #11 stays open.

## Validation and reproduction

307 RPC unit tests, three default Tokio adapter tests, four opt-in adapter tests, and 48 mixed-peer cases pass. RPC and both harness configurations pass all-target Clippy with warnings denied. Default dependency inspection excludes all Compio packages; default runtime inspection reports `tokio current_thread`, and selecting disabled Compio fails explicitly. Existing tests cover late metadata updates, attached descriptors, dynamic projections and release during collection. Coverage floors and tolerances are unchanged.

The [source patch](source.patch) applies to `625fe56c292e418ac033a636fd4a041b8b7f76e6` and reconstructs measured tree `f838c38a887d03e725c97951d2b56e47b901e3c9`. Later report/documentation edits do not change compiled source. Build with `CARGO_INCREMENTAL=0 cargo build --locked --release --features compio-experiment --manifest-path benchmarks/rpc-compio/Cargo.toml`; select `CAPNTPROTO_BENCH_RUNTIME=tokio` on both peers. Omit the feature for the normal Tokio-only build.

Retained evidence: `target/portable-tokio-resume-20261017`, 1133 files plus `SHA256SUMS`, manifest SHA-256 `e31ae9b9410940333a990c68a27826bbbc2233fa233dbab790bc003a0dac214f`. This contains raw latency samples, profiles, decoded and original traces, frozen binaries, source, validation logs and reproduction scripts. Earlier experiment archives remain unchanged.
