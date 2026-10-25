# Local direct-output rustls TLS comparison

Five fresh-process repetitions, 50 warmups and 500 measured operations each. Mean ± sample SD of run means, µs. Setup excluded; no trimming. Whole batches for concurrent cases. Local exploration, not dedicated-host qualification.

| Case | New legacy | Before legacy | New portable | Before portable | C++ |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary-small | 33.12 ± 4.95 | 34.92 ± 4.69 | 35.77 ± 6.50 | 34.30 ± 5.69 | 50.26 ± 7.71 |
| dependent-read | 64.13 ± 10.63 | 72.38 ± 11.74 | 89.96 ± 16.28 | 105.94 ± 15.14 | 121.94 ± 17.73 |
| pipelined-read | 49.38 ± 4.83 | 45.62 ± 6.08 | 70.97 ± 12.67 | 85.05 ± 19.65 | 92.24 ± 19.34 |
| bulk-64k | 129.98 ± 7.77 | 127.40 ± 13.47 | 128.70 ± 21.45 | 129.63 ± 17.35 | 178.21 ± 17.29 |
| bulk-1m | 1495.76 ± 167.30 | 1423.06 ± 109.44 | 1425.56 ± 330.19 | 1541.78 ± 160.47 | 2568.11 ± 475.83 |
| bulk-4m | 5753.30 ± 300.21 | 5975.38 ± 596.56 | 5556.27 ± 518.30 | 5572.87 ± 283.40 | 9901.72 ± 1169.50 |
| deep-32 | 33.30 ± 1.28 | 32.44 ± 1.04 | 34.38 ± 1.96 | 33.13 ± 0.94 | 43.38 ± 3.69 |
| concurrent-32 | 103.91 ± 16.83 | 105.00 ± 16.67 | 147.86 ± 23.82 | 157.21 ± 25.93 | 379.74 ± 51.33 |
| services-4-deep-16-clients-8 | 1149.28 ± 45.80 | 1141.39 ± 49.27 | 1222.53 ± 54.62 | 1198.60 ± 50.88 | 1642.57 ± 80.58 |
