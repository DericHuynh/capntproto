# Local upstream encryption TLS comparison

Five fresh-process repetitions, 50 warmups and 500 measured operations each. Mean ± sample SD of run means, µs. Setup excluded; no trimming. Whole batches for concurrent cases. Local exploration, not dedicated-host qualification.

| Case | New legacy | Before legacy | New portable | Before portable | C++ |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary-small | 30.42 ± 5.45 | 34.10 ± 4.56 | 33.52 ± 6.65 | 31.18 ± 5.87 | 47.82 ± 11.48 |
| dependent-read | 60.15 ± 6.14 | 67.98 ± 14.55 | 78.82 ± 7.37 | 92.11 ± 24.17 | 96.38 ± 21.12 |
| pipelined-read | 41.43 ± 10.04 | 44.58 ± 8.52 | 80.35 ± 22.44 | 64.19 ± 22.63 | 79.88 ± 19.10 |
| bulk-64k | 97.39 ± 0.91 | 100.99 ± 8.72 | 109.74 ± 25.37 | 102.36 ± 9.06 | 170.22 ± 24.87 |
| bulk-1m | 1241.14 ± 53.16 | 1384.05 ± 227.08 | 1353.00 ± 239.99 | 1301.56 ± 132.23 | 2062.60 ± 157.20 |
| bulk-4m | 5839.86 ± 449.68 | 5595.21 ± 239.16 | 5892.36 ± 424.52 | 5510.62 ± 144.23 | 8922.01 ± 727.21 |
| deep-32 | 36.07 ± 6.53 | 36.03 ± 6.81 | 35.25 ± 1.18 | 37.57 ± 8.72 | 51.17 ± 9.12 |
| concurrent-32 | 95.12 ± 8.31 | 100.79 ± 18.46 | 132.17 ± 8.33 | 142.78 ± 23.40 | 323.03 ± 14.88 |
| services-4-deep-16-clients-8 | 1182.13 ± 88.96 | 1166.77 ± 89.15 | 1242.11 ± 53.17 | 1190.34 ± 44.47 | 1710.08 ± 50.00 |
