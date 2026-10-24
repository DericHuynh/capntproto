# Marginal protocol work

Both peers summed. Small cases: paired 10/50 operations, 2 warmups; bulk: 2/8 operations, 1 warmup. Instructions are Callgrind simulation; allocations are cumulative DHAT blocks. Instrumented latency is excluded.

| Case | Driver | Before instructions | Direct instructions | Change | Before memcpy instructions | Direct memcpy instructions | Before / direct allocation blocks |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| unary-small | legacy | 46,446 | 46,389 | -0.12% | 1,453 | 1,453 | 7.00 / 7.00 |
| unary-small | portable | 47,249 | 47,073 | -0.37% | 1,667 | 1,600 | 10.00 / 10.00 |
| pipelined-read | legacy | 96,282 | 96,246 | -0.04% | 4,092 | 4,092 | 58.00 / 58.00 |
| pipelined-read | portable | 146,862 | 146,872 | +0.01% | 5,425 | 5,461 | 67.00 / 67.00 |
| bulk-1m | legacy | 18,069,546 | 18,068,827 | -0.00% | 6,585,005 | 6,584,707 | 145.00 / 145.00 |
| bulk-1m | portable | 18,113,539 | 18,107,376 | -0.03% | 6,615,753 | 6,614,812 | 156.00 / 156.00 |
