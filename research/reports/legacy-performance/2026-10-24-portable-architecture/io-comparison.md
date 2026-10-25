# Successful socket calls

All peers: mean ± sample SD of five paired (100 - 10)/90 operation slopes, with two identical warmups. TLS ciphertext socket calls; diagnostic timings are excluded from latency results.

| Case | Implementation | Reads / operation | Writes / operation |
| --- | --- | ---: | ---: |
| dependent-read | legacy | 4.000 ± 0.000 | 4.000 ± 0.000 |
| dependent-read | portable | 4.000 ± 0.000 | 4.000 ± 0.000 |
| dependent-read | legacy-baseline | 4.000 ± 0.000 | 4.000 ± 0.000 |
| dependent-read | portable-baseline | 4.000 ± 0.000 | 4.000 ± 0.000 |
| dependent-read | cpp | 4.000 ± 0.000 | 4.000 ± 0.000 |
| pipelined-read | legacy | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | portable | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | legacy-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | portable-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | cpp | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | legacy | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | portable | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | legacy-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | portable-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | cpp | 1.998 ± 0.005 | 2.000 ± 0.000 |
| fanout-32 | legacy | 2.000 ± 0.000 | 2.000 ± 0.000 |
| fanout-32 | portable | 2.000 ± 0.000 | 2.000 ± 0.000 |
| fanout-32 | legacy-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| fanout-32 | portable-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| fanout-32 | cpp | 10.000 ± 0.000 | 10.000 ± 0.000 |
| services-4-deep-16-clients-8 | legacy | 27.820 ± 0.257 | 27.851 ± 0.214 |
| services-4-deep-16-clients-8 | portable | 33.138 ± 0.937 | 33.324 ± 0.888 |
| services-4-deep-16-clients-8 | legacy-baseline | 27.529 ± 0.716 | 27.553 ± 0.708 |
| services-4-deep-16-clients-8 | portable-baseline | 33.149 ± 0.749 | 33.307 ± 0.702 |
| services-4-deep-16-clients-8 | cpp | 50.233 ± 0.614 | 50.653 ± 0.521 |
| bulk-1m | legacy | 29.033 ± 3.911 | 18.000 ± 0.000 |
| bulk-1m | portable | 28.142 ± 1.932 | 18.000 ± 0.000 |
| bulk-1m | legacy-baseline | 29.784 ± 2.948 | 18.000 ± 0.000 |
| bulk-1m | portable-baseline | 28.087 ± 2.358 | 18.000 ± 0.000 |
| bulk-1m | cpp | 258.013 ± 0.033 | 258.000 ± 0.016 |
