# Pipeline and dependent-read cleanup — 2026-10-22

Portable RPC maintenance now keeps its scan cursor in local variables while skipping empty phases, then commits it once before polling the next owner. This removes the separate helper call and repeated driver-state stores for each empty phase. Phase order, ownership, resource limits, cooperative budgets, protocol behavior and the public API are unchanged. The accepted production change is confined to `crates/capntproto-rpc-wire/src/owned/connection/maintenance.rs`.

The target results are mixed: TLS pipelined-read mean latency is 10.0% lower and QUIC dependent-read is 6.7% lower; TLS dependent-read is 4.6% higher and QUIC pipelined-read is 1.0% higher. Across these four cells the geometric mean of latency ratios is 2.94% lower. Run-to-run spread is substantial; this does not establish a reliable overall speedup. Legacy remains faster in all four target cells.

## Target comparison

Five independent process repetitions, each with 1,000 warmups and 5,000 retained measured operations: 350,000 primary samples. Values are microseconds per complete two-call operation, mean ± sample SD of the five run means. Run ranges, pooled percentiles and every sample are retained. Setup/handshakes are excluded; validation and application cleanup are included. Frozen legacy and portable are from d913417; all peers of the candidate portable run the new binary. C++ uses the same TLS fixtures at pinned revision 0de72d8d8cec6b69edaa29de51d3bd490341f9c2. No build, test, profiler, compression or second native campaign overlapped these measurements.

| Transport | Case | Legacy | Portable before | Portable after | C++ | Latency reduction |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| tls | dependent-read | 66.85 ± 11.05 | 79.67 ± 9.86 | 83.37 ± 9.75 | 91.63 ± 12.25 | -4.65% |
| tls | pipelined-read | 44.22 ± 6.66 | 69.97 ± 6.26 | 62.97 ± 4.94 | 70.71 ± 7.10 | +10.01% |
| quic | dependent-read | 93.27 ± 8.31 | 118.64 ± 9.04 | 110.70 ± 7.88 | unsupported | +6.69% |
| quic | pipelined-read | 50.34 ± 2.17 | 83.74 ± 4.09 | 84.56 ± 3.14 | unsupported | -0.98% |

## Adjacent workloads

These separate regression measurements use five repetitions, 100 warmups and 1,000 measured operations per repetition, retaining another 210,000 samples. Concurrent values measure the complete batch. These means also include regressions; the change is not qualification for removing legacy.

| Transport | Case | Legacy | Portable before | Portable after | C++ | Latency reduction |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| tls | unary-small | 34.83 ± 7.14 | 32.77 ± 7.06 | 32.51 ± 6.42 | 47.26 ± 8.67 | +0.80% |
| tls | fanout-32 | 142.67 ± 9.11 | 180.99 ± 21.15 | 191.00 ± 5.72 | 349.19 ± 38.98 | -5.53% |
| tls | write-read | 57.80 ± 2.47 | 70.26 ± 15.70 | 57.58 ± 4.29 | 85.64 ± 10.07 | +18.05% |
| tls | callback | 62.94 ± 11.73 | 78.00 ± 17.10 | 82.16 ± 20.37 | 83.40 ± 6.64 | -5.33% |
| tls | deep-32-concurrent-32 | 341.72 ± 46.21 | 364.13 ± 39.56 | 386.24 ± 39.44 | 501.34 ± 37.89 | -6.07% |
| tls | services-4-deep-16-clients-8 | 1384.37 ± 76.79 | 1091.43 ± 50.03 | 1135.63 ± 67.14 | 1761.68 ± 97.69 | -4.05% |
| quic | unary-small | 51.32 ± 8.06 | 51.08 ± 10.09 | 41.35 ± 8.27 | unsupported | +19.06% |
| quic | fanout-32 | 196.24 ± 13.28 | 222.69 ± 19.75 | 219.63 ± 22.38 | unsupported | +1.37% |
| quic | write-read | 78.36 ± 10.28 | 92.74 ± 17.49 | 92.17 ± 14.52 | unsupported | +0.61% |
| quic | callback | 121.15 ± 25.06 | 136.13 ± 19.88 | 148.02 ± 27.38 | unsupported | -8.73% |
| quic | deep-32-concurrent-32 | 397.44 ± 25.41 | 429.98 ± 36.83 | 436.57 ± 22.62 | unsupported | -1.53% |
| quic | services-4-deep-16-clients-8 | 1732.05 ± 111.16 | 1641.36 ± 38.50 | 1653.49 ± 46.24 | unsupported | -0.74% |

## Traces and profiles

Whole-peer ciphertext traces cover two warmups and ten measured operations, including handshakes and shutdown. Successful socket writes/reads below are syscalls, not Cap’n Proto message counts; QUIC also includes ACKs, flow-control traffic and batched datagrams. Positive counts and attempted calls are unchanged versus the simultaneously traced frozen portable. TLS counts are below C++ in these equivalent fixtures. QUIC retains three more successful lifecycle writes and reads than legacy; the pinned C++ revision has no QUIC adapter.

| Transport | Case | Legacy writes/reads | Portable before writes/reads | Portable after writes/reads | C++ writes/reads |
| --- | --- | ---: | ---: | ---: | ---: |
| tls | dependent-read | 54/53 | 54/53 | 54/53 | 56/55 |
| tls | pipelined-read | 30/29 | 30/29 | 30/29 | 32/31 |
| quic | dependent-read | 56/60 | 59/63 | 59/63 | unsupported |
| quic | pipelined-read | 32/36 | 35/39 | 35/39 | unsupported |

Callgrind profiles instrument both peers for ten warmups and 1,000 measured operations, counting the entire lifecycle. These are simulated instructions and conditional branch misses, not hardware PMU counters or native timings.

| Transport | Case | Instruction reduction | Conditional branch-miss reduction |
| --- | --- | ---: | ---: |
| tls | dependent-read | +0.08% | +2.16% |
| tls | pipelined-read | -0.40% | +3.62% |
| quic | dependent-read | +0.08% | -1.03% |
| quic | pipelined-read | -0.28% | -0.03% |

TLS shows fewer simulated branch misses; the QUIC profiles show no corresponding branch reduction. Instruction counts are essentially unchanged, with a small increase for pipelining. The remaining performance gap is broader than this cleanup walk. Portable still uses a general bounded scheduler for capability lifetime and promise propagation. C++ delivers Returns through their question owners in `vendor/capnproto/c++/src/capnp/rpc.c++:4022–4101` and batches writes with `evalLast` in `rpc-twoparty.c++:175–202`. Equivalent TLS socket counts already match legacy, so reducing socket calls alone cannot explain or remove the CPU gap.

## Rejected experiment and validation

An earlier experiment delivered used pipelines through their indexed call owner, eagerly settled a bounded set of matching observers, and applied the normal cleanup preference to direct capability returns. It preserved authorization, reflected-route barriers, storage-pressure fallback and late projections in tests. However, its eight-case, five-run comparison was mixed. Longer TLS profiles increased pipelined instructions by 0.74% and conditional branch misses by 1.49%; dependent-read branch misses rose 7.13%. All five files from that experiment were restored to d913417 before the accepted cleanup change. Its separate raw campaigns, patches, failed development checks and exact executable remain in the evidence; those timings are not merged into the accepted results.

Passed on the accepted source: 155 wire tests, 336 RPC tests, strict Clippy for both crates and their test targets, a no-default-features wire check, formatting, diff checks and source patch replay. The candidate passed 226 encrypted end-to-end/security checks and 64 mixed frozen-legacy/new-portable/C++ checks across the target and regression campaigns. Existing tests exercise one-step budgets, retained vacant capacity, pipeline cleanup, FIFO backpressure, cancellation and capability lifetime. Formal models and coverage were not requalified. TLS 1.3 verification, QUIC v1, ALPN, pacing and flow control remain enabled; no 0-RTT/resumption is introduced.

## Reproduction and evidence

```sh
python3 benchmarks/rpc/secure-runner.py --rust FROZEN_DRIVER \
  --portable-baseline FROZEN_DRIVER --portable NEW_DRIVER --cpp CPP \
  --transport both --cases dependent-read,pipelined-read --mixed \
  --repetitions 5 --warmup 1000 --iterations 5000 --output NEW_DIRECTORY
```

Source base: `d913417ec67fb51dfc2d7aa2dc98ab70636b7686`. Source-only tree: `b1a37766ab0445b2aa0dfad6a68cfc8aeecd331e`. Candidate SHA-256: `6ec8ab2a87223d0308af75eed275691fafbf080d82a67ed82629672429cb9590`. No production source changed after this executable was built.

Verified evidence: `/workspace/capntproto/target/portable-pipeline-cleanup-20261022.tar.xz` (7092 files), SHA-256 `769c30b122f2eb13748b05aa60466648ce89be34c137ff0d60932c7e4e9d5ba0`. Both experiment executables are stored as verified lossless VCDIFF deltas against the frozen d913417 executable. Dependency archive SHA-256: `ac3b96983495093a113d4548a2e9c9b5431dfbd6241d091f0387686c3f44c9ca`. Exact decoder, license, reconstruction instructions, source hashes and reconstructed binary hashes are included. Original executables remain sealed alongside the archive in the execution environment. Test fixture private keys remain local and are not committed.

Gap #11 remains open. C++ QUIC parity and universal latency improvements have not been demonstrated.
