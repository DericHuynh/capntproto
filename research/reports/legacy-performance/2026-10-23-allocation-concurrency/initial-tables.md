## Native latency

Mean ± sample SD of the five repetition means, in microseconds. Negative change means lower latency. C++ QUIC is unsupported.

| Transport | Case | Frozen legacy | Candidate legacy | Current portable | C++ | Legacy change |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| tls | loopback-0 | 31.514 ± 0.205 | 31.805 ± 0.554 | 32.792 ± 0.319 | 46.260 ± 0.447 | +0.92% |
| tls | loopback-64 | 31.524 ± 0.203 | 32.068 ± 0.890 | 32.878 ± 0.215 | 46.736 ± 0.609 | +1.73% |
| tls | loopback-1024 | 33.742 ± 0.217 | 34.329 ± 0.355 | 35.347 ± 0.214 | 48.842 ± 0.340 | +1.74% |
| tls | loopback-65536 | 162.813 ± 1.971 | 161.356 ± 0.910 | 163.147 ± 1.195 | 206.165 ± 1.278 | -0.90% |
| tls | unary-empty | 31.199 ± 0.137 | 31.411 ± 0.162 | 32.566 ± 0.240 | 46.367 ± 0.216 | +0.68% |
| tls | unary-small | 31.643 ± 0.477 | 31.619 ± 0.578 | 33.549 ± 0.746 | 46.813 ± 1.063 | -0.07% |
| tls | unary-1k | 34.005 ± 0.587 | 33.962 ± 0.209 | 35.358 ± 0.377 | 48.936 ± 0.487 | -0.13% |
| tls | fanout-32 | 181.770 ± 1.278 | 180.594 ± 1.170 | 214.029 ± 1.056 | 430.711 ± 9.206 | -0.65% |
| tls | dependent-read | 82.097 ± 19.498 | 83.133 ± 19.826 | 99.176 ± 17.443 | 109.135 ± 0.500 | +1.26% |
| tls | pipelined-read | 47.787 ± 0.089 | 48.325 ± 0.207 | 69.774 ± 0.715 | 79.040 ± 0.577 | +1.13% |
| tls | write-read | 66.725 ± 5.156 | 66.647 ± 2.995 | 67.316 ± 0.592 | 104.326 ± 22.755 | -0.12% |
| tls | callback | 93.158 ± 21.717 | 94.896 ± 22.914 | 100.482 ± 24.578 | 122.799 ± 32.267 | +1.87% |
| tls | bulk-64k | 161.448 ± 1.065 | 161.766 ± 1.877 | 164.736 ± 3.621 | 220.383 ± 35.004 | +0.20% |
| tls | bulk-256k | 432.991 ± 1.235 | 444.711 ± 22.350 | 455.336 ± 27.346 | 798.468 ± 3.586 | +2.71% |
| tls | bulk-1m | 1600.828 ± 8.897 | 1647.277 ± 68.583 | 1608.080 ± 10.873 | 3053.521 ± 15.753 | +2.90% |
| tls | bulk-4m | 7608.448 ± 153.101 | 7608.102 ± 154.398 | 7642.650 ± 115.128 | 13247.288 ± 230.170 | -0.00% |
| tls | deep-8 | 33.805 ± 0.237 | 34.258 ± 0.903 | 35.815 ± 0.656 | 49.642 ± 0.948 | +1.34% |
| tls | deep-32 | 40.785 ± 0.667 | 41.212 ± 0.751 | 42.565 ± 1.303 | 56.058 ± 0.687 | +1.05% |
| tls | concurrent-32 | 122.466 ± 1.066 | 120.848 ± 0.414 | 153.999 ± 0.728 | 458.625 ± 3.346 | -1.32% |
| tls | deep-32-concurrent-32 | 410.430 ± 0.539 | 403.951 ± 2.043 | 439.562 ± 2.462 | 618.383 ± 8.800 | -1.58% |
| tls | services-4-deep-16 | 271.893 ± 1.129 | 272.120 ± 1.277 | 305.467 ± 2.129 | 362.904 ± 3.413 | +0.08% |
| tls | services-4-deep-16-concurrent-32 | 1661.058 ± 8.792 | 1642.657 ± 13.032 | 1793.598 ± 5.937 | 2515.149 ± 9.860 | -1.11% |
| tls | services-4-deep-16-clients-8 | 1687.190 ± 23.253 | 1659.156 ± 29.875 | 1793.811 ± 35.657 | 2409.857 ± 38.257 | -1.66% |
| tls | services-8-deep-16-clients-8 | 3468.620 ± 27.237 | 3422.996 ± 16.615 | 3589.225 ± 114.825 | 4951.236 ± 73.996 | -1.32% |
| quic | loopback-0 | 51.777 ± 13.266 | 47.708 ± 10.794 | 58.569 ± 16.169 | — | -7.86% |
| quic | loopback-64 | 42.160 ± 0.289 | 43.072 ± 0.452 | 47.257 ± 0.660 | — | +2.16% |
| quic | loopback-1024 | 44.777 ± 0.470 | 45.418 ± 0.538 | 49.643 ± 1.372 | — | +1.43% |
| quic | loopback-65536 | 197.116 ± 27.005 | 182.828 ± 0.604 | 194.026 ± 15.283 | — | -7.25% |
| quic | unary-empty | 56.786 ± 13.550 | 53.858 ± 12.119 | 58.428 ± 15.972 | — | -5.16% |
| quic | unary-small | 43.901 ± 2.287 | 45.069 ± 4.033 | 46.766 ± 1.094 | — | +2.66% |
| quic | unary-1k | 44.909 ± 0.801 | 45.149 ± 0.462 | 50.648 ± 2.731 | — | +0.54% |
| quic | fanout-32 | 229.294 ± 1.633 | 238.197 ± 23.972 | 259.695 ± 32.647 | — | +3.88% |
| quic | dependent-read | 97.282 ± 1.263 | 115.494 ± 26.271 | 150.859 ± 41.014 | — | +18.72% |
| quic | pipelined-read | 70.533 ± 16.644 | 74.123 ± 15.888 | 113.106 ± 32.104 | — | +5.09% |
| quic | write-read | 106.819 ± 27.989 | 108.027 ± 26.544 | 95.946 ± 1.983 | — | +1.13% |
| quic | callback | 93.195 ± 1.651 | 96.184 ± 5.014 | 117.235 ± 0.554 | — | +3.21% |
| quic | bulk-64k | 183.464 ± 1.600 | 184.071 ± 2.771 | 185.058 ± 0.993 | — | +0.33% |
| quic | bulk-256k | 487.527 ± 33.857 | 460.990 ± 1.194 | 483.283 ± 5.008 | — | -5.44% |
| quic | bulk-1m | 1820.828 ± 38.047 | 1795.755 ± 67.289 | 1759.341 ± 47.397 | — | -1.38% |
| quic | bulk-4m | 8382.993 ± 89.469 | 8275.620 ± 64.753 | 8421.570 ± 60.369 | — | -1.28% |
| quic | deep-8 | 45.663 ± 1.102 | 49.719 ± 9.234 | 55.945 ± 13.570 | — | +8.88% |
| quic | deep-32 | 52.257 ± 0.258 | 53.136 ± 0.955 | 58.662 ± 1.154 | — | +1.68% |
| quic | concurrent-32 | 156.755 ± 1.332 | 175.284 ± 46.666 | 180.967 ± 4.808 | — | +11.82% |
| quic | deep-32-concurrent-32 | 489.107 ± 36.353 | 495.903 ± 35.231 | 518.266 ± 33.518 | — | +1.39% |
| quic | services-4-deep-16 | 375.373 ± 3.036 | 375.329 ± 5.014 | 393.888 ± 4.798 | — | -0.01% |
| quic | services-4-deep-16-concurrent-32 | 2000.725 ± 0.790 | 2000.301 ± 0.784 | 2001.772 ± 2.641 | — | -0.02% |
| quic | services-4-deep-16-clients-8 | 2084.833 ± 21.096 | 2056.857 ± 43.703 | 2058.372 ± 49.006 | — | -1.34% |
| quic | services-8-deep-16-clients-8 | 4265.609 ± 20.294 | 4179.894 ± 14.909 | 3963.854 ± 96.717 | — | -2.01% |

## Allocation and instruction diagnostics

Short/long marginal counts, summed across every peer, per complete operation or concurrent batch. These are cumulative allocations and simulated instructions, not peak memory or instrumented latency.

| Transport | Case | Frozen blocks | Candidate blocks | Blocks change | Frozen bytes | Candidate bytes | Instructions change |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| tls | dependent-read | 120.00 | 118.00 | -1.67% | 16697.0 | 16681.0 | -0.16% |
| tls | pipelined-read | 137.00 | 135.00 | -1.46% | 18257.0 | 18241.0 | -0.43% |
| quic | pipelined-read | 159.04 | 156.98 | -1.30% | 19146.6 | 19124.4 | -0.23% |
| tls | concurrent-32 | 786.00 | 754.00 | -4.07% | 130420.0 | 130164.0 | -0.58% |
| tls | deep-32-concurrent-32 | 802.00 | 770.00 | -3.99% | 562167.8 | 561912.0 | -0.13% |
| tls | services-4-deep-16-clients-8 | 4756.51 | 4564.00 | -4.05% | 1796606.1 | 1793492.3 | -0.58% |
| quic | concurrent-32 | 920.98 | 889.27 | -3.44% | 106927.3 | 106702.1 | -0.35% |
| quic | deep-32-concurrent-32 | 958.49 | 926.49 | -3.34% | 442314.2 | 442058.5 | -0.19% |

| Transport | Case | Legacy blocks | Portable blocks | C++ blocks |
| --- | --- | ---: | ---: | ---: |
| tls | dependent-read | 118.00 | 64.00 | 247.00 |
| tls | pipelined-read | 135.00 | 77.00 | 233.50 |
| quic | pipelined-read | 156.98 | 93.01 | — |
| tls | concurrent-32 | 754.00 | 362.00 | 2062.49 |
| tls | deep-32-concurrent-32 | 770.00 | 378.00 | 2146.49 |
| tls | services-4-deep-16-clients-8 | 4564.00 | 2553.67 | 8556.11 |
| quic | concurrent-32 | 889.27 | 431.82 | — |
| quic | deep-32-concurrent-32 | 926.49 | 467.47 | — |

These totals include the fixture and encrypted transport, including different TLS libraries; allocation count alone does not determine latency.

## Ciphertext I/O diagnostics

Positive socket reads/writes per operation, estimated from the difference between 100- and 10-operation runs, each with five repetitions. Full counts, failed attempts, setup and teardown remain in the raw artifacts. QUIC counts include control traffic. This is a measured comparison, not a universal syscall-count guarantee.

| Transport | Case | Frozen legacy R/W | Candidate legacy R/W | C++ R/W |
| --- | --- | ---: | ---: | ---: |
| tls | fanout-32 | 14.000 / 2.000 | 14.000 / 2.000 | 10.000 / 10.000 |
| tls | dependent-read | 4.000 / 4.000 | 4.000 / 4.000 | 4.000 / 4.000 |
| tls | pipelined-read | 2.000 / 2.000 | 2.000 / 2.000 | 2.000 / 2.000 |
| tls | concurrent-32 | 4.000 / 2.000 | 4.000 / 2.000 | 2.000 / 2.000 |
| tls | services-4-deep-16-clients-8 | 78.018 / 27.998 | 78.433 / 28.004 | 51.731 / 51.804 |
| quic | fanout-32 | 5.998 / 1.998 | 6.000 / 2.000 | — |
| quic | dependent-read | 4.000 / 4.000 | 4.000 / 4.000 | — |
| quic | pipelined-read | 1.998 / 1.998 | 2.000 / 2.000 | — |
| quic | concurrent-32 | 2.000 / 2.000 | 2.000 / 2.000 | — |
| quic | services-4-deep-16-clients-8 | 60.578 / 55.227 | 60.936 / 55.931 | — |

All read/write attempts per operation, including nonpositive and unfinished calls:

| Transport | Case | Frozen attempts R/W | Candidate attempts R/W | C++ attempts R/W |
| --- | --- | ---: | ---: | ---: |
| tls | fanout-32 | 14.000 / 2.000 | 14.000 / 2.000 | 11.998 / 10.000 |
| tls | dependent-read | 4.000 / 4.000 | 4.000 / 4.000 | 7.998 / 4.000 |
| tls | pipelined-read | 2.000 / 2.000 | 2.000 / 2.000 | 4.004 / 2.000 |
| tls | concurrent-32 | 4.000 / 2.000 | 4.000 / 2.000 | 3.991 / 2.000 |
| tls | services-4-deep-16-clients-8 | 78.018 / 27.998 | 78.433 / 28.004 | 85.962 / 51.804 |
| quic | fanout-32 | 7.996 / 1.998 | 8.004 / 2.000 | — |
| quic | dependent-read | 8.002 / 4.000 | 8.002 / 4.000 | — |
| quic | pipelined-read | 4.000 / 1.998 | 4.000 / 2.000 | — |
| quic | concurrent-32 | 4.000 / 2.000 | 4.000 / 2.000 | — |
| quic | services-4-deep-16-clients-8 | 107.513 / 55.227 | 108.416 / 55.931 | — |

Complete 100-operation process captures, including startup and shutdown (mean attempts across five runs):

| Transport | Case | Frozen attempts R/W | Candidate attempts R/W | C++ attempts R/W |
| --- | --- | ---: | ---: | ---: |
| tls | fanout-32 | 1434.000 / 211.000 | 1434.000 / 211.000 | 1238.000 / 1028.000 |
| tls | dependent-read | 414.000 / 415.000 | 414.000 / 415.000 | 830.200 / 416.000 |
| tls | pipelined-read | 210.000 / 211.000 | 210.000 / 211.000 | 424.000 / 212.000 |
| tls | concurrent-32 | 414.000 / 211.000 | 414.000 / 211.000 | 422.000 / 212.000 |
| tls | services-4-deep-16-clients-8 | 8028.400 / 2938.800 | 8072.200 / 2941.000 | 8970.400 / 5390.000 |
| quic | fanout-32 | 836.600 / 212.800 | 837.000 / 213.000 | — |
| quic | dependent-read | 837.000 / 417.000 | 837.000 / 417.000 | — |
| quic | pipelined-read | 428.600 / 212.800 | 429.000 / 213.000 | — |
| quic | concurrent-32 | 427.000 / 213.000 | 427.000 / 213.000 | — |
| quic | services-4-deep-16-clients-8 | 11213.200 / 5753.400 | 11303.400 / 5821.400 | — |
