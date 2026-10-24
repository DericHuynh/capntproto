# Local latency comparison

Five fresh-process repetitions, 50 warmups, 500 measured operations. Mean ± sample SD of run means in µs. Whole batches for concurrent cases. Both frozen controls use `87be38e`; exploratory local measurements.

| Transport | Case | Legacy | Portable | Frozen portable | C++ |
| --- | --- | ---: | ---: | ---: | ---: |
| tls | unary-small | 30.40 ± 4.75 | 30.68 ± 5.14 | 32.30 ± 7.10 | 40.52 ± 9.35 |
| tls | fanout-32 | 129.40 ± 15.20 | 169.72 ± 27.26 | 167.83 ± 24.71 | 336.35 ± 47.96 |
| tls | dependent-read | 64.25 ± 12.41 | 75.17 ± 12.15 | 96.04 ± 21.08 | 97.35 ± 15.86 |
| tls | pipelined-read | 36.22 ± 4.79 | 66.12 ± 19.13 | 64.07 ± 14.06 | 76.36 ± 13.82 |
| tls | concurrent-32 | 106.39 ± 1.55 | 135.92 ± 6.02 | 132.52 ± 3.08 | 335.89 ± 8.74 |
| tls | deep-32-concurrent-32 | 312.93 ± 39.02 | 344.11 ± 47.93 | 365.53 ± 46.76 | 469.36 ± 28.27 |
| tls | services-4-deep-16-concurrent-32 | 1296.07 ± 273.31 | 1369.08 ± 64.24 | 1361.08 ± 27.07 | 1965.64 ± 81.37 |
| quic | unary-small | 41.10 ± 8.94 | 36.73 ± 1.42 | 48.73 ± 14.22 | — |
| quic | fanout-32 | 137.62 ± 9.49 | 167.38 ± 4.24 | 170.06 ± 9.18 | — |
| quic | dependent-read | 77.60 ± 18.95 | 101.97 ± 19.20 | 90.22 ± 3.03 | — |
| quic | pipelined-read | 43.77 ± 2.06 | 81.37 ± 17.89 | 90.37 ± 21.94 | — |
| quic | concurrent-32 | 109.72 ± 9.74 | 154.79 ± 19.54 | 160.32 ± 29.54 | — |
| quic | deep-32-concurrent-32 | 364.19 ± 63.35 | 416.78 ± 31.75 | 425.63 ± 40.40 | — |
| quic | services-4-deep-16-concurrent-32 | 1394.77 ± 143.86 | 1488.35 ± 64.50 | 1521.02 ± 46.85 | — |
