# Direct QUIC packet delivery — 2026-10-22

Active QUIC routes now point weakly to the shared session state. The UDP router feeds its receive buffer directly to Quiche and wakes parked RPC readers immediately. This removes the per-datagram allocation/copy and packet-channel handoff. Before acceptance, the existing bounded pending queue remains; activation drains it without awaiting, then switches every connection ID to the active destination. The packet task retains the route until recovery/shutdown finishes, and a per-session failure stays isolated from other connections.

The router yields after 16 received packets to bound decryption work. A socket Pending resets the counter, since it already yielded; the next receive burst receives a fresh budget. A readiness flag preserves a deferred executor turn before flushing ACK/credit, without repeatedly searching for readable streams in the packet pump. C++/KJ likewise batches outgoing messages with `evalLast` in `vendor/capnproto/c++/src/capnp/rpc-twoparty.c++:175–202`. This is a scheduling reference, not equivalent QUIC performance evidence: the pinned C++ revision has no QUIC adapter.

TLS 1.3, CA/hostname/IP verification, RPC ALPN, full handshakes, pacing, congestion control, flow control, and the exclusion of resumption/0-RTT remain enabled. This shared transport change affects both RPC engines; the legacy and portable controls below are frozen at commit 32c3307, not legacy rebuilt with the candidate.

The primary 24-case comparison has **1.0% lower geometric-mean latency**, lower portable means in **14/24**, and lower means than frozen legacy in **5/24**. Each case has equal weight in the geometric mean of latency ratios. The overall difference is small relative to the run-to-run spread. These observed means do not establish a reliable overall speedup, and several cases regress.

## Five-run comparison

Every cell uses five independent process repetitions and 1,000 retained samples: 360,000 primary samples. Loopbacks use 10,000 warmups; workflows and topology cases use 100. CPU placement and host counters are retained. Values are microseconds per operation or entire concurrent batch, mean ± sample SD of the five run means. Setup/handshake are excluded; application validation and cleanup are included. Run ranges and pooled percentiles are in report.json. No build, test, profiler, or other campaign overlapped native timing.

| Case | Frozen legacy | Frozen portable | New portable | Latency reduction |
| --- | ---: | ---: | ---: | ---: |
| loopback-0 | 41.82 ± 9.73 | 46.07 ± 14.04 | 42.91 ± 11.45 | +6.9% |
| loopback-64 | 41.65 ± 11.52 | 43.85 ± 11.93 | 43.37 ± 4.11 | +1.1% |
| loopback-1024 | 49.65 ± 10.47 | 50.79 ± 16.65 | 53.16 ± 13.70 | -4.7% |
| loopback-65536 | 130.45 ± 15.18 | 162.81 ± 28.48 | 120.41 ± 10.42 | +26.0% |
| unary-empty | 42.97 ± 9.46 | 45.51 ± 12.23 | 38.38 ± 4.64 | +15.7% |
| unary-small | 39.68 ± 10.06 | 44.14 ± 11.25 | 37.38 ± 2.06 | +15.3% |
| unary-1k | 38.59 ± 3.47 | 39.36 ± 1.88 | 40.00 ± 2.97 | -1.6% |
| fanout-32 | 184.50 ± 26.73 | 203.68 ± 36.03 | 217.38 ± 33.14 | -6.7% |
| dependent-read | 92.35 ± 22.72 | 105.94 ± 4.10 | 134.38 ± 23.09 | -26.8% |
| pipelined-read | 58.68 ± 20.14 | 96.63 ± 14.22 | 104.29 ± 16.01 | -7.9% |
| write-read | 80.56 ± 12.63 | 90.94 ± 11.80 | 90.27 ± 11.71 | +0.7% |
| callback | 75.46 ± 5.62 | 99.05 ± 5.78 | 93.71 ± 3.96 | +5.4% |
| bulk-64k | 135.12 ± 25.44 | 136.70 ± 17.74 | 132.42 ± 24.06 | +3.1% |
| bulk-256k | 306.51 ± 19.09 | 344.85 ± 20.36 | 339.56 ± 80.08 | +1.5% |
| bulk-1m | 1599.94 ± 139.12 | 1721.14 ± 90.20 | 1786.53 ± 266.72 | -3.8% |
| bulk-4m | 6325.13 ± 228.61 | 6427.27 ± 316.41 | 6363.41 ± 490.97 | +1.0% |
| deep-8 | 47.90 ± 2.22 | 54.73 ± 12.42 | 56.25 ± 6.78 | -2.8% |
| deep-32 | 50.56 ± 7.02 | 62.04 ± 16.93 | 59.72 ± 14.15 | +3.7% |
| concurrent-32 | 132.16 ± 14.64 | 157.19 ± 8.99 | 151.54 ± 6.72 | +3.6% |
| deep-32-concurrent-32 | 411.66 ± 21.67 | 448.58 ± 8.87 | 441.15 ± 16.75 | +1.7% |
| services-4-deep-16 | 418.20 ± 60.79 | 368.74 ± 46.04 | 401.40 ± 110.97 | -8.9% |
| services-4-deep-16-concurrent-32 | 1773.53 ± 144.10 | 1963.57 ± 107.37 | 1939.91 ± 74.76 | +1.2% |
| services-4-deep-16-clients-8 | 1473.08 ± 41.63 | 1517.87 ± 64.86 | 1621.53 ± 58.04 | -6.8% |
| services-8-deep-16-clients-8 | 3151.39 ± 246.48 | 3368.06 ± 111.63 | 3527.48 ± 167.48 | -4.7% |

The complete table retains every primary regression. Gap #11 stays open. The earlier prototype carried its receive budget across socket waits: its primary 1 MiB mean was 24.7% higher, and its separate confirmation below was 5.2% higher. These are superseded-prototype results, not measurements of the final burst-reset executable; none replace a primary cell above.

| Superseded prototype confirmation | Frozen portable | New portable | Latency reduction |
| --- | ---: | ---: | ---: |
| callback | 136.38 ± 17.97 | 121.16 ± 16.18 | +11.2% |
| bulk-1m | 1731.63 ± 189.96 | 1821.20 ± 277.94 | -5.2% |

## Ciphertext traces and CPU profiles

Separate traces cover every peer in all 24 cases. Positive and attempted reads/writes are all no greater in 20/24 comparisons with frozen portable and 3/24 with frozen legacy. Counts cover the whole process, including handshake, shutdown, ACK/credit/recovery traffic and UDP batching. They are not RPC-message counts or timed-region counts; universal read/write parity is not established.

| Case | Old portable writes | New portable writes | Old portable reads | New portable reads |
| --- | ---: | ---: | ---: | ---: |
| unary-small | 36 | 35 | 39 | 38 |
| pipelined-read | 36 | 35 | 41 | 39 |
| bulk-1m | 823 | 840 | 1988 | 2010 |
| concurrent-32 | 36 | 35 | 44 | 38 |
| services-4-deep-16-clients-8 | 736 | 739 | 850 | 847 |
| services-8-deep-16-clients-8 | 1208 | 1187 | 1424 | 1396 |

Callgrind covers all peers for ten warmups and 100 measured batches, counting the complete lifecycle. These are simulated counters, not hardware PMU measurements or native timing.

| Portable case | Instruction reduction | Conditional branch-miss reduction |
| --- | ---: | ---: |
| concurrent-32 | -0.91% | +1.30% |
| services-4-deep-16-clients-8 | +2.13% | +2.54% |

The frozen eight-service baseline disconnected under instrumentation; that failed run is retained and excluded from CPU comparisons. Eight-service native measurements and syscall traces remain included.

## Experiments and verification

Two vectored-write experiments were rejected and fully reverted. Stack gathering had 2.70% higher geometric-mean latency across 24 cases; owned-buffer gathering had 2.72% higher latency, despite reducing instructions in the concurrent profiles. Their exact frozen binaries are retained as verified lossless VCDIFF deltas against the sealed 32c3307 executable, alongside source patches, full five-run tables and raw trials. An earlier direct-routing prototype has a separate seven-case screen and three-case trace. The deferred-flush prototype also has its own complete 24-case campaign and confirmation before the receive-budget reset. These results are not merged into the final primary campaign.

Passed: 226 encrypted end-to-end checks, 19 TLS/QUIC security and lifecycle tests, 199 non-model library tests (two ignored, 28 TLC tests filtered), strict Clippy for QUIC and benchmark targets, formatting/diff checks, and source patch replay. The primary campaign also passed 48 mixed frozen-legacy/new-portable checks. The stalled-reader regression now proves a second authenticated connection can exchange ping/pong through the same endpoints while a 3 MiB transfer remains flow-blocked. Formal models and coverage were not requalified.

Several portable RPC workflows remain behind the controls. This remains draft architecture work, not qualification for removing legacy or claiming a universal performance improvement. The endpoint still receives individual UDP datagrams, and RPC parsing, capability handling and application scheduling remain separate costs. The existing queue was a small fraction of concurrent instruction cost; removing it helps the scheduling path but does not eliminate the broader RPC gap.

## Reproduction and evidence

```sh
python3 benchmarks/rpc/secure-runner.py --rust FROZEN_DRIVER \
  --portable-baseline FROZEN_DRIVER --portable NEW_DRIVER --cpp CPP \
  --transport quic --mixed --repetitions 5 --iterations 1000 --output NEW_DIRECTORY
```

Source base: `32c3307522e01a3a14e9eb592b8a4852585bf67c`. Built source-only tree: `93579e86106ad6e634de75324fb7e0aa44b9250a`. Candidate SHA-256: `17def75aecb43a7359d0cc2c8d6c0882cb58aed47c0dc9070f6a70d66d106f04`. No production source changed after this executable was built. Source patch replay and archived source hashes passed.

Evidence: `/workspace/capntproto/target/portable-quic-routing-20261022.tar.xz` (20043 verified files), SHA-256 `ac3b96983495093a113d4548a2e9c9b5431dfbd6241d091f0387686c3f44c9ca`. Frozen controls remain in the unchanged prior archive, SHA-256 `bdef9917ca7fc38f3b56e173ed064b329aa7215164739c4f44a3c67e2ba055a8`. The final binary is stored in full; four earlier binaries use verified lossless VCDIFF deltas, with reconstruction hashes/instructions in evidence/binary-deltas.json and the decoder plus license under evidence/tools. Test fixture private keys remain local and are not committed. Results cover established loopback connections on this host; remote links, loss and handshake latency were not benchmarked.
