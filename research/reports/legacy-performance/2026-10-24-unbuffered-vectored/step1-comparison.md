# Local vectored Unbuffered TLS comparison

Five fresh-process repetitions, 50 warmups and 500 measured operations each. Mean ± sample SD of run means, µs. Setup excluded; no trimming. Whole batches for concurrent cases. Local exploration, not dedicated-host qualification.

| Case | New legacy | Before legacy | New portable | Before portable | C++ |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary-small | 29.46 ± 4.69 | 27.24 ± 2.48 | 30.51 ± 5.56 | 30.53 ± 5.47 | 41.59 ± 8.88 |
| dependent-read | 56.74 ± 4.01 | 61.79 ± 9.33 | 72.22 ± 4.12 | 100.42 ± 19.38 | 119.13 ± 24.11 |
| pipelined-read | 34.82 ± 1.82 | 35.07 ± 1.21 | 65.97 ± 22.00 | 55.34 ± 1.63 | 68.85 ± 19.08 |
| bulk-64k | 100.11 ± 4.28 | 107.99 ± 18.86 | 101.26 ± 3.39 | 103.16 ± 2.78 | 157.58 ± 25.83 |
| bulk-1m | 1249.60 ± 72.75 | 1321.52 ± 239.38 | 1190.35 ± 87.55 | 1222.11 ± 275.05 | 2008.99 ± 121.19 |
| bulk-4m | 5498.94 ± 363.27 | 5963.38 ± 682.82 | 5921.45 ± 671.12 | 6116.63 ± 347.31 | 9857.30 ± 912.45 |
| deep-32 | 45.06 ± 1.47 | 44.40 ± 1.16 | 43.13 ± 5.91 | 42.94 ± 6.10 | 51.96 ± 6.83 |
| concurrent-32 | 113.56 ± 13.64 | 105.61 ± 11.40 | 144.10 ± 7.19 | 142.20 ± 12.24 | 355.77 ± 11.04 |
| services-4-deep-16-clients-8 | 1303.13 ± 166.40 | 1202.65 ± 43.64 | 1246.24 ± 42.98 | 1295.25 ± 150.00 | 1783.46 ± 115.44 |
