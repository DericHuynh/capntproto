## Native latency

Mean ± sample SD of the five repetition means, in microseconds. Negative change means lower latency. C++ QUIC is unsupported.

| Transport | Case | Frozen legacy | Candidate legacy | Current portable | C++ | Legacy change |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| tls | loopback-0 | 59.970 ± 11.677 | 52.978 ± 12.946 | 56.854 ± 15.850 | 86.667 ± 28.341 | -11.66% |
| tls | loopback-64 | 42.026 ± 5.752 | 40.738 ± 2.125 | 40.262 ± 1.550 | 66.364 ± 22.942 | -3.07% |
| tls | loopback-1024 | 42.159 ± 1.413 | 41.677 ± 0.430 | 42.811 ± 1.426 | 67.636 ± 20.471 | -1.14% |
| tls | loopback-65536 | 188.594 ± 1.117 | 195.694 ± 16.737 | 199.565 ± 24.268 | 244.488 ± 1.922 | +3.76% |
| tls | unary-empty | 44.385 ± 11.483 | 44.567 ± 11.859 | 44.587 ± 10.357 | 69.562 ± 22.348 | +0.41% |
| tls | unary-small | 44.616 ± 11.640 | 49.375 ± 13.470 | 47.249 ± 12.655 | 69.119 ± 21.477 | +10.67% |
| tls | unary-1k | 42.056 ± 0.679 | 41.441 ± 0.399 | 42.079 ± 0.120 | 59.590 ± 0.757 | -1.46% |
| tls | fanout-32 | 210.910 ± 2.470 | 209.820 ± 1.839 | 246.858 ± 3.555 | 486.746 ± 29.364 | -0.52% |
| tls | dependent-read | 89.902 ± 0.635 | 90.260 ± 1.251 | 102.431 ± 0.629 | 150.945 ± 43.016 | +0.40% |
| tls | pipelined-read | 58.320 ± 0.527 | 58.259 ± 0.819 | 78.232 ± 0.923 | 92.693 ± 1.053 | -0.10% |
| tls | write-read | 79.897 ± 1.139 | 79.410 ± 0.363 | 81.342 ± 1.524 | 114.940 ± 2.750 | -0.61% |
| tls | callback | 86.414 ± 0.559 | 85.585 ± 0.481 | 98.945 ± 0.957 | 121.198 ± 1.457 | -0.96% |
| tls | bulk-64k | 187.423 ± 1.235 | 188.232 ± 0.633 | 188.701 ± 1.825 | 243.328 ± 1.565 | +0.43% |
| tls | bulk-256k | 499.770 ± 56.727 | 474.902 ± 1.550 | 501.027 ± 26.727 | 1011.763 ± 14.977 | -4.98% |
| tls | bulk-1m | 1697.420 ± 8.398 | 1706.847 ± 26.708 | 1753.427 ± 84.485 | 3962.810 ± 59.787 | +0.56% |
| tls | bulk-4m | 8975.109 ± 632.194 | 8966.809 ± 232.916 | 8926.515 ± 156.531 | 17952.927 ± 773.767 | -0.09% |
| tls | deep-8 | 46.151 ± 1.483 | 50.487 ± 12.146 | 51.922 ± 13.523 | 74.962 ± 22.531 | +9.39% |
| tls | deep-32 | 54.089 ± 1.127 | 53.362 ± 1.022 | 53.710 ± 0.574 | 73.199 ± 0.494 | -1.35% |
| tls | concurrent-32 | 206.087 ± 78.812 | 179.754 ± 42.646 | 201.722 ± 44.910 | 607.387 ± 122.458 | -12.78% |
| tls | deep-32-concurrent-32 | 594.943 ± 167.916 | 545.916 ± 115.514 | 556.521 ± 126.893 | 825.437 ± 196.435 | -8.24% |
| tls | services-4-deep-16 | 384.360 ± 4.154 | 388.118 ± 2.676 | 417.144 ± 3.810 | 512.874 ± 6.940 | +0.98% |
| tls | services-4-deep-16-concurrent-32 | 2455.317 ± 60.980 | 2428.856 ± 37.122 | 2474.115 ± 43.889 | 3945.028 ± 101.415 | -1.08% |
| tls | services-4-deep-16-clients-8 | 2483.992 ± 62.866 | 2448.033 ± 70.635 | 2542.246 ± 71.897 | 3691.673 ± 84.690 | -1.45% |
| tls | services-8-deep-16-clients-8 | 5170.483 ± 73.787 | 5123.633 ± 111.476 | 5297.574 ± 172.300 | 7261.084 ± 359.640 | -0.91% |
| quic | loopback-0 | 67.849 ± 20.430 | 57.714 ± 11.596 | 76.762 ± 19.433 | — | -14.94% |
| quic | loopback-64 | 52.691 ± 0.542 | 60.050 ± 16.796 | 56.878 ± 0.704 | — | +13.97% |
| quic | loopback-1024 | 55.360 ± 0.425 | 54.510 ± 0.314 | 59.279 ± 0.670 | — | -1.54% |
| quic | loopback-65536 | 208.935 ± 2.030 | 207.168 ± 1.582 | 211.975 ± 0.969 | — | -0.85% |
| quic | unary-empty | 56.640 ± 9.148 | 52.539 ± 0.550 | 57.960 ± 1.941 | — | -7.24% |
| quic | unary-small | 52.894 ± 0.585 | 59.538 ± 14.176 | 65.485 ± 17.748 | — | +12.56% |
| quic | unary-1k | 58.358 ± 7.371 | 60.039 ± 11.258 | 59.031 ± 0.326 | — | +2.88% |
| quic | fanout-32 | 386.030 ± 82.943 | 337.718 ± 94.689 | 359.053 ± 85.790 | — | -12.52% |
| quic | dependent-read | 132.531 ± 27.537 | 128.068 ± 19.808 | 145.064 ± 2.425 | — | -3.37% |
| quic | pipelined-read | 86.649 ± 25.318 | 85.356 ± 25.172 | 143.780 ± 39.283 | — | -1.49% |
| quic | write-read | 130.547 ± 31.546 | 156.309 ± 30.679 | 129.674 ± 19.449 | — | +19.73% |
| quic | callback | 148.896 ± 34.067 | 147.318 ± 28.891 | 154.115 ± 25.547 | — | -1.06% |
| quic | bulk-64k | 233.402 ± 58.808 | 222.330 ± 24.864 | 239.615 ± 26.688 | — | -4.74% |
| quic | bulk-256k | 529.586 ± 14.634 | 546.281 ± 22.160 | 556.970 ± 24.948 | — | +3.15% |
| quic | bulk-1m | 2068.108 ± 218.182 | 1953.077 ± 31.588 | 1947.069 ± 32.651 | — | -5.56% |
| quic | bulk-4m | 9640.176 ± 188.764 | 9830.498 ± 468.746 | 9892.372 ± 436.918 | — | +1.97% |
| quic | deep-8 | 55.565 ± 0.369 | 55.544 ± 0.508 | 69.052 ± 19.839 | — | -0.04% |
| quic | deep-32 | 86.031 ± 29.833 | 109.849 ± 12.016 | 94.205 ± 30.949 | — | +27.68% |
| quic | concurrent-32 | 235.737 ± 95.358 | 174.670 ± 1.219 | 248.927 ± 82.631 | — | -25.90% |
| quic | deep-32-concurrent-32 | 582.809 ± 114.810 | 759.460 ± 181.819 | 707.910 ± 116.402 | — | +30.31% |
| quic | services-4-deep-16 | 512.849 ± 8.603 | 509.472 ± 3.693 | 513.385 ± 10.295 | — | -0.66% |
| quic | services-4-deep-16-concurrent-32 | 2948.719 ± 39.891 | 2918.308 ± 67.843 | 2842.400 ± 37.501 | — | -1.03% |
| quic | services-4-deep-16-clients-8 | 3030.240 ± 43.620 | 2994.255 ± 27.737 | 2590.345 ± 81.689 | — | -1.19% |
| quic | services-8-deep-16-clients-8 | 6282.951 ± 65.115 | 6357.719 ± 269.914 | 4947.586 ± 89.625 | — | +1.19% |

## Allocation and instruction diagnostics

Short/long marginal counts, summed across every peer, per complete operation or concurrent batch. These are cumulative allocations and simulated instructions, not peak memory or instrumented latency.

| Transport | Case | Frozen blocks | Candidate blocks | Blocks change | Frozen bytes | Candidate bytes | Instructions change |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| tls | dependent-read | 120.00 | 118.00 | -1.67% | 16697.0 | 16681.0 | -0.05% |
| tls | pipelined-read | 137.00 | 135.00 | -1.46% | 18257.0 | 18241.0 | -0.41% |
| quic | pipelined-read | 159.04 | 157.04 | -1.26% | 19146.7 | 19130.5 | -0.23% |
| tls | concurrent-32 | 786.00 | 754.00 | -4.07% | 130420.2 | 130163.9 | -0.57% |
| tls | deep-32-concurrent-32 | 802.00 | 770.00 | -3.99% | 562167.9 | 561911.9 | -0.13% |
| tls | services-4-deep-16-clients-8 | 4754.42 | 4564.00 | -4.01% | 1790207.5 | 1793492.2 | -0.70% |
| quic | concurrent-32 | 920.62 | 888.93 | -3.44% | 106895.6 | 106670.4 | -0.28% |
| quic | deep-32-concurrent-32 | 958.56 | 926.20 | -3.38% | 442119.8 | 442028.4 | -0.11% |

| Transport | Case | Legacy blocks | Portable blocks | C++ blocks |
| --- | --- | ---: | ---: | ---: |
| tls | dependent-read | 118.00 | 64.00 | 247.00 |
| tls | pipelined-read | 135.00 | 77.00 | 233.50 |
| quic | pipelined-read | 157.04 | 92.98 | — |
| tls | concurrent-32 | 754.00 | 362.00 | 2062.49 |
| tls | deep-32-concurrent-32 | 770.00 | 378.00 | 2146.49 |
| tls | services-4-deep-16-clients-8 | 4564.00 | 2548.84 | 8567.11 |
| quic | concurrent-32 | 888.93 | 432.44 | — |
| quic | deep-32-concurrent-32 | 926.20 | 467.16 | — |

These totals include the fixture and encrypted transport, including different TLS libraries; allocation count alone does not determine latency.

## Ciphertext I/O diagnostics

Positive socket reads/writes per operation, estimated from the difference between 100- and 10-operation runs, each with five repetitions. Full counts, failed attempts, setup and teardown remain in the raw artifacts. QUIC counts include control traffic. This is a measured comparison, not a universal syscall-count guarantee.

| Transport | Case | Frozen legacy R/W | Candidate legacy R/W | C++ R/W |
| --- | --- | ---: | ---: | ---: |
| tls | fanout-32 | 14.000 / 2.000 | 14.000 / 2.000 | 10.000 / 10.000 |
| tls | dependent-read | 4.000 / 4.000 | 4.000 / 4.000 | 4.000 / 4.000 |
| tls | pipelined-read | 2.000 / 2.000 | 2.000 / 2.000 | 2.000 / 2.000 |
| tls | concurrent-32 | 4.000 / 2.000 | 4.000 / 2.000 | 2.000 / 2.000 |
| tls | services-4-deep-16-clients-8 | 78.769 / 27.931 | 78.676 / 28.022 | 51.238 / 51.469 |
| quic | fanout-32 | 6.000 / 2.000 | 6.004 / 2.004 | — |
| quic | dependent-read | 4.002 / 4.002 | 4.000 / 4.000 | — |
| quic | pipelined-read | 1.998 / 1.998 | 1.998 / 1.998 | — |
| quic | concurrent-32 | 2.000 / 2.000 | 2.000 / 2.000 | — |
| quic | services-4-deep-16-clients-8 | 61.582 / 57.878 | 61.722 / 58.347 | — |

All read/write attempts per operation, including nonpositive and unfinished calls:

| Transport | Case | Frozen attempts R/W | Candidate attempts R/W | C++ attempts R/W |
| --- | --- | ---: | ---: | ---: |
| tls | fanout-32 | 14.000 / 2.000 | 14.000 / 2.000 | 12.007 / 10.000 |
| tls | dependent-read | 4.000 / 4.000 | 4.000 / 4.000 | 7.993 / 4.000 |
| tls | pipelined-read | 2.000 / 2.000 | 2.000 / 2.000 | 4.000 / 2.000 |
| tls | concurrent-32 | 4.000 / 2.000 | 4.000 / 2.000 | 3.993 / 2.000 |
| tls | services-4-deep-16-clients-8 | 78.769 / 27.933 | 78.676 / 28.024 | 84.911 / 51.469 |
| quic | fanout-32 | 8.000 / 2.000 | 8.009 / 2.004 | — |
| quic | dependent-read | 8.004 / 4.002 | 8.000 / 4.000 | — |
| quic | pipelined-read | 3.993 / 1.998 | 3.998 / 1.998 | — |
| quic | concurrent-32 | 4.000 / 2.000 | 3.998 / 2.000 | — |
| quic | services-4-deep-16-clients-8 | 110.620 / 57.878 | 111.204 / 58.347 | — |

Complete 100-operation process captures, including startup and shutdown (mean attempts across five runs):

| Transport | Case | Frozen attempts R/W | Candidate attempts R/W | C++ attempts R/W |
| --- | --- | ---: | ---: | ---: |
| tls | fanout-32 | 1434.000 / 211.000 | 1434.000 / 211.000 | 1238.600 / 1028.000 |
| tls | dependent-read | 414.000 / 415.000 | 414.000 / 415.000 | 829.200 / 416.000 |
| tls | pipelined-read | 210.000 / 211.000 | 210.000 / 211.000 | 422.600 / 212.000 |
| tls | concurrent-32 | 414.000 / 211.000 | 414.000 / 211.000 | 421.800 / 212.000 |
| tls | services-4-deep-16-clients-8 | 8103.400 / 2933.600 | 8094.200 / 2938.400 | 8861.400 / 5351.800 |
| quic | fanout-32 | 836.600 / 212.800 | 837.000 / 213.000 | — |
| quic | dependent-read | 836.600 / 416.800 | 836.600 / 416.800 | — |
| quic | pipelined-read | 428.000 / 212.600 | 428.600 / 212.800 | — |
| quic | concurrent-32 | 426.800 / 212.800 | 427.200 / 213.000 | — |
| quic | services-4-deep-16-clients-8 | 11563.200 / 6047.000 | 11634.400 / 6103.600 | — |
