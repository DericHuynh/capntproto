# Local syscall diagnostics

Five paired 10/100-iteration captures, with two warmups in each. Counts include both peers; fixed handshake/teardown costs are subtracted. These instrumented timings are excluded from the latency comparison.

| Case | Implementation | Reads / operation | Writes / operation |
| --- | --- | ---: | ---: |
| dependent-read | legacy | 4.000 ± 0.000 | 4.000 ± 0.000 |
| dependent-read | portable | 4.000 ± 0.000 | 4.000 ± 0.000 |
| dependent-read | cpp | 3.998 ± 0.005 | 4.000 ± 0.000 |
| dependent-read | legacy-baseline | 4.000 ± 0.000 | 4.000 ± 0.000 |
| dependent-read | portable-baseline | 4.000 ± 0.000 | 4.000 ± 0.000 |
| pipelined-read | legacy | 2.002 ± 0.005 | 2.000 ± 0.000 |
| pipelined-read | portable | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | cpp | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | legacy-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| pipelined-read | portable-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | legacy | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | portable | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | cpp | 2.000 ± 0.000 | 2.000 ± 0.000 |
| concurrent-32 | legacy-baseline | 2.000 ± 0.000 | 1.998 ± 0.005 |
| concurrent-32 | portable-baseline | 2.000 ± 0.000 | 2.000 ± 0.000 |
| bulk-1m | legacy | 32.242 ± 6.955 | 18.000 ± 0.000 |
| bulk-1m | portable | 28.493 ± 4.098 | 18.000 ± 0.000 |
| bulk-1m | cpp | 258.002 ± 0.012 | 258.004 ± 0.010 |
| bulk-1m | legacy-baseline | 27.360 ± 0.959 | 18.000 ± 0.000 |
| bulk-1m | portable-baseline | 27.549 ± 1.419 | 18.000 ± 0.000 |
