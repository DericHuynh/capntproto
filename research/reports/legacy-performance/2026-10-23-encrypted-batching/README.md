# Encrypted batching allocation comparison — 23 October 2026

[Dedicated CI run 40016773400](https://github.com/DericHuynh/capntproto/actions/runs/40016773400) completed successfully, including all 336 DHAT trials, fixture and certificate-rejection checks, analysis, and droplet/key cleanup. Candidate: `bbe8af44a55f49b3c82138aef9955e4e23aa9bde`. Frozen legacy control: `8aa48c0367ab0a49d27babf4211abaa4d152da1d`. Current portable shares the encrypted batching adapter. No local performance measurements were run.

**Legacy meets 27 of 48 allocation targets; 21 gaps remain and no cases are unmeasured.** Eighteen TLS and nine QUIC cases meet the target. The target is cumulative blocks per complete operation or concurrent batch, summed across all peers, <= min(current portable, C++). All 24 supported TLS legacy cases are below C++; portable supplies the lower target throughout. C++ has no QUIC implementation. The same-run frozen legacy meets 12 targets.

See the [complete comparison](allocation-comparison.md), [counts and cumulative bytes](allocation-comparison.json), [profile scope](profile-scope.json), and [summary](summary.json).

| Workload | Frozen legacy | Candidate legacy | Current portable | C++ |
| --- | ---: | ---: | ---: | ---: |
| TLS unary-small | 15.00 | 12.00 | 12.00 | 91.49 |
| TLS dependent-read | 86.00 | 81.00 | 62.00 | 246.98 |
| TLS pipelined-read | 106.00 | 103.00 | 74.00 | 233.49 |
| TLS callback | 57.00 | 51.00 | 46.00 | 204.98 |
| TLS concurrent-32 | 338.00 | 304.00 | 360.00 | 2062.49 |
| TLS services-4-deep-16 | 94.44 | 82.47 | 73.00 | 362.02 |
| TLS services-4-deep-16-concurrent-32 | 2346.00 | 2210.02 | 2239.00 | 8303.98 |
| TLS services-4-deep-16-clients-8 | 2484.78 | 2328.00 | 2253.93 | 8562.62 |
| TLS services-8-deep-16-clients-8 | 5211.91 | 4910.27 | 4794.91 | 16972.44 |
| QUIC concurrent-32 | 473.27 | 316.64 | 371.84 | unsupported |
| QUIC services-4-deep-16-concurrent-32 | 2953.20 | 2380.64 | 2407.58 | unsupported |
| QUIC services-4-deep-16-clients-8 | 3210.62 | 2649.89 | 2592.49 | unsupported |
| QUIC services-8-deep-16-clients-8 | 6571.96 | 5465.51 | 5349.80 | unsupported |
| QUIC bulk-4m | 5460.38 | 5456.16 | 5407.67 | unsupported |

## Attributed reductions

Raw TLS unary-small stacks show one eliminated rustls temporary slice vector per peer and one eliminated answer-status allocation on the server. These account for the entire 15 -> 12 block reduction. Client counts fall from 9 to 8 and service counts from 6 to 4; current portable contributes 7 and 5. The final-owner status cache now also handles responses released after their answer-table entry.

For QUIC concurrent-32, quiche send-buffer allocations fall from 64 to 2 on each peer: **128 -> 4 per complete batch**. Coalescing covers the batch's segment tables and payloads, so it removes 124 allocations, rather than merely one allocation per call. Status reuse removes another 32 server allocations. Those sites explain 156 of the measured 156.62-block reduction; the remaining fractional change includes transport/background work. This is allocation evidence, not a native speedup claim.

[Selected per-peer attribution](batching-attribution.json) records those sites and their totals. Counts derive from paired 5/50-iteration DHAT trials with 10 warmups. Every all-peer total includes every site. The selected site counters omit sites below 0.5 blocks per operation and are not a partition of the full total.

## Remaining gaps

The six TLS gaps are dependent-read (+19 blocks), pipelined-read (+29), callback (+5), serial four-service traversal (+9.47), four-service/eight-client concurrency (+74.07), and eight-service/eight-client concurrency (+115.36). Later dispatch, request ownership, typed reply, and lazy streaming changes target sites in these paths; their measurements must be reported separately.

The fifteen QUIC gaps include the corresponding RPC paths, small fractional differences on otherwise similar unary/deep/write-read workloads, and bulk-4m (+48.49). Packetization, ACKs, and background allocations make the fractional QUIC comparisons sensitive to transport behavior. A small measured excess remains a gap; it is not rounded into parity or rerun selectively to obtain a passing result.

The bulk-4m trace also identifies duplicate connection-ID allocation in the shared endpoint: legacy allocates 285.24 client and 285.96 server blocks in the endpoint's short-header parser, then repeats the same counts inside quiche's receive parser. The endpoint only needs borrowed destination bytes for route lookup. Removing that routing copy is a subsequent candidate and affects both current drivers. These saved counts identify the site; they are not a measured saving for the subsequent code.

This allocation-only campaign has no native latency, syscall parity, or peak/retained memory results. Cumulative bytes are available in the comparison; reduced block counts alone do not establish reduced latency or peak memory.

## Preserved evidence and following qualifications

- Analysis artifact 12534014201: 118072108 bytes, SHA-256 `b69995427c2074bba25457e849b8ea3c4a3e793b3d5bcb5ea2fa02abf77859d4`.
- Raw report artifact 12534906478: 282828331 bytes, SHA-256 `b3ff9d18f2d370d7955687db30a872a153eea241232ef4294f8b3e095cfce732`.

Both downloaded ZIPs match their published artifact digests. No DHAT trial failed or lacked a peer pair. The in-artifact provenance says `analysis-before-upload`; final workflow success was checked independently.

[Run 40022705577](https://github.com/DericHuynh/capntproto/actions/runs/40022705577) qualifies `1a2d77250d49691a600ca97552003d95d053c202` against this candidate, with five native timing repetitions, selected syscall traces, Callgrind, and the complete allocation matrix.

[Run 40030571555](https://github.com/DericHuynh/capntproto/actions/runs/40030571555) qualifies the later direct typed replies and lazy streaming ownership from `3aec134bd089265aa47397085a8bfc4fffab6f04` against frozen `1a2d772`. Its source `4fa3011888fa728546c86435d601caf22943b7c4` adds only an isolated workflow caller to that implementation. The GitHub connector launched this branch because the workspace CLI credential expired. It invokes the same reusable performance workflow, passes the frozen control explicitly, and retains the existing dedicated concurrency slot. It was launched after 40022705577 acquired that slot.

Neither following run includes the later borrowed QUIC routing change. Correctness validation and the pre-existing portable native early-pipeline timeout are documented in the [implementation report](../2026-10-23-allocation-parity/README.md).
