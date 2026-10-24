# Local syscall diagnostics

Five paired 10/100-iteration captures, with two warmups in each. Counts include both peers; fixed handshake/teardown costs are subtracted. These instrumented timings are excluded from the latency comparison.

| Case | Implementation | Reads / operation | Writes / operation |
| --- | --- | ---: | ---: |
| dependent-read | legacy | 4.000 ± 0.000 | 4.000 ± 0.000 |
| dependent-read | portable | 4.000 ± 0.000 | 4.000 ± 0.000 |
| dependent-read | cpp | 4.004 ± 0.006 | 4.000 ± 0.000 |
| dependent-read | legacy-baseline | 4.000 ± 0.000 | 4.000 ± 0.000 |
| dependent-read | portable-baseline | 4.000 ± 0.000 | 4.000 ± 0.000 |
| pipelined-read | legacy | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | portable | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | cpp | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | legacy-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | portable-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | legacy | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | portable | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | cpp | 1.998 ± 0.005 | 2.000 ± 0.000 |
| concurrent-32 | legacy-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | portable-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| bulk-1m | legacy | 31.131 ± 6.554 | 18.000 ± 0.000 |
| bulk-1m | portable | 32.142 ± 7.782 | 18.000 ± 0.000 |
| bulk-1m | cpp | 258.000 ± 0.011 | 257.991 ± 0.012 |
| bulk-1m | legacy-baseline | 28.929 ± 3.120 | 18.000 ± 0.000 |
| bulk-1m | portable-baseline | 28.304 ± 2.487 | 18.000 ± 0.000 |
