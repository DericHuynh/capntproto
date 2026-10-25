# Current portable versus legacy — 2026-10-23

Legacy remains faster in the six targeted workloads. The evidence points to more protocol/ownership bookkeeping in portable, especially for capability-bearing and pipelined calls. Portable already allocates substantially less and matches legacy’s steady socket-write/read growth in the three traced workloads. Reducing allocations or writes alone does not address the main observed gap.

This report diagnoses `7993d5214ca133b8a4c524d7ec30030d41fe4622`, now on `main`. Runtime code and fixtures were not modified. Both variants run the same release executable, selected independently on each peer. All 763 files in its frozen source manifest match the working tree.

## Fresh native comparison

Five separate process repetitions, 1,000 warmups and 5,000 measured operations each. Mean ± sample SD of run means, in microseconds. Client and server use separate physical cores; variants rotate. The TLS 1.3/QUIC handshake is outside timing, while result validation and destruction are inside. Fanout and concurrent cases measure whole batches of 32 calls.

| Transport | Case | Legacy µs | Portable µs | Portable overhead |
|---|---|---:|---:|---:|
| tls | fanout-32 | 163.17 ± 16.12 | 188.42 ± 15.64 | +15.5% |
| tls | dependent-read | 73.07 ± 8.45 | 86.09 ± 12.63 | +17.8% |
| tls | pipelined-read | 44.62 ± 7.85 | 62.92 ± 13.71 | +41.0% |
| tls | callback | 64.45 ± 10.11 | 77.24 ± 10.05 | +19.8% |
| tls | concurrent-32 | 114.87 ± 9.47 | 143.01 ± 12.46 | +24.5% |
| tls | deep-32-concurrent-32 | 333.16 ± 32.15 | 374.21 ± 19.96 | +12.3% |
| quic | fanout-32 | 181.82 ± 18.64 | 225.59 ± 10.81 | +24.1% |
| quic | dependent-read | 88.89 ± 7.65 | 131.59 ± 23.64 | +48.0% |
| quic | pipelined-read | 50.66 ± 3.57 | 89.01 ± 8.11 | +75.7% |
| quic | callback | 98.47 ± 4.37 | 123.10 ± 9.39 | +25.0% |
| quic | concurrent-32 | 130.95 ± 7.82 | 165.82 ± 17.58 | +26.6% |
| quic | deep-32-concurrent-32 | 389.28 ± 14.09 | 439.52 ± 18.45 | +12.9% |

Across these selected cases, geometric mean overhead is **21.5% for TLS** and **33.9% for QUIC**. These are focused gap measurements, not a new full-suite aggregate.

The [previous full 24-case report](../2026-10-23-direct-paths/README.md) measures this same source and executable: portable had lower means than legacy in 7/24 TLS and 5/24 QUIC cases, with geometric mean overhead of 6.84% and 8.39%. Bulk and service-topology results there remain relevant; they were not rerun here. Native spread is substantial, and differences between campaigns do not imply a code regression. All fresh run means and spread are in [latency.json](latency.json).

## Which peer adds the gap?

A separate five-run campaign mixes the implementations without changing transport or application work. Each heading is **client → server**; values are mean ± run SD in µs.

| Transport / case | Legacy → legacy | Legacy → portable | Portable → legacy | Portable → portable |
|---|---:|---:|---:|---:|
| tls / dependent-read | 72.20 ± 4.29 | 77.51 ± 5.85 | 78.80 ± 7.30 | 90.69 ± 8.64 |
| tls / pipelined-read | 47.38 ± 4.04 | 55.89 ± 9.48 | 61.96 ± 5.69 | 70.31 ± 9.56 |
| quic / dependent-read | 103.08 ± 16.35 | 111.88 ± 15.49 | 117.63 ± 19.96 | 133.99 ± 13.70 |
| quic / pipelined-read | 54.03 ± 7.59 | 75.69 ± 14.47 | 81.64 ± 10.29 | 96.10 ± 11.55 |

Both peers contribute. In TLS pipelining, switching only the client adds 14.6 µs to the legacy/legacy mean; switching only the server adds 8.5 µs. The CPU profiles agree with that direction: estimated client instructions rise from 48,185 to 75,400 per pipelined operation, while server instructions rise from 60,197 to 74,901. Mixed results include interactions between peers and runtime integration, so the differences are not exact additive component timings.

## CPU work and branch/cache behavior

Callgrind profiles both complete peer lifecycles at 50 and 500 operations, each after 10 warmups. The table uses `(500-operation count − 50-operation count) / 450` to reduce startup contribution. There is one profile per point. These are **simulated counters**, not native time or hardware PMU measurements.

| Transport / case | Extra instructions | Extra conditional branch misses | Extra D1 read misses |
|---|---:|---:|---:|
| tls / fanout-32 | +13.0% | +39.4% | +34.2% |
| tls / dependent-read | +25.2% | +41.0% | +84.5% |
| tls / pipelined-read | +38.7% | +87.6% | +83.1% |
| tls / deep-32-concurrent-32 | +5.1% | +29.4% | +41.9% |
| quic / fanout-32 | +5.3% | +10.9% | +25.4% |
| quic / dependent-read | +14.6% | +29.4% | +36.3% |
| quic / pipelined-read | +24.1% | +52.0% | +42.6% |
| quic / deep-32-concurrent-32 | +2.8% | +7.3% | +29.5% |

TLS pipelining rises from approximately **108,382 to 150,301 instructions per operation**. Exclusive source attribution shows these portable-minus-legacy changes in both TLS and QUIC: **+17,601 instructions in the hosted portable code, +12,889 in the wire layer, and +11,663 in the engine**. The identical deltas across transports are strong evidence of RPC bookkeeping overhead. Those are gross source-category differences, not independent projected savings: legacy-specific dispatch disappears from the portable profile, allocation work is lower, and shared code also changes.

Within portable TLS pipelining, `coordinator/work.rs` accounts for about 3,451 instructions per operation, wire `connection/maintenance.rs` 2,741, wire `owned.rs` 2,447, and engine `clients.rs` 1,903. Costs are distributed across the ownership path. The most prominent portable-specific conditional-miss sites include `poll_active_work`, the receive/output pump, and `refresh_exchange`; no single branch explains the whole gap. Inlining attributes much work to standard-library files, so these file counts are not complete component costs. See [profiles.json](profiles.json) and [attribution.json](attribution.json).

## Allocation is already cheaper

DHAT uses the same 50/500-operation subtraction. Values below are cumulative allocation blocks and allocated bytes per whole operation, across both peers; they are not peak or retained memory.

| Transport / case | Legacy blocks | Portable blocks | Legacy bytes | Portable bytes |
|---|---:|---:|---:|---:|
| tls / fanout-32 | 798.0 | 374.0 | 413,924 | 309,796 |
| tls / dependent-read | 120.0 | 66.0 | 16,697 | 10,740 |
| tls / pipelined-read | 137.0 | 79.0 | 18,257 | 11,748 |
| tls / deep-32-concurrent-32 | 802.0 | 378.0 | 562,168 | 408,376 |
| quic / fanout-32 | 944.1 | 456.1 | 292,721 | 188,593 |
| quic / dependent-read | 152.0 | 94.0 | 18,468 | 12,542 |
| quic / pipelined-read | 159.0 | 95.0 | 19,144 | 12,635 |
| quic / deep-32-concurrent-32 | 955.3 | 467.3 | 441,395 | 287,607 |

For TLS pipelining, portable allocates **42.3% fewer blocks and 35.7% fewer bytes**, while executing 38.7% more instructions. Exclusive allocator attribution is about 8,193 instructions lower in portable per operation. Allocation volume cannot explain legacy’s lead. This does not rule out costly individual allocations or memory-layout effects.

The remaining allocation stacks are useful architectural markers: portable TLS pipelining creates about **three cached reader arenas on the client and seven on the server per operation**, including admission, outgoing publication and control handling. One server arena comes from reopening the retained answer during `Finish`. `Frame::shared_reader()` does cache within a Frame; the repeated ownership boundary creates a new Frame around the retained lease. This is a specific re-reading opportunity, not evidence that every pointer access reparses its message. See [allocations.json](allocations.json) and [reader-arena-sites.json](reader-arena-sites.json).

## Socket traffic is not the dominant gap

Fresh `strace` runs have five repetitions at 10 and 100 operations, both with two warmups. Counts include both peers’ full lifecycle, handshake and shutdown. They count successful socket syscalls, not RPC frames or QUIC packets. The 100-operation means are:

| Transport / case | Legacy writes / reads | Portable writes / reads |
|---|---:|---:|
| tls / fanout-32 | 210.0 / 1433.0 | 210.0 / 1433.0 |
| tls / dependent-read | 414.0 / 413.0 | 414.0 / 413.0 |
| tls / pipelined-read | 210.0 / 209.0 | 210.0 / 209.0 |
| quic / fanout-32 | 212.6 / 624.6 | 215.0 / 627.0 |
| quic / dependent-read | 416.4 / 420.4 | 418.8 / 422.8 |
| quic / pipelined-read | 212.6 / 216.6 | 215.0 / 219.0 |

TLS counts match. For QUIC pipelining the portable excess is **2.4 writes and 2.4 reads at both 10 and 100 operations**; it does not grow with these additional 90 operations. One portable write and one read occur after the client starts emitting its final result JSON in all five 100-operation traces, versus zero for legacy. That portion is outside the measured workload. The remainder is consistent with fixed lifecycle/control differences; ciphertext alone does not identify its frames. This evidence does not establish universal I/O parity.

Both drivers add approximately two successful writes per pipelined/fanout operation and four per dependent-read operation over this range. TLS pipelining also makes fewer `epoll` calls in portable: 214.8 versus 424.0 across both peers at 100 operations. QUIC counts are close: 628.8 versus 627.4. These instrumented syscall counts do not measure task wakeups or native scheduling delay. They argue against explaining this gap as “portable performs more kernel polls.” All counts, per-peer values and spread are in [traces.json](traces.json); the after-result boundary is in [post-result-io.json](post-result-io.json).

## Why legacy does less work

1. **Capability-bearing completion still crosses several portable owners.** Legacy’s [Return dispatch](../../../../crates/capntproto-rpc/src/rpc/dispatch.rs) finds the question, receives capabilities, creates its response and calls [QuestionRef::fulfill_result](../../../../crates/capntproto-rpc/src/rpc.rs), settling the pipeline and fulfiller directly. Portable’s [ingress](../../../../crates/capntproto-rpc/src/scoped/wire/coordinator/ingress.rs) admits into the wire ledger, delivers through the hosted call owner, checks the [observer/authorization fence](../../../../crates/capntproto-rpc-wire/src/owned/connection/awaiting.rs), then [binds/exports capabilities, refreshes hooks and releases wire output](../../../../crates/capntproto-rpc/src/scoped/wire.rs) before final hosted completion. The latest redesign removed the shared pipeline future; that removed future is not the current explanation. The fastest plain-result path still declines active pipelines and capability-bearing results.

2. **Publication and cleanup retain serialized ownership where legacy keeps direct metadata.** Portable’s [capability-bearing Finish path](../../../../crates/capntproto-rpc-wire/src/owned/answer.rs) retains the original Return lease in a new Frame and reads it to perform grant cleanup. Legacy’s `handle_finish()` takes `answer.result_exports` directly. DHAT confirms the corresponding fresh reader arena; the source shows why it exists. Portable already skips this for certified plain answers. Preserving the reader or storing the exact validated grant metadata would extend that simplification to the remaining path.

3. **Coordinating those owners remains real work.** The [coordinator](../../../../crates/capntproto-rpc/src/scoped/wire/coordinator/work.rs), [server stages](../../../../crates/capntproto-rpc/src/scoped/wire/server/worker.rs), [exchange-ready queues](../../../../crates/capntproto-rpc-wire/src/owned/connection/exchange_work.rs), message leases and capability graph each maintain their part of progress and lifetime. Exact queues replaced broad scans, but one RPC can still update multiple representations of the same operation. The instruction and branch/cache profiles support simplifying these transitions; they do not support blaming the former whole-queue mutex or assuming every wake makes a syscall.

## Highest-value next changes

- Fuse ordinary capability-bearing Return admission with its hosted completion owner after the required observer fence, reducing temporary bind/export/release transitions. This directly targets the largest client-side gap.
- Retain validated grant/release metadata on answers, or preserve their reader owner through cleanup, so `Finish` does not reconstruct a Frame and reader for an already-published Return.
- Extend synchronous server completion beyond the current plain-result shortcut, keeping one owner through dispatch, result collection and publication. Keep deferred stages only when execution, ordering or capacity actually suspends.

These are architectural hypotheses to implement and remeasure, not promised speedups. Bounded admission, authorization, cancellation and pipeline ordering remain required; their state can be represented more directly. The evidence does not prioritize more allocator pooling, AES tuning or fewer steady socket writes for these particular gaps.

## Evidence and limits

The campaign validated **200 native/mixed trials**, **120 traced trials**, **64 Callgrind peer profiles**, and **64 DHAT peer profiles**. Native timing, traces and the two profiler campaigns ran separately. No runtime code changed, so unit/integration tests, coverage and Clippy were not rerun. Earlier direct-path validation covers the exact measured source. This campaign does not include a new C++ comparison or a full multi-hop/bulk sweep.

[evidence-index.json](evidence-index.json) records the commit, executable hash, complete source manifest and toolchain/build provenance. [validation.json](validation.json) records campaign dimensions and limitations. The local evidence archive described by [archive.json](archive.json) includes the executable, every native sample, raw syscall traces, complete profiles/allocation stacks, analysis scripts and reproduction commands. Native variance and instrumentation effects prevent attributing every microsecond to a specific component; the source-level conclusions are supported by several independent measurements rather than by an isolated latency number.
