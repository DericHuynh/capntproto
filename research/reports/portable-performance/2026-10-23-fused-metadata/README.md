# Portable path fusion and retained cleanup metadata

The common ownership and cleanup paths do less work, but this is **not an overall latency win**. Across 24 cases, final portable geometric mean latency is **1.30% lower on TLS** and **4.52% higher on QUIC** than the original portable control. It remains **5.16% above legacy on TLS** and **12.72% above legacy on QUIC**. Portable has lower observed means than C++ in all 24 TLS cases, with geometric mean latency **30.92% lower**. C++ QUIC is unavailable.

Focused profiles show about 1% fewer simulated instructions and one fewer allocation per operation. Simulated conditional branch misses increase. The direct single-export release adds no meaningful benefit to these steady-state read fixtures: their Finish messages normally preserve returned-capability ownership. Explicit release, underflow and local-owner lifetime tests cover the cleanup path without scratch storage.

This change removes repeated ownership conversion and completion work in the portable driver, then retains validated export metadata to avoid decoding Return messages during common Finish cleanup. The implementation was measured in three stages against frozen controls.

## Implementation

1. **Transfer received capabilities directly into hosted clients.** `ClientHooks::import_client()` consumes the graph client, releasing it on failure. Received network parameter and result tables collect once, then transfer each bound client directly. The previous retain-then-release pair is removed. Endpoint construction inspects the route once instead of three times. Borrowed clients still have their ordinary explicit export operation.
2. **Complete direct wire results in one publication.** Prepared calls transfer their payload and successful status to the same completion state. Pipeline completion precedes response readiness; early pipelines, cancellation, response context and custom result hooks retain their semantics. Result ownership conversion lives with completion, replacing the separate shared-results adapter and subsequent completion call on this path.
3. **Validate and retain cleanup facts together.** Return lifecycle and descriptor validation share the frame reader. The descriptor walk produces a private empty/single-export certificate, including the exact multiplicity of repeated grants. The answer receives this metadata only when the corresponding frame, grants and output commit. Reflected descriptors contribute no sender-owned grant. Finish can release that export without acquiring a temporary frame lease or decoding the Return again.
4. **Use a direct deferred release for a single export.** `Exports::release_deferred()` checks and updates one export, and queues retired ownership for explicit draining. Finish no longer clears and scans a full scratch table for this case. Underflow preserves both ledgers, local call leases keep their owners alive, and no owner destructor runs during the combined transition.

Empty/single-export metadata lives inline and adds no heap allocation. Multiple distinct result exports retain the existing failure-atomic descriptor transaction over the immutable frame. This keeps the fixed-storage API allocation-free without adding a second variable-capacity container to every answer. The full result frame remains available for retained pipelines.

The reference is the existing legacy `result_exports` handling and C++ `Answer::resultExports` / `handleFinish()`: retain cleanup facts at publication and apply them directly at Finish. Portable still validates publication and respects its scoped ownership, backpressure, cancellation and pipeline ordering rules. The server worker already advances ready stages in one poll; merging those state types would not remove a scheduling hop.

Code: [hosted client ownership](../../../../crates/capntproto-rpc/src/scoped.rs), [response completion](../../../../crates/capntproto-rpc/src/scoped/response.rs), [Return publication and Finish](../../../../crates/capntproto-rpc-wire/src/owned/answer.rs), [descriptor validation](../../../../crates/capntproto-rpc-wire/src/publication.rs), [deferred export release](../../../../crates/capntproto-rpc-engine/src/exports.rs), [legacy cleanup](../../../../crates/capntproto-rpc/src/rpc/dispatch.rs), [C++ cleanup](../../../../vendor/capnproto/c++/src/capnp/rpc.c++).

## Measurement

Base commit: `7993d5214ca133b8a4c524d7ec30030d41fe4622`. The original portable and legacy controls use the same frozen executable; C++ is also frozen. Every stage has its own binary digest, source hashes, patch and changed-source snapshot. Fixtures are unchanged. Rust 1.97, Clang 19, fat LTO, one codegen unit, release debug level 1 and overflow checks match the controls.

Native comparisons have five process repetitions with pinned physical CPUs and rotating variant order. Stage comparisons use 1,000 warmups and 5,000 measured whole operations in four focused cases; full qualification uses 100 warmups and 1,000 measurements in all 24 cases per transport. Tables show microseconds ± sample SD of the five repetition means. Concurrent batches are whole fixture operations. TLS 1.3 and QUIC encryption are enabled; handshakes are excluded from timing. The pinned C++ fixture has no QUIC implementation.

Each stage comparison uses its immediate predecessor as the portable control. The full comparison returns to the original portable control. Ratios from separate campaigns should not be multiplied to claim a cumulative speedup. Run-to-run latency spread is material, so small changes in means are descriptive rather than proof of a general win.

Both-peer Callgrind and DHAT diagnostics use 10 warmups at 50 and 500 measured operations. Marginal estimates subtract the two complete lifecycles and divide by 450. Original portable and legacy profiles are reused from the sealed legacy-gap campaign with the identical executable and fixture settings. Callgrind counters are simulated, not hardware PMU or native latency; DHAT counts cumulative allocations, not retained or peak memory. One profile per point does not establish statistical confidence. Socket traces use five repetitions, two warmups and 10/100 operations, and include handshake, shutdown, ciphertext and QUIC control traffic; positive syscall counts are not RPC frame counts.

Builds, tests, native timing, tracing, profiling and compression are serialized. One initial release link was killed under memory pressure; archived raw caches were evicted only after checking their existing archive digests, and the rebuild succeeded. A brief workspace-control disconnect did not discard completed trial data. Raw evidence records these events and all intermediate measurement results.

## Stage comparisons

Each row is a separate five-run campaign against the preceding stage; negative means lower latency.

| Stage | Control | TLS change | QUIC change |
| --- | --- | ---: | ---: |
| Ownership and completion fusion | Original portable | +1.37% | -4.92% |
| Retained metadata and shared validation | Fusion | -3.19% | +5.48% |
| Direct single-export cleanup | Metadata | +4.51% | -0.91% |

The stage timing changes are mixed and are not reliable isolated speedup estimates. Every trial and spread value remains in [comparisons.json](comparisons.json).

## Full repeated comparison

### TLS

| Case | Original portable | Final portable | Legacy | C++ | Change vs original |
| --- | ---: | ---: | ---: | ---: | ---: |
| loopback-0 | 41.69 ± 10.80 | 36.27 ± 5.30 | 38.99 ± 4.62 | 44.39 ± 7.65 | -12.99% |
| loopback-64 | 37.96 ± 5.34 | 35.27 ± 6.17 | 31.56 ± 6.09 | 51.19 ± 7.13 | -7.10% |
| loopback-1024 | 39.57 ± 6.15 | 36.24 ± 6.41 | 34.01 ± 5.23 | 47.49 ± 9.81 | -8.42% |
| loopback-65536 | 115.94 ± 14.79 | 130.38 ± 23.44 | 133.05 ± 21.75 | 159.27 ± 8.95 | +12.45% |
| unary-empty | 30.82 ± 5.74 | 33.36 ± 5.39 | 31.73 ± 4.72 | 46.63 ± 9.17 | +8.24% |
| unary-small | 33.58 ± 5.73 | 36.29 ± 6.65 | 30.29 ± 4.92 | 46.03 ± 9.53 | +8.10% |
| unary-1k | 26.24 ± 0.52 | 29.87 ± 7.03 | 28.83 ± 4.81 | 45.12 ± 14.49 | +13.83% |
| fanout-32 | 188.32 ± 14.99 | 179.42 ± 32.53 | 155.61 ± 20.67 | 330.64 ± 31.33 | -4.73% |
| dependent-read | 102.74 ± 27.10 | 86.73 ± 13.51 | 74.24 ± 12.78 | 105.64 ± 25.37 | -15.58% |
| pipelined-read | 69.10 ± 7.03 | 74.15 ± 11.89 | 52.12 ± 12.25 | 81.49 ± 15.27 | +7.31% |
| write-read | 62.93 ± 9.11 | 64.80 ± 11.68 | 69.96 ± 9.50 | 86.55 ± 17.88 | +2.98% |
| callback | 88.72 ± 17.59 | 81.15 ± 4.68 | 76.84 ± 7.78 | 103.82 ± 22.10 | -8.53% |
| bulk-64k | 113.49 ± 15.36 | 105.05 ± 2.36 | 119.33 ± 20.72 | 157.39 ± 9.71 | -7.44% |
| bulk-256k | 276.61 ± 22.10 | 276.17 ± 74.27 | 249.15 ± 17.00 | 558.76 ± 30.28 | -0.16% |
| bulk-1m | 1019.97 ± 105.82 | 1223.33 ± 250.71 | 1107.16 ± 198.71 | 2205.07 ± 333.63 | +19.94% |
| bulk-4m | 4602.47 ± 280.41 | 4579.78 ± 211.67 | 4683.57 ± 485.66 | 9370.64 ± 603.50 | -0.49% |
| deep-8 | 39.00 ± 6.83 | 33.39 ± 7.06 | 34.97 ± 5.40 | 45.63 ± 6.00 | -14.39% |
| deep-32 | 37.82 ± 7.54 | 40.54 ± 8.75 | 41.42 ± 7.50 | 46.96 ± 4.35 | +7.21% |
| concurrent-32 | 145.48 ± 19.61 | 135.12 ± 20.13 | 107.83 ± 11.02 | 329.87 ± 21.97 | -7.12% |
| deep-32-concurrent-32 | 388.10 ± 49.35 | 363.71 ± 31.89 | 336.21 ± 36.96 | 521.25 ± 48.39 | -6.28% |
| services-4-deep-16 | 233.53 ± 18.98 | 204.06 ± 16.53 | 177.08 ± 9.37 | 277.02 ± 26.09 | -12.62% |
| services-4-deep-16-concurrent-32 | 1551.37 ± 48.71 | 1586.43 ± 103.71 | 1394.38 ± 111.68 | 2023.54 ± 112.14 | +2.26% |
| services-4-deep-16-clients-8 | 1199.58 ± 76.27 | 1196.77 ± 27.97 | 1408.83 ± 107.15 | 1723.59 ± 60.31 | -0.23% |
| services-8-deep-16-clients-8 | 2457.49 ± 94.87 | 2535.87 ± 191.10 | 2997.09 ± 201.37 | 3672.81 ± 75.78 | +3.19% |

### QUIC

| Case | Original portable | Final portable | Legacy | Change vs original |
| --- | ---: | ---: | ---: | ---: |
| loopback-0 | 47.35 ± 15.80 | 51.53 ± 11.99 | 42.51 ± 4.55 | +8.84% |
| loopback-64 | 40.85 ± 9.27 | 49.83 ± 8.92 | 43.66 ± 9.85 | +21.99% |
| loopback-1024 | 43.36 ± 9.41 | 51.90 ± 14.31 | 40.30 ± 10.65 | +19.68% |
| loopback-65536 | 138.36 ± 21.12 | 153.87 ± 15.76 | 126.32 ± 9.99 | +11.21% |
| unary-empty | 36.90 ± 1.99 | 46.46 ± 14.98 | 34.99 ± 5.78 | +25.89% |
| unary-small | 34.72 ± 2.75 | 41.88 ± 9.90 | 36.94 ± 9.72 | +20.61% |
| unary-1k | 43.93 ± 5.29 | 47.23 ± 14.67 | 35.80 ± 1.24 | +7.51% |
| fanout-32 | 200.38 ± 16.26 | 192.28 ± 10.02 | 188.53 ± 28.02 | -4.04% |
| dependent-read | 127.32 ± 6.98 | 126.76 ± 20.71 | 91.72 ± 12.25 | -0.44% |
| pipelined-read | 85.81 ± 8.86 | 85.35 ± 11.55 | 60.96 ± 13.15 | -0.54% |
| write-read | 73.80 ± 8.73 | 85.94 ± 12.58 | 69.02 ± 8.61 | +16.45% |
| callback | 97.52 ± 4.61 | 113.03 ± 18.63 | 98.12 ± 22.60 | +15.90% |
| bulk-64k | 123.71 ± 11.15 | 127.82 ± 15.75 | 123.46 ± 12.62 | +3.33% |
| bulk-256k | 456.47 ± 123.13 | 403.08 ± 94.50 | 433.87 ± 123.39 | -11.70% |
| bulk-1m | 1669.40 ± 241.12 | 1475.42 ± 271.28 | 1531.71 ± 179.72 | -11.62% |
| bulk-4m | 6354.96 ± 223.41 | 6515.31 ± 475.74 | 6111.07 ± 265.02 | +2.52% |
| deep-8 | 45.11 ± 7.59 | 48.35 ± 12.10 | 45.65 ± 10.18 | +7.18% |
| deep-32 | 66.56 ± 10.81 | 57.71 ± 13.43 | 49.33 ± 12.99 | -13.30% |
| concurrent-32 | 164.14 ± 12.44 | 168.00 ± 32.90 | 138.81 ± 6.02 | +2.35% |
| deep-32-concurrent-32 | 417.98 ± 15.89 | 433.73 ± 17.09 | 369.30 ± 30.94 | +3.77% |
| services-4-deep-16 | 316.85 ± 27.54 | 317.15 ± 34.90 | 329.83 ± 19.97 | +0.09% |
| services-4-deep-16-concurrent-32 | 1666.76 ± 32.25 | 1662.21 ± 57.70 | 1661.66 ± 55.74 | -0.27% |
| services-4-deep-16-clients-8 | 1708.24 ± 63.01 | 1693.96 ± 174.88 | 1748.98 ± 95.54 | -0.84% |
| services-8-deep-16-clients-8 | 3449.03 ± 177.31 | 3336.03 ± 242.48 | 3723.18 ± 339.60 | -3.28% |

## CPU and allocation evidence

Estimated marginal instructions per whole operation, summed across both peers:

| Transport / case | Legacy | Original | Fusion | Metadata | Direct cleanup |
| --- | ---: | ---: | ---: | ---: | ---: |
| TLS / dependent-read | 123,370 | 154,428 | 154,124 | 152,406 | 152,397 |
| TLS / pipelined-read | 108,382 | 150,301 | 150,043 | 148,321 | 148,322 |
| QUIC / pipelined-read | 150,199 | 186,396 | 186,028 | 184,379 | 184,600 |

Compared with original portable, final TLS dependent-read instructions change −1.32%, conditional misses +8.92%, and D1 read misses −1.74%. TLS pipelined-read changes −1.32%, +4.88%, and −4.55%; QUIC pipelined-read changes −0.96%, +2.02%, and +1.20%. These are Callgrind simulations, not hardware counters.

| Transport / case | Original blocks / bytes | Final blocks / bytes |
| --- | ---: | ---: |
| TLS / dependent-read | 66.00 / 10740 | 65.00 / 10652 |
| TLS / pipelined-read | 79.00 / 11748 | 78.00 / 11660 |
| QUIC / pipelined-read | 95.01 / 12635 | 94.04 / 12550 |

TLS pipelined-read cached reader-arena allocations fall from 7 to 6 on the server; the client remains at 3. The saved Finish reader accounts for one allocation and approximately 88 allocated bytes per operation. Portable still performs substantially more instructions than legacy despite allocating fewer blocks.

## Socket traces

Both-peer positive socket calls at 100 measured operations plus two warmups; full lifecycle averages over five repetitions. Each cell is **writes / reads**.

| Transport / case | Original portable | Final portable | Legacy | C++ |
| --- | ---: | ---: | ---: | ---: |
| TLS / fanout-32 | 210.0 / 1433.0 | 210.0 / 1433.0 | 210.0 / 1433.0 | 1028.0 / 1027.0 |
| TLS / dependent-read | 414.0 / 413.0 | 414.0 / 413.0 | 414.0 / 413.0 | 416.0 / 415.0 |
| TLS / pipelined-read | 210.0 / 209.0 | 210.0 / 209.0 | 210.0 / 209.0 | 212.0 / 211.0 |
| QUIC / fanout-32 | 215.0 / 627.0 | 215.0 / 627.0 | 212.0 / 624.0 | Unavailable |
| QUIC / dependent-read | 419.0 / 423.0 | 419.0 / 423.0 | 416.4 / 420.4 | Unavailable |
| QUIC / pipelined-read | 215.0 / 219.0 | 215.0 / 219.0 | 212.2 / 216.2 | Unavailable |

The sampled TLS counts match legacy. QUIC counts match the original portable control and remain slightly above legacy; the earlier whole-lifecycle gap is not eliminated. Fanout reads exceed C++ even though writes are fewer. Neither universal syscall parity nor RPC-frame parity is established. Complete 10/100-operation counts, spread and both peers are retained in [traces.json](traces.json).

## Validation and remaining architecture

- 737 core tests: 358 RPC, 219 engine and 160 wire.
- 86 fixed-storage and 89 growing-storage portable verification tests.
- 36 integration tests across encrypted RPC, native pipeline migration, scoped Unix and scoped vat fixtures.
- 206 encrypted compatibility/authentication checks, including mixed engines and C++ TLS.
- Embedded `thumbv7em-none-eabi` check, strict Clippy across six crates/all targets, and formatting.

The ownership API and private publication certificate are retained as architectural changes, not as evidence of a universal performance improvement. QUIC regresses in this full campaign, and native timing remains noisy. Multiple-export cleanup still decodes the retained frame; the large route, answer/pipeline ownership and scheduling differences from legacy remain. Final TLS pipeline instructions are still about 37% above legacy. Collapsing source-level wrappers alone does not remove most of that work.

The archive contains intermediate binaries, source snapshots, every sample, trace, profile, validation log, reproduction scripts and hashes. The original baseline/legacy profile archive is identified in `reference-evidence/source.json`. [Validation](validation.json), [evidence index](evidence-index.json), and [archive digest](archive.json) describe the complete scope.
