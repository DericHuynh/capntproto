# Encrypted RPC transport comparison — 2026-10-22

TLS 1.3 is now the default for the benchmark TCP paths, including every microservice hop, gRPC, WebSocket, independent upstream Rust, and the pinned C++/KJ reference. Both legacy and portable also run on the production standard QUIC v1 adapter. Certificate and hostname verification remain enabled; there is no plaintext fallback.

KJ has no QUIC adapter at the pinned revision. C++ QUIC is explicitly unsupported. TLS-to-TLS and QUIC-to-QUIC results remain separate; this report does not compare Rust QUIC against C++ TCP as equivalent transports.

All 24 core cases use five independent process repetitions and 1,000 retained samples per repetition. Loopback uses 10,000 warmups; workflows and topology batches use 100. Every sample includes application construction, validation and cleanup. Full handshakes and bootstrap/topology setup finish before timing. All samples remain included; ± is sample SD of the five run means, not a confidence interval. Times below are microseconds per operation or entire concurrent batch.

There are 830 native trials, 830,000 retained latency samples, and 192 separate mixed-engine checks. Portable beats C++ on mean latency in 23/24 TLS cases and on both mean and pooled median in 23/24. Portable beats legacy on mean in 8/24 TLS cases and 4/24 QUIC cases. These are the first encrypted controls, not a measured speedup from the previous plaintext implementation.

| Case | TLS legacy | TLS portable | TLS C++ | QUIC legacy | QUIC portable |
| --- | ---: | ---: | ---: | ---: | ---: |
| loopback-0 | 29.48 ± 5.39 | 31.72 ± 6.36 | 42.93 ± 8.35 | 49.92 ± 11.16 | 50.42 ± 6.63 |
| loopback-64 | 30.59 ± 4.69 | 31.51 ± 8.49 | 34.82 ± 1.52 | 50.67 ± 9.68 | 50.97 ± 7.29 |
| loopback-1024 | 26.49 ± 1.23 | 32.97 ± 4.58 | 49.65 ± 7.53 | 48.77 ± 1.02 | 52.59 ± 5.01 |
| loopback-65536 | 102.13 ± 2.68 | 108.35 ± 6.13 | 190.59 ± 62.42 | 148.02 ± 18.15 | 164.68 ± 21.51 |
| unary-empty | 35.26 ± 7.73 | 32.19 ± 7.05 | 42.71 ± 4.27 | 54.23 ± 15.50 | 61.02 ± 13.74 |
| unary-small | 27.20 ± 5.16 | 25.66 ± 1.41 | 35.75 ± 1.51 | 57.57 ± 9.60 | 51.53 ± 2.33 |
| unary-1k | 30.48 ± 5.20 | 31.70 ± 6.33 | 41.45 ± 6.71 | 45.08 ± 2.43 | 49.06 ± 3.16 |
| fanout-32 | 158.62 ± 24.02 | 177.39 ± 19.06 | 317.29 ± 10.10 | 198.95 ± 17.69 | 212.64 ± 17.45 |
| dependent-read | 65.46 ± 7.94 | 77.46 ± 2.34 | 87.94 ± 10.08 | 116.52 ± 15.02 | 155.09 ± 20.77 |
| pipelined-read | 49.44 ± 6.76 | 64.07 ± 6.82 | 74.99 ± 6.47 | 64.71 ± 7.68 | 118.83 ± 31.95 |
| write-read | 69.57 ± 10.65 | 62.22 ± 9.78 | 76.21 ± 5.24 | 98.10 ± 15.98 | 99.65 ± 5.96 |
| callback | 66.46 ± 8.25 | 104.61 ± 13.97 | 91.97 ± 8.12 | 115.90 ± 21.37 | 171.28 ± 45.27 |
| bulk-64k | 109.00 ± 4.81 | 117.42 ± 12.98 | 158.30 ± 17.74 | 144.57 ± 15.09 | 142.43 ± 3.85 |
| bulk-256k | 299.73 ± 108.89 | 256.04 ± 45.02 | 601.01 ± 89.04 | 465.92 ± 105.55 | 433.72 ± 81.01 |
| bulk-1m | 953.54 ± 47.85 | 1017.08 ± 76.70 | 2362.26 ± 185.20 | 1518.62 ± 307.01 | 1538.10 ± 234.80 |
| bulk-4m | 4829.93 ± 336.49 | 4628.66 ± 285.34 | 9464.59 ± 750.68 | 6637.94 ± 164.75 | 6950.42 ± 346.44 |
| deep-8 | 33.88 ± 5.39 | 36.35 ± 6.62 | 48.17 ± 8.88 | 49.30 ± 1.27 | 54.67 ± 6.38 |
| deep-32 | 37.42 ± 4.75 | 38.80 ± 6.88 | 54.52 ± 9.22 | 67.58 ± 16.52 | 67.56 ± 14.75 |
| concurrent-32 | 109.56 ± 14.72 | 136.52 ± 17.13 | 336.28 ± 25.41 | 143.91 ± 26.10 | 170.93 ± 25.11 |
| deep-32-concurrent-32 | 294.52 ± 21.38 | 378.18 ± 38.74 | 497.91 ± 21.32 | 368.97 ± 35.02 | 399.55 ± 38.57 |
| services-4-deep-16 | 175.34 ± 24.83 | 197.22 ± 26.35 | 278.64 ± 54.72 | 409.34 ± 70.00 | 492.39 ± 62.37 |
| services-4-deep-16-concurrent-32 | 1395.80 ± 70.66 | 1220.47 ± 110.41 | 2120.90 ± 150.31 | 1593.87 ± 34.47 | 1887.76 ± 57.06 |
| services-4-deep-16-clients-8 | 1434.33 ± 34.76 | 1286.07 ± 43.24 | 1904.67 ± 26.41 | 1430.33 ± 16.92 | 1556.49 ± 46.16 |
| services-8-deep-16-clients-8 | 2884.24 ± 146.70 | 2166.47 ± 175.97 | 3706.82 ± 88.29 | 2968.35 ± 175.61 | 3037.96 ± 65.16 |

## Application protocol controls

| Case | TLS upstream Rust | gRPC/TLS | WSS |
| --- | ---: | ---: | ---: |
| loopback-0 | 27.14 ± 0.88 | 41.84 ± 4.43 | 23.99 ± 3.44 |
| loopback-64 | 31.15 ± 5.12 | 48.29 ± 10.77 | 23.66 ± 3.39 |
| loopback-1024 | 34.67 ± 4.58 | 58.73 ± 13.73 | 24.98 ± 2.69 |
| loopback-65536 | 122.22 ± 10.25 | 172.10 ± 39.38 | 110.36 ± 13.19 |
| unary-empty | 34.66 ± 7.55 | 52.20 ± 6.77 | 33.32 ± 5.21 |
| unary-small | 30.34 ± 5.07 | 43.21 ± 0.94 | 22.61 ± 0.55 |
| unary-1k | 33.75 ± 5.56 | 51.49 ± 9.85 | 25.57 ± 1.76 |
| fanout-32 | 347.01 ± 36.24 | 584.31 ± 22.50 | 191.54 ± 46.52 |
| dependent-read | 69.31 ± 7.94 | 110.30 ± 31.29 | 50.88 ± 7.64 |
| pipelined-read | 57.07 ± 8.15 | unsupported | unsupported |
| write-read | 71.33 ± 13.37 | 112.78 ± 8.48 | 55.69 ± 7.42 |
| callback | 79.52 ± 8.77 | 61.23 ± 9.47 | 57.14 ± 6.10 |
| bulk-64k | 118.35 ± 3.52 | 170.94 ± 30.38 | 119.35 ± 13.01 |
| bulk-256k | 271.66 ± 18.93 | 598.79 ± 60.22 | 445.74 ± 30.00 |
| bulk-1m | 1010.05 ± 52.43 | 1908.66 ± 691.38 | 2130.40 ± 116.03 |
| bulk-4m | 4618.69 ± 529.65 | 11080.73 ± 2174.53 | 11773.23 ± 188.39 |

## Verification and tracing

226 final end-to-end checks passed: every supported application fixture, mixed callbacks/pipelines/concurrent chains, wrong-hostname and untrusted-CA rejection, TLS 1.3 negotiation and TLS 1.2 rejection on every TCP fixture. The OpenSSL probes negotiated TLS_AES_256_GCM_SHA384 and verified the certificate. Benchmark unit tests (19), quality/report tests (22), Python tests (5), and strict benchmark Clippy passed. No production RPC engine implementation changed in this rollout.

Ciphertext syscall traces cover all 24 TLS and QUIC core cases in separate diagnostic runs. Their latency samples are not used above. Counts include every service peer, handshakes, shutdown, TLS records and QUIC ACK/retry/control packets. They are not Cap’n Proto message counts and must not be compared to historical plaintext frame counts. The trace parser separates attempted/positive calls and byte/datagram units; the old plaintext frame decoder rejects encrypted captures. Full per-peer journals and aggregate counts are in the evidence and report.json.

In the separate full-process traces, portable has no more positive or attempted reads and writes in 19/24 TLS comparisons against C++ and 2/24 QUIC comparisons against legacy. Setup and control packets are included, so this is not timed-operation message parity.

Builds, unit tests and instrumentation did not overlap the native campaign. The same frozen Rust executable selects legacy/portable on every peer; the C++ source pin remains 0de72d8d8cec6b69edaa29de51d3bd490341f9c2. Crypto stacks differ as documented in the benchmark guide. Measurements are loopback on this host with recorded CPU placement; no network loss, remote network or handshake performance claim is made.

## Evidence

Source base: `8d3f72081f610b8aec4d54f0986c2a8e8f576791`; source-only tree: `513fc410e1a406383d27acdec99965f78504356b`; source patch SHA-256: `e56a2aa69e72c6420e39265ce4afe5d96b5af631c90cf5c71e135a1d08145783`.

Archive: `/workspace/capntproto/target/encrypted-rpc-20261022.tar.xz`; SHA-256 `40da619a790fbfe08e22f55c310e2755f6adb14ac19e3af3f5a38261ad87d9ce`. It contains frozen executables, raw samples, ciphertext captures, certificate fingerprints, fixture scripts, source patch and validation logs. Test certificate private keys are not part of the committed report.

The standard benchmark bundle generates a fresh seven-day fixture. CI runs the encrypted end-to-end checks. Two stale GitHub Action version comments were corrected after the preceding CI run stopped at workflow validation; action commit pins were unchanged. Gap #11 stays open, and legacy remains available.
