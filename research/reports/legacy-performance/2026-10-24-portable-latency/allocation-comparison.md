# Allocation regression checks

Whole operation, all peers: paired 10/50-operation DHAT slopes with two identical warmups. Single diagnostic pair, not repeated latency or QUIC allocation qualification.

| Transport | Case | Implementation | Allocations / operation | Bytes / operation |
| --- | --- | --- | ---: | ---: |
| tls | unary-small | legacy | 7.000 | 911.900 |
| tls | unary-small | portable | 7.000 | 951.900 |
| tls | unary-small | portable-baseline | 7.000 | 952.100 |
| tls | unary-small | cpp | 91.500 | 64938.050 |
| tls | pipelined-read | legacy | 58.000 | 7448.200 |
| tls | pipelined-read | portable | 58.000 | 8228.000 |
| tls | pipelined-read | portable-baseline | 58.000 | 8227.900 |
| tls | pipelined-read | cpp | 233.500 | 155454.975 |
| tls | concurrent-32 | legacy | 202.000 | 40520.200 |
| tls | concurrent-32 | portable | 198.000 | 37704.000 |
| tls | concurrent-32 | portable-baseline | 198.000 | 37704.000 |
| tls | concurrent-32 | cpp | 2062.500 | 1585407.975 |
| quic | unary-small | legacy | 21.700 | 2296.400 |
| quic | unary-small | portable | 20.800 | 2210.000 |
| quic | unary-small | portable-baseline | 21.150 | 2245.000 |
| quic | pipelined-read | legacy | 73.025 | 11106.725 |
| quic | pipelined-read | portable | 71.800 | 11726.200 |
| quic | pipelined-read | portable-baseline | 72.125 | 11760.125 |
| quic | concurrent-32 | legacy | 218.725 | 52677.000 |
| quic | concurrent-32 | portable | 214.525 | 49728.275 |
| quic | concurrent-32 | portable-baseline | 213.775 | 49657.525 |
