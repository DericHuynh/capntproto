# Local latency comparison

Five fresh-process repetitions, 50 warmups, 500 measured operations. Mean ± sample SD of run means in µs. Whole batches for concurrent cases. Both frozen controls use `8f7f500`; exploratory local measurements.

| Transport | Case | Legacy | Portable | Frozen portable | C++ |
| --- | --- | ---: | ---: | ---: | ---: |
| tls | unary-small | 33.95 ± 6.89 | 34.34 ± 5.14 | 29.50 ± 3.32 | 48.99 ± 9.52 |
| tls | fanout-32 | 144.26 ± 26.81 | 189.61 ± 20.88 | 187.12 ± 29.31 | 346.80 ± 36.21 |
| tls | dependent-read | 65.03 ± 12.77 | 90.27 ± 20.67 | 81.30 ± 15.02 | 117.96 ± 20.95 |
| tls | pipelined-read | 36.66 ± 2.65 | 66.96 ± 15.56 | 57.18 ± 3.71 | 63.15 ± 3.20 |
| tls | concurrent-32 | 101.53 ± 16.27 | 123.93 ± 8.00 | 149.01 ± 21.35 | 354.10 ± 20.99 |
| tls | deep-32-concurrent-32 | 305.84 ± 39.40 | 383.22 ± 35.46 | 383.70 ± 49.56 | 526.18 ± 58.46 |
| tls | services-4-deep-16-concurrent-32 | 1345.43 ± 234.58 | 1655.72 ± 243.34 | 1534.78 ± 151.94 | 2001.45 ± 38.55 |
| quic | unary-small | 39.49 ± 8.41 | 45.59 ± 10.98 | 41.87 ± 3.22 | — |
| quic | fanout-32 | 167.46 ± 37.83 | 217.01 ± 39.50 | 210.78 ± 38.23 | — |
| quic | dependent-read | 82.10 ± 24.61 | 119.08 ± 38.22 | 95.17 ± 4.16 | — |
| quic | pipelined-read | 61.38 ± 16.93 | 94.53 ± 26.98 | 109.12 ± 30.93 | — |
| quic | concurrent-32 | 119.86 ± 23.50 | 160.62 ± 34.06 | 138.39 ± 10.27 | — |
| quic | deep-32-concurrent-32 | 370.46 ± 34.47 | 413.28 ± 36.55 | 417.61 ± 40.06 | — |
| quic | services-4-deep-16-concurrent-32 | 1461.00 ± 86.74 | 1645.62 ± 105.46 | 1676.45 ± 106.63 | — |
