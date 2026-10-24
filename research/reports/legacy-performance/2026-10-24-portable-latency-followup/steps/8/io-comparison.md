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
| pipelined-read | legacy-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | portable-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | cpp | 1.996 ± 0.010 | 2.000 ± 0.000 |
| concurrent-32 | legacy | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | portable | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | legacy-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | portable-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | cpp | 2.002 ± 0.005 | 2.000 ± 0.000 |
| fanout-32 | legacy | 2.000 ± 0.000 | 2.000 ± 0.000 |
| fanout-32 | portable | 2.000 ± 0.000 | 2.000 ± 0.000 |
| fanout-32 | legacy-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| fanout-32 | portable-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| fanout-32 | cpp | 10.000 ± 0.000 | 10.000 ± 0.000 |
| services-4-deep-16-clients-8 | legacy | 27.304 ± 0.191 | 27.342 ± 0.195 |
| services-4-deep-16-clients-8 | portable | 32.569 ± 0.699 | 32.753 ± 0.676 |
| services-4-deep-16-clients-8 | legacy-baseline | 27.193 ± 0.606 | 27.231 ± 0.592 |
| services-4-deep-16-clients-8 | portable-baseline | 32.356 ± 0.645 | 32.576 ± 0.604 |
| services-4-deep-16-clients-8 | cpp | 49.864 ± 0.393 | 50.344 ± 0.304 |
| bulk-1m | legacy | 28.553 ± 2.256 | 18.000 ± 0.000 |
| bulk-1m | portable | 30.360 ± 4.582 | 18.000 ± 0.000 |
| bulk-1m | legacy-baseline | 28.044 ± 1.180 | 18.000 ± 0.000 |
| bulk-1m | portable-baseline | 28.836 ± 2.751 | 18.000 ± 0.000 |
| bulk-1m | cpp | 258.000 ± 0.021 | 258.009 ± 0.020 |
