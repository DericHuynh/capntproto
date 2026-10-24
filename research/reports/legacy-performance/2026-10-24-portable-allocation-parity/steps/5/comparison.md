# Local direct-frame TLS comparison

Five fresh-process repetitions, 50 warmups and 500 measured operations each. Mean ± sample SD of run means, µs. Setup excluded; no trimming. Whole batches for concurrent cases. Local exploration, not dedicated-host qualification.

| Case | New legacy | Before legacy | New portable | Before portable | C++ |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary-small | 32.21 ± 4.63 | 34.15 ± 6.94 | 30.78 ± 5.44 | 37.46 ± 5.92 | 48.72 ± 8.48 |
| pipelined-read | 41.83 ± 8.68 | 40.42 ± 7.57 | 71.98 ± 20.00 | 68.95 ± 13.89 | 87.82 ± 16.61 |
| bulk-1m | 1281.98 ± 96.66 | 1406.51 ± 224.61 | 1294.49 ± 200.73 | 1434.51 ± 157.55 | 2394.87 ± 601.34 |
