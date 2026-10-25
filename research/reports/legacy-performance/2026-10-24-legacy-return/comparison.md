# Legacy factoring diagnostics

All peers; paired (50−10)/40 slopes, two warmups. Single diagnostic pairs, not latency evidence.

| Transport | Case | Metric | Frozen legacy | Factored legacy | Change |
| --- | --- | --- | ---: | ---: | ---: |
| tls | unary-small | Ir | 46,142.900 | 46,202.850 | +0.130% |
| tls | unary-small | Bcm | 323.900 | 326.650 | +0.849% |
| tls | unary-small | Bim | 166.050 | 187.050 | +12.647% |
| tls | dependent-read | Ir | 118,540.850 | 118,642.600 | +0.086% |
| tls | dependent-read | Bcm | 926.700 | 964.425 | +4.071% |
| tls | dependent-read | Bim | 397.900 | 444.025 | +11.592% |
| tls | pipelined-read | Ir | 97,153.925 | 97,160.850 | +0.007% |
| tls | pipelined-read | Bcm | 772.125 | 800.200 | +3.636% |
| tls | pipelined-read | Bim | 331.975 | 351.950 | +6.017% |
| tls | concurrent-32 | Ir | 670,453.900 | 670,402.500 | -0.008% |
| tls | concurrent-32 | Bcm | 2,222.650 | 2,103.825 | -5.346% |
| tls | concurrent-32 | Bim | 554.950 | 347.975 | -37.296% |
| tls | callback | Ir | 104,248.400 | 104,245.700 | -0.003% |
| tls | callback | Bcm | 863.500 | 895.950 | +3.758% |
| tls | callback | Bim | 449.025 | 501.000 | +11.575% |
| tls | deep-32-concurrent-32 | Ir | 3,542,616.175 | 3,542,428.400 | -0.005% |
| tls | deep-32-concurrent-32 | Bcm | 4,112.600 | 4,199.625 | +2.116% |
| tls | deep-32-concurrent-32 | Bim | 2,582.925 | 2,379.025 | -7.894% |
| tls | unary-small | blocks | 7.000 | 7.000 | +0.000% |
| tls | unary-small | bytes | 911.800 | 912.000 | +0.022% |
| tls | pipelined-read | blocks | 58.000 | 58.000 | +0.000% |
| tls | pipelined-read | bytes | 7,448.200 | 7,448.200 | +0.000% |
| tls | concurrent-32 | blocks | 202.000 | 202.000 | +0.000% |
| tls | concurrent-32 | bytes | 40,520.000 | 40,520.100 | +0.000% |
| quic | unary-small | blocks | 21.450 | 21.850 | +1.865% |
| quic | unary-small | bytes | 2,279.925 | 2,316.075 | +1.586% |
| quic | pipelined-read | blocks | 72.850 | 72.150 | -0.961% |
| quic | pipelined-read | bytes | 11,092.275 | 11,022.675 | -0.627% |
| quic | concurrent-32 | blocks | 218.700 | 219.025 | +0.149% |
| quic | concurrent-32 | bytes | 52,588.675 | 52,623.200 | +0.066% |
