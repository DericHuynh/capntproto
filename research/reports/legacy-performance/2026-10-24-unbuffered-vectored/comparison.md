# Local vectored Unbuffered TLS comparison

Five fresh-process repetitions, 50 warmups and 500 measured operations each. Mean ± sample SD of run means, µs. Setup excluded; no trimming. Whole batches for concurrent cases. Local exploration, not dedicated-host qualification.

| Case | New legacy | Before legacy | New portable | Before portable | C++ |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary-small | 29.85 ± 5.39 | 30.18 ± 5.99 | 38.84 ± 6.93 | 33.33 ± 5.77 | 49.80 ± 8.08 |
| dependent-read | 68.51 ± 14.49 | 71.48 ± 15.08 | 88.20 ± 21.17 | 75.20 ± 9.85 | 105.76 ± 24.02 |
| pipelined-read | 36.49 ± 5.30 | 37.39 ± 8.35 | 55.68 ± 5.63 | 77.88 ± 20.97 | 70.55 ± 8.96 |
| bulk-64k | 136.30 ± 32.60 | 131.36 ± 28.24 | 138.29 ± 34.74 | 124.20 ± 26.42 | 205.64 ± 38.65 |
| bulk-1m | 1387.21 ± 213.52 | 1345.87 ± 143.20 | 1238.51 ± 72.34 | 1250.83 ± 81.31 | 2334.18 ± 561.50 |
| bulk-4m | 5740.44 ± 614.99 | 5673.28 ± 229.18 | 6165.62 ± 597.18 | 6224.91 ± 491.89 | 9609.00 ± 960.06 |
| deep-32 | 33.68 ± 2.02 | 34.96 ± 5.98 | 34.16 ± 1.38 | 37.50 ± 7.19 | 47.85 ± 5.58 |
| concurrent-32 | 102.56 ± 8.64 | 106.47 ± 13.41 | 131.24 ± 8.95 | 138.73 ± 7.39 | 320.82 ± 16.73 |
| services-4-deep-16-clients-8 | 1140.37 ± 19.18 | 1140.43 ± 39.94 | 1168.13 ± 50.77 | 1176.44 ± 55.29 | 1704.57 ± 76.18 |
