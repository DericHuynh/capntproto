# Encrypted latency comparison

Microseconds per complete operation or concurrent batch. Mean ± sample standard deviation of the five repetition means; each repetition contains 1000 measured operations. This spread is not a confidence interval. C++ QUIC is unsupported.

| Transport | Workload | Frozen legacy | Legacy | Portable | C++ |
| --- | --- | ---: | ---: | ---: | ---: |
| tls | loopback-0 | 39.82 ± 0.96 | 39.32 ± 0.35 | 39.98 ± 0.26 | 56.65 ± 0.95 |
| tls | loopback-64 | 39.26 ± 1.00 | 39.25 ± 1.05 | 40.37 ± 0.26 | 56.71 ± 0.89 |
| tls | loopback-1024 | 42.04 ± 0.55 | 41.98 ± 0.34 | 42.58 ± 0.55 | 59.62 ± 0.78 |
| tls | loopback-65536 | 194.55 ± 1.81 | 193.99 ± 0.71 | 198.01 ± 0.94 | 250.22 ± 6.65 |
| tls | unary-empty | 44.11 ± 11.52 | 39.06 ± 1.62 | 45.21 ± 12.48 | 72.72 ± 24.10 |
| tls | unary-small | 38.52 ± 0.36 | 38.75 ± 0.27 | 39.81 ± 0.50 | 57.04 ± 1.37 |
| tls | unary-1k | 42.03 ± 0.84 | 47.02 ± 11.96 | 48.71 ± 13.27 | 59.85 ± 0.81 |
| tls | fanout-32 | 214.70 ± 35.58 | 238.51 ± 82.37 | 289.61 ± 50.62 | 475.05 ± 19.83 |
| tls | dependent-read | 100.81 ± 27.47 | 95.55 ± 24.48 | 102.16 ± 0.65 | 176.87 ± 46.33 |
| tls | pipelined-read | 58.45 ± 0.90 | 51.88 ± 0.58 | 84.49 ± 13.84 | 96.89 ± 12.10 |
| tls | write-read | 86.17 ± 14.97 | 97.95 ± 24.65 | 98.74 ± 25.86 | 132.25 ± 30.67 |
| tls | callback | 96.19 ± 25.63 | 84.62 ± 0.55 | 112.71 ± 27.17 | 131.91 ± 26.10 |
| tls | bulk-64k | 194.59 ± 0.79 | 198.21 ± 9.45 | 194.84 ± 0.53 | 245.00 ± 2.47 |
| tls | bulk-256k | 478.39 ± 1.96 | 490.87 ± 25.87 | 503.36 ± 17.21 | 1073.76 ± 63.73 |
| tls | bulk-1m | 1714.30 ± 22.15 | 1712.74 ± 17.57 | 1718.23 ± 15.58 | 3933.47 ± 44.83 |
| tls | bulk-4m | 8496.04 ± 140.46 | 8563.34 ± 145.15 | 8534.36 ± 160.77 | 17319.31 ± 314.18 |
| tls | deep-8 | 64.42 ± 12.98 | 60.40 ± 15.17 | 55.83 ± 16.94 | 81.42 ± 29.38 |
| tls | deep-32 | 50.14 ± 0.41 | 58.06 ± 18.07 | 54.78 ± 10.18 | 67.06 ± 1.07 |
| tls | concurrent-32 | 186.38 ± 68.08 | 144.67 ± 26.76 | 218.64 ± 47.37 | 730.70 ± 285.06 |
| tls | deep-32-concurrent-32 | 622.02 ± 175.20 | 436.75 ± 1.86 | 599.12 ± 162.95 | 965.11 ± 302.91 |
| tls | services-4-deep-16 | 372.42 ± 2.18 | 369.52 ± 1.95 | 401.92 ± 2.55 | 498.12 ± 3.07 |
| tls | services-4-deep-16-concurrent-32 | 2167.62 ± 44.22 | 2114.38 ± 57.78 | 2359.73 ± 53.98 | 3695.19 ± 120.55 |
| tls | services-4-deep-16-clients-8 | 2237.42 ± 30.70 | 2198.86 ± 26.79 | 2484.64 ± 41.60 | 3545.88 ± 33.87 |
| tls | services-8-deep-16-clients-8 | 4716.08 ± 45.68 | 4585.55 ± 31.47 | 5000.05 ± 50.86 | 6901.93 ± 105.24 |
| quic | loopback-0 | 51.76 ± 0.37 | 52.33 ± 0.56 | 56.53 ± 0.36 | unsupported |
| quic | loopback-64 | 52.06 ± 0.28 | 52.37 ± 0.43 | 57.04 ± 0.73 | unsupported |
| quic | loopback-1024 | 57.28 ± 6.54 | 61.59 ± 15.81 | 68.43 ± 19.44 | unsupported |
| quic | loopback-65536 | 219.80 ± 0.99 | 221.14 ± 1.27 | 225.13 ± 1.22 | unsupported |
| quic | unary-empty | 59.38 ± 15.81 | 59.16 ± 16.06 | 56.66 ± 0.34 | unsupported |
| quic | unary-small | 66.27 ± 17.83 | 65.96 ± 19.82 | 65.40 ± 18.72 | unsupported |
| quic | unary-1k | 56.70 ± 6.25 | 60.20 ± 13.91 | 60.48 ± 2.18 | unsupported |
| quic | fanout-32 | 222.25 ± 2.02 | 232.52 ± 15.95 | 271.26 ± 2.81 | unsupported |
| quic | dependent-read | 116.41 ± 0.50 | 111.38 ± 0.62 | 143.59 ± 1.18 | unsupported |
| quic | pipelined-read | 82.55 ± 23.31 | 66.83 ± 0.44 | 102.70 ± 4.69 | unsupported |
| quic | write-read | 105.83 ± 1.35 | 104.98 ± 0.99 | 115.48 ± 0.48 | unsupported |
| quic | callback | 110.68 ± 0.41 | 111.32 ± 1.38 | 139.25 ± 0.85 | unsupported |
| quic | bulk-64k | 220.82 ± 0.63 | 220.68 ± 0.79 | 236.69 ± 26.63 | unsupported |
| quic | bulk-256k | 551.85 ± 58.96 | 581.01 ± 23.75 | 608.73 ± 56.20 | unsupported |
| quic | bulk-1m | 2014.05 ± 96.35 | 2013.57 ± 57.89 | 2014.40 ± 77.75 | unsupported |
| quic | bulk-4m | 9710.37 ± 206.35 | 9835.03 ± 162.64 | 9928.48 ± 260.01 | unsupported |
| quic | deep-8 | 54.65 ± 0.36 | 54.58 ± 0.32 | 59.64 ± 0.38 | unsupported |
| quic | deep-32 | 63.33 ± 0.75 | 62.62 ± 0.45 | 68.10 ± 0.79 | unsupported |
| quic | concurrent-32 | 165.83 ± 35.75 | 145.42 ± 5.04 | 208.59 ± 32.96 | unsupported |
| quic | deep-32-concurrent-32 | 539.95 ± 103.12 | 463.77 ± 3.42 | 561.04 ± 80.94 | unsupported |
| quic | services-4-deep-16 | 541.83 ± 4.13 | 537.25 ± 3.68 | 514.02 ± 10.05 | unsupported |
| quic | services-4-deep-16-concurrent-32 | 2625.19 ± 33.99 | 2569.63 ± 32.13 | 2729.83 ± 26.61 | unsupported |
| quic | services-4-deep-16-clients-8 | 2594.87 ± 30.70 | 2549.93 ± 67.04 | 2565.51 ± 20.19 | unsupported |
| quic | services-8-deep-16-clients-8 | 5224.70 ± 42.16 | 5069.94 ± 44.95 | 4755.66 ± 198.63 | unsupported |
