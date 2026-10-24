# Allocation regression checks

Whole operation, all peers: paired 10/50-operation DHAT slopes with two identical warmups. Single diagnostic pair, not repeated latency or QUIC allocation qualification.

| Transport | Case | Implementation | Allocations / operation | Bytes / operation |
| --- | --- | --- | ---: | ---: |
| tls | unary-small | legacy | 7.000 | 911.900 |
| tls | unary-small | portable | 7.000 | 952.100 |
| tls | unary-small | portable-baseline | 7.000 | 952.200 |
| tls | unary-small | cpp | 91.500 | 64937.950 |
| tls | pipelined-read | legacy | 58.000 | 7447.800 |
| tls | pipelined-read | portable | 58.000 | 8228.000 |
| tls | pipelined-read | portable-baseline | 58.000 | 8227.800 |
| tls | pipelined-read | cpp | 233.500 | 155455.000 |
| tls | concurrent-32 | legacy | 202.000 | 40520.100 |
| tls | concurrent-32 | portable | 198.000 | 37704.000 |
| tls | concurrent-32 | portable-baseline | 198.000 | 37704.000 |
| tls | concurrent-32 | cpp | 2062.500 | 1585407.950 |
| quic | unary-small | legacy | 21.325 | 2261.125 |
| quic | unary-small | portable | 21.125 | 2244.125 |
| quic | unary-small | portable-baseline | 20.800 | 2210.000 |
| quic | pipelined-read | legacy | 73.050 | 11107.200 |
| quic | pipelined-read | portable | 71.800 | 11726.000 |
| quic | pipelined-read | portable-baseline | 72.500 | 11795.600 |
| quic | concurrent-32 | legacy | 219.425 | 52746.800 |
| quic | concurrent-32 | portable | 214.175 | 49693.475 |
| quic | concurrent-32 | portable-baseline | 214.475 | 49726.725 |
