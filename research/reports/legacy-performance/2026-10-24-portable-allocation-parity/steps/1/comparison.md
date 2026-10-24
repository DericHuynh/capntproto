# Local direct-frame TLS comparison

Five fresh-process repetitions, 50 warmups and 500 measured operations each. Mean ± sample SD of run means, µs. Setup excluded; no trimming. Whole batches for concurrent cases. Local exploration, not dedicated-host qualification.

| Case | New legacy | Before legacy | New portable | Before portable | C++ |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary-small | 32.00 ± 6.82 | 34.67 ± 10.46 | 35.98 ± 8.15 | 35.91 ± 10.46 | 48.54 ± 13.94 |
| pipelined-read | 33.82 ± 2.65 | 33.31 ± 1.07 | 53.90 ± 5.35 | 55.04 ± 5.67 | 61.22 ± 4.66 |
| bulk-1m | 1306.08 ± 194.76 | 1410.35 ± 225.60 | 1352.04 ± 173.32 | 1494.43 ± 195.36 | 2713.50 ± 608.04 |
