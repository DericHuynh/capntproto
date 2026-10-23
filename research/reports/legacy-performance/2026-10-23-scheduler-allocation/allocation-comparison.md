# Allocation comparison

Cumulative allocation blocks per complete operation, summed across every peer.
Short/long DHAT subtraction; concurrent cases count a complete batch. C++ QUIC is unsupported.
A missing peer, trial, or required comparator leaves the target unmeasured. Values below are rounded only for display.

Target met in 12/48 cases; 36 gaps; 0 unmeasured.

| Transport | Workload | Frozen legacy | Legacy | Portable | C++ | Target | Excess | Status |
| --- | --- | ---: | ---: | ---: | ---: | --- | ---: | --- |
| tls | loopback-0 | 15.00 | 15.00 | 12.00 | 91.49 | portable | +3.00 | gap |
| tls | loopback-64 | 15.00 | 15.00 | 12.00 | 91.49 | portable | +3.00 | gap |
| tls | loopback-1024 | 15.00 | 15.00 | 12.00 | 91.49 | portable | +3.00 | gap |
| tls | loopback-65536 | 47.00 | 47.00 | 56.00 | 262.51 | portable | -9.00 | met |
| tls | unary-empty | 15.00 | 15.00 | 12.00 | 91.49 | portable | +3.00 | gap |
| tls | unary-small | 15.00 | 15.00 | 12.00 | 91.49 | portable | +3.00 | gap |
| tls | unary-1k | 15.00 | 15.00 | 12.00 | 91.49 | portable | +3.00 | gap |
| tls | fanout-32 | 350.00 | 350.00 | 374.00 | 2110.53 | portable | -24.00 | met |
| tls | dependent-read | 86.00 | 86.00 | 64.00 | 246.98 | portable | +22.00 | gap |
| tls | pipelined-read | 106.00 | 106.00 | 76.00 | 233.49 | portable | +30.00 | gap |
| tls | write-read | 31.00 | 31.00 | 25.00 | 180.98 | portable | +6.00 | gap |
| tls | callback | 57.00 | 57.00 | 47.00 | 204.98 | portable | +10.00 | gap |
| tls | bulk-64k | 47.00 | 47.00 | 56.00 | 263.27 | portable | -9.00 | met |
| tls | bulk-256k | 120.56 | 121.00 | 127.02 | 674.60 | portable | -6.02 | met |
| tls | bulk-1m | 316.11 | 317.71 | 333.42 | 2318.47 | portable | -15.71 | met |
| tls | bulk-4m | 1066.87 | 1061.62 | 1176.07 | 8887.78 | portable | -114.44 | met |
| tls | deep-8 | 16.00 | 16.00 | 13.00 | 96.49 | portable | +3.00 | gap |
| tls | deep-32 | 16.00 | 16.00 | 13.00 | 96.49 | portable | +3.00 | gap |
| tls | concurrent-32 | 338.00 | 338.00 | 362.00 | 2062.49 | portable | -24.00 | met |
| tls | deep-32-concurrent-32 | 354.00 | 354.00 | 378.00 | 2146.58 | portable | -24.00 | met |
| tls | services-4-deep-16 | 94.84 | 94.00 | 73.00 | 361.98 | portable | +21.00 | gap |
| tls | services-4-deep-16-concurrent-32 | 2538.00 | 2346.00 | 2247.00 | 8303.98 | portable | +99.00 | gap |
| tls | services-4-deep-16-clients-8 | 2611.09 | 2484.91 | 2291.33 | 8571.56 | portable | +193.58 | gap |
| tls | services-8-deep-16-clients-8 | 5588.47 | 5202.80 | 4822.11 | 16928.00 | portable | +380.69 | gap |
| quic | loopback-0 | 29.93 | 29.93 | 26.13 | — | portable | +3.80 | gap |
| quic | loopback-64 | 29.91 | 29.31 | 26.13 | — | portable | +3.18 | gap |
| quic | loopback-1024 | 29.33 | 29.96 | 26.44 | — | portable | +3.51 | gap |
| quic | loopback-65536 | 122.04 | 122.64 | 128.09 | — | portable | -5.44 | met |
| quic | unary-empty | 29.16 | 29.47 | 25.82 | — | portable | +3.64 | gap |
| quic | unary-small | 29.51 | 29.20 | 26.13 | — | portable | +3.07 | gap |
| quic | unary-1k | 29.80 | 29.80 | 26.11 | — | portable | +3.69 | gap |
| quic | fanout-32 | 499.22 | 499.51 | 456.09 | — | portable | +43.42 | gap |
| quic | dependent-read | 117.89 | 118.22 | 92.47 | — | portable | +25.76 | gap |
| quic | pipelined-read | 128.47 | 128.49 | 92.16 | — | portable | +36.33 | gap |
| quic | write-read | 58.87 | 59.53 | 53.42 | — | portable | +6.11 | gap |
| quic | callback | 87.53 | 87.20 | 75.13 | — | portable | +12.07 | gap |
| quic | bulk-64k | 122.40 | 121.76 | 128.38 | — | portable | -6.62 | met |
| quic | bulk-256k | 373.64 | 373.73 | 383.13 | — | portable | -9.40 | met |
| quic | bulk-1m | 1377.00 | 1367.71 | 1375.67 | — | portable | -7.96 | met |
| quic | bulk-4m | 5407.58 | 5429.76 | 5396.27 | — | portable | +33.49 | gap |
| quic | deep-8 | 30.64 | 30.64 | 26.84 | — | portable | +3.80 | gap |
| quic | deep-32 | 30.64 | 30.62 | 27.16 | — | portable | +3.47 | gap |
| quic | concurrent-32 | 472.64 | 472.96 | 432.42 | — | portable | +40.53 | gap |
| quic | deep-32-concurrent-32 | 509.93 | 509.96 | 467.76 | — | portable | +42.20 | gap |
| quic | services-4-deep-16 | 183.82 | 184.20 | 161.84 | — | portable | +22.36 | gap |
| quic | services-4-deep-16-concurrent-32 | 3144.64 | 2952.36 | 2595.64 | — | portable | +356.71 | gap |
| quic | services-4-deep-16-clients-8 | 3306.64 | 3202.04 | 2746.60 | — | portable | +455.44 | gap |
| quic | services-8-deep-16-clients-8 | 6855.42 | 6449.22 | 5662.80 | — | portable | +786.42 | gap |

Allocation counts do not establish latency or peak-memory improvements.
