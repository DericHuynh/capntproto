# Portable QUIC stream optimization — 2026-10-22

The standard QUIC v1 adapter now reads and writes Quiche stream buffers directly, removing the Tokio duplex relay and its two preallocated 128 KiB scratch buffers. The packet task retains UDP transmission, recovery, pacing, congestion control and flow control. When an RPC reader becomes ready, it gets one executor turn before packet flushing so responses can share packets with ACK and credit updates. Connection borrows never cross an await; blocked writers only wake when they can make progress. Authenticated receive bytes remain drainable after the packet task terminates.

TLS 1.3, certificate and hostname verification, RPC ALPN, full handshakes, and the exclusion of resumption/0-RTT are unchanged. This is a shared transport optimization used by both RPC engines. The controls below are frozen legacy and portable executables from commit 55669b7; they are not a measurement of legacy rebuilt with the new adapter. The pinned C++/KJ reference has no QUIC adapter, so no cross-transport C++ speedup is claimed.

Across all 24 cases, portable has 13.6% lower geometric-mean latency and lower means in 21/24 cases versus frozen portable. It beats frozen legacy on mean in 14/24. Each case has equal weight in the geometric mean of latency ratios; this is not an aggregate throughput measure. These are observed means, not confidence intervals or proof of a universal improvement.

## Five-run comparison

All cells use five independent process repetitions with 1,000 retained samples each: 360,000 qualified samples. Loopbacks use 10,000 warmups; workflows and topology batches use 100. Every sample retains application construction, validation and cleanup. Handshake/bootstrap/setup are excluded. CPU placement and host counters are recorded. Values are microseconds per operation or entire concurrent batch, mean ± sample SD of the five run means. Run ranges and pooled percentiles are in report.json.

| Case | Frozen legacy | Frozen portable | New portable | Latency reduction |
| --- | ---: | ---: | ---: | ---: |
| loopback-0 | 44.53 ± 3.63 | 48.97 ± 4.62 | 42.08 ± 11.08 | +14.1% |
| loopback-64 | 44.70 ± 4.05 | 48.57 ± 4.19 | 41.45 ± 4.73 | +14.7% |
| loopback-1024 | 51.41 ± 2.40 | 59.52 ± 16.41 | 49.94 ± 14.09 | +16.1% |
| loopback-65536 | 192.25 ± 41.89 | 167.36 ± 31.50 | 147.67 ± 26.73 | +11.8% |
| unary-empty | 40.55 ± 2.77 | 54.12 ± 15.72 | 36.77 ± 1.10 | +32.1% |
| unary-small | 47.19 ± 11.46 | 49.97 ± 2.23 | 43.56 ± 7.14 | +12.8% |
| unary-1k | 59.60 ± 13.03 | 62.03 ± 15.00 | 49.02 ± 9.89 | +21.0% |
| fanout-32 | 195.64 ± 22.41 | 220.26 ± 17.27 | 204.19 ± 21.97 | +7.3% |
| dependent-read | 119.55 ± 15.66 | 160.51 ± 18.52 | 128.96 ± 31.90 | +19.7% |
| pipelined-read | 70.66 ± 20.70 | 95.32 ± 8.93 | 87.63 ± 9.21 | +8.1% |
| write-read | 98.23 ± 6.72 | 112.42 ± 12.24 | 89.47 ± 13.22 | +20.4% |
| callback | 113.51 ± 22.39 | 137.12 ± 25.52 | 113.92 ± 16.62 | +16.9% |
| bulk-64k | 194.08 ± 24.18 | 195.08 ± 31.38 | 172.83 ± 20.49 | +11.4% |
| bulk-256k | 474.74 ± 86.70 | 410.66 ± 112.78 | 396.73 ± 95.61 | +3.4% |
| bulk-1m | 1382.69 ± 79.18 | 1550.22 ± 312.16 | 1614.86 ± 211.06 | -4.2% |
| bulk-4m | 6111.34 ± 259.09 | 6717.29 ± 398.25 | 6097.08 ± 288.86 | +9.2% |
| deep-8 | 62.35 ± 14.27 | 60.77 ± 6.35 | 48.46 ± 5.86 | +20.3% |
| deep-32 | 58.39 ± 6.36 | 76.08 ± 25.67 | 47.56 ± 7.42 | +37.5% |
| concurrent-32 | 133.78 ± 15.81 | 166.90 ± 18.32 | 157.76 ± 19.61 | +5.5% |
| deep-32-concurrent-32 | 352.41 ± 29.18 | 419.63 ± 58.10 | 408.67 ± 26.48 | +2.6% |
| services-4-deep-16 | 454.45 ± 74.26 | 464.01 ± 36.65 | 348.07 ± 31.33 | +25.0% |
| services-4-deep-16-concurrent-32 | 1646.87 ± 99.76 | 1908.65 ± 86.43 | 1774.45 ± 55.17 | +7.0% |
| services-4-deep-16-clients-8 | 1394.94 ± 19.26 | 1488.36 ± 82.23 | 1500.70 ± 19.84 | -0.8% |
| services-8-deep-16-clients-8 | 2974.43 ± 142.19 | 3208.01 ± 117.26 | 3229.72 ± 80.04 | -0.7% |

The 1 MiB primary result is 4.2% slower, while a separate isolated five-run confirmation is 1.6% faster (1472.68 ± 163.27 versus 1496.72 ± 194.41 µs). The primary result remains in the table. The four- and eight-service, eight-client means are 0.8% and 0.7% higher respectively. These cases do not establish an improvement.

An accidentally overlapping confirmation started during the final primary service-chain case. Both original overlapping runs are retained and marked unqualified. The entire five-run eight-service case was repeated in isolation and replaces only that case in the qualified table; report.json records its source directory. The new runner takes an exclusive campaign lock and rejects active benchmark peers, builds and profilers before native runs. No build, test or profiler overlapped the qualified timing runs.

## Packet and CPU evidence

Separate syscall traces cover all 24 cases on all peers. Portable has no greater positive or attempted reads and writes in 22/24 comparisons with frozen portable, and 23/24 with frozen legacy. Full-process ciphertext counts include handshake, shutdown, ACK/credit/recovery traffic and UDP batching; they are not application message counts or timed-region counts.

| Case | Old portable writes | New portable writes | Old portable reads | New portable reads |
| --- | ---: | ---: | ---: | ---: |
| unary-small | 60 | 36 | 63 | 39 |
| pipelined-read | 60 | 36 | 65 | 41 |
| callback | 108 | 60 | 111 | 63 |
| bulk-1m | 894 | 874 | 2153 | 2024 |
| services-8-deep-16-clients-8 | 1341 | 1387 | 1550 | 1582 |

Callgrind used ten warmups and 100 measured operations per diagnostic process, counting the entire client/server lifecycle. These are simulated counters, not hardware PMU measurements or native latency.

| Portable case | Instruction reduction | Conditional branch-miss reduction |
| --- | ---: | ---: |
| unary-small | 13.0% | 22.0% |
| pipelined-read | 8.3% | 12.1% |
| callback | 15.7% | 19.3% |

The intermediate direct-buffer prototype reduced 4 MiB mean latency by 11.8% in its focused comparison but did not improve callbacks or the concurrent service chain. Adding the reader turn before flushing improved all four focused cases versus that prototype. Both experiments, executables, profiles and complete raw trials are retained.

## Verification and remaining work

Passed: 226 encrypted end-to-end checks, 19 TLS/QUIC security and lifecycle tests, four QUIC adapter unit tests, 199 non-model library tests (two ignored), 22 quality tests, seven Python tests, strict Clippy for the benchmark/QUIC/quality targets, and formatting/diff checks. The new 3 MiB regression exercises a stalled reader, bounded flow control, runtime fairness, FIN/drop delivery and writes after shutdown. The benchmark tests verify frozen-control identity and exclusive campaign locking. The quality security fixture helper now precedes the test module, fixing its strict Clippy ordering error.

An initial broad library invocation failed 28 TLC tests because TLA2TOOLS_JAR was unset. The explicit non-model partition passed; formal models and coverage were not requalified in this change. The 48 mixed frozen-legacy/new-portable checks also passed.

The endpoint packet queue and per-connection packet tasks remain. The eight-service/eight-client trace still has more calls, and portable still trails frozen legacy on several pipelined/concurrent workloads. Profiling cross-connection readiness and packet batching is the next architectural target; these results do not prove its cause or justify changing fairness limits. Gap #11 remains open and legacy stays available.

## Reproduction and evidence

```sh
python3 benchmarks/rpc/secure-runner.py --rust FROZEN_DRIVER \
  --portable-baseline FROZEN_DRIVER --portable NEW_DRIVER --cpp CPP \
  --transport quic --mixed --repetitions 5 --iterations 1000 --output NEW_DIRECTORY
```

Source base: `55669b793bcc82448a50d4232731f61ac642532f`. Final source-only tree: `fa4541f6b9ab36faff8aca0c8524b5110c53e7f6`. Built source tree: `746d6c9f214ddcc22da62e8711bcaf227468cee3`. Only Python campaign/guard tests and benchmark documentation changed after the final Rust executable was built; hashes and archived harness versions verify every run. Source patch replay passed.

Evidence: `/workspace/capntproto/target/portable-quic-direct-20261022.tar.xz` (7016 verified files), SHA-256 `bdef9917ca7fc38f3b56e173ed064b329aa7215164739c4f44a3c67e2ba055a8`. Frozen controls remain in the unchanged encrypted baseline archive, SHA-256 `40da619a790fbfe08e22f55c310e2755f6adb14ac19e3af3f5a38261ad87d9ce`. Test fixture private keys remain local evidence and are not committed. Measurements cover established loopback connections on this host, not remote networks, packet loss or handshake performance.
