# Local syscall diagnostics

Five paired 10/100-iteration captures, with two warmups in each. Counts include both peers; fixed handshake/teardown costs are subtracted. These instrumented timings are excluded from the latency comparison.

| Case | Implementation | Reads / operation | Writes / operation |
| --- | --- | ---: | ---: |
| dependent-read | legacy | 4.000 ± 0.000 | 4.000 ± 0.000 |
| dependent-read | portable | 4.000 ± 0.000 | 4.000 ± 0.000 |
| dependent-read | cpp | 4.007 ± 0.006 | 4.000 ± 0.000 |
| dependent-read | legacy-baseline | 4.000 ± 0.000 | 4.000 ± 0.000 |
| dependent-read | portable-baseline | 4.000 ± 0.000 | 4.000 ± 0.000 |
| pipelined-read | legacy | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | portable | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | cpp | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | legacy-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | portable-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | legacy | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | portable | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | cpp | 2.004 ± 0.013 | 2.000 ± 0.000 |
| concurrent-32 | legacy-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | portable-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| bulk-1m | legacy | 32.280 ± 6.621 | 18.000 ± 0.000 |
| bulk-1m | portable | 35.169 ± 6.937 | 18.000 ± 0.000 |
| bulk-1m | cpp | 258.056 ± 0.094 | 258.000 ± 0.000 |
| bulk-1m | legacy-baseline | 35.360 ± 7.179 | 18.000 ± 0.000 |
| bulk-1m | portable-baseline | 36.824 ± 6.374 | 18.000 ± 0.000 |
