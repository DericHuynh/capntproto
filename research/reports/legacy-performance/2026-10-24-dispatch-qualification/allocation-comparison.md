# Allocation comparison

Cumulative allocation blocks per complete operation, summed across every peer.
Short/long DHAT subtraction; concurrent cases count a complete batch. C++ QUIC is unsupported.
A missing peer, trial, or required comparator leaves the target unmeasured. Values below are rounded only for display.

Target met in 44/48 cases; 4 gaps; 0 unmeasured.

| Transport | Workload | Frozen legacy | Legacy | Portable | C++ | Target | Excess | Status |
| --- | --- | ---: | ---: | ---: | ---: | --- | ---: | --- |
| tls | loopback-0 | 12.00 | 10.00 | 12.00 | 91.49 | portable | -2.00 | met |
| tls | loopback-64 | 12.00 | 10.00 | 12.00 | 91.49 | portable | -2.00 | met |
| tls | loopback-1024 | 12.00 | 10.00 | 12.00 | 91.49 | portable | -2.00 | met |
| tls | loopback-65536 | 44.00 | 42.00 | 54.00 | 263.89 | portable | -12.00 | met |
| tls | unary-empty | 12.00 | 10.00 | 12.00 | 91.49 | portable | -2.00 | met |
| tls | unary-small | 12.00 | 10.00 | 12.00 | 91.49 | portable | -2.00 | met |
| tls | unary-1k | 12.00 | 10.00 | 12.00 | 91.49 | portable | -2.00 | met |
| tls | fanout-32 | 316.00 | 252.00 | 372.00 | 2110.49 | portable | -120.00 | met |
| tls | dependent-read | 81.00 | 53.00 | 57.00 | 246.98 | portable | -4.00 | met |
| tls | pipelined-read | 103.00 | 65.00 | 69.00 | 233.49 | portable | -4.00 | met |
| tls | write-read | 25.00 | 21.00 | 25.00 | 180.98 | portable | -4.00 | met |
| tls | callback | 51.00 | 43.00 | 46.00 | 204.98 | portable | -3.00 | met |
| tls | bulk-64k | 44.00 | 42.00 | 54.00 | 264.29 | portable | -12.00 | met |
| tls | bulk-256k | 116.98 | 117.47 | 124.09 | 693.44 | portable | -6.62 | met |
| tls | bulk-1m | 320.62 | 348.40 | 321.60 | 2382.42 | portable | +26.80 | gap |
| tls | bulk-4m | 1164.27 | 1144.04 | 1094.76 | 8920.33 | portable | +49.29 | gap |
| tls | deep-8 | 13.00 | 11.00 | 13.00 | 96.49 | portable | -2.00 | met |
| tls | deep-32 | 13.00 | 11.00 | 13.00 | 96.49 | portable | -2.00 | met |
| tls | concurrent-32 | 304.00 | 240.00 | 360.00 | 2062.49 | portable | -120.00 | met |
| tls | deep-32-concurrent-32 | 320.00 | 256.00 | 376.00 | 2146.49 | portable | -120.00 | met |
| tls | services-4-deep-16 | 81.62 | 68.00 | 73.00 | 361.98 | portable | -5.00 | met |
| tls | services-4-deep-16-concurrent-32 | 2210.11 | 1762.00 | 2239.00 | 8303.98 | portable | -477.00 | met |
| tls | services-4-deep-16-clients-8 | 2327.60 | 1881.18 | 2255.62 | 8547.78 | portable | -374.44 | met |
| tls | services-8-deep-16-clients-8 | 4911.73 | 3951.73 | 4782.53 | 16915.47 | portable | -830.80 | met |
| quic | loopback-0 | 26.60 | 24.64 | 25.84 | — | portable | -1.20 | met |
| quic | loopback-64 | 26.62 | 24.62 | 25.82 | — | portable | -1.20 | met |
| quic | loopback-1024 | 26.96 | 24.31 | 26.13 | — | portable | -1.82 | met |
| quic | loopback-65536 | 117.64 | 115.64 | 124.40 | — | portable | -8.76 | met |
| quic | unary-empty | 26.64 | 24.62 | 26.73 | — | portable | -2.11 | met |
| quic | unary-small | 26.47 | 24.60 | 26.13 | — | portable | -1.53 | met |
| quic | unary-1k | 26.29 | 24.93 | 26.47 | — | portable | -1.53 | met |
| quic | fanout-32 | 357.22 | 293.47 | 410.36 | — | portable | -116.89 | met |
| quic | dependent-read | 109.20 | 81.51 | 84.84 | — | portable | -3.33 | met |
| quic | pipelined-read | 117.18 | 79.62 | 82.84 | — | portable | -3.22 | met |
| quic | write-read | 53.22 | 48.87 | 52.82 | — | portable | -3.96 | met |
| quic | callback | 78.87 | 71.49 | 74.16 | — | portable | -2.67 | met |
| quic | bulk-64k | 117.09 | 114.80 | 124.38 | — | portable | -9.58 | met |
| quic | bulk-256k | 373.82 | 372.18 | 379.20 | — | portable | -7.02 | met |
| quic | bulk-1m | 1376.60 | 1392.93 | 1382.53 | — | portable | +10.40 | gap |
| quic | bulk-4m | 5471.20 | 5521.87 | 5428.42 | — | portable | +93.44 | gap |
| quic | deep-8 | 27.62 | 25.93 | 27.44 | — | portable | -1.51 | met |
| quic | deep-32 | 27.62 | 25.62 | 27.47 | — | portable | -1.84 | met |
| quic | concurrent-32 | 316.49 | 252.98 | 372.13 | — | portable | -119.16 | met |
| quic | deep-32-concurrent-32 | 381.27 | 317.16 | 434.76 | — | portable | -117.60 | met |
| quic | services-4-deep-16 | 171.27 | 156.49 | 162.09 | — | portable | -5.60 | met |
| quic | services-4-deep-16-concurrent-32 | 2381.02 | 1933.22 | 2407.00 | — | portable | -473.78 | met |
| quic | services-4-deep-16-clients-8 | 2634.40 | 2197.13 | 2593.36 | — | portable | -396.22 | met |
| quic | services-8-deep-16-clients-8 | 5476.18 | 4449.64 | 5340.00 | — | portable | -890.36 | met |

Allocation counts do not establish latency or peak-memory improvements.
