# Legacy factoring diagnostics

All peers; paired (50−10)/40 slopes, two warmups. Single diagnostic pairs, not latency evidence.

| Transport | Case | Metric | Frozen legacy | Factored legacy | Change |
| --- | --- | --- | ---: | ---: | ---: |
| tls | unary-small | Ir | 46,139.075 | 46,168.675 | +0.064% |
| tls | unary-small | Bcm | 306.225 | 300.825 | -1.763% |
| tls | unary-small | Bim | 169.050 | 164.050 | -2.958% |
| tls | dependent-read | Ir | 118,527.325 | 118,547.825 | +0.017% |
| tls | dependent-read | Bcm | 851.250 | 909.250 | +6.814% |
| tls | dependent-read | Bim | 412.025 | 403.975 | -1.954% |
| tls | pipelined-read | Ir | 97,102.025 | 97,157.650 | +0.057% |
| tls | pipelined-read | Bcm | 726.100 | 773.075 | +6.469% |
| tls | pipelined-read | Bim | 312.950 | 316.025 | +0.983% |
| tls | concurrent-32 | Ir | 670,373.450 | 670,518.325 | +0.022% |
| tls | concurrent-32 | Bcm | 1,796.525 | 1,813.800 | +0.962% |
| tls | concurrent-32 | Bim | 782.950 | 554.075 | -29.232% |
| tls | callback | Ir | 104,301.000 | 104,307.975 | +0.007% |
| tls | callback | Bcm | 812.650 | 863.525 | +6.260% |
| tls | callback | Bim | 474.150 | 447.000 | -5.726% |
| tls | deep-32-concurrent-32 | Ir | 3,542,522.550 | 3,542,442.775 | -0.002% |
| tls | deep-32-concurrent-32 | Bcm | 2,824.525 | 2,860.300 | +1.267% |
| tls | deep-32-concurrent-32 | Bim | 2,876.075 | 2,604.975 | -9.426% |
| tls | unary-small | blocks | 7.000 | 7.000 | +0.000% |
| tls | unary-small | bytes | 911.800 | 912.100 | +0.033% |
| tls | pipelined-read | blocks | 58.000 | 58.000 | +0.000% |
| tls | pipelined-read | bytes | 7,448.000 | 7,447.900 | -0.001% |
| tls | concurrent-32 | blocks | 202.000 | 202.000 | +0.000% |
| tls | concurrent-32 | bytes | 40,519.800 | 40,520.000 | +0.000% |
| quic | unary-small | blocks | 21.325 | 22.050 | +3.400% |
| quic | unary-small | bytes | 2,261.125 | 2,330.600 | +3.073% |
| quic | pipelined-read | blocks | 73.050 | 72.700 | -0.479% |
| quic | pipelined-read | bytes | 11,106.600 | 11,071.800 | -0.313% |
| quic | concurrent-32 | blocks | 218.700 | 218.700 | +0.000% |
| quic | concurrent-32 | bytes | 52,676.125 | 52,676.125 | +0.000% |
