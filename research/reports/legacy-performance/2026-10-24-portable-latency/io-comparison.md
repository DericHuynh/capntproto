# Successful socket calls

All peers: mean ± sample SD of five paired (100 - 10)/90 operation slopes, with two identical warmups. TLS ciphertext socket calls; diagnostic timings are excluded from latency results.

| Case | Implementation | Reads / operation | Writes / operation |
| --- | --- | ---: | ---: |
| dependent-read | legacy | 4.000 ± 0.000 | 4.000 ± 0.000 |
| dependent-read | portable | 4.000 ± 0.000 | 4.000 ± 0.000 |
| dependent-read | legacy-baseline | 4.000 ± 0.000 | 4.000 ± 0.000 |
| dependent-read | portable-baseline | 4.000 ± 0.000 | 4.000 ± 0.000 |
| dependent-read | cpp | 4.004 ± 0.006 | 4.000 ± 0.000 |
| pipelined-read | legacy | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | portable | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | legacy-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | portable-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | cpp | 2.002 ± 0.005 | 2.000 ± 0.000 |
| concurrent-32 | legacy | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | portable | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | legacy-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | portable-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | cpp | 2.000 ± 0.000 | 2.000 ± 0.000 |
| fanout-32 | legacy | 2.000 ± 0.000 | 2.000 ± 0.000 |
| fanout-32 | portable | 2.000 ± 0.000 | 2.000 ± 0.000 |
| fanout-32 | legacy-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| fanout-32 | portable-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| fanout-32 | cpp | 9.998 ± 0.009 | 10.000 ± 0.000 |
| services-4-deep-16-clients-8 | legacy | 27.184 ± 0.502 | 27.189 ± 0.512 |
| services-4-deep-16-clients-8 | portable | 32.700 ± 0.671 | 32.931 ± 0.591 |
| services-4-deep-16-clients-8 | legacy-baseline | 26.982 ± 0.238 | 27.004 ± 0.240 |
| services-4-deep-16-clients-8 | portable-baseline | 32.862 ± 0.472 | 33.056 ± 0.412 |
| services-4-deep-16-clients-8 | cpp | 50.091 ± 0.595 | 50.544 ± 0.529 |
| bulk-1m | legacy | 29.289 ± 1.954 | 18.000 ± 0.000 |
| bulk-1m | portable | 29.740 ± 3.766 | 18.000 ± 0.000 |
| bulk-1m | legacy-baseline | 28.818 ± 2.322 | 18.000 ± 0.000 |
| bulk-1m | portable-baseline | 30.236 ± 5.777 | 18.000 ± 0.000 |
| bulk-1m | cpp | 258.002 ± 0.014 | 258.000 ± 0.016 |
