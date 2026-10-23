# Portable RPC: CI droplet validation and borrowed Return validation

Borrowed Return validation removes one 88-byte reader arena and about 0.13–0.15% of instructions in the profiled reads. Native latency is mixed: −0.81% over TLS and +2.08% over QUIC versus the same-campaign frozen portable control. The implementation remains experimental and is not a qualified overall latency improvement.

All timing, syscall tracing, Callgrind and DHAT evidence in this report was collected on the DigitalOcean droplets provisioned by the repository's dedicated benchmark workflow. Development workspace and GitHub-hosted runners were used only for builds, correctness checks and analysis of saved files.

## Implementation

Capability-bearing and reflected Return publication previously called `Frame::with`, which allocated a retained reader arena merely to validate the outgoing envelope. These publication paths now use a private borrowed root. A retained reader is created only when later access needs it. Promotion transfers the remaining traversal allowance; it cannot replenish words consumed by earlier validation. Fixed storage, growing storage, finite/zero/unlimited budgets, repeated access and access after promotion are covered.

The change preserves payload ownership, publication checks and nesting limits. The public API adds only a documented traversal-budget snapshot; the borrowed validation helper is private. This is a small reduction in work, not a claim that portable's larger architecture gap is solved.

The CI bundle fix includes its certificate fixture directory and rejects unsupported entries before provisioning resources. The new encrypted campaign uses the existing droplet lifecycle and unconditional cleanup, and an optional frozen portable build allows a same-campaign comparison.

## Method and limits

- 24 application cases per transport; five independent process repetitions, 100 warmups and 1,000 measured operations each. Tables show microseconds per benchmark operation, mean ± sample SD of the five run means. A concurrent operation can contain a batch of RPCs.
- TLS 1.3 with certificate and hostname verification on every connection and service hop; QUIC uses the same production adapter for both Rust drivers. Handshakes precede warmup and timing. C++/KJ has no supported QUIC fixture and is omitted there.
- Rust TLS uses rustls/ring; KJ TLS uses OpenSSL; QUIC uses quiche/BoringSSL. TLS comparisons are whole implementations with equivalent fixture semantics, not an isolated RPC-core comparison.
- CI selects a dedicated-vCPU droplet plan. Peers run over loopback on distinct guest-reported cores of the client droplet; deep service chains share the available server cores. The other provisioned droplet remains idle. These results isolate local driver costs and do not measure VPC latency.
- All uninstrumented trials finish before diagnostics. Profiles sum client and service costs: (500-operation run − 50-operation run) / 450. Callgrind instruction, cache and branch counts are simulated, not native hardware counters or wall time. DHAT measures cumulative allocated blocks/bytes, not peak retained memory.
- Ciphertext syscall traces use five repetitions at 10 and 100 measured operations after two warmups. They include connection setup and teardown. Positive read/write counts are socket operations, not RPC-message counts; the two-point slope separates fixed lifecycle work from steady growth.
- Five-run means still have substantial spread in several short cases. Execution order rotates but five repetitions do not perfectly balance every position. A lower mean alone does not establish a repeatable latency win. Same-campaign frozen-control comparisons take precedence over differences between droplets.

## Candidate decision

The borrowed validation path removes exactly one retained reader arena per profiled operation: dependent TLS reads go from 8 to 7 arena allocations, and pipelined TLS/QUIC reads go from 9 to 8. This arena is 88 bytes. Total marginal instructions fall by only 0.13–0.15%, so the change is a small work reduction.

**The latency outcome is mixed and does not qualify as an overall speedup.** Against the frozen portable build in the same campaign, the geometric mean changes by −0.81% over TLS and +2.08% over QUIC. Only 12/24 TLS and 9/24 QUIC means improve. The experiment remains on the feature branch; main is unchanged. The goal of outperforming legacy in every benchmark remains unmet; complete read/write-count parity with C++ also remains unmet.

Against current legacy, the candidate's geometric-mean latency is 4.22% higher over TLS and 7.50% higher over QUIC; it has lower means in only 5/24 cases on each transport. It has lower means than C++ in all 24 TLS cases, with a geometric-mean ratio 37.56% lower. There is no C++ QUIC comparison.

TLS pipelined-read changes from 97.275 ± 28.166 to 91.449 ± 13.836 µs. QUIC pipelined-read changes from 108.091 ± 0.367 to 111.080 ± 8.865 µs, and QUIC dependent-read changes from 172.136 ± 39.411 to 206.880 ± 53.433 µs. These spreads do not establish that a tiny instruction reduction caused any particular wall-time change. Larger, steadier cases also need attention: the eight-service QUIC case is 3.99% slower than the frozen control (5252.056 ± 150.981 versus 5050.525 ± 52.831 µs). The exact values below and in JSON are authoritative.

### Same-campaign profile comparison

Both peers, marginal work per operation. Branch misses are Callgrind simulation. Total QUIC allocations include small transport-dependent differences in addition to the one removed reader arena.

| Operation | Frozen Ir | Candidate Ir | Ir change | Frozen Bcm | Candidate Bcm | Frozen blocks | Candidate blocks | Frozen bytes | Candidate bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| TLS dependent-read | 153804.79 | 153591.81 | -0.14% | 1559.77 | 1537.72 | 65.00 | 64.00 | 10651.98 | 10564.00 |
| TLS pipelined-read | 150050.58 | 149833.02 | -0.14% | 1912.16 | 1887.86 | 78.00 | 77.00 | 11660.00 | 11571.99 |
| QUIC pipelined-read | 186773.53 | 186526.92 | -0.13% | 2872.10 | 2951.37 | 94.01 | 92.98 | 12546.43 | 12455.37 |

Simulated conditional branch misses decrease for the two TLS profiles but increase by 2.76% for pipelined QUIC. Fewer instructions and allocations therefore do not establish a universal improvement in the remaining work.

### TLS native comparison

Mean ± sample SD of five run means, in µs. A positive delta means the candidate is slower. Frozen portable is `719a694`; candidate and current legacy are built from `ad54795`.

| Case | Legacy | Frozen portable | Candidate portable | C++ | Candidate vs frozen | Candidate vs legacy |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| loopback-0 | 42.573 ± 1.312 | 48.641 ± 11.575 | 46.873 ± 11.770 | 89.048 ± 26.774 | -3.63% | +10.10% |
| loopback-64 | 41.839 ± 0.844 | 42.331 ± 0.703 | 42.288 ± 0.762 | 62.137 ± 2.551 | -0.10% | +1.07% |
| loopback-1024 | 50.279 ± 9.981 | 46.890 ± 5.726 | 50.571 ± 9.438 | 73.647 ± 20.539 | +7.85% | +0.58% |
| loopback-65536 | 221.778 ± 53.787 | 202.468 ± 10.689 | 219.150 ± 44.987 | 303.318 ± 60.384 | +8.24% | -1.19% |
| unary-empty | 42.958 ± 0.589 | 42.932 ± 0.734 | 45.611 ± 4.326 | 62.739 ± 1.446 | +6.24% | +6.17% |
| unary-small | 41.450 ± 0.778 | 42.173 ± 0.431 | 42.968 ± 0.896 | 68.607 ± 13.684 | +1.89% | +3.66% |
| unary-1k | 59.589 ± 14.154 | 66.581 ± 12.063 | 61.705 ± 11.642 | 86.999 ± 17.879 | -7.32% | +3.55% |
| fanout-32 | 251.665 ± 37.583 | 279.811 ± 41.525 | 270.832 ± 17.992 | 586.715 ± 39.620 | -3.21% | +7.62% |
| dependent-read | 106.933 ± 26.389 | 128.765 ± 32.557 | 128.605 ± 28.457 | 189.940 ± 51.274 | -0.12% | +20.27% |
| pipelined-read | 70.820 ± 20.170 | 97.275 ± 28.166 | 91.449 ± 13.836 | 109.302 ± 13.386 | -5.99% | +29.13% |
| write-read | 84.365 ± 0.803 | 87.464 ± 5.621 | 90.179 ± 10.058 | 141.379 ± 38.579 | +3.10% | +6.89% |
| callback | 100.534 ± 20.493 | 113.682 ± 19.916 | 123.089 ± 25.188 | 170.082 ± 32.999 | +8.27% | +22.43% |
| bulk-64k | 208.378 ± 22.854 | 212.228 ± 30.459 | 216.778 ± 46.538 | 258.858 ± 4.548 | +2.14% | +4.03% |
| bulk-256k | 505.960 ± 24.515 | 519.856 ± 19.409 | 525.307 ± 22.906 | 1093.277 ± 31.217 | +1.05% | +3.82% |
| bulk-1m | 1767.996 ± 26.109 | 1774.320 ± 28.971 | 1782.928 ± 32.567 | 4248.843 ± 155.872 | +0.49% | +0.84% |
| bulk-4m | 9400.324 ± 468.818 | 9492.853 ± 183.373 | 9463.661 ± 210.759 | 18779.854 ± 277.452 | -0.31% | +0.67% |
| deep-8 | 61.820 ± 13.719 | 51.943 ± 9.167 | 51.330 ± 10.804 | 69.696 ± 4.640 | -1.18% | -16.97% |
| deep-32 | 55.941 ± 1.281 | 63.073 ± 18.060 | 55.253 ± 1.538 | 76.330 ± 2.230 | -12.40% | -1.23% |
| concurrent-32 | 215.289 ± 76.470 | 256.596 ± 73.398 | 207.789 ± 50.831 | 911.933 ± 249.437 | -19.02% | -3.48% |
| deep-32-concurrent-32 | 579.056 ± 140.378 | 585.814 ± 91.865 | 575.191 ± 60.630 | 843.354 ± 141.607 | -1.81% | -0.67% |
| services-4-deep-16 | 391.515 ± 2.096 | 417.110 ± 2.537 | 417.987 ± 5.391 | 516.166 ± 5.204 | +0.21% | +6.76% |
| services-4-deep-16-concurrent-32 | 2484.382 ± 76.852 | 2521.426 ± 52.290 | 2519.325 ± 91.432 | 4186.463 ± 87.822 | -0.08% | +1.41% |
| services-4-deep-16-clients-8 | 2580.391 ± 94.239 | 2633.743 ± 77.263 | 2645.671 ± 66.102 | 3668.008 ± 73.814 | +0.45% | +2.53% |
| services-8-deep-16-clients-8 | 5217.706 ± 157.952 | 5299.137 ± 61.880 | 5330.356 ± 150.077 | 7423.574 ± 195.325 | +0.59% | +2.16% |

### QUIC native comparison

Mean ± sample SD of five run means, in µs. A positive delta means the candidate is slower. Frozen portable is `719a694`; candidate and current legacy are built from `ad54795`.

| Case | Legacy | Frozen portable | Candidate portable | C++ | Candidate vs frozen | Candidate vs legacy |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| loopback-0 | 65.059 ± 14.943 | 75.370 ± 21.049 | 77.246 ± 20.809 | Unsupported | +2.49% | +18.73% |
| loopback-64 | 54.215 ± 0.667 | 58.418 ± 0.491 | 59.189 ± 0.833 | Unsupported | +1.32% | +9.17% |
| loopback-1024 | 64.835 ± 16.355 | 70.071 ± 18.305 | 77.252 ± 21.676 | Unsupported | +10.25% | +19.15% |
| loopback-65536 | 217.700 ± 1.124 | 224.009 ± 0.722 | 223.562 ± 1.895 | Unsupported | -0.20% | +2.69% |
| unary-empty | 67.359 ± 17.240 | 77.820 ± 16.048 | 76.834 ± 20.824 | Unsupported | -1.27% | +14.07% |
| unary-small | 74.427 ± 17.787 | 68.868 ± 17.349 | 75.513 ± 22.171 | Unsupported | +9.65% | +1.46% |
| unary-1k | 58.478 ± 1.407 | 62.453 ± 1.050 | 61.336 ± 1.085 | Unsupported | -1.79% | +4.89% |
| fanout-32 | 276.952 ± 9.474 | 299.263 ± 8.109 | 324.875 ± 59.815 | Unsupported | +8.56% | +17.30% |
| dependent-read | 156.175 ± 32.671 | 172.136 ± 39.411 | 206.880 ± 53.433 | Unsupported | +20.18% | +32.47% |
| pipelined-read | 90.015 ± 24.832 | 108.091 ± 0.367 | 111.080 ± 8.865 | Unsupported | +2.76% | +23.40% |
| write-read | 135.926 ± 33.288 | 147.985 ± 38.057 | 136.961 ± 34.425 | Unsupported | -7.45% | +0.76% |
| callback | 149.841 ± 30.772 | 165.364 ± 24.183 | 174.965 ± 28.794 | Unsupported | +5.81% | +16.77% |
| bulk-64k | 303.654 ± 55.651 | 302.830 ± 47.179 | 307.798 ± 57.319 | Unsupported | +1.64% | +1.36% |
| bulk-256k | 579.660 ± 34.871 | 681.076 ± 66.528 | 625.824 ± 44.522 | Unsupported | -8.11% | +7.96% |
| bulk-1m | 2053.714 ± 44.754 | 2097.602 ± 53.462 | 2031.528 ± 39.269 | Unsupported | -3.15% | -1.08% |
| bulk-4m | 10744.204 ± 448.158 | 10900.520 ± 461.771 | 11093.499 ± 466.058 | Unsupported | +1.77% | +3.25% |
| deep-8 | 64.242 ± 10.712 | 69.269 ± 14.712 | 77.627 ± 20.343 | Unsupported | +12.07% | +20.83% |
| deep-32 | 67.117 ± 1.675 | 89.831 ± 25.746 | 83.273 ± 25.167 | Unsupported | -7.30% | +24.07% |
| concurrent-32 | 194.993 ± 10.210 | 237.895 ± 51.220 | 234.386 ± 41.305 | Unsupported | -1.47% | +20.20% |
| deep-32-concurrent-32 | 643.183 ± 93.550 | 602.709 ± 23.807 | 626.499 ± 71.411 | Unsupported | +3.95% | -2.59% |
| services-4-deep-16 | 517.508 ± 2.581 | 519.366 ± 7.499 | 524.560 ± 7.669 | Unsupported | +1.00% | +1.36% |
| services-4-deep-16-concurrent-32 | 2937.181 ± 68.835 | 2833.455 ± 54.119 | 2851.992 ± 64.436 | Unsupported | +0.65% | -2.90% |
| services-4-deep-16-clients-8 | 3160.508 ± 35.935 | 2672.545 ± 54.543 | 2650.548 ± 59.994 | Unsupported | -0.82% | -16.14% |
| services-8-deep-16-clients-8 | 6513.850 ± 111.259 | 5050.525 ± 52.831 | 5252.056 ± 150.981 | Unsupported | +3.99% | -19.37% |

### Ciphertext read/write comparison

Both peers summed. The counts at 100 operations include setup/teardown; each cell is mean ± sample SD over five traces. Slopes use the means at 10 and 100 operations, divided by 90.

| Transport / case | Implementation | Reads at 100 | Writes at 100 | Reads/op slope | Writes/op slope |
| --- | --- | ---: | ---: | ---: | ---: |
| tls / fanout-32 | legacy | 1433.00 ± 0.00 | 210.00 ± 0.00 | 14.0000 | 2.0000 |
| tls / fanout-32 | portable | 1433.00 ± 0.00 | 210.00 ± 0.00 | 14.0000 | 2.0000 |
| tls / fanout-32 | cpp | 1027.00 ± 0.00 | 1028.00 ± 0.00 | 10.0000 | 10.0000 |
| tls / fanout-32 | portable-baseline | 1433.00 ± 0.00 | 210.00 ± 0.00 | 14.0000 | 2.0000 |
| tls / dependent-read | legacy | 413.00 ± 0.00 | 414.00 ± 0.00 | 4.0000 | 4.0000 |
| tls / dependent-read | portable | 413.00 ± 0.00 | 414.00 ± 0.00 | 4.0000 | 4.0000 |
| tls / dependent-read | cpp | 415.00 ± 0.00 | 416.00 ± 0.00 | 4.0000 | 4.0000 |
| tls / dependent-read | portable-baseline | 413.00 ± 0.00 | 414.00 ± 0.00 | 4.0000 | 4.0000 |
| tls / pipelined-read | legacy | 209.00 ± 0.00 | 210.00 ± 0.00 | 2.0000 | 2.0000 |
| tls / pipelined-read | portable | 209.00 ± 0.00 | 210.00 ± 0.00 | 2.0000 | 2.0000 |
| tls / pipelined-read | cpp | 211.00 ± 0.00 | 212.00 ± 0.00 | 2.0000 | 2.0000 |
| tls / pipelined-read | portable-baseline | 209.00 ± 0.00 | 210.00 ± 0.00 | 2.0000 | 2.0000 |
| quic / fanout-32 | legacy | 625.00 ± 0.00 | 213.00 ± 0.00 | 6.0000 | 2.0000 |
| quic / fanout-32 | portable | 627.00 ± 0.00 | 215.00 ± 0.00 | 6.0000 | 2.0000 |
| quic / fanout-32 | portable-baseline | 627.00 ± 0.00 | 215.00 ± 0.00 | 6.0000 | 2.0000 |
| quic / dependent-read | legacy | 421.00 ± 0.00 | 417.00 ± 0.00 | 4.0000 | 4.0000 |
| quic / dependent-read | portable | 424.00 ± 0.00 | 419.00 ± 0.00 | 4.0022 | 4.0000 |
| quic / dependent-read | portable-baseline | 423.40 ± 0.55 | 419.00 ± 0.00 | 3.9933 | 4.0000 |
| quic / pipelined-read | legacy | 217.00 ± 0.00 | 213.00 ± 0.00 | 2.0000 | 2.0000 |
| quic / pipelined-read | portable | 219.20 ± 0.45 | 215.00 ± 0.00 | 1.9933 | 2.0000 |
| quic / pipelined-read | portable-baseline | 219.60 ± 0.55 | 215.00 ± 0.00 | 2.0044 | 2.0000 |

Portable and legacy match TLS counts in these three cases. Dependent and pipelined TLS slopes also match C++; fanout still needs 14 successful reads/op versus C++'s 10, while using 2 writes/op versus C++'s 10. QUIC retains a small fixed lifecycle difference, with essentially matching steady slopes and minor receive variation. Ciphertext traces cannot identify the exact RPC verbs responsible for lifecycle packets. No claim is made for untraced cases.

## Baseline droplet campaign

The frozen pre-change portable implementation was slower than legacy by a geometric mean of **6.01% over TLS** and **8.14% over QUIC** (24 equally weighted cases per transport). It had lower means in 6/24 TLS and 5/24 QUIC cases. Portable had lower means than C++ in all 24 TLS cases, with a geometric-mean latency ratio 31.90% lower. These are observations from this campaign, not a universal performance guarantee.

| Profiled operation | Portable instructions | Legacy instructions | Portable excess | Portable allocations | Legacy allocations |
| --- | ---: | ---: | ---: | ---: | ---: |
| TLS dependent-read | 153799 | 126076 | +21.99% | 65.00 | 120.00 |
| TLS pipelined-read | 150052 | 111580 | +34.48% | 78.00 | 137.00 |
| QUIC pipelined-read | 186863 | 153617 | +21.64% | 94.01 | 158.98 |

The gap is not explained by allocation volume or extra steady-state socket writes in these cases. Exclusive instruction attribution places substantial work in the hosted portable adapter, wire admission/publication and engine; lower allocator costs partly offset it. Source categories describe where instructions are attributed, not a causal decomposition of latency. Inlining also attributes substantial portable work to Rust core/standard-library files.

## Remaining architecture work

The candidate removes one avoidable retained reader, but it does not remove the larger portable protocol path. The profiles and source inspection identify three concrete follow-ups:

1. **Carry publication facts from construction through commit.** The hosted data-only path already uses a private prepared-Return certificate. Capability-bearing replies still stage descriptors, build/copy an output frame, and then decode that frame again for publication validation. Extend the private, owner-bound construction certificate to the descriptor batch and cleanup facts; arbitrary caller-provided frames must keep full validation. Retry, cancellation and Finish ordering must remain transactional.
2. **Reduce coordinator work for capability traffic.** The coordinator has direct paths for plain calls/replies, but broader progress invalidates several work lanes and rebuilds the general dispatch adapters. Measure the capability pipeline path before replacing this with exact ready-operation notifications; fairness, backpressure and cancellation cannot depend on optimistic idle assumptions.
3. **Inspect TLS fanout receive buffering.** Both Rust drivers have the same 14-read slope versus C++'s 10, despite using fewer writes. This shared transport/reader issue is separate from portable's legacy gap. Do not add arbitrary batching delay to improve syscall counts at the expense of latency.

These are remaining hypotheses and design targets, not additional measured optimizations. The committed JSON retains the complete native, allocation, branch, cache and syscall comparisons so subsequent steps can use the same frozen controls.

## Hosts, provenance and cleanup

The independent baseline campaign used `c-4` in `lon1`, with an Intel Xeon Platinum 8280 guest CPU. The candidate campaign used `c-4` in `nyc1`, with an Intel Xeon Platinum 8168 guest CPU. Both reported four guest CPUs, one thread per core, and Ubuntu 24.04. Capacity selection moved regions between campaigns, so the runtime change is assessed against the frozen portable binary inside the candidate campaign, not by subtracting times across these two hosts.

The baseline's maximum observed single-CPU steal fraction was 0.52%; the candidate's was 9.09% (aggregate CPU maxima 0.42% and 2.94%). These are coarse whole-trial snapshots, including setup and teardown, not measurements confined to timed RPCs. They establish additional host variability; they do not identify the cause of an individual latency sample. No samples were discarded after observing their results. The cgroup quota setting was unavailable; throttling fields were available in all 600 baseline and 840 candidate trials, with no reported increments. Availability and observed counts are retained in each summary.

- [Baseline measurement run 39902949530](https://github.com/DericHuynh/capntproto/actions/runs/39902949530), source `719a694ac001ee89095485c8b65db32c249dfacc`.
- [Candidate measurement run 39903830343](https://github.com/DericHuynh/capntproto/actions/runs/39903830343), source `ad54795a3e36ae8cbb255dbc6d38e9ad5e55a7cd`; frozen portable `719a694ac001ee89095485c8b65db32c249dfacc`.
- [Saved-artifact analysis](https://github.com/DericHuynh/capntproto/actions/runs/39909021760), analyzer source `fbe94ddcfe44f8689dcfe2b310ac5504d7f78b2e`. This job only downloads and reads evidence; it never launches workloads.
- [Baseline raw samples/traces/profiles](https://github.com/DericHuynh/capntproto/actions/runs/39902949530/artifacts/12481116715): 52,705,764-byte artifact, SHA-256 `f62cca37828b8b6e794585f9da675d7f0aaff64a5525dfb4d415ad776860b2aa`.
- [Candidate raw samples/traces/profiles](https://github.com/DericHuynh/capntproto/actions/runs/39903830343/artifacts/12481234407): 73,965,870-byte artifact, SHA-256 `e37dc5d135859efa02ff4ac45ab6edc484caf166bcaeecf55402dfc567d9eba6`.

Raw and analysis artifacts expire on 2026-11-22 under the workflow retention policy. The committed [baseline summary](baseline.json), [candidate summary](candidate.json), [validation](validation.json) and [provenance](provenance.json) retain means, all five run means, spread, marginal profiles, syscall counts, source/binary hashes and artifact identities after that date. Full raw samples and allocation/call stacks remain in the linked artifacts until expiry.

Both measurement runs and their unconditional cleanup checks succeeded. Baseline droplets `611806712` and `611806856`, and candidate droplets `611810779` and `611810924`, were deleted; SSH-key cleanup was also checked. The candidate report/analysis additions do not change the runtime code that was measured.

## Correctness and tooling

Local correctness checks passed: 161 wire tests in release mode, 17 selected-struct tests in release mode, the same 17 with `sync_reader`, and 358 RPC tests in the development profile (553 total). Strict Clippy for core/wire/RPC, a `thumbv7em-none-eabi` no-default-features wire build, formatting and diff checks passed. Cloud lifecycle/packaging tests (25), Python harness tests (8), and strict dev Clippy passed.

The local fat-LTO RPC test compilation was killed by workspace memory pressure; it did not reach test execution. The RPC correctness suite was rerun successfully in the development profile. Wire and selected-struct suites did complete in release mode. An existing exact allocation-count test required `black_box` to prevent fat LTO from eliding the entire tested allocate/read/free sequence; the assertion still requires exactly one allocation.

Both CI benchmark builds passed 25 cloud tests, 20 fixture identity tests, 8 Python tests, all 70 supported workflow/implementation smoke cells, 72 cross-language checks, and 226 encrypted correctness/authentication checks. These are correctness checks, not performance measurements. This campaign does not represent a full coverage or workspace-wide quality-gate run.
