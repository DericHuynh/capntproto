# Local syscall diagnostics

Five paired 10/100-iteration captures, with two warmups in each. Counts include both peers; fixed handshake/teardown costs are subtracted. These instrumented timings are excluded from the latency comparison.

| Case | Implementation | Reads / operation | Writes / operation |
| --- | --- | ---: | ---: |
| dependent-read | legacy | 4.000 ± 0.000 | 4.000 ± 0.000 |
| dependent-read | portable | 4.000 ± 0.000 | 4.000 ± 0.000 |
| dependent-read | cpp | 4.002 ± 0.005 | 4.000 ± 0.000 |
| dependent-read | legacy-baseline | 4.000 ± 0.000 | 4.000 ± 0.000 |
| dependent-read | portable-baseline | 4.000 ± 0.000 | 4.000 ± 0.000 |
| pipelined-read | legacy | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | portable | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | cpp | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | legacy-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | portable-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | legacy | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | portable | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | cpp | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | legacy-baseline | 4.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | portable-baseline | 4.000 ± 0.000 | 2.000 ± 0.000 |
| bulk-1m | legacy | 35.533 ± 4.164 | 18.000 ± 0.000 |
| bulk-1m | portable | 29.553 ± 5.234 | 18.000 ± 0.000 |
| bulk-1m | cpp | 258.004 ± 0.019 | 258.000 ± 0.022 |
| bulk-1m | legacy-baseline | 140.807 ± 2.983 | 34.000 ± 0.000 |
| bulk-1m | portable-baseline | 137.244 ± 2.381 | 34.000 ± 0.000 |
