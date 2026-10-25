# Local direct-frame TLS comparison

Five fresh-process repetitions, 50 warmups and 500 measured operations each. Mean ± sample SD of run means, µs. Setup excluded; no trimming. Whole batches for concurrent cases. Local exploration, not dedicated-host qualification.

| Case | New legacy | Before legacy | New portable | Before portable | C++ |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary-small | 31.61 ± 6.16 | 25.91 ± 0.77 | 35.74 ± 10.86 | 33.90 ± 11.82 | 48.33 ± 11.87 |
| dependent-read | 62.30 ± 11.90 | 57.19 ± 4.16 | 85.02 ± 20.91 | 80.69 ± 23.23 | 97.07 ± 21.14 |
| pipelined-read | 32.60 ± 0.79 | 32.97 ± 0.36 | 52.41 ± 1.12 | 52.43 ± 2.95 | 58.63 ± 2.98 |
| bulk-64k | 100.31 ± 8.12 | 98.32 ± 5.11 | 97.99 ± 2.45 | 115.25 ± 20.47 | 160.80 ± 15.15 |
| bulk-1m | 1379.63 ± 185.85 | 1240.31 ± 36.11 | 1286.65 ± 246.45 | 1199.45 ± 52.65 | 2110.91 ± 342.17 |
| bulk-4m | 5451.04 ± 227.27 | 5855.02 ± 318.13 | 5813.62 ± 344.30 | 5710.65 ± 618.93 | 10321.02 ± 1190.47 |
| deep-32 | 42.77 ± 1.04 | 43.03 ± 1.28 | 44.78 ± 2.78 | 43.90 ± 1.25 | 51.63 ± 1.60 |
| concurrent-32 | 101.54 ± 9.67 | 96.46 ± 8.30 | 143.40 ± 23.37 | 140.77 ± 17.48 | 327.80 ± 5.05 |
| services-4-deep-16-clients-8 | 1110.84 ± 32.96 | 1145.39 ± 50.94 | 1212.43 ± 37.83 | 1209.52 ± 47.10 | 1681.15 ± 42.00 |
