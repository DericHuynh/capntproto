# Local direct-frame TLS comparison

Five fresh-process repetitions, 50 warmups and 500 measured operations each. Mean ± sample SD of run means, µs. Setup excluded; no trimming. Whole batches for concurrent cases. Local exploration, not dedicated-host qualification.

| Case | New legacy | Before legacy | New portable | Before portable | C++ |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary-small | 36.43 ± 5.65 | 38.24 ± 6.26 | 35.66 ± 5.50 | 37.42 ± 6.13 | 50.36 ± 8.20 |
| pipelined-read | 44.68 ± 1.98 | 45.27 ± 2.45 | 66.14 ± 2.82 | 65.55 ± 1.87 | 70.88 ± 2.57 |
| bulk-1m | 1420.56 ± 235.52 | 1409.60 ± 190.28 | 1431.88 ± 280.87 | 1463.70 ± 230.77 | 2488.11 ± 520.11 |
