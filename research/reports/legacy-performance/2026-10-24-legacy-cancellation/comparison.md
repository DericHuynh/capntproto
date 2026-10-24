# Legacy factoring diagnostics

All peers; paired (50−10)/40 slopes, two warmups. Single diagnostic pairs, not latency evidence.

| Transport | Case | Metric | Frozen legacy | Factored legacy | Change |
| --- | --- | --- | ---: | ---: | ---: |
| tls | unary-small | Ir | 46,094.900 | 46,086.175 | -0.019% |
| tls | unary-small | Bcm | 337.875 | 335.675 | -0.651% |
| tls | unary-small | Bim | 177.875 | 186.025 | +4.582% |
| tls | dependent-read | Ir | 118,605.100 | 118,545.325 | -0.050% |
| tls | dependent-read | Bcm | 957.400 | 982.950 | +2.669% |
| tls | dependent-read | Bim | 417.050 | 440.075 | +5.521% |
| tls | pipelined-read | Ir | 97,201.875 | 97,163.200 | -0.040% |
| tls | pipelined-read | Bcm | 761.150 | 816.275 | +7.242% |
| tls | pipelined-read | Bim | 327.025 | 330.975 | +1.208% |
| tls | concurrent-32 | Ir | 670,466.025 | 670,415.075 | -0.008% |
| tls | concurrent-32 | Bcm | 1,949.725 | 2,164.375 | +11.009% |
| tls | concurrent-32 | Bim | 694.975 | 365.950 | -47.343% |
| tls | callback | Ir | 104,143.350 | 104,357.900 | +0.206% |
| tls | callback | Bcm | 925.875 | 918.350 | -0.813% |
| tls | callback | Bim | 489.975 | 483.975 | -1.225% |
| tls | deep-32-concurrent-32 | Ir | 3,542,617.200 | 3,542,374.875 | -0.007% |
| tls | deep-32-concurrent-32 | Bcm | 2,913.525 | 2,935.100 | +0.741% |
| tls | deep-32-concurrent-32 | Bim | 2,743.000 | 2,492.075 | -9.148% |
| tls | unary-small | blocks | 7.000 | 7.000 | +0.000% |
| tls | unary-small | bytes | 911.900 | 912.000 | +0.011% |
| tls | pipelined-read | blocks | 58.000 | 58.000 | +0.000% |
| tls | pipelined-read | bytes | 7,448.100 | 7,447.900 | -0.003% |
| tls | concurrent-32 | blocks | 202.000 | 202.000 | +0.000% |
| tls | concurrent-32 | bytes | 40,520.000 | 40,519.800 | -0.000% |
| quic | unary-small | blocks | 21.375 | 22.050 | +3.158% |
| quic | unary-small | bytes | 2,262.275 | 2,331.200 | +3.047% |
| quic | pipelined-read | blocks | 72.725 | 73.025 | +0.413% |
| quic | pipelined-read | bytes | 11,072.875 | 11,106.525 | +0.304% |
| quic | concurrent-32 | blocks | 219.425 | 219.075 | -0.160% |
| quic | concurrent-32 | bytes | 52,747.000 | 52,712.000 | -0.066% |
