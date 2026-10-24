# Local latency comparison

Five fresh-process repetitions, 50 warmups, 500 measured operations. Mean ± sample SD of run means in µs. Whole batches for concurrent cases. Both frozen controls use `8f7f500`; exploratory local measurements.

| Transport | Case | Legacy | Portable | Frozen portable | C++ |
| --- | --- | ---: | ---: | ---: | ---: |
| tls | unary-small | 38.56 ± 5.55 | 41.34 ± 1.27 | 40.96 ± 1.37 | 57.33 ± 1.05 |
| tls | fanout-32 | 152.15 ± 24.38 | 202.38 ± 16.95 | 190.20 ± 19.86 | 396.63 ± 42.48 |
| tls | dependent-read | 70.88 ± 9.05 | 101.09 ± 24.14 | 108.99 ± 18.92 | 112.38 ± 20.16 |
| tls | pipelined-read | 40.15 ± 6.88 | 68.53 ± 11.50 | 83.30 ± 20.49 | 71.94 ± 16.27 |
| tls | concurrent-32 | 110.13 ± 12.94 | 142.90 ± 22.33 | 141.33 ± 24.40 | 363.77 ± 42.40 |
| tls | deep-32-concurrent-32 | 360.16 ± 52.72 | 402.36 ± 57.94 | 468.22 ± 109.35 | 558.50 ± 119.15 |
| tls | services-4-deep-16-concurrent-32 | 1342.15 ± 238.02 | 1610.02 ± 190.26 | 1551.68 ± 114.92 | 2080.77 ± 108.07 |
| quic | unary-small | 43.78 ± 10.81 | 48.57 ± 14.21 | 40.62 ± 7.26 | — |
| quic | fanout-32 | 150.38 ± 27.92 | 191.81 ± 19.77 | 204.62 ± 24.53 | — |
| quic | dependent-read | 74.69 ± 5.05 | 97.35 ± 6.99 | 112.79 ± 34.64 | — |
| quic | pipelined-read | 42.26 ± 2.97 | 86.13 ± 22.01 | 77.47 ± 5.35 | — |
| quic | concurrent-32 | 124.16 ± 21.40 | 170.50 ± 36.60 | 158.14 ± 32.24 | — |
| quic | deep-32-concurrent-32 | 346.90 ± 63.89 | 387.67 ± 44.90 | 446.31 ± 73.79 | — |
| quic | services-4-deep-16-concurrent-32 | 1712.27 ± 513.48 | 1825.29 ± 108.13 | 2078.49 ± 153.38 | — |
