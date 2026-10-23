# Allocation comparison

Cumulative allocation blocks per complete operation, summed across every peer.
Short/long DHAT subtraction; concurrent cases count a complete batch. C++ QUIC is unsupported.
A missing peer, trial, or required comparator leaves the target unmeasured. Values below are rounded only for display.

Target met in 10/48 cases; 38 gaps; 0 unmeasured.

| Transport | Workload | Frozen legacy | Legacy | Portable | C++ | Target | Excess | Status |
| --- | --- | ---: | ---: | ---: | ---: | --- | ---: | --- |
| tls | loopback-0 | 28.00 | 15.00 | 12.00 | 91.49 | portable | +3.00 | gap |
| tls | loopback-64 | 28.00 | 15.00 | 12.00 | 91.49 | portable | +3.00 | gap |
| tls | loopback-1024 | 28.00 | 15.00 | 12.00 | 91.49 | portable | +3.00 | gap |
| tls | loopback-65536 | 58.00 | 47.00 | 56.00 | 261.22 | portable | -9.00 | met |
| tls | unary-empty | 28.00 | 15.00 | 12.00 | 91.49 | portable | +3.00 | gap |
| tls | unary-small | 28.00 | 15.00 | 12.00 | 91.49 | portable | +3.00 | gap |
| tls | unary-1k | 28.00 | 15.00 | 12.00 | 91.49 | portable | +3.00 | gap |
| tls | fanout-32 | 766.00 | 350.00 | 374.00 | 2110.49 | portable | -24.00 | met |
| tls | dependent-read | 118.00 | 86.00 | 64.00 | 246.98 | portable | +22.00 | gap |
| tls | pipelined-read | 135.00 | 106.00 | 76.00 | 233.49 | portable | +30.00 | gap |
| tls | write-read | 57.00 | 31.00 | 25.00 | 180.98 | portable | +6.00 | gap |
| tls | callback | 87.00 | 57.00 | 47.00 | 204.98 | portable | +10.00 | gap |
| tls | bulk-64k | 58.00 | 47.00 | 56.00 | 262.38 | portable | -9.00 | met |
| tls | bulk-256k | 132.00 | 120.91 | 128.18 | 684.78 | portable | -7.27 | met |
| tls | bulk-1m | 335.73 | 325.53 | 321.96 | 2415.62 | portable | +3.58 | gap |
| tls | bulk-4m | 1195.02 | 1153.18 | 1202.82 | 8905.02 | portable | -49.64 | met |
| tls | deep-8 | 29.00 | 16.00 | 13.00 | 96.49 | portable | +3.00 | gap |
| tls | deep-32 | 29.00 | 16.00 | 13.00 | 96.49 | portable | +3.00 | gap |
| tls | concurrent-32 | 754.00 | 338.00 | 362.00 | 2062.49 | portable | -24.00 | met |
| tls | deep-32-concurrent-32 | 770.00 | 354.00 | 378.00 | 2146.49 | portable | -24.00 | met |
| tls | services-4-deep-16 | 155.00 | 93.38 | 73.00 | 360.67 | portable | +20.38 | gap |
| tls | services-4-deep-16-concurrent-32 | 4490.00 | 2538.51 | 2343.00 | 8303.98 | portable | +195.51 | gap |
| tls | services-4-deep-16-clients-8 | 4563.09 | 2612.00 | 2344.60 | 8564.47 | portable | +267.40 | gap |
| tls | services-8-deep-16-clients-8 | 9593.67 | 5613.56 | 5004.60 | 16943.09 | portable | +608.96 | gap |
| quic | loopback-0 | 42.64 | 29.33 | 26.11 | — | portable | +3.22 | gap |
| quic | loopback-64 | 42.29 | 29.93 | 26.16 | — | portable | +3.78 | gap |
| quic | loopback-1024 | 42.93 | 29.62 | 25.80 | — | portable | +3.82 | gap |
| quic | loopback-65536 | 133.62 | 122.33 | 128.40 | — | portable | -6.07 | met |
| quic | unary-empty | 42.47 | 29.64 | 25.80 | — | portable | +3.84 | gap |
| quic | unary-small | 42.47 | 29.80 | 26.11 | — | portable | +3.69 | gap |
| quic | unary-1k | 42.49 | 29.82 | 25.82 | — | portable | +4.00 | gap |
| quic | fanout-32 | 914.87 | 499.22 | 456.40 | — | portable | +42.82 | gap |
| quic | dependent-read | 150.22 | 118.18 | 92.13 | — | portable | +26.04 | gap |
| quic | pipelined-read | 157.49 | 128.47 | 92.42 | — | portable | +36.04 | gap |
| quic | write-read | 85.20 | 59.51 | 53.16 | — | portable | +6.36 | gap |
| quic | callback | 117.53 | 87.20 | 75.44 | — | portable | +11.76 | gap |
| quic | bulk-64k | 133.07 | 122.42 | 128.07 | — | portable | -5.64 | met |
| quic | bulk-256k | 389.02 | 376.38 | 383.51 | — | portable | -7.13 | met |
| quic | bulk-1m | 1395.64 | 1405.09 | 1383.87 | — | portable | +21.22 | gap |
| quic | bulk-4m | 5468.02 | 5440.56 | 5377.89 | — | portable | +62.67 | gap |
| quic | deep-8 | 43.93 | 30.96 | 27.13 | — | portable | +3.82 | gap |
| quic | deep-32 | 43.62 | 30.93 | 27.47 | — | portable | +3.47 | gap |
| quic | concurrent-32 | 888.98 | 472.96 | 432.13 | — | portable | +40.82 | gap |
| quic | deep-32-concurrent-32 | 926.24 | 510.27 | 467.44 | — | portable | +42.82 | gap |
| quic | services-4-deep-16 | 244.04 | 183.62 | 162.44 | — | portable | +21.18 | gap |
| quic | services-4-deep-16-concurrent-32 | 5097.16 | 3144.62 | 2691.24 | — | portable | +453.38 | gap |
| quic | services-4-deep-16-clients-8 | 5298.18 | 3337.18 | 2836.04 | — | portable | +501.13 | gap |
| quic | services-8-deep-16-clients-8 | 10984.44 | 6925.16 | 5903.87 | — | portable | +1021.29 | gap |

Allocation counts do not establish latency or peak-memory improvements.
