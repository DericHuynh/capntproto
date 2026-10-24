# Local latency comparison

Five fresh-process repetitions, 50 warmups, 500 measured operations. Mean ± sample SD of run means in µs. Whole batches for concurrent cases. Both frozen controls use `8f7f500`; exploratory local measurements.

| Transport | Case | Legacy | Portable | Frozen portable | C++ |
| --- | --- | ---: | ---: | ---: | ---: |
| tls | unary-small | 27.69 ± 5.49 | 29.92 ± 6.51 | 31.87 ± 6.91 | 41.51 ± 9.94 |
| tls | fanout-32 | 130.20 ± 5.53 | 167.80 ± 27.25 | 161.75 ± 9.68 | 319.95 ± 36.70 |
| tls | dependent-read | 61.82 ± 8.01 | 82.68 ± 17.21 | 80.28 ± 18.55 | 88.07 ± 7.22 |
| tls | pipelined-read | 42.48 ± 7.11 | 77.57 ± 18.37 | 76.31 ± 24.99 | 79.18 ± 19.69 |
| tls | concurrent-32 | 117.85 ± 14.28 | 137.38 ± 10.63 | 137.88 ± 25.31 | 367.06 ± 30.06 |
| tls | deep-32-concurrent-32 | 314.66 ± 24.01 | 377.01 ± 34.82 | 381.22 ± 48.75 | 540.88 ± 52.69 |
| tls | services-4-deep-16-concurrent-32 | 1171.80 ± 62.06 | 1442.42 ± 62.33 | 1440.43 ± 27.51 | 2025.32 ± 145.04 |
| quic | unary-small | 31.94 ± 2.42 | 34.74 ± 0.80 | 36.15 ± 1.57 | — |
| quic | fanout-32 | 158.37 ± 23.64 | 202.47 ± 19.97 | 198.04 ± 24.17 | — |
| quic | dependent-read | 88.86 ± 13.64 | 142.67 ± 25.96 | 121.95 ± 16.41 | — |
| quic | pipelined-read | 58.65 ± 15.67 | 101.28 ± 30.90 | 104.72 ± 29.74 | — |
| quic | concurrent-32 | 128.45 ± 26.66 | 154.53 ± 31.07 | 165.85 ± 25.04 | — |
| quic | deep-32-concurrent-32 | 375.01 ± 55.09 | 417.12 ± 34.35 | 435.28 ± 50.30 | — |
| quic | services-4-deep-16-concurrent-32 | 1395.51 ± 75.08 | 1615.45 ± 26.96 | 1639.07 ± 107.84 | — |
