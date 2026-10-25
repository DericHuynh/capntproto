# Legacy dispatch qualification — 24 October 2026

[Dedicated run 40022705577](https://github.com/DericHuynh/capntproto/actions/runs/40022705577) **passed**, including build/fixture checks, measurements, report validation, and droplet/key cleanup. It completed at 23:56 UTC on 23 October. Candidate implementation: `1a2d77250d49691a600ca97552003d95d053c202`; frozen legacy control: `bbe8af44a55f49b3c82138aef9955e4e23aa9bde`. Current portable was rebuilt from the candidate source. This report analyzes saved droplet evidence only.

**Legacy meets 44/48 allocation targets: 22 TLS and 22 QUIC cases. Four gaps remain; none are unmeasured.** The same-run frozen legacy meets 27/48. The target is cumulative DHAT blocks per complete operation or concurrent batch, summed over every peer, <= min(current portable, C++). C++ is supported only for TLS; portable is the lower allocation comparator in every case. All 24 TLS legacy counts are below C++.

The campaign contains 840 native trials (five repetitions of 1000 measured operations for each supported workload/implementation), 336 DHAT trials and 64 Callgrind trials. No profile failed or lacked a peer pair. All 448 per-peer allocation block and byte totals in the saved allocation stacks were independently checked against the summary. All five-repetition means and standard deviations were recomputed and matched.

## Allocation improvements and remaining gaps

The candidate includes borrowed completion for immediate capability-bearing handlers, absent resolution tasks for settled exports, empty request-box reuse, lazy local shortening cancellation, cancellation-only answer tasks, direct shared export targets, and bounded coalescing of large QUIC frame prefixes. Shared changes also improve portable and are included in its comparator. See the [implementation report](../2026-10-23-allocation-parity/README.md) for correctness coverage and the recorded pre-existing native portable early-pipeline timeout.

| Workload | Frozen legacy | Candidate legacy | Current portable | C++ |
| --- | ---: | ---: | ---: | ---: |
| TLS unary-small | 12.00 | 10.00 | 12.00 | 91.49 |
| TLS dependent-read | 81.00 | 53.00 | 57.00 | 246.98 |
| TLS pipelined-read | 103.00 | 65.00 | 69.00 | 233.49 |
| TLS callback | 51.00 | 43.00 | 46.00 | 204.98 |
| TLS concurrent-32 | 304.00 | 240.00 | 360.00 | 2062.49 |
| TLS services-4-deep-16-clients-8 | 2327.60 | 1881.18 | 2255.62 | 8547.78 |
| TLS services-8-deep-16-clients-8 | 4911.73 | 3951.73 | 4782.53 | 16915.47 |
| QUIC pipelined-read | 117.18 | 79.62 | 82.84 | unsupported |
| QUIC concurrent-32 | 316.49 | 252.98 | 372.13 | unsupported |
| QUIC services-8-deep-16-clients-8 | 5476.18 | 4449.64 | 5340.00 | unsupported |

Dependent reads, pipelines, callbacks, and all measured concurrent service-chain patterns meet the allocation target. The [complete 48-case allocation table](allocation-comparison.md) and [counts plus cumulative bytes](allocation-comparison.json) retain the frozen control. The only remaining allocation gaps are large transfers:

| Transport / workload | Legacy | Portable target | Excess blocks |
| --- | ---: | ---: | ---: |
| TLS bulk-1m | 348.40 | 321.60 | +26.80 |
| TLS bulk-4m | 1144.04 | 1094.76 | +49.29 |
| QUIC bulk-1m | 1392.93 | 1382.53 | +10.40 |
| QUIC bulk-4m | 5521.87 | 5428.42 | +93.44 |

## What the remaining allocation stacks show

The TLS excess is concentrated in rustls receive-buffer growth. For bulk-1m, legacy has 70.40 deframer-buffer allocations versus portable's 33.60; for bulk-4m, the counts are 98.04 versus 38.76. Legacy has fewer allocations outside that site, so the net gaps are smaller (26.80 and 49.29). Both drivers have the same encrypted-record allocation totals: 130 blocks per bulk-1m operation and 514 per bulk-4m operation across both peers. Received-plaintext counts are also effectively equal.

Pinned rustls 0.23.46 `DeframerVecBuffer::prepare_read()` reduces its buffer toward 4096 bytes with `shrink_to()` when empty, then grows it again to assemble larger TLS records. The trace records the corresponding `Vec::resize`/growth allocations. Receive scheduling and record boundaries affect how often this occurs. Bounded record-buffer retention is the next area to investigate. The existing data identifies the allocation site; it does not prove that a particular adapter change will remove it or improve latency. These bulk workloads were not in the syscall-trace selection.

QUIC bulk-4m spends 2059.20 legacy blocks versus 2055.71 portable blocks on stream send buffers. Larger differences occur in transport-parameter connection-ID clones (1011.62 versus 966.29), duplicated header parsing (583.98 versus 573.53 at each of the endpoint and quiche layers), and receive ranges/queues. Quiche 0.30.0 constructs `tls::ExData` with a clone of local transport parameters before checking `handshake_completed`; the completed path still performs post-handshake TLS processing. That processing must be preserved when investigating metadata ownership. Packetization, acknowledgements, and scheduling affect these counts; the measured excess remains a gap.

[Per-peer bulk attribution](bulk-attribution.json) records non-overlapping named sites and an explicit residual. Every full total includes all sites; named categories omit sites below 0.5 blocks per operation. These are attribution counts, not claims of achievable savings.

## Native latency: five repetitions with spread

Against C++, legacy has lower mean latency in **24/24 supported TLS cases**, with **40.33% lower latency by the geometric mean of case ratios**. The smallest observed advantage is 13.43% in deep-32. Only 16/24 TLS cases reach 30% lower mean latency; this is not a 30%-everywhere result. C++ has no QUIC comparator.

Against current portable, legacy has lower means in **20/24 TLS and 20/24 QUIC cases**; geometric-mean latency reductions are **9.27% TLS** and **8.78% QUIC**. Against frozen legacy, those reductions are 2.63% and 1.55%, with lower means in 16/24 and 15/24 cases respectively. Allocation wins do not guarantee latency wins.

The [full latency table](latency-comparison.md) reports microseconds per complete operation/batch as mean ± standard deviation of the five repetition means. The JSON summary also retains all repetition means, ranges, and per-operation distribution statistics. Some rows vary substantially: TLS dependent-read is 95.55 ± 24.48 µs for legacy versus 102.16 ± 0.65 µs for portable. The means alone do not establish a stable advantage in such rows. No repetition is omitted or selected to improve the comparison.

The eight cases with higher legacy means than portable are TLS bulk-64k (+1.73%), bulk-4m (+0.34%), deep-8 (+8.19%), deep-32 (+5.99%); and QUIC unary-empty (+4.43%), unary-small (+0.86%), services-4-deep-16 (+4.52%), services-8-deep-16-clients-8 (+6.61%). These are measured mean differences, not significance claims. The smallest differences overlap the observed spread.

The native host counters report zero throttled trials and zero throttled microseconds across all 840 trials. They also record nonzero CPU steal, with a maximum aggregate sample of 2.86%. Dedicated droplets reduce environmental interference; the measured repetition spread still matters. The comparison covers complete encrypted implementations, including their different TLS libraries.

## Read/write counts

The [I/O table](io-comparison.md) and [machine-readable counts](io-comparison.json) sum successful ciphertext socket reads/writes over every peer, using (100-operation mean minus 10-operation mean)/90 with five repetitions at each length. Attempts including nonpositive results are retained separately. QUIC sendmmsg calls count syscalls rather than individual datagrams.

Legacy matches C++ on TLS dependent-read (4 reads / 4 writes) and pipelined-read (2 / 2). It still uses more successful reads for concurrent-32 (4 versus 2), fanout-32 (14 versus 10), and services-4-deep-16-clients-8 (79.01 versus 51.09). Writes are equal or lower across the five traced TLS cases. Thus the older goal of reads and writes <= C++ everywhere remains unmet. Neither bulk transfer syscall parity nor untraced cases can be inferred from this subset.

## Provenance and later candidates

The downloaded analysis artifact 12539573623 is 132437690 bytes with SHA-256 `ba126590bd603ff4149234224124eba4399e949c7edb8f1f1b560e49060a428a`; its digest was verified. Its allocation-sites file was parsed directly from the ZIP without extracting its 3.6 GB expansion. All 448 peer totals match the summary ([validation](site-validation.json)). The published [summary](qualification-summary.json) copies the relevant measurement fields and omits the full function stacks and infrastructure addresses; complete source data remains in the verified artifact. The in-artifact source status is `analysis-before-upload`; final workflow success and cleanup were checked separately.

The raw report artifact 12539313389 exists in the successful run (538082323 bytes; published SHA-256 `4ff5e4ab9be88a0766b44de9983596ae49777c3c7c632adb40a3ab83726cc889`). It was not downloaded or independently verified here: it exceeds the connector's 512 MiB limit, and direct CLI download returned HTTP 403. The analysis artifact contains the complete normalized allocation stacks used for this report.

This measured source predates lazy streaming ownership and direct typed replies (`3aec134`), which are being qualified in [run 40030571555](https://github.com/DericHuynh/capntproto/actions/runs/40030571555). It also predates borrowed QUIC routing IDs (`1a9e5fc`), whose [run 40031849761](https://github.com/DericHuynh/capntproto/actions/runs/40031849761) has passed its sequencing gate and started building. Neither later optimization is credited with this run's savings.
