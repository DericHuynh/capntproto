# Scheduler allocation comparison — 23 October 2026

[Dedicated CI run 40010928237](https://github.com/DericHuynh/capntproto/actions/runs/40010928237) completed successfully, including all 336 DHAT trials, fixture checks, analysis, and droplet/key cleanup. Candidate: `8aa48c0367ab0a49d27babf4211abaa4d152da1d`. Frozen legacy control: `931c91ce3dd2751208fbdd3365c8286d126cac45`. Current portable shares the candidate scheduler. No local performance measurements were run.

**Legacy meets 12 of 48 allocation targets; 36 gaps remain and no cases are unmeasured.** The target is cumulative blocks per complete operation or concurrent batch, summed across all peers, <= min(current portable, C++). C++ has no QUIC implementation. All 24 equivalent TLS legacy cases are below C++; portable supplies the lower target throughout. See the [complete table](allocation-comparison.md), [counts and bytes](allocation-comparison.json), [profile scope](profile-scope.json), and [summary](summary.json).

| Workload | Frozen legacy | Candidate legacy | Current portable | C++ |
| --- | ---: | ---: | ---: | ---: |
| TLS unary-small | 15 | 15 | 12 | 91.49 |
| TLS dependent-read | 86 | 86 | 64 | 246.98 |
| TLS pipelined-read | 106 | 106 | 76 | 233.49 |
| TLS concurrent-32 | 338 | 338 | 362 | 2062.49 |
| TLS services-4-deep-16 | 94.84 | 94 | 73 | 361.98 |
| TLS services-4-deep-16-concurrent-32 | 2538 | 2346 | 2247 | 8303.98 |
| TLS services-4-deep-16-clients-8 | 2611.09 | 2484.91 | 2291.33 | 8571.56 |
| TLS services-8-deep-16-clients-8 | 5588.47 | 5202.80 | 4822.11 | 16928 |
| QUIC concurrent-32 | 472.64 | 472.96 | 432.42 | unsupported |
| QUIC services-4-deep-16-concurrent-32 | 3144.64 | 2952.36 | 2595.64 | unsupported |
| QUIC services-4-deep-16-clients-8 | 3306.64 | 3202.04 | 2746.60 | unsupported |
| QUIC services-8-deep-16-clients-8 | 6855.42 | 6449.22 | 5662.80 | unsupported |

The scheduler removes a specific concurrent allocation site. Raw TLS stacks show 64 driver-owned FuturesUnordered nodes per promoted service: 32 protected-execution tasks and 32 answer-completion tasks. Candidate scheduler reuse removes those nodes. The fixture's own concurrent work remains unchanged and is excluded from this attribution.

| TLS workload | Frozen driver nodes per batch | Candidate driver nodes | Total legacy reduction |
| --- | ---: | ---: | ---: |
| Four services, concurrency 32 | 192 | 0 | 192 |
| Four services, eight clients | 128 | 0 | 126.18 |
| Eight services, eight clients | 384 | 0 | 385.67 |

[Per-peer attribution](scheduler-attribution.json) records exact totals and driver node counts. In the eight-client cases, the first service has eight connections with four calls each and did not promote its scheduler; deeper hops aggregate the calls and did. This explains why four services remove 128 nodes with eight clients but 192 with one concurrent client. Residual fractional changes include transport/background allocations and are not assigned to the scheduler.

The same-run frozen legacy meets 11 targets. Its sole status change relative to candidate is QUIC bulk-1m (1377 versus 1367.71 candidate and 1375.67 portable); packet/background variation is enough to change that ordering. The earlier ownership campaign met 10 targets. Those pass-count changes do not establish a scheduler benefit for large transfers. The concurrent service chains still exceed portable after the measured node removal, and portable also benefits from the shared scheduler implementation.

This campaign contains allocation counts and cumulative allocated bytes only. It does not measure native latency, read/write parity, or retained/peak memory. Pairs use 10 warmups and 5/50 measured iterations; count deltas divide by 45. Native timing requires the separate five-repetition qualification campaign.

## Preserved evidence

- Analysis artifact 12531515983, 118315039 bytes, SHA-256 `df41fbd4d8457688bd8d7348d29e664d37355dd93fb4ee5f83ff6fb9c93bc0ef`.
- Raw report artifact 12531496596, 282993388 bytes, SHA-256 `91fdfb0140608193aa4e8d093c380d3ad45f39a925c350ab64b76c8321332d2d`.

Both downloaded ZIPs were checked against their artifact digests. The raw report includes every peer's DHAT data and stderr. The in-artifact source status is `analysis-before-upload`; final workflow completion was checked separately and is successful.

## Following candidates

[Run 40016773400](https://github.com/DericHuynh/capntproto/actions/runs/40016773400) measures encrypted batching and final-owner answer-status reuse (`bbe8af44a55f49b3c82138aef9955e4e23aa9bde`) against this scheduler revision.

[Run 40022705577](https://github.com/DericHuynh/capntproto/actions/runs/40022705577) qualifies `1a2d77250d49691a600ca97552003d95d053c202` against frozen `bbe8af4`: borrowed capability-bearing completion, empty request reuse, cancellation-only answer tasks, shared export dispatch targets, lazy shortening cancellation, and the bounded large QUIC prefix. It runs native timing five times with mean/spread, selected ciphertext syscall traces, Callgrind, and the full DHAT matrix. Those candidates remain unmeasured in this report. Correctness validation and the known pre-existing portable early-pipeline timeout are documented in the [implementation report](../2026-10-23-allocation-parity/README.md).
