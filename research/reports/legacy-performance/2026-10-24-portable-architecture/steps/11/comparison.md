# Local latency comparison

Five fresh-process repetitions, 50 warmups, 500 measured operations. Mean ± sample SD of run means in µs. Whole batches for concurrent cases. Both frozen controls use `87be38e`; exploratory local measurements.

| Transport | Case | Legacy | Portable | Frozen portable | C++ |
| --- | --- | ---: | ---: | ---: | ---: |
| tls | unary-small | 27.67 ± 3.21 | 27.37 ± 0.95 | 33.73 ± 7.63 | 43.54 ± 8.74 |
| tls | fanout-32 | 135.91 ± 25.50 | 150.55 ± 2.32 | 150.58 ± 4.98 | 307.15 ± 15.45 |
| tls | dependent-read | 81.44 ± 1.92 | 96.44 ± 15.41 | 85.48 ± 9.27 | 101.60 ± 8.38 |
| tls | pipelined-read | 37.75 ± 8.54 | 78.37 ± 22.97 | 53.00 ± 0.90 | 62.20 ± 6.59 |
| tls | concurrent-32 | 88.93 ± 3.85 | 116.80 ± 3.54 | 124.37 ± 16.85 | 332.70 ± 29.05 |
| tls | deep-32-concurrent-32 | 282.61 ± 30.51 | 351.49 ± 52.69 | 374.52 ± 48.87 | 464.92 ± 31.57 |
| tls | services-4-deep-16-concurrent-32 | 1182.41 ± 46.74 | 1371.07 ± 23.04 | 1364.84 ± 10.25 | 1939.54 ± 50.82 |
| quic | unary-small | 38.76 ± 8.72 | 42.96 ± 11.76 | 42.74 ± 9.85 | — |
| quic | fanout-32 | 156.52 ± 31.61 | 199.54 ± 21.72 | 198.51 ± 33.35 | — |
| quic | dependent-read | 76.18 ± 5.14 | 99.16 ± 4.00 | 118.80 ± 26.14 | — |
| quic | pipelined-read | 45.54 ± 2.16 | 82.62 ± 3.17 | 81.79 ± 2.47 | — |
| quic | concurrent-32 | 122.14 ± 23.20 | 165.72 ± 26.44 | 159.58 ± 21.22 | — |
| quic | deep-32-concurrent-32 | 364.28 ± 34.63 | 378.59 ± 28.70 | 391.99 ± 36.65 | — |
| quic | services-4-deep-16-concurrent-32 | 1364.99 ± 76.82 | 1537.20 ± 32.54 | 1530.75 ± 59.29 | — |
