# Final allocation catalog

Warm, startup-subtracted DHAT slopes from paired 5/50 operations, each with 10 warmups; allocations summed over every peer. Concurrent operations are whole batches. QUIC packet processing introduces run variation; see the repeated captures. These are allocation blocks, not bytes or latency.

| Transport | Case | Legacy blocks | Portable blocks | C++ blocks | Portable − legacy bytes |
| --- | --- | ---: | ---: | ---: | ---: |
| tls | unary-small | 7.000 | 7.000 | 91.489 | +40.0 |
| tls | dependent-read | 46.000 | 46.000 | 246.978 | +527.8 |
| tls | pipelined-read | 58.000 | 58.000 | 233.489 | +780.1 |
| tls | bulk-64k | 23.000 | 23.000 | 262.867 | +672.0 |
| tls | bulk-1m | 145.000 | 143.000 | 2363.800 | +592.2 |
| tls | bulk-4m | 529.000 | 527.000 | 8899.356 | +592.0 |
| tls | deep-32 | 8.000 | 8.000 | 96.489 | +39.9 |
| tls | concurrent-32 | 202.000 | 198.000 | 2062.489 | -2815.8 |
| tls | deep-32-concurrent-32 | 208.000 | 204.000 | 2146.622 | -2815.9 |
| tls | services-4-deep-16-clients-8 | 1660.089 | 1571.756 | 8559.822 | -9666.5 |
| quic | unary-small | 21.311 | 21.133 | — | -10.3 |
| quic | dependent-read | 74.489 | 73.800 | — | +470.6 |
| quic | pipelined-read | 72.600 | 71.800 | — | +667.6 |
| quic | bulk-64k | 104.022 | 101.044 | — | -58.2 |
| quic | bulk-1m | 1244.978 | 1252.578 | — | +2534.9 |
| quic | bulk-4m | 4969.022 | 4913.222 | — | -574.0 |
| quic | deep-32 | 22.178 | 22.467 | — | +39.4 |
| quic | concurrent-32 | 218.622 | 214.156 | — | -2964.1 |
| quic | deep-32-concurrent-32 | 277.267 | 270.422 | — | -3521.7 |
| quic | services-4-deep-16-clients-8 | 2019.689 | 1941.089 | — | -9217.6 |
| tls | loopback-0 | 7.000 | 7.000 | 91.489 | +40.1 |
| tls | loopback-64 | 7.000 | 7.000 | 91.489 | +40.0 |
| tls | loopback-1024 | 7.000 | 7.000 | 91.489 | +40.1 |
| tls | loopback-65536 | 23.000 | 23.000 | 263.844 | +672.1 |
| tls | unary-empty | 7.000 | 7.000 | 91.489 | +40.0 |
| tls | unary-1k | 7.000 | 7.000 | 91.489 | +39.9 |
| tls | fanout-32 | 206.000 | 202.000 | 2110.756 | -2815.9 |
| tls | write-read | 15.000 | 15.000 | 180.978 | +80.3 |
| tls | callback | 36.000 | 34.000 | 204.978 | -24.1 |
| tls | bulk-256k | 49.000 | 47.000 | 687.756 | +592.2 |
| tls | deep-8 | 8.000 | 8.000 | 96.489 | +39.9 |
| tls | services-4-deep-16 | 55.600 | 52.911 | 361.444 | -834.4 |
| tls | services-4-deep-16-concurrent-32 | 1581.911 | 1563.000 | 8304.489 | -2595.0 |
| tls | services-8-deep-16-clients-8 | 3522.222 | 3383.222 | 16967.533 | -14839.2 |
| quic | loopback-0 | 21.622 | 21.133 | — | -41.6 |
| quic | loopback-64 | 21.622 | 20.844 | — | -71.8 |
| quic | loopback-1024 | 21.622 | 21.444 | — | -10.8 |
| quic | loopback-65536 | 104.378 | 101.422 | — | -58.2 |
| quic | unary-empty | 21.933 | 21.156 | — | -71.6 |
| quic | unary-1k | 21.489 | 20.822 | — | -53.5 |
| quic | fanout-32 | 255.511 | 248.356 | — | -3570.6 |
| quic | write-read | 43.178 | 43.467 | — | +116.4 |
| quic | callback | 64.200 | 62.311 | — | -6.8 |
| quic | bulk-256k | 335.956 | 333.000 | — | +737.4 |
| quic | deep-8 | 22.289 | 22.467 | — | +22.0 |
| quic | services-4-deep-16 | 139.933 | 136.444 | — | -928.6 |
| quic | services-4-deep-16-concurrent-32 | 1778.800 | 1756.600 | — | -3297.5 |
| quic | services-8-deep-16-clients-8 | 4118.089 | 4029.933 | — | +845.4 |
