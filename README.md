# Capntproto verification reports

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-30T20:15:40Z · run 41121764267 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41121764267) · commit `7a67fee84e8f` · **failure**

![Cargo test results](docs/reports/cargo-history.svg)

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-30T20:15:40Z · run 41121764267 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41121764267) · commit `7a67fee84e8f` · **failure**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-30T20:15:40Z · run 41121764267 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41121764267) · commit `7a67fee84e8f` · **failure**

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

Latest coverage run: [2026-10-30T21:56:32Z · run 41134677956 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41134677956) · commit `6c29732965e7` · **failure**

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-30T20:15:40Z · run 41121764267 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41121764267) · commit `7a67fee84e8f` · **failure**

Measurement evidence: **passed**. Independently validated encrypted benchmark values&#46; Workflow status and diagnostic completion are separate&#59; unavailable values remain blank&#46; Operations include whole concurrent batches and every service hop&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Current TCP fixtures use TLS 1.3, including upstream Rust Cap’n Proto, C++, gRPC over HTTP/2 and WebSockets over WSS; native encrypted transports and standard QUIC are identified separately. Historical data retains its recorded fixture contract. The dedicated campaign also reports recursive data, concurrent calls and independent service chains for gRPC and binary Protobuf WebSockets in its encrypted comparison artifact. Bars compare the recorded workload, rather than universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.


## Exhaustive benchmark tables

Measured run: [2026-10-30T20:15:40Z · run 41121764267 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41121764267) · commit `7a67fee84e8f` · **failure**

All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.

### All encrypted latency comparisons

| Transport | Case | Implementation | Samples | Minimum µs | Mean µs | Sample SD µs | Run mean SD µs | Run mean min µs | Run mean max µs | p50 µs | p95 µs | p99 µs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tls | loopback&#45;0 | cpp | 5000 | 45.376 | 69.903 | 15.897 | 14.700 | 50.395 | 83.365 | 72.595 | 93.895 | 107.946 |
| tls | loopback&#45;0 | grpc | 5000 | 52.125 | 74.805 | 19.080 | 17.202 | 57.787 | 93.009 | 73.853 | 104.382 | 122.942 |
| tls | loopback&#45;0 | legacy | 5000 | 29.597 | 42.669 | 66.573 | 13.661 | 32.997 | 63.710 | 34.571 | 57.250 | 74.024 |
| tls | loopback&#45;0 | portable | 5000 | 31.388 | 49.997 | 14.463 | 11.826 | 34.954 | 62.095 | 51.386 | 71.643 | 98.368 |
| tls | loopback&#45;0 | websocket | 5000 | 28.322 | 41.422 | 9.961 | 8.478 | 32.289 | 50.869 | 42.814 | 56.303 | 73.103 |
| tls | loopback&#45;64 | cpp | 5000 | 45.575 | 51.558 | 6.818 | 1.257 | 50.332 | 53.513 | 49.809 | 65.561 | 78.028 |
| tls | loopback&#45;64 | grpc | 5000 | 52.067 | 58.625 | 6.991 | 1.254 | 57.663 | 60.677 | 56.583 | 72.856 | 86.248 |
| tls | loopback&#45;64 | legacy | 5000 | 27.376 | 33.267 | 5.162 | 0.658 | 32.435 | 34.162 | 31.858 | 43.165 | 52.297 |
| tls | loopback&#45;64 | portable | 5000 | 31.311 | 35.797 | 22.189 | 0.713 | 34.833 | 36.802 | 34.031 | 46.611 | 55.631 |
| tls | loopback&#45;64 | websocket | 5000 | 24.777 | 30.138 | 4.639 | 0.257 | 29.795 | 30.467 | 29.139 | 39.109 | 47.601 |
| tls | loopback&#45;1024 | cpp | 5000 | 47.495 | 55.596 | 10.592 | 3.412 | 53.510 | 61.575 | 53.251 | 78.430 | 99.590 |
| tls | loopback&#45;1024 | grpc | 5000 | 56.022 | 74.473 | 21.967 | 15.838 | 63.725 | 102.308 | 65.833 | 109.376 | 143.734 |
| tls | loopback&#45;1024 | legacy | 5000 | 29.674 | 37.605 | 6.245 | 1.137 | 36.415 | 38.970 | 36.293 | 49.935 | 62.538 |
| tls | loopback&#45;1024 | portable | 5000 | 33.601 | 38.882 | 7.354 | 1.294 | 37.774 | 41.014 | 37.613 | 52.786 | 66.541 |
| tls | loopback&#45;1024 | websocket | 5000 | 23.089 | 30.575 | 6.062 | 1.108 | 29.520 | 31.841 | 29.143 | 42.693 | 54.386 |
| tls | loopback&#45;65536 | cpp | 5000 | 201.341 | 269.371 | 61.980 | 48.207 | 228.527 | 347.078 | 237.637 | 374.613 | 398.183 |
| tls | loopback&#45;65536 | grpc | 5000 | 201.001 | 265.239 | 46.152 | 40.391 | 238.064 | 336.807 | 250.033 | 356.375 | 388.451 |
| tls | loopback&#45;65536 | legacy | 5000 | 144.029 | 176.865 | 35.832 | 36.572 | 157.118 | 242.064 | 162.545 | 252.714 | 272.367 |
| tls | loopback&#45;65536 | portable | 5000 | 149.795 | 219.382 | 46.939 | 45.591 | 166.298 | 259.389 | 238.789 | 281.916 | 302.695 |
| tls | loopback&#45;65536 | websocket | 5000 | 164.402 | 210.832 | 45.697 | 42.191 | 181.056 | 278.048 | 187.754 | 293.949 | 311.982 |
| tls | unary&#45;empty | cpp | 5000 | 45.549 | 57.012 | 13.591 | 12.013 | 50.311 | 78.434 | 51.675 | 82.752 | 98.577 |
| tls | unary&#45;empty | grpc | 5000 | 52.740 | 67.013 | 18.952 | 16.564 | 58.125 | 96.602 | 59.796 | 104.991 | 136.060 |
| tls | unary&#45;empty | legacy | 5000 | 29.429 | 32.852 | 4.509 | 0.352 | 32.539 | 33.427 | 30.940 | 42.461 | 50.978 |
| tls | unary&#45;empty | portable | 5000 | 31.228 | 35.227 | 5.210 | 0.749 | 34.364 | 36.399 | 33.573 | 46.388 | 55.454 |
| tls | unary&#45;empty | websocket | 5000 | 21.978 | 33.012 | 4.573 | 0.351 | 32.590 | 33.444 | 31.626 | 42.984 | 51.295 |
| tls | unary&#45;small | cpp | 5000 | 45.262 | 54.894 | 11.509 | 5.067 | 50.540 | 63.621 | 51.518 | 79.863 | 93.831 |
| tls | unary&#45;small | grpc | 5000 | 53.568 | 70.853 | 19.886 | 17.231 | 60.937 | 101.285 | 61.300 | 106.272 | 139.490 |
| tls | unary&#45;small | legacy | 5000 | 29.980 | 41.010 | 11.851 | 9.880 | 33.600 | 52.561 | 34.521 | 59.655 | 82.721 |
| tls | unary&#45;small | portable | 5000 | 31.368 | 44.497 | 11.392 | 10.800 | 36.357 | 56.518 | 38.122 | 64.054 | 75.321 |
| tls | unary&#45;small | websocket | 5000 | 25.136 | 36.221 | 9.461 | 7.413 | 30.684 | 46.638 | 32.874 | 50.680 | 67.192 |
| tls | unary&#45;1k | cpp | 5000 | 47.564 | 56.233 | 9.189 | 1.530 | 54.510 | 58.580 | 54.187 | 72.399 | 90.987 |
| tls | unary&#45;1k | grpc | 5000 | 57.111 | 67.301 | 11.274 | 2.613 | 64.852 | 71.622 | 64.653 | 88.067 | 110.460 |
| tls | unary&#45;1k | legacy | 5000 | 32.217 | 36.628 | 5.003 | 0.865 | 35.496 | 37.501 | 35.972 | 47.061 | 56.176 |
| tls | unary&#45;1k | portable | 5000 | 33.542 | 38.420 | 6.044 | 0.352 | 38.066 | 39.003 | 36.806 | 49.795 | 61.301 |
| tls | unary&#45;1k | websocket | 5000 | 20.383 | 30.211 | 5.304 | 0.679 | 29.670 | 31.345 | 29.018 | 39.427 | 49.444 |
| tls | fanout&#45;32 | cpp | 5000 | 397.402 | 522.716 | 115.664 | 80.088 | 462.955 | 652.333 | 479.607 | 775.044 | 825.267 |
| tls | fanout&#45;32 | grpc | 5000 | 676.318 | 809.346 | 119.192 | 48.262 | 755.025 | 850.421 | 779.098 | 1090.267 | 1164.295 |
| tls | fanout&#45;32 | legacy | 5000 | 158.500 | 200.570 | 48.732 | 43.645 | 179.089 | 278.458 | 181.637 | 309.831 | 358.115 |
| tls | fanout&#45;32 | portable | 5000 | 190.001 | 241.488 | 57.626 | 59.582 | 212.399 | 348.036 | 217.580 | 366.038 | 393.460 |
| tls | fanout&#45;32 | websocket | 5000 | 193.678 | 254.378 | 54.251 | 50.037 | 225.019 | 343.567 | 233.817 | 365.900 | 417.150 |
| tls | dependent&#45;read | cpp | 5000 | 106.373 | 134.441 | 28.752 | 22.812 | 121.400 | 174.988 | 123.067 | 196.948 | 218.929 |
| tls | dependent&#45;read | grpc | 5000 | 110.312 | 127.823 | 16.784 | 3.766 | 125.364 | 134.370 | 124.968 | 159.707 | 187.449 |
| tls | dependent&#45;read | legacy | 5000 | 66.263 | 76.345 | 9.742 | 1.586 | 74.657 | 78.645 | 74.819 | 95.426 | 113.619 |
| tls | dependent&#45;read | portable | 5000 | 84.820 | 96.185 | 11.448 | 0.852 | 95.304 | 97.350 | 94.942 | 118.355 | 139.807 |
| tls | dependent&#45;read | websocket | 5000 | 47.314 | 59.814 | 9.017 | 0.909 | 58.650 | 61.109 | 57.985 | 76.238 | 90.955 |
| tls | pipelined&#45;read | cpp | 5000 | 76.986 | 86.306 | 10.017 | 2.010 | 84.932 | 89.853 | 83.120 | 107.021 | 122.453 |
| tls | pipelined&#45;read | legacy | 5000 | 40.807 | 46.480 | 7.395 | 1.333 | 45.072 | 48.346 | 45.684 | 59.014 | 70.002 |
| tls | pipelined&#45;read | portable | 5000 | 63.286 | 72.210 | 9.550 | 0.950 | 71.208 | 73.434 | 70.211 | 91.359 | 110.033 |
| tls | write&#45;read | cpp | 5000 | 93.057 | 123.472 | 30.353 | 21.773 | 106.187 | 151.830 | 109.302 | 182.327 | 217.850 |
| tls | write&#45;read | grpc | 5000 | 111.006 | 157.608 | 44.486 | 37.368 | 124.759 | 205.001 | 135.102 | 230.312 | 281.729 |
| tls | write&#45;read | legacy | 5000 | 61.528 | 74.839 | 15.424 | 8.696 | 68.457 | 89.828 | 69.532 | 106.215 | 124.459 |
| tls | write&#45;read | portable | 5000 | 64.683 | 73.638 | 9.969 | 1.531 | 72.582 | 76.267 | 71.662 | 93.229 | 114.220 |
| tls | write&#45;read | websocket | 5000 | 47.397 | 70.276 | 16.588 | 14.361 | 59.144 | 87.437 | 65.886 | 100.971 | 115.380 |
| tls | callback | cpp | 5000 | 98.103 | 129.060 | 28.200 | 26.127 | 113.096 | 175.232 | 116.441 | 184.263 | 207.783 |
| tls | callback | grpc | 5000 | 55.521 | 72.580 | 17.108 | 12.922 | 64.353 | 94.896 | 65.306 | 100.329 | 126.047 |
| tls | callback | legacy | 5000 | 65.760 | 84.628 | 18.273 | 16.484 | 76.543 | 114.094 | 76.806 | 118.596 | 138.505 |
| tls | callback | portable | 5000 | 81.661 | 106.295 | 26.506 | 25.293 | 92.991 | 151.467 | 94.072 | 157.870 | 185.518 |
| tls | callback | websocket | 5000 | 47.887 | 63.069 | 10.764 | 5.709 | 59.180 | 72.797 | 59.741 | 83.630 | 99.660 |
| tls | bulk&#45;64k | cpp | 5000 | 201.743 | 284.122 | 67.273 | 53.174 | 227.395 | 337.089 | 254.616 | 387.398 | 433.237 |
| tls | bulk&#45;64k | grpc | 5000 | 203.580 | 279.763 | 61.067 | 48.238 | 237.601 | 343.799 | 253.027 | 382.800 | 432.192 |
| tls | bulk&#45;64k | legacy | 5000 | 145.347 | 209.696 | 49.172 | 46.016 | 162.293 | 259.088 | 228.575 | 281.371 | 315.589 |
| tls | bulk&#45;64k | portable | 5000 | 149.228 | 199.746 | 49.093 | 45.156 | 166.012 | 263.952 | 175.184 | 286.040 | 321.837 |
| tls | bulk&#45;64k | websocket | 5000 | 166.423 | 212.405 | 44.906 | 39.565 | 186.048 | 278.753 | 190.235 | 296.056 | 321.382 |
| tls | bulk&#45;256k | cpp | 5000 | 775.467 | 907.543 | 113.135 | 37.696 | 872.885 | 956.037 | 884.550 | 1209.645 | 1292.591 |
| tls | bulk&#45;256k | grpc | 5000 | 791.743 | 1200.896 | 219.840 | 180.842 | 971.689 | 1378.223 | 1189.993 | 1508.255 | 1576.328 |
| tls | bulk&#45;256k | legacy | 5000 | 438.764 | 524.484 | 74.889 | 62.339 | 489.746 | 635.609 | 500.877 | 691.746 | 726.067 |
| tls | bulk&#45;256k | portable | 5000 | 446.455 | 553.297 | 83.397 | 57.866 | 506.710 | 644.665 | 523.906 | 720.070 | 756.803 |
| tls | bulk&#45;256k | websocket | 5000 | 926.780 | 1029.304 | 60.675 | 26.642 | 1000.443 | 1060.253 | 1025.278 | 1139.805 | 1204.092 |
| tls | bulk&#45;1m | cpp | 5000 | 3046.877 | 3383.663 | 338.196 | 120.856 | 3260.262 | 3523.710 | 3300.174 | 4383.270 | 4874.076 |
| tls | bulk&#45;1m | grpc | 5000 | 2041.895 | 3991.798 | 970.885 | 1000.271 | 2854.517 | 4763.434 | 4334.315 | 5427.766 | 5719.413 |
| tls | bulk&#45;1m | legacy | 5000 | 1515.873 | 1746.209 | 198.536 | 60.871 | 1659.706 | 1793.250 | 1674.039 | 2159.485 | 2295.898 |
| tls | bulk&#45;1m | portable | 5000 | 1519.924 | 1811.206 | 215.808 | 104.935 | 1701.272 | 1927.103 | 1703.836 | 2165.741 | 2253.896 |
| tls | bulk&#45;1m | websocket | 5000 | 3730.317 | 4239.026 | 384.722 | 261.969 | 4076.122 | 4703.060 | 4093.069 | 5032.887 | 5335.803 |
| tls | bulk&#45;4m | cpp | 5000 | 13918.186 | 16251.387 | 2170.531 | 903.788 | 15381.110 | 17718.387 | 15294.926 | 20711.708 | 21519.491 |
| tls | bulk&#45;4m | grpc | 5000 | 17403.955 | 22851.531 | 2484.902 | 1808.145 | 20627.601 | 24502.396 | 22514.533 | 26845.494 | 27703.916 |
| tls | bulk&#45;4m | legacy | 5000 | 7469.280 | 9117.233 | 1032.593 | 581.079 | 8300.370 | 9653.398 | 8620.046 | 10763.137 | 11166.389 |
| tls | bulk&#45;4m | portable | 5000 | 7585.999 | 8960.066 | 913.562 | 674.481 | 8309.106 | 9888.300 | 8562.680 | 10586.856 | 11039.337 |
| tls | bulk&#45;4m | websocket | 5000 | 24898.713 | 28350.023 | 2745.644 | 715.849 | 27411.324 | 29162.004 | 26908.935 | 33183.590 | 33928.442 |
| tls | deep&#45;8 | cpp | 5000 | 47.690 | 60.477 | 14.204 | 13.292 | 52.875 | 84.148 | 54.678 | 88.098 | 103.964 |
| tls | deep&#45;8 | grpc | 5000 | 63.665 | 81.693 | 19.111 | 18.048 | 71.127 | 113.504 | 72.838 | 117.872 | 136.046 |
| tls | deep&#45;8 | legacy | 5000 | 32.075 | 40.610 | 9.590 | 8.458 | 35.663 | 55.660 | 36.724 | 57.067 | 69.052 |
| tls | deep&#45;8 | portable | 5000 | 33.748 | 45.087 | 12.466 | 9.295 | 37.968 | 56.055 | 38.613 | 65.525 | 78.718 |
| tls | deep&#45;8 | websocket | 5000 | 28.392 | 45.791 | 11.199 | 8.693 | 38.931 | 56.421 | 44.528 | 67.116 | 77.422 |
| tls | deep&#45;32 | cpp | 5000 | 54.039 | 68.398 | 17.134 | 12.131 | 60.136 | 88.344 | 61.466 | 102.003 | 119.597 |
| tls | deep&#45;32 | grpc | 5000 | 80.589 | 105.834 | 25.429 | 22.901 | 90.470 | 145.667 | 94.338 | 153.736 | 174.827 |
| tls | deep&#45;32 | legacy | 5000 | 37.919 | 50.617 | 13.065 | 6.249 | 44.890 | 57.605 | 44.557 | 73.475 | 90.243 |
| tls | deep&#45;32 | portable | 5000 | 40.114 | 54.632 | 14.564 | 11.424 | 46.446 | 74.317 | 46.687 | 80.212 | 95.239 |
| tls | deep&#45;32 | websocket | 5000 | 46.987 | 74.457 | 19.348 | 15.073 | 57.844 | 90.789 | 73.101 | 106.771 | 125.802 |
| tls | concurrent&#45;32 | cpp | 5000 | 447.091 | 528.849 | 47.166 | 16.139 | 506.892 | 547.523 | 524.076 | 609.880 | 677.055 |
| tls | concurrent&#45;32 | grpc | 5000 | 552.622 | 643.387 | 56.528 | 26.122 | 618.560 | 681.109 | 637.154 | 753.660 | 782.767 |
| tls | concurrent&#45;32 | legacy | 5000 | 107.108 | 140.944 | 35.639 | 28.956 | 122.476 | 190.519 | 126.018 | 215.120 | 242.384 |
| tls | concurrent&#45;32 | portable | 5000 | 135.010 | 170.486 | 42.040 | 36.864 | 151.843 | 236.346 | 155.547 | 271.128 | 298.822 |
| tls | concurrent&#45;32 | websocket | 5000 | 99.482 | 177.668 | 39.443 | 24.193 | 154.091 | 214.415 | 168.450 | 249.924 | 282.540 |
| tls | deep&#45;32&#45;concurrent&#45;32 | cpp | 5000 | 592.685 | 805.615 | 136.372 | 107.181 | 695.720 | 924.898 | 817.327 | 1016.147 | 1060.989 |
| tls | deep&#45;32&#45;concurrent&#45;32 | grpc | 5000 | 1379.054 | 1807.547 | 123.548 | 29.027 | 1761.672 | 1832.785 | 1825.442 | 1971.921 | 2054.347 |
| tls | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 397.938 | 490.622 | 49.083 | 30.786 | 461.250 | 532.987 | 486.553 | 571.596 | 608.592 |
| tls | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 405.723 | 495.152 | 50.014 | 34.732 | 461.956 | 534.332 | 486.862 | 585.098 | 624.825 |
| tls | deep&#45;32&#45;concurrent&#45;32 | websocket | 5000 | 659.914 | 896.769 | 154.636 | 101.142 | 767.240 | 1013.634 | 832.667 | 1117.185 | 1181.321 |
| tls | services&#45;4&#45;deep&#45;16 | cpp | 5000 | 336.157 | 404.552 | 41.566 | 16.139 | 389.270 | 429.706 | 399.847 | 477.901 | 535.862 |
| tls | services&#45;4&#45;deep&#45;16 | grpc | 5000 | 479.317 | 574.122 | 51.565 | 18.408 | 551.597 | 593.691 | 568.048 | 673.390 | 734.811 |
| tls | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 246.044 | 296.368 | 37.877 | 8.814 | 284.139 | 308.452 | 293.299 | 341.286 | 384.243 |
| tls | services&#45;4&#45;deep&#45;16 | portable | 5000 | 286.684 | 346.181 | 33.311 | 10.073 | 332.929 | 356.467 | 344.018 | 399.934 | 444.620 |
| tls | services&#45;4&#45;deep&#45;16 | websocket | 5000 | 198.253 | 290.048 | 47.512 | 13.037 | 275.749 | 310.660 | 285.013 | 368.698 | 414.866 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | 5000 | 2481.111 | 2797.970 | 133.468 | 26.957 | 2767.756 | 2831.778 | 2785.990 | 3037.824 | 3190.588 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | 5000 | 4331.382 | 4732.297 | 237.504 | 29.253 | 4696.325 | 4766.183 | 4689.813 | 5194.338 | 5471.702 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1484.019 | 1655.158 | 86.078 | 8.973 | 1646.198 | 1665.129 | 1654.107 | 1808.145 | 1891.837 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1594.268 | 1768.319 | 82.623 | 16.171 | 1745.880 | 1784.394 | 1768.890 | 1905.511 | 2008.163 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | 5000 | 1286.496 | 1581.955 | 107.569 | 28.143 | 1550.527 | 1622.396 | 1573.513 | 1763.726 | 1862.216 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 2082.080 | 2644.695 | 178.837 | 17.451 | 2620.976 | 2664.749 | 2642.751 | 2948.227 | 3119.803 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 3659.343 | 4546.055 | 279.568 | 53.438 | 4463.385 | 4599.816 | 4533.686 | 5007.635 | 5267.854 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1258.835 | 1692.424 | 107.928 | 15.690 | 1671.728 | 1715.068 | 1687.396 | 1869.487 | 1969.048 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1385.729 | 1809.998 | 119.493 | 41.355 | 1767.393 | 1871.865 | 1806.024 | 2006.366 | 2079.672 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1138.658 | 1521.141 | 146.914 | 71.306 | 1484.181 | 1648.555 | 1509.776 | 1780.300 | 1896.426 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 4128.409 | 5466.370 | 318.179 | 87.406 | 5385.451 | 5562.482 | 5424.478 | 6021.335 | 6407.961 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 6835.302 | 8886.537 | 584.219 | 31.941 | 8851.712 | 8934.231 | 8886.694 | 9842.110 | 10404.977 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2875.524 | 3457.539 | 160.411 | 25.652 | 3419.905 | 3485.690 | 3433.545 | 3751.019 | 3913.274 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2849.115 | 3633.942 | 250.738 | 80.895 | 3535.281 | 3737.130 | 3630.905 | 4035.346 | 4277.327 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 2212.648 | 2882.144 | 243.098 | 12.339 | 2864.464 | 2895.495 | 2844.656 | 3333.379 | 3592.057 |
| quic | loopback&#45;0 | legacy | 5000 | 39.685 | 70.249 | 12.151 | 2.450 | 68.349 | 74.369 | 68.245 | 89.258 | 115.713 |
| quic | loopback&#45;0 | portable | 5000 | 71.993 | 80.817 | 10.309 | 3.941 | 78.271 | 87.746 | 78.179 | 98.763 | 121.761 |
| quic | loopback&#45;64 | legacy | 5000 | 39.639 | 48.452 | 10.357 | 5.952 | 44.616 | 58.951 | 44.940 | 70.999 | 83.502 |
| quic | loopback&#45;64 | portable | 5000 | 43.366 | 61.441 | 16.823 | 15.698 | 49.092 | 80.267 | 51.628 | 88.659 | 103.684 |
| quic | loopback&#45;1024 | legacy | 5000 | 41.881 | 61.793 | 16.637 | 15.247 | 45.606 | 78.245 | 67.989 | 86.880 | 101.621 |
| quic | loopback&#45;1024 | portable | 5000 | 46.152 | 58.276 | 14.726 | 14.481 | 51.249 | 84.174 | 52.082 | 87.528 | 100.108 |
| quic | loopback&#45;65536 | legacy | 5000 | 184.767 | 269.447 | 61.787 | 48.675 | 206.466 | 326.504 | 297.055 | 351.242 | 382.484 |
| quic | loopback&#45;65536 | portable | 5000 | 191.052 | 250.831 | 54.833 | 49.428 | 210.005 | 328.130 | 225.586 | 348.693 | 367.282 |
| quic | unary&#45;empty | legacy | 5000 | 38.802 | 44.403 | 6.138 | 0.980 | 43.293 | 45.591 | 42.740 | 57.752 | 68.866 |
| quic | unary&#45;empty | portable | 5000 | 43.580 | 48.957 | 6.740 | 1.057 | 47.807 | 50.116 | 47.223 | 62.163 | 74.002 |
| quic | unary&#45;small | legacy | 5000 | 39.558 | 44.272 | 5.962 | 1.013 | 43.659 | 46.071 | 42.639 | 56.122 | 67.773 |
| quic | unary&#45;small | portable | 5000 | 43.306 | 50.753 | 10.385 | 4.006 | 48.201 | 57.860 | 48.237 | 70.857 | 96.558 |
| quic | unary&#45;1k | legacy | 5000 | 41.259 | 59.685 | 19.121 | 17.973 | 46.583 | 83.903 | 49.173 | 91.286 | 122.649 |
| quic | unary&#45;1k | portable | 5000 | 45.902 | 57.555 | 16.017 | 4.296 | 52.645 | 61.314 | 51.903 | 87.856 | 111.743 |
| quic | fanout&#45;32 | legacy | 5000 | 185.518 | 233.638 | 54.507 | 40.389 | 208.701 | 305.050 | 214.859 | 353.584 | 388.868 |
| quic | fanout&#45;32 | portable | 5000 | 219.301 | 247.997 | 20.273 | 3.598 | 245.951 | 254.356 | 246.985 | 286.042 | 308.670 |
| quic | dependent&#45;read | legacy | 5000 | 87.656 | 100.196 | 11.034 | 1.669 | 98.551 | 102.844 | 99.122 | 121.424 | 140.408 |
| quic | dependent&#45;read | portable | 5000 | 117.047 | 149.738 | 38.921 | 31.225 | 130.856 | 204.988 | 133.702 | 233.401 | 257.396 |
| quic | pipelined&#45;read | legacy | 5000 | 53.119 | 84.979 | 20.672 | 16.509 | 58.674 | 97.013 | 88.375 | 113.587 | 145.320 |
| quic | pipelined&#45;read | portable | 5000 | 83.261 | 121.683 | 31.495 | 24.721 | 95.109 | 154.048 | 111.456 | 170.770 | 188.156 |
| quic | write&#45;read | legacy | 5000 | 81.272 | 110.752 | 28.027 | 21.275 | 94.045 | 145.131 | 96.268 | 159.193 | 179.984 |
| quic | write&#45;read | portable | 5000 | 89.599 | 126.552 | 34.651 | 33.436 | 101.339 | 169.188 | 107.276 | 184.629 | 201.720 |
| quic | callback | legacy | 5000 | 86.047 | 123.722 | 34.837 | 24.354 | 94.030 | 151.243 | 112.127 | 177.272 | 231.043 |
| quic | callback | portable | 5000 | 111.772 | 174.385 | 43.224 | 36.981 | 133.343 | 208.735 | 188.016 | 232.001 | 270.163 |
| quic | bulk&#45;64k | legacy | 5000 | 184.261 | 240.054 | 54.197 | 41.474 | 207.553 | 306.490 | 214.167 | 339.070 | 363.573 |
| quic | bulk&#45;64k | portable | 5000 | 186.401 | 221.571 | 38.137 | 27.344 | 205.674 | 270.294 | 209.308 | 312.504 | 343.964 |
| quic | bulk&#45;256k | legacy | 5000 | 465.954 | 564.882 | 679.030 | 51.185 | 509.116 | 647.983 | 522.862 | 758.069 | 832.567 |
| quic | bulk&#45;256k | portable | 5000 | 484.268 | 553.000 | 397.391 | 39.934 | 528.988 | 623.909 | 533.951 | 740.375 | 818.832 |
| quic | bulk&#45;1m | legacy | 5000 | 1724.052 | 2008.527 | 833.756 | 125.147 | 1868.970 | 2144.295 | 1881.359 | 2650.534 | 2843.933 |
| quic | bulk&#45;1m | portable | 5000 | 1714.507 | 2008.335 | 626.210 | 115.209 | 1934.615 | 2212.161 | 1900.333 | 2597.996 | 2731.077 |
| quic | bulk&#45;4m | legacy | 5000 | 8430.741 | 9568.176 | 1113.351 | 369.122 | 9190.836 | 10003.046 | 9300.639 | 12302.636 | 13156.208 |
| quic | bulk&#45;4m | portable | 5000 | 8471.531 | 9810.044 | 1302.624 | 200.039 | 9531.356 | 10057.641 | 9476.329 | 12670.069 | 13693.906 |
| quic | deep&#45;8 | legacy | 5000 | 42.791 | 54.633 | 13.849 | 8.790 | 47.729 | 69.104 | 48.595 | 79.614 | 98.234 |
| quic | deep&#45;8 | portable | 5000 | 47.419 | 67.710 | 18.669 | 18.150 | 53.874 | 89.098 | 57.326 | 98.260 | 114.702 |
| quic | deep&#45;32 | legacy | 5000 | 50.372 | 57.488 | 7.649 | 0.378 | 57.005 | 57.993 | 55.786 | 72.989 | 86.773 |
| quic | deep&#45;32 | portable | 5000 | 54.567 | 62.252 | 8.310 | 0.358 | 61.741 | 62.713 | 60.128 | 78.099 | 93.237 |
| quic | concurrent&#45;32 | legacy | 5000 | 123.547 | 140.884 | 15.831 | 3.791 | 137.843 | 147.372 | 139.438 | 170.617 | 197.601 |
| quic | concurrent&#45;32 | portable | 5000 | 151.849 | 221.962 | 60.308 | 58.857 | 168.982 | 294.053 | 194.058 | 318.611 | 340.095 |
| quic | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 433.469 | 516.649 | 45.464 | 24.176 | 494.142 | 551.292 | 511.596 | 593.406 | 627.923 |
| quic | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 445.947 | 531.181 | 45.415 | 23.240 | 501.951 | 561.011 | 527.502 | 603.713 | 632.189 |
| quic | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 376.718 | 440.338 | 34.122 | 4.887 | 431.709 | 443.693 | 438.742 | 494.341 | 533.894 |
| quic | services&#45;4&#45;deep&#45;16 | portable | 5000 | 381.651 | 442.323 | 33.539 | 9.241 | 427.700 | 449.838 | 441.368 | 498.677 | 538.246 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1704.295 | 1928.749 | 100.077 | 13.245 | 1909.408 | 1945.441 | 1928.494 | 2072.158 | 2128.647 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1742.246 | 1978.602 | 78.923 | 8.170 | 1970.040 | 1991.623 | 1981.245 | 2102.806 | 2164.398 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1676.584 | 2054.367 | 133.295 | 27.162 | 2019.630 | 2095.161 | 2051.032 | 2262.470 | 2347.942 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1560.007 | 2039.330 | 176.335 | 32.861 | 2010.308 | 2089.825 | 2055.191 | 2299.399 | 2399.228 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 3444.713 | 4117.256 | 197.652 | 29.478 | 4086.054 | 4165.715 | 4097.918 | 4462.976 | 4633.584 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3065.921 | 4048.061 | 341.938 | 33.821 | 4001.215 | 4096.780 | 4073.173 | 4572.385 | 4772.095 |

### Unsupported comparison cells

| Transport | Case | Implementation | Status |
| --- | --- | --- | --- |
| tls | pipelined&#45;read | grpc | Unsupported by this fixture&#47;protocol |
| tls | pipelined&#45;read | websocket | Unsupported by this fixture&#47;protocol |
| quic | loopback&#45;0 | cpp | Unsupported by this fixture&#47;protocol |
| quic | loopback&#45;64 | cpp | Unsupported by this fixture&#47;protocol |
| quic | loopback&#45;1024 | cpp | Unsupported by this fixture&#47;protocol |
| quic | loopback&#45;65536 | cpp | Unsupported by this fixture&#47;protocol |
| quic | unary&#45;empty | cpp | Unsupported by this fixture&#47;protocol |
| quic | unary&#45;small | cpp | Unsupported by this fixture&#47;protocol |
| quic | unary&#45;1k | cpp | Unsupported by this fixture&#47;protocol |
| quic | fanout&#45;32 | cpp | Unsupported by this fixture&#47;protocol |
| quic | dependent&#45;read | cpp | Unsupported by this fixture&#47;protocol |
| quic | pipelined&#45;read | cpp | Unsupported by this fixture&#47;protocol |
| quic | write&#45;read | cpp | Unsupported by this fixture&#47;protocol |
| quic | callback | cpp | Unsupported by this fixture&#47;protocol |
| quic | bulk&#45;64k | cpp | Unsupported by this fixture&#47;protocol |
| quic | bulk&#45;256k | cpp | Unsupported by this fixture&#47;protocol |
| quic | bulk&#45;1m | cpp | Unsupported by this fixture&#47;protocol |
| quic | bulk&#45;4m | cpp | Unsupported by this fixture&#47;protocol |
| quic | deep&#45;8 | cpp | Unsupported by this fixture&#47;protocol |
| quic | deep&#45;32 | cpp | Unsupported by this fixture&#47;protocol |
| quic | concurrent&#45;32 | cpp | Unsupported by this fixture&#47;protocol |
| quic | deep&#45;32&#45;concurrent&#45;32 | cpp | Unsupported by this fixture&#47;protocol |
| quic | services&#45;4&#45;deep&#45;16 | cpp | Unsupported by this fixture&#47;protocol |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | Unsupported by this fixture&#47;protocol |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | Unsupported by this fixture&#47;protocol |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | Unsupported by this fixture&#47;protocol |
| quic | loopback&#45;0 | grpc | Unsupported by this fixture&#47;protocol |
| quic | loopback&#45;64 | grpc | Unsupported by this fixture&#47;protocol |
| quic | loopback&#45;1024 | grpc | Unsupported by this fixture&#47;protocol |
| quic | loopback&#45;65536 | grpc | Unsupported by this fixture&#47;protocol |
| quic | unary&#45;empty | grpc | Unsupported by this fixture&#47;protocol |
| quic | unary&#45;small | grpc | Unsupported by this fixture&#47;protocol |
| quic | unary&#45;1k | grpc | Unsupported by this fixture&#47;protocol |
| quic | fanout&#45;32 | grpc | Unsupported by this fixture&#47;protocol |
| quic | dependent&#45;read | grpc | Unsupported by this fixture&#47;protocol |
| quic | pipelined&#45;read | grpc | Unsupported by this fixture&#47;protocol |
| quic | write&#45;read | grpc | Unsupported by this fixture&#47;protocol |
| quic | callback | grpc | Unsupported by this fixture&#47;protocol |
| quic | bulk&#45;64k | grpc | Unsupported by this fixture&#47;protocol |
| quic | bulk&#45;256k | grpc | Unsupported by this fixture&#47;protocol |
| quic | bulk&#45;1m | grpc | Unsupported by this fixture&#47;protocol |
| quic | bulk&#45;4m | grpc | Unsupported by this fixture&#47;protocol |
| quic | deep&#45;8 | grpc | Unsupported by this fixture&#47;protocol |
| quic | deep&#45;32 | grpc | Unsupported by this fixture&#47;protocol |
| quic | concurrent&#45;32 | grpc | Unsupported by this fixture&#47;protocol |
| quic | deep&#45;32&#45;concurrent&#45;32 | grpc | Unsupported by this fixture&#47;protocol |
| quic | services&#45;4&#45;deep&#45;16 | grpc | Unsupported by this fixture&#47;protocol |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | Unsupported by this fixture&#47;protocol |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | Unsupported by this fixture&#47;protocol |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | Unsupported by this fixture&#47;protocol |
| quic | loopback&#45;0 | websocket | Unsupported by this fixture&#47;protocol |
| quic | loopback&#45;64 | websocket | Unsupported by this fixture&#47;protocol |
| quic | loopback&#45;1024 | websocket | Unsupported by this fixture&#47;protocol |
| quic | loopback&#45;65536 | websocket | Unsupported by this fixture&#47;protocol |
| quic | unary&#45;empty | websocket | Unsupported by this fixture&#47;protocol |
| quic | unary&#45;small | websocket | Unsupported by this fixture&#47;protocol |
| quic | unary&#45;1k | websocket | Unsupported by this fixture&#47;protocol |
| quic | fanout&#45;32 | websocket | Unsupported by this fixture&#47;protocol |
| quic | dependent&#45;read | websocket | Unsupported by this fixture&#47;protocol |
| quic | pipelined&#45;read | websocket | Unsupported by this fixture&#47;protocol |
| quic | write&#45;read | websocket | Unsupported by this fixture&#47;protocol |
| quic | callback | websocket | Unsupported by this fixture&#47;protocol |
| quic | bulk&#45;64k | websocket | Unsupported by this fixture&#47;protocol |
| quic | bulk&#45;256k | websocket | Unsupported by this fixture&#47;protocol |
| quic | bulk&#45;1m | websocket | Unsupported by this fixture&#47;protocol |
| quic | bulk&#45;4m | websocket | Unsupported by this fixture&#47;protocol |
| quic | deep&#45;8 | websocket | Unsupported by this fixture&#47;protocol |
| quic | deep&#45;32 | websocket | Unsupported by this fixture&#47;protocol |
| quic | concurrent&#45;32 | websocket | Unsupported by this fixture&#47;protocol |
| quic | deep&#45;32&#45;concurrent&#45;32 | websocket | Unsupported by this fixture&#47;protocol |
| quic | services&#45;4&#45;deep&#45;16 | websocket | Unsupported by this fixture&#47;protocol |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | Unsupported by this fixture&#47;protocol |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | Unsupported by this fixture&#47;protocol |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | Unsupported by this fixture&#47;protocol |


