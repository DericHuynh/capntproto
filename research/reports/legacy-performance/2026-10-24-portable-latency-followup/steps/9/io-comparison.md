# Successful socket calls

All peers: mean ± sample SD of five paired (100 - 10)/90 operation slopes, with two identical warmups. TLS ciphertext socket calls; diagnostic timings are excluded from latency results.

| Case | Implementation | Reads / operation | Writes / operation |
| --- | --- | ---: | ---: |
| dependent-read | legacy | 4.000 ± 0.000 | 4.000 ± 0.000 |
| dependent-read | portable | 4.000 ± 0.000 | 4.000 ± 0.000 |
| dependent-read | legacy-baseline | 4.000 ± 0.000 | 4.000 ± 0.000 |
| dependent-read | portable-baseline | 4.000 ± 0.000 | 4.000 ± 0.000 |
| dependent-read | cpp | 4.002 ± 0.005 | 4.000 ± 0.000 |
| pipelined-read | legacy | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | portable | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | legacy-baseline | 1.998 ± 0.005 | 2.000 ± 0.000 |
| pipelined-read | portable-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | cpp | 2.000 ± 0.008 | 2.000 ± 0.000 |
| concurrent-32 | legacy | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | portable | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | legacy-baseline | 2.004 ± 0.006 | 2.000 ± 0.000 |
| concurrent-32 | portable-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | cpp | 1.998 ± 0.005 | 2.000 ± 0.000 |
| fanout-32 | legacy | 2.000 ± 0.000 | 2.000 ± 0.000 |
| fanout-32 | portable | 2.000 ± 0.000 | 2.000 ± 0.000 |
| fanout-32 | legacy-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| fanout-32 | portable-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| fanout-32 | cpp | 10.000 ± 0.000 | 10.000 ± 0.000 |
| services-4-deep-16-clients-8 | legacy | 27.273 ± 0.356 | 27.302 ± 0.343 |
| services-4-deep-16-clients-8 | portable | 32.911 ± 0.922 | 33.138 ± 0.861 |
| services-4-deep-16-clients-8 | legacy-baseline | 27.469 ± 0.462 | 27.502 ± 0.446 |
| services-4-deep-16-clients-8 | portable-baseline | 32.987 ± 0.813 | 33.180 ± 0.741 |
| services-4-deep-16-clients-8 | cpp | 50.020 ± 0.483 | 50.462 ± 0.396 |
| bulk-1m | legacy | 29.720 ± 2.607 | 18.000 ± 0.000 |
| bulk-1m | portable | 29.051 ± 3.372 | 18.000 ± 0.000 |
| bulk-1m | legacy-baseline | 29.004 ± 1.860 | 18.000 ± 0.000 |
| bulk-1m | portable-baseline | 28.571 ± 1.883 | 18.000 ± 0.000 |
| bulk-1m | cpp | 258.018 ± 0.028 | 258.004 ± 0.019 |
