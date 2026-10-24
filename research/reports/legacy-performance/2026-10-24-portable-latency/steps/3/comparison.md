# Local latency comparison

Five fresh-process repetitions, 50 warmups, 500 measured operations. Mean ± sample SD of run means in µs. Whole batches for concurrent cases. Both frozen controls use `8f7f500`; exploratory local measurements.

| Transport | Case | Legacy | Portable | Frozen portable | C++ |
| --- | --- | ---: | ---: | ---: | ---: |
| tls | unary-small | 34.19 ± 4.71 | 35.77 ± 8.49 | 35.11 ± 5.58 | 45.92 ± 7.21 |
| tls | fanout-32 | 132.61 ± 11.96 | 182.07 ± 19.65 | 162.58 ± 10.25 | 336.55 ± 40.02 |
| tls | dependent-read | 63.67 ± 12.80 | 76.85 ± 4.43 | 75.01 ± 4.99 | 88.61 ± 5.82 |
| tls | pipelined-read | 34.97 ± 1.00 | 65.54 ± 20.55 | 54.08 ± 2.65 | 67.90 ± 12.97 |
| tls | concurrent-32 | 90.38 ± 2.08 | 123.24 ± 2.29 | 130.11 ± 11.86 | 316.22 ± 12.41 |
| tls | deep-32-concurrent-32 | 301.49 ± 43.43 | 322.00 ± 21.55 | 362.72 ± 50.31 | 511.69 ± 27.53 |
| tls | services-4-deep-16-concurrent-32 | 1256.25 ± 107.16 | 1520.65 ± 64.72 | 1481.70 ± 75.89 | 2234.11 ± 213.48 |
| quic | unary-small | 38.10 ± 9.32 | 41.40 ± 9.42 | 40.92 ± 12.17 | — |
| quic | fanout-32 | 152.15 ± 10.44 | 199.42 ± 31.42 | 197.68 ± 22.05 | — |
| quic | dependent-read | 96.82 ± 28.00 | 161.04 ± 42.52 | 140.79 ± 35.93 | — |
| quic | pipelined-read | 55.23 ± 8.69 | 115.77 ± 27.08 | 100.20 ± 16.11 | — |
| quic | concurrent-32 | 111.89 ± 12.99 | 165.68 ± 22.49 | 150.83 ± 15.78 | — |
| quic | deep-32-concurrent-32 | 322.04 ± 27.43 | 393.93 ± 43.99 | 405.07 ± 49.75 | — |
| quic | services-4-deep-16-concurrent-32 | 1338.84 ± 35.19 | 1584.73 ± 38.27 | 1570.39 ± 79.25 | — |
