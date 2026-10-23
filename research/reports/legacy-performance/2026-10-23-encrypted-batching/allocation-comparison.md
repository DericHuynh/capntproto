# Allocation comparison

Cumulative allocation blocks per complete operation, summed across every peer.
Short/long DHAT subtraction; concurrent cases count a complete batch. C++ QUIC is unsupported.
A missing peer, trial, or required comparator leaves the target unmeasured. Values below are rounded only for display.

Target met in 27/48 cases; 21 gaps; 0 unmeasured.

| Transport | Workload | Frozen legacy | Legacy | Portable | C++ | Target | Excess | Status |
| --- | --- | ---: | ---: | ---: | ---: | --- | ---: | --- |
| tls | loopback-0 | 15.00 | 12.00 | 12.00 | 91.49 | portable | +0.00 | met |
| tls | loopback-64 | 15.00 | 12.00 | 12.00 | 91.49 | portable | +0.00 | met |
| tls | loopback-1024 | 15.00 | 12.00 | 12.00 | 91.49 | portable | +0.00 | met |
| tls | loopback-65536 | 47.00 | 44.00 | 54.00 | 262.78 | portable | -10.00 | met |
| tls | unary-empty | 15.00 | 12.00 | 12.00 | 91.49 | portable | +0.00 | met |
| tls | unary-small | 15.00 | 12.00 | 12.00 | 91.49 | portable | +0.00 | met |
| tls | unary-1k | 15.00 | 12.00 | 12.00 | 91.49 | portable | +0.00 | met |
| tls | fanout-32 | 350.00 | 316.00 | 372.00 | 2110.49 | portable | -56.00 | met |
| tls | dependent-read | 86.00 | 81.00 | 62.00 | 246.98 | portable | +19.00 | gap |
| tls | pipelined-read | 106.00 | 103.00 | 74.00 | 233.49 | portable | +29.00 | gap |
| tls | write-read | 31.00 | 25.00 | 25.00 | 180.98 | portable | +0.00 | met |
| tls | callback | 57.00 | 51.00 | 46.00 | 204.98 | portable | +5.00 | gap |
| tls | bulk-64k | 47.00 | 44.00 | 54.00 | 262.82 | portable | -10.00 | met |
| tls | bulk-256k | 120.82 | 119.82 | 128.09 | 685.53 | portable | -8.27 | met |
| tls | bulk-1m | 325.89 | 324.71 | 328.89 | 2374.96 | portable | -4.18 | met |
| tls | bulk-4m | 1090.42 | 1076.18 | 1196.69 | 8852.20 | portable | -120.51 | met |
| tls | deep-8 | 16.00 | 13.00 | 13.00 | 96.49 | portable | +0.00 | met |
| tls | deep-32 | 16.00 | 13.00 | 13.00 | 96.49 | portable | +0.00 | met |
| tls | concurrent-32 | 338.00 | 304.00 | 360.00 | 2062.49 | portable | -56.00 | met |
| tls | deep-32-concurrent-32 | 354.00 | 320.00 | 376.00 | 2146.49 | portable | -56.00 | met |
| tls | services-4-deep-16 | 94.44 | 82.47 | 73.00 | 362.02 | portable | +9.47 | gap |
| tls | services-4-deep-16-concurrent-32 | 2346.00 | 2210.02 | 2239.00 | 8303.98 | portable | -28.98 | met |
| tls | services-4-deep-16-clients-8 | 2484.78 | 2328.00 | 2253.93 | 8562.62 | portable | +74.07 | gap |
| tls | services-8-deep-16-clients-8 | 5211.91 | 4910.27 | 4794.91 | 16972.44 | portable | +115.36 | gap |
| quic | loopback-0 | 29.62 | 26.93 | 26.13 | — | portable | +0.80 | gap |
| quic | loopback-64 | 29.31 | 26.31 | 26.44 | — | portable | -0.13 | met |
| quic | loopback-1024 | 29.31 | 26.93 | 25.82 | — | portable | +1.11 | gap |
| quic | loopback-65536 | 122.13 | 117.64 | 124.38 | — | portable | -6.73 | met |
| quic | unary-empty | 29.16 | 26.49 | 26.13 | — | portable | +0.36 | gap |
| quic | unary-small | 29.51 | 26.49 | 25.80 | — | portable | +0.69 | gap |
| quic | unary-1k | 29.20 | 26.47 | 25.80 | — | portable | +0.67 | gap |
| quic | fanout-32 | 499.20 | 357.18 | 410.67 | — | portable | -53.49 | met |
| quic | dependent-read | 117.89 | 109.22 | 90.11 | — | portable | +19.11 | gap |
| quic | pipelined-read | 128.49 | 117.49 | 88.13 | — | portable | +29.36 | gap |
| quic | write-read | 59.22 | 53.18 | 53.13 | — | portable | +0.04 | gap |
| quic | callback | 87.51 | 79.20 | 74.13 | — | portable | +5.07 | gap |
| quic | bulk-64k | 122.09 | 117.11 | 124.38 | — | portable | -7.27 | met |
| quic | bulk-256k | 378.27 | 378.38 | 384.13 | — | portable | -5.76 | met |
| quic | bulk-1m | 1385.29 | 1380.67 | 1389.73 | — | portable | -9.07 | met |
| quic | bulk-4m | 5460.38 | 5456.16 | 5407.67 | — | portable | +48.49 | gap |
| quic | deep-8 | 30.96 | 27.93 | 27.47 | — | portable | +0.47 | gap |
| quic | deep-32 | 30.64 | 27.91 | 27.16 | — | portable | +0.76 | gap |
| quic | concurrent-32 | 473.27 | 316.64 | 371.84 | — | portable | -55.20 | met |
| quic | deep-32-concurrent-32 | 510.22 | 381.09 | 434.16 | — | portable | -53.07 | met |
| quic | services-4-deep-16 | 182.84 | 172.18 | 162.42 | — | portable | +9.76 | gap |
| quic | services-4-deep-16-concurrent-32 | 2953.20 | 2380.64 | 2407.58 | — | portable | -26.93 | met |
| quic | services-4-deep-16-clients-8 | 3210.62 | 2649.89 | 2592.49 | — | portable | +57.40 | gap |
| quic | services-8-deep-16-clients-8 | 6571.96 | 5465.51 | 5349.80 | — | portable | +115.71 | gap |

Allocation counts do not establish latency or peak-memory improvements.
