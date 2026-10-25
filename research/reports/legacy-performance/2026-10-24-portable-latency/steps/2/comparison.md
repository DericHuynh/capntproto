# Local latency comparison

Five fresh-process repetitions, 50 warmups, 500 measured operations. Mean ± sample SD of run means in µs. Whole batches for concurrent cases. Both frozen controls use `8f7f500`; exploratory local measurements.

| Transport | Case | Legacy | Portable | Frozen portable | C++ |
| --- | --- | ---: | ---: | ---: | ---: |
| tls | unary-small | 44.59 ± 8.73 | 46.16 ± 5.34 | 41.65 ± 2.96 | 54.79 ± 2.23 |
| tls | fanout-32 | 154.69 ± 11.43 | 224.01 ± 10.84 | 221.04 ± 11.11 | 446.30 ± 16.95 |
| tls | dependent-read | 83.95 ± 7.63 | 118.37 ± 1.11 | 115.85 ± 8.40 | 123.49 ± 17.15 |
| tls | pipelined-read | 47.38 ± 7.39 | 79.66 ± 22.31 | 93.06 ± 11.80 | 89.21 ± 16.01 |
| tls | concurrent-32 | 116.84 ± 17.11 | 158.51 ± 26.83 | 169.72 ± 11.74 | 416.73 ± 43.49 |
| tls | deep-32-concurrent-32 | 298.32 ± 35.65 | 366.67 ± 42.06 | 342.87 ± 39.76 | 458.53 ± 16.69 |
| tls | services-4-deep-16-concurrent-32 | 1175.32 ± 37.38 | 1428.38 ± 35.01 | 1400.95 ± 52.52 | 1930.80 ± 85.35 |
| quic | unary-small | 40.84 ± 10.39 | 43.36 ± 12.80 | 52.91 ± 15.90 | — |
| quic | fanout-32 | 138.17 ± 3.73 | 191.69 ± 21.39 | 184.83 ± 16.50 | — |
| quic | dependent-read | 79.61 ± 9.67 | 112.94 ± 31.18 | 116.07 ± 33.74 | — |
| quic | pipelined-read | 43.03 ± 1.35 | 75.70 ± 4.72 | 74.27 ± 2.51 | — |
| quic | concurrent-32 | 119.20 ± 24.11 | 160.43 ± 34.21 | 154.57 ± 31.52 | — |
| quic | deep-32-concurrent-32 | 340.16 ± 34.22 | 407.24 ± 39.97 | 394.32 ± 29.21 | — |
| quic | services-4-deep-16-concurrent-32 | 1416.93 ± 130.89 | 1665.09 ± 73.77 | 1641.29 ± 120.58 | — |
