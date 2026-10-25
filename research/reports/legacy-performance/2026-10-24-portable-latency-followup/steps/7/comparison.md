# Local latency comparison

Five fresh-process repetitions, 50 warmups, 500 measured operations. Mean ± sample SD of run means in µs. Whole batches for concurrent cases. Both frozen controls use `b311711`; exploratory local measurements.

| Transport | Case | Legacy | Portable | Frozen portable | C++ |
| --- | --- | ---: | ---: | ---: | ---: |
| tls | unary-small | 29.56 ± 3.70 | 29.30 ± 6.64 | 34.77 ± 7.76 | 44.70 ± 7.22 |
| tls | fanout-32 | 151.48 ± 17.23 | 185.47 ± 24.49 | 179.83 ± 22.47 | 317.78 ± 9.80 |
| tls | dependent-read | 61.64 ± 12.99 | 77.05 ± 11.30 | 71.54 ± 2.36 | 84.60 ± 3.45 |
| tls | pipelined-read | 43.10 ± 6.83 | 65.05 ± 7.93 | 62.23 ± 7.56 | 66.24 ± 6.31 |
| tls | concurrent-32 | 103.34 ± 16.47 | 124.92 ± 8.98 | 129.77 ± 13.35 | 330.71 ± 31.67 |
| tls | deep-32-concurrent-32 | 296.83 ± 45.38 | 355.02 ± 39.34 | 327.32 ± 33.45 | 461.54 ± 52.71 |
| tls | services-4-deep-16-concurrent-32 | 1192.02 ± 73.65 | 1373.42 ± 33.42 | 1364.87 ± 14.80 | 1923.14 ± 89.24 |
| quic | unary-small | 40.87 ± 12.07 | 50.60 ± 9.86 | 42.62 ± 10.16 | — |
| quic | fanout-32 | 167.75 ± 18.66 | 221.24 ± 47.86 | 206.26 ± 31.51 | — |
| quic | dependent-read | 80.98 ± 18.40 | 114.37 ± 31.69 | 109.59 ± 18.55 | — |
| quic | pipelined-read | 44.38 ± 2.92 | 78.81 ± 7.28 | 77.27 ± 1.95 | — |
| quic | concurrent-32 | 101.48 ± 1.81 | 136.20 ± 5.20 | 150.92 ± 28.52 | — |
| quic | deep-32-concurrent-32 | 347.07 ± 42.45 | 394.18 ± 56.80 | 381.25 ± 25.24 | — |
| quic | services-4-deep-16-concurrent-32 | 1400.25 ± 86.59 | 1516.88 ± 58.50 | 1514.59 ± 41.18 | — |
