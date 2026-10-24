# Allocation regression checks

Whole operation, all peers: paired 10/50-operation DHAT slopes with two identical warmups. Single diagnostic pair, not repeated latency or QUIC allocation qualification.

| Transport | Case | Implementation | Allocations / operation | Bytes / operation |
| --- | --- | --- | ---: | ---: |
| tls | unary-small | legacy | 7.000 | 912.000 |
| tls | unary-small | portable | 7.000 | 968.100 |
| tls | unary-small | portable-baseline | 7.000 | 952.200 |
| tls | unary-small | cpp | 91.500 | 64938.025 |
| tls | pipelined-read | legacy | 58.000 | 7448.100 |
| tls | pipelined-read | portable | 58.000 | 8324.100 |
| tls | pipelined-read | portable-baseline | 58.000 | 8228.000 |
| tls | pipelined-read | cpp | 233.500 | 155454.925 |
| tls | concurrent-32 | legacy | 202.000 | 40520.000 |
| tls | concurrent-32 | portable | 198.000 | 38215.900 |
| tls | concurrent-32 | portable-baseline | 198.000 | 37703.800 |
| tls | concurrent-32 | cpp | 2062.500 | 1585408.000 |
| quic | unary-small | legacy | 21.725 | 2297.075 |
| quic | unary-small | portable | 21.500 | 2295.600 |
| quic | unary-small | portable-baseline | 21.150 | 2244.800 |
| quic | pipelined-read | legacy | 73.050 | 11107.800 |
| quic | pipelined-read | portable | 72.175 | 11857.475 |
| quic | pipelined-read | portable-baseline | 72.500 | 11795.000 |
| quic | concurrent-32 | legacy | 218.725 | 52677.200 |
| quic | concurrent-32 | portable | 214.125 | 50204.125 |
| quic | concurrent-32 | portable-baseline | 214.150 | 49693.075 |
