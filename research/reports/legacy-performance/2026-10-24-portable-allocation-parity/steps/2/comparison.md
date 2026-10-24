# Local direct-frame TLS comparison

Five fresh-process repetitions, 50 warmups and 500 measured operations each. Mean ± sample SD of run means, µs. Setup excluded; no trimming. Whole batches for concurrent cases. Local exploration, not dedicated-host qualification.

| Case | New legacy | Before legacy | New portable | Before portable | C++ |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary-small | 36.44 ± 6.18 | 38.13 ± 18.55 | 30.46 ± 5.85 | 33.08 ± 9.84 | 45.01 ± 11.38 |
| pipelined-read | 47.36 ± 9.42 | 42.82 ± 8.05 | 75.27 ± 20.26 | 74.36 ± 18.74 | 83.79 ± 22.66 |
| bulk-1m | 1425.42 ± 201.68 | 1310.81 ± 94.21 | 1303.70 ± 259.04 | 1227.90 ± 41.92 | 2034.09 ± 164.69 |
