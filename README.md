# Capntproto verification reports

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-31T05:05:03Z · run 41171142037 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41171142037) · commit `256241c2801c` · **failure**

![Cargo test results](docs/reports/cargo-history.svg)

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-31T05:05:03Z · run 41171142037 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41171142037) · commit `256241c2801c` · **failure**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-31T05:05:03Z · run 41171142037 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41171142037) · commit `256241c2801c` · **failure**

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

Latest coverage run: [2026-10-31T05:05:02Z · run 41171141966 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41171141966) · commit `256241c2801c` · **failure**

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-31T05:05:03Z · run 41171142037 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41171142037) · commit `256241c2801c` · **failure**

Measurement evidence: **passed**. Independently validated encrypted benchmark values&#46; Workflow status and diagnostic completion are separate&#59; unavailable values remain blank&#46; Operations include whole concurrent batches and every service hop&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Current TCP fixtures use TLS 1.3, including upstream Rust Cap’n Proto, C++, gRPC over HTTP/2 and WebSockets over WSS; native encrypted transports and standard QUIC are identified separately. Historical data retains its recorded fixture contract. The dedicated campaign also reports recursive data, concurrent calls and independent service chains for gRPC and binary Protobuf WebSockets in its encrypted comparison artifact. Bars compare the recorded workload, rather than universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.


## Exhaustive benchmark tables

Measured run: [2026-10-31T05:05:03Z · run 41171142037 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41171142037) · commit `256241c2801c` · **failure**

All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.

### All encrypted latency comparisons

| Transport | Case | Implementation | Samples | Minimum µs | Mean µs | Sample SD µs | Run mean SD µs | Run mean min µs | Run mean max µs | p50 µs | p95 µs | p99 µs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tls | loopback&#45;0 | cpp | 5000 | 45.244 | 64.760 | 15.494 | 14.462 | 51.334 | 81.946 | 58.683 | 89.383 | 102.571 |
| tls | loopback&#45;0 | grpc | 5000 | 52.325 | 76.373 | 17.629 | 15.476 | 59.198 | 91.980 | 81.311 | 103.234 | 115.431 |
| tls | loopback&#45;0 | legacy | 5000 | 30.133 | 44.320 | 9.825 | 9.482 | 33.691 | 51.865 | 47.125 | 59.416 | 68.633 |
| tls | loopback&#45;0 | portable | 5000 | 31.583 | 47.965 | 11.220 | 10.862 | 35.934 | 56.681 | 51.923 | 65.510 | 75.402 |
| tls | loopback&#45;0 | websocket | 5000 | 26.572 | 39.906 | 8.903 | 6.605 | 31.757 | 47.078 | 43.172 | 51.156 | 61.532 |
| tls | loopback&#45;64 | cpp | 5000 | 45.846 | 52.433 | 5.707 | 0.887 | 51.278 | 53.592 | 50.944 | 64.898 | 75.323 |
| tls | loopback&#45;64 | grpc | 5000 | 52.966 | 59.627 | 6.316 | 0.495 | 59.191 | 60.216 | 57.918 | 72.943 | 84.700 |
| tls | loopback&#45;64 | legacy | 5000 | 29.991 | 35.226 | 5.298 | 1.932 | 33.683 | 38.581 | 33.515 | 46.356 | 54.713 |
| tls | loopback&#45;64 | portable | 5000 | 31.362 | 36.127 | 4.883 | 0.743 | 35.220 | 36.729 | 34.775 | 47.084 | 56.351 |
| tls | loopback&#45;64 | websocket | 5000 | 24.439 | 30.010 | 4.355 | 0.352 | 29.763 | 30.579 | 29.004 | 38.610 | 48.279 |
| tls | loopback&#45;1024 | cpp | 5000 | 48.128 | 68.733 | 41.849 | 15.759 | 54.271 | 86.015 | 57.815 | 94.852 | 107.552 |
| tls | loopback&#45;1024 | grpc | 5000 | 56.170 | 85.187 | 18.585 | 18.617 | 64.362 | 99.419 | 91.403 | 112.169 | 126.425 |
| tls | loopback&#45;1024 | legacy | 5000 | 32.403 | 47.532 | 9.891 | 9.424 | 37.046 | 55.679 | 50.406 | 62.353 | 72.291 |
| tls | loopback&#45;1024 | portable | 5000 | 33.527 | 51.600 | 12.507 | 12.468 | 37.490 | 61.167 | 56.259 | 70.207 | 79.657 |
| tls | loopback&#45;1024 | websocket | 5000 | 24.660 | 39.273 | 9.240 | 7.801 | 30.054 | 45.021 | 39.446 | 54.500 | 64.256 |
| tls | loopback&#45;65536 | cpp | 5000 | 200.994 | 282.510 | 62.264 | 61.000 | 225.824 | 352.761 | 253.570 | 370.392 | 388.638 |
| tls | loopback&#45;65536 | grpc | 5000 | 200.629 | 292.766 | 53.904 | 55.341 | 231.800 | 339.065 | 312.440 | 366.962 | 389.490 |
| tls | loopback&#45;65536 | legacy | 5000 | 145.101 | 176.597 | 33.279 | 34.817 | 159.119 | 238.785 | 161.612 | 246.230 | 270.007 |
| tls | loopback&#45;65536 | portable | 5000 | 151.735 | 202.121 | 44.430 | 47.702 | 167.174 | 255.525 | 176.317 | 270.366 | 284.645 |
| tls | loopback&#45;65536 | websocket | 5000 | 161.143 | 201.380 | 39.312 | 41.259 | 180.485 | 275.089 | 184.357 | 282.604 | 300.603 |
| tls | unary&#45;empty | cpp | 5000 | 45.597 | 51.916 | 5.968 | 0.846 | 51.065 | 53.246 | 50.426 | 64.754 | 75.945 |
| tls | unary&#45;empty | grpc | 5000 | 53.090 | 60.183 | 6.348 | 1.183 | 58.472 | 61.434 | 58.431 | 73.984 | 84.566 |
| tls | unary&#45;empty | legacy | 5000 | 29.981 | 34.013 | 4.442 | 0.245 | 33.652 | 34.327 | 32.858 | 43.435 | 53.576 |
| tls | unary&#45;empty | portable | 5000 | 31.165 | 35.511 | 4.835 | 0.299 | 35.105 | 35.925 | 34.248 | 46.465 | 55.656 |
| tls | unary&#45;empty | websocket | 5000 | 26.928 | 31.749 | 4.118 | 0.564 | 31.118 | 32.573 | 31.003 | 40.155 | 49.503 |
| tls | unary&#45;small | cpp | 5000 | 45.572 | 69.340 | 15.847 | 16.015 | 51.270 | 81.809 | 74.948 | 92.631 | 104.695 |
| tls | unary&#45;small | grpc | 5000 | 53.478 | 75.644 | 19.628 | 15.795 | 59.941 | 97.723 | 64.852 | 105.884 | 121.642 |
| tls | unary&#45;small | legacy | 5000 | 29.421 | 43.780 | 9.284 | 9.014 | 33.108 | 50.512 | 46.728 | 57.720 | 66.984 |
| tls | unary&#45;small | portable | 5000 | 31.173 | 46.513 | 12.053 | 11.029 | 34.808 | 58.093 | 51.616 | 64.328 | 75.116 |
| tls | unary&#45;small | websocket | 5000 | 25.390 | 39.315 | 7.814 | 4.619 | 31.676 | 43.383 | 39.798 | 50.846 | 60.876 |
| tls | unary&#45;1k | cpp | 5000 | 48.273 | 61.301 | 13.248 | 12.746 | 55.095 | 84.095 | 55.282 | 88.050 | 99.153 |
| tls | unary&#45;1k | grpc | 5000 | 57.283 | 79.782 | 19.518 | 19.472 | 64.026 | 101.392 | 68.832 | 111.227 | 127.322 |
| tls | unary&#45;1k | legacy | 5000 | 32.127 | 41.942 | 9.235 | 7.331 | 36.006 | 54.181 | 37.570 | 56.027 | 69.212 |
| tls | unary&#45;1k | portable | 5000 | 33.498 | 38.392 | 5.191 | 0.996 | 37.569 | 40.060 | 36.947 | 49.306 | 59.751 |
| tls | unary&#45;1k | websocket | 5000 | 24.371 | 30.714 | 4.746 | 0.983 | 29.607 | 31.620 | 29.791 | 39.824 | 50.013 |
| tls | fanout&#45;32 | cpp | 5000 | 402.647 | 613.779 | 137.830 | 129.885 | 484.406 | 748.975 | 599.859 | 809.407 | 854.635 |
| tls | fanout&#45;32 | grpc | 5000 | 680.475 | 883.443 | 143.822 | 139.824 | 752.125 | 1037.085 | 804.467 | 1088.536 | 1131.758 |
| tls | fanout&#45;32 | legacy | 5000 | 157.469 | 196.911 | 45.472 | 48.601 | 173.014 | 283.814 | 175.336 | 292.390 | 316.961 |
| tls | fanout&#45;32 | portable | 5000 | 191.405 | 240.741 | 56.144 | 60.661 | 211.456 | 349.187 | 215.153 | 359.503 | 378.563 |
| tls | fanout&#45;32 | websocket | 5000 | 197.200 | 270.906 | 52.474 | 54.280 | 229.821 | 332.299 | 247.198 | 358.213 | 380.381 |
| tls | dependent&#45;read | cpp | 5000 | 106.664 | 126.207 | 10.986 | 3.944 | 121.376 | 130.706 | 124.197 | 146.989 | 161.566 |
| tls | dependent&#45;read | grpc | 5000 | 111.695 | 128.185 | 10.866 | 1.882 | 124.910 | 129.443 | 125.739 | 148.979 | 167.597 |
| tls | dependent&#45;read | legacy | 5000 | 66.359 | 76.715 | 7.245 | 2.060 | 74.698 | 79.083 | 75.107 | 91.122 | 103.082 |
| tls | dependent&#45;read | portable | 5000 | 83.935 | 98.112 | 9.067 | 1.771 | 96.668 | 101.143 | 96.539 | 115.434 | 130.179 |
| tls | dependent&#45;read | websocket | 5000 | 47.560 | 59.511 | 7.000 | 1.383 | 57.828 | 60.785 | 58.210 | 72.996 | 85.053 |
| tls | pipelined&#45;read | cpp | 5000 | 130.909 | 144.495 | 9.560 | 0.648 | 143.927 | 145.359 | 141.403 | 163.770 | 176.989 |
| tls | pipelined&#45;read | legacy | 5000 | 40.995 | 68.486 | 12.118 | 11.521 | 47.970 | 75.273 | 70.688 | 85.892 | 98.050 |
| tls | pipelined&#45;read | portable | 5000 | 66.001 | 115.365 | 24.620 | 22.934 | 74.382 | 127.119 | 120.952 | 140.632 | 157.798 |
| tls | write&#45;read | cpp | 5000 | 93.694 | 143.239 | 30.573 | 32.187 | 107.287 | 168.462 | 154.515 | 184.758 | 196.066 |
| tls | write&#45;read | grpc | 5000 | 112.004 | 184.930 | 31.490 | 32.565 | 126.885 | 203.295 | 194.008 | 221.939 | 237.421 |
| tls | write&#45;read | legacy | 5000 | 62.119 | 91.663 | 18.092 | 18.404 | 70.419 | 106.370 | 98.126 | 117.581 | 129.896 |
| tls | write&#45;read | portable | 5000 | 65.118 | 100.205 | 22.436 | 23.332 | 74.404 | 119.567 | 109.016 | 130.183 | 144.585 |
| tls | write&#45;read | websocket | 5000 | 49.429 | 77.760 | 16.341 | 13.756 | 60.372 | 89.776 | 79.076 | 104.209 | 114.330 |
| tls | callback | cpp | 5000 | 98.850 | 115.147 | 12.772 | 1.765 | 113.007 | 117.562 | 112.837 | 133.548 | 147.969 |
| tls | callback | grpc | 5000 | 56.401 | 64.812 | 6.224 | 1.865 | 61.921 | 66.961 | 63.263 | 78.059 | 87.575 |
| tls | callback | legacy | 5000 | 65.567 | 76.083 | 6.846 | 0.797 | 75.362 | 77.438 | 74.276 | 90.586 | 101.381 |
| tls | callback | portable | 5000 | 81.506 | 93.188 | 8.312 | 1.217 | 91.882 | 94.862 | 90.846 | 109.969 | 124.087 |
| tls | callback | websocket | 5000 | 47.322 | 57.729 | 6.467 | 1.066 | 55.963 | 58.838 | 56.435 | 70.559 | 80.709 |
| tls | bulk&#45;64k | cpp | 5000 | 200.724 | 223.154 | 14.443 | 2.146 | 220.474 | 226.412 | 220.040 | 251.676 | 270.243 |
| tls | bulk&#45;64k | grpc | 5000 | 202.239 | 232.506 | 15.267 | 2.728 | 229.725 | 236.164 | 228.662 | 263.874 | 278.975 |
| tls | bulk&#45;64k | legacy | 5000 | 144.017 | 159.640 | 11.124 | 3.001 | 157.127 | 164.706 | 156.747 | 181.465 | 196.071 |
| tls | bulk&#45;64k | portable | 5000 | 151.149 | 166.750 | 12.132 | 1.396 | 165.495 | 168.636 | 163.395 | 189.856 | 209.579 |
| tls | bulk&#45;64k | websocket | 5000 | 162.636 | 179.412 | 12.764 | 2.298 | 176.177 | 181.575 | 176.470 | 202.993 | 216.963 |
| tls | bulk&#45;256k | cpp | 5000 | 774.011 | 862.059 | 62.624 | 26.393 | 846.059 | 907.937 | 852.706 | 919.515 | 1190.855 |
| tls | bulk&#45;256k | grpc | 5000 | 495.672 | 844.103 | 289.342 | 299.250 | 558.726 | 1281.975 | 865.104 | 1398.558 | 1510.151 |
| tls | bulk&#45;256k | legacy | 5000 | 430.583 | 517.935 | 80.737 | 81.326 | 472.973 | 661.147 | 479.985 | 681.846 | 713.067 |
| tls | bulk&#45;256k | portable | 5000 | 439.775 | 494.642 | 48.408 | 23.225 | 481.130 | 535.989 | 482.965 | 640.660 | 699.794 |
| tls | bulk&#45;256k | websocket | 5000 | 905.422 | 1001.320 | 54.424 | 44.174 | 972.726 | 1078.883 | 989.346 | 1109.885 | 1142.767 |
| tls | bulk&#45;1m | cpp | 5000 | 3001.208 | 3744.536 | 632.979 | 541.024 | 3215.185 | 4459.847 | 3324.970 | 4689.712 | 4819.755 |
| tls | bulk&#45;1m | grpc | 5000 | 2222.173 | 4439.190 | 787.082 | 785.680 | 3112.142 | 5199.582 | 4503.100 | 5584.958 | 5883.301 |
| tls | bulk&#45;1m | legacy | 5000 | 1499.609 | 1732.104 | 205.746 | 198.979 | 1586.218 | 2039.719 | 1620.280 | 2090.145 | 2142.154 |
| tls | bulk&#45;1m | portable | 5000 | 1506.031 | 1754.230 | 209.966 | 163.804 | 1596.704 | 2022.863 | 1636.846 | 2093.679 | 2143.126 |
| tls | bulk&#45;1m | websocket | 5000 | 3673.318 | 4217.001 | 320.176 | 225.530 | 4041.416 | 4588.459 | 4119.900 | 4911.050 | 5119.047 |
| tls | bulk&#45;4m | cpp | 5000 | 13986.118 | 15644.153 | 1684.206 | 897.686 | 15062.306 | 17199.981 | 15055.051 | 20025.541 | 20588.477 |
| tls | bulk&#45;4m | grpc | 5000 | 17590.919 | 21311.383 | 1981.570 | 1601.361 | 19345.750 | 23115.015 | 21922.820 | 23715.222 | 26500.910 |
| tls | bulk&#45;4m | legacy | 5000 | 7680.637 | 8357.361 | 382.272 | 83.944 | 8223.042 | 8419.470 | 8295.361 | 8735.486 | 10232.674 |
| tls | bulk&#45;4m | portable | 5000 | 7767.371 | 8528.134 | 566.804 | 279.828 | 8371.038 | 9026.014 | 8384.264 | 10123.099 | 10480.819 |
| tls | bulk&#45;4m | websocket | 5000 | 25006.222 | 27780.912 | 2689.770 | 2188.583 | 26263.999 | 31547.652 | 26243.633 | 32300.309 | 32544.386 |
| tls | deep&#45;8 | cpp | 5000 | 48.482 | 73.551 | 15.545 | 15.841 | 55.918 | 85.714 | 79.535 | 95.238 | 106.484 |
| tls | deep&#45;8 | grpc | 5000 | 64.230 | 97.309 | 21.898 | 22.744 | 71.539 | 116.331 | 105.926 | 126.939 | 140.065 |
| tls | deep&#45;8 | legacy | 5000 | 32.511 | 37.299 | 4.437 | 0.908 | 36.087 | 38.440 | 36.057 | 46.513 | 55.996 |
| tls | deep&#45;8 | portable | 5000 | 34.209 | 48.225 | 12.011 | 12.196 | 38.714 | 62.269 | 41.056 | 66.012 | 77.360 |
| tls | deep&#45;8 | websocket | 5000 | 27.981 | 40.095 | 9.313 | 5.992 | 36.902 | 50.751 | 37.967 | 58.050 | 68.019 |
| tls | deep&#45;32 | cpp | 5000 | 54.925 | 88.844 | 15.846 | 13.854 | 64.612 | 97.171 | 91.726 | 109.523 | 120.641 |
| tls | deep&#45;32 | grpc | 5000 | 87.075 | 147.240 | 12.163 | 0.887 | 146.238 | 148.208 | 145.546 | 169.361 | 180.802 |
| tls | deep&#45;32 | legacy | 5000 | 62.424 | 68.689 | 5.782 | 0.488 | 68.133 | 69.427 | 67.517 | 81.035 | 91.180 |
| tls | deep&#45;32 | portable | 5000 | 40.694 | 68.637 | 12.753 | 12.330 | 46.619 | 74.931 | 71.774 | 86.184 | 96.844 |
| tls | deep&#45;32 | websocket | 5000 | 74.656 | 89.355 | 9.711 | 1.243 | 87.299 | 90.432 | 87.580 | 106.650 | 118.486 |
| tls | concurrent&#45;32 | cpp | 5000 | 444.285 | 528.672 | 35.460 | 24.619 | 501.585 | 552.480 | 529.456 | 585.933 | 604.103 |
| tls | concurrent&#45;32 | grpc | 5000 | 550.192 | 663.492 | 57.116 | 49.520 | 608.306 | 720.846 | 664.197 | 749.721 | 773.420 |
| tls | concurrent&#45;32 | legacy | 5000 | 108.672 | 128.138 | 17.174 | 5.983 | 124.103 | 138.583 | 124.110 | 153.818 | 204.681 |
| tls | concurrent&#45;32 | portable | 5000 | 136.759 | 220.807 | 55.499 | 60.305 | 153.060 | 267.893 | 249.507 | 286.909 | 298.568 |
| tls | concurrent&#45;32 | websocket | 5000 | 90.963 | 176.557 | 40.781 | 32.865 | 147.954 | 221.801 | 166.849 | 246.032 | 267.210 |
| tls | deep&#45;32&#45;concurrent&#45;32 | cpp | 5000 | 591.292 | 776.061 | 132.734 | 126.678 | 665.672 | 924.826 | 708.770 | 988.076 | 1039.273 |
| tls | deep&#45;32&#45;concurrent&#45;32 | grpc | 5000 | 1367.029 | 1626.787 | 138.200 | 126.602 | 1523.309 | 1786.532 | 1583.683 | 1862.811 | 1914.610 |
| tls | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 402.819 | 471.466 | 40.570 | 30.355 | 443.086 | 517.018 | 467.099 | 543.288 | 575.871 |
| tls | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 411.505 | 494.589 | 37.543 | 30.880 | 458.706 | 518.878 | 495.887 | 555.470 | 584.799 |
| tls | deep&#45;32&#45;concurrent&#45;32 | websocket | 5000 | 660.899 | 833.781 | 137.723 | 129.283 | 739.397 | 1029.514 | 765.919 | 1076.094 | 1117.677 |
| tls | services&#45;4&#45;deep&#45;16 | cpp | 5000 | 342.296 | 400.143 | 27.335 | 4.584 | 395.112 | 405.882 | 397.585 | 444.867 | 466.929 |
| tls | services&#45;4&#45;deep&#45;16 | grpc | 5000 | 479.392 | 559.467 | 31.819 | 5.712 | 551.396 | 565.750 | 556.511 | 613.020 | 635.572 |
| tls | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 255.420 | 297.956 | 22.932 | 3.632 | 292.724 | 302.834 | 294.858 | 337.628 | 359.878 |
| tls | services&#45;4&#45;deep&#45;16 | portable | 5000 | 299.923 | 349.079 | 25.356 | 4.748 | 343.500 | 354.453 | 346.577 | 391.318 | 412.633 |
| tls | services&#45;4&#45;deep&#45;16 | websocket | 5000 | 212.864 | 287.514 | 36.488 | 2.174 | 284.212 | 289.623 | 282.325 | 353.400 | 378.048 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | 5000 | 2484.482 | 2750.427 | 91.819 | 35.857 | 2701.049 | 2801.543 | 2745.818 | 2909.362 | 2970.632 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | 5000 | 4235.154 | 4673.048 | 149.002 | 57.654 | 4627.389 | 4746.478 | 4664.655 | 4929.366 | 5053.073 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1473.542 | 1607.111 | 58.619 | 11.993 | 1593.942 | 1625.069 | 1602.725 | 1696.619 | 1736.819 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1608.195 | 1748.507 | 55.050 | 9.594 | 1736.877 | 1757.902 | 1744.523 | 1837.167 | 1873.652 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | 5000 | 1319.223 | 1575.435 | 93.560 | 52.859 | 1530.870 | 1644.854 | 1567.359 | 1743.232 | 1811.555 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 2146.740 | 2599.373 | 131.087 | 41.184 | 2546.363 | 2655.870 | 2609.716 | 2794.919 | 2869.820 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 3595.601 | 4384.208 | 236.802 | 102.500 | 4226.205 | 4496.347 | 4363.695 | 4791.785 | 4907.218 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1200.518 | 1602.785 | 62.197 | 22.983 | 1579.061 | 1631.193 | 1599.763 | 1709.497 | 1751.500 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1510.568 | 1794.351 | 96.818 | 36.856 | 1757.294 | 1841.174 | 1783.565 | 1959.740 | 2011.863 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1151.391 | 1470.208 | 115.867 | 20.637 | 1454.771 | 1505.287 | 1465.111 | 1666.861 | 1759.468 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 4112.382 | 5449.482 | 251.168 | 60.727 | 5371.289 | 5524.479 | 5417.792 | 5907.407 | 6137.436 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 6936.838 | 8926.598 | 538.648 | 121.520 | 8809.060 | 9132.518 | 8962.936 | 9718.872 | 10070.028 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 3069.480 | 3391.533 | 118.798 | 24.794 | 3363.846 | 3423.755 | 3380.998 | 3578.083 | 3707.961 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2853.434 | 3622.494 | 242.799 | 126.609 | 3473.342 | 3761.712 | 3626.047 | 4012.900 | 4120.545 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 2337.324 | 2873.898 | 229.825 | 29.670 | 2829.331 | 2897.355 | 2832.610 | 3334.294 | 3586.640 |
| quic | loopback&#45;0 | legacy | 5000 | 39.145 | 47.052 | 8.995 | 5.194 | 43.822 | 56.286 | 43.807 | 68.138 | 77.361 |
| quic | loopback&#45;0 | portable | 5000 | 43.619 | 49.657 | 5.411 | 0.750 | 48.476 | 50.566 | 48.351 | 61.749 | 71.646 |
| quic | loopback&#45;64 | legacy | 5000 | 39.532 | 50.367 | 11.174 | 10.830 | 45.066 | 69.726 | 45.024 | 72.021 | 83.197 |
| quic | loopback&#45;64 | portable | 5000 | 43.776 | 59.062 | 15.028 | 13.032 | 50.133 | 80.443 | 50.745 | 84.844 | 98.428 |
| quic | loopback&#45;1024 | legacy | 5000 | 41.150 | 47.133 | 5.364 | 0.654 | 46.444 | 48.212 | 45.699 | 58.930 | 67.152 |
| quic | loopback&#45;1024 | portable | 5000 | 46.038 | 51.983 | 5.837 | 0.652 | 50.914 | 52.663 | 50.395 | 64.531 | 75.692 |
| quic | loopback&#45;65536 | legacy | 5000 | 181.427 | 200.786 | 12.417 | 3.049 | 196.767 | 204.897 | 198.289 | 224.492 | 240.771 |
| quic | loopback&#45;65536 | portable | 5000 | 188.846 | 208.012 | 12.357 | 1.780 | 205.579 | 209.805 | 205.447 | 231.529 | 244.932 |
| quic | unary&#45;empty | legacy | 5000 | 39.349 | 45.122 | 5.522 | 0.995 | 43.725 | 46.370 | 43.622 | 57.020 | 67.212 |
| quic | unary&#45;empty | portable | 5000 | 43.223 | 49.399 | 5.605 | 0.814 | 48.635 | 50.632 | 47.987 | 61.267 | 70.489 |
| quic | unary&#45;small | legacy | 5000 | 38.827 | 45.101 | 5.061 | 0.993 | 43.975 | 46.323 | 43.675 | 56.249 | 65.762 |
| quic | unary&#45;small | portable | 5000 | 43.197 | 49.965 | 5.465 | 0.934 | 48.965 | 51.368 | 48.478 | 61.908 | 72.216 |
| quic | unary&#45;1k | legacy | 5000 | 40.891 | 46.838 | 5.198 | 0.919 | 45.475 | 47.721 | 45.489 | 57.694 | 68.477 |
| quic | unary&#45;1k | portable | 5000 | 45.875 | 64.975 | 16.919 | 17.462 | 51.823 | 84.848 | 54.668 | 91.433 | 103.490 |
| quic | fanout&#45;32 | legacy | 5000 | 184.265 | 203.100 | 13.854 | 2.743 | 199.904 | 207.110 | 199.842 | 228.588 | 245.868 |
| quic | fanout&#45;32 | portable | 5000 | 220.642 | 242.519 | 14.051 | 2.687 | 238.482 | 245.240 | 239.300 | 267.868 | 280.962 |
| quic | dependent&#45;read | legacy | 5000 | 85.983 | 97.984 | 8.936 | 1.898 | 96.335 | 100.916 | 95.688 | 116.590 | 130.714 |
| quic | dependent&#45;read | portable | 5000 | 118.568 | 132.649 | 10.977 | 0.414 | 132.206 | 133.143 | 129.627 | 156.088 | 169.497 |
| quic | pipelined&#45;read | legacy | 5000 | 51.683 | 58.817 | 6.416 | 0.588 | 58.194 | 59.709 | 57.122 | 72.395 | 85.851 |
| quic | pipelined&#45;read | portable | 5000 | 84.117 | 93.557 | 9.265 | 0.740 | 92.620 | 94.344 | 90.760 | 114.854 | 127.357 |
| quic | write&#45;read | legacy | 5000 | 79.601 | 92.364 | 9.203 | 1.021 | 91.224 | 93.788 | 90.147 | 112.373 | 125.683 |
| quic | write&#45;read | portable | 5000 | 89.429 | 102.136 | 9.612 | 1.052 | 100.894 | 103.425 | 99.575 | 121.747 | 137.502 |
| quic | callback | legacy | 5000 | 85.429 | 97.306 | 9.247 | 1.464 | 95.985 | 99.729 | 94.940 | 115.967 | 132.347 |
| quic | callback | portable | 5000 | 112.097 | 125.623 | 11.249 | 2.469 | 123.024 | 128.908 | 122.714 | 147.316 | 168.482 |
| quic | bulk&#45;64k | legacy | 5000 | 183.180 | 198.961 | 13.582 | 1.092 | 197.113 | 199.918 | 195.671 | 226.974 | 245.774 |
| quic | bulk&#45;64k | portable | 5000 | 184.556 | 201.967 | 14.254 | 1.188 | 200.780 | 203.306 | 198.034 | 231.941 | 251.023 |
| quic | bulk&#45;256k | legacy | 5000 | 459.388 | 496.100 | 21.463 | 2.999 | 491.169 | 498.872 | 493.103 | 534.020 | 554.623 |
| quic | bulk&#45;256k | portable | 5000 | 475.865 | 518.241 | 21.953 | 1.626 | 516.188 | 520.129 | 515.478 | 557.398 | 576.722 |
| quic | bulk&#45;1m | legacy | 5000 | 1712.990 | 2032.819 | 648.343 | 269.726 | 1858.958 | 2508.703 | 1867.283 | 2625.694 | 2697.287 |
| quic | bulk&#45;1m | portable | 5000 | 1721.847 | 2146.975 | 362.533 | 361.656 | 1872.945 | 2627.621 | 1923.976 | 2716.458 | 2782.750 |
| quic | bulk&#45;4m | legacy | 5000 | 8705.741 | 9822.147 | 997.256 | 627.658 | 9453.770 | 10940.030 | 9528.773 | 12759.984 | 13161.898 |
| quic | bulk&#45;4m | portable | 5000 | 8906.494 | 10110.061 | 1073.272 | 359.207 | 9663.241 | 10480.608 | 9808.740 | 12876.295 | 13420.774 |
| quic | deep&#45;8 | legacy | 5000 | 41.842 | 48.384 | 5.067 | 0.811 | 47.841 | 49.808 | 47.166 | 59.290 | 68.834 |
| quic | deep&#45;8 | portable | 5000 | 47.008 | 53.883 | 6.125 | 0.838 | 52.658 | 54.993 | 52.099 | 67.155 | 78.539 |
| quic | deep&#45;32 | legacy | 5000 | 50.065 | 57.075 | 6.115 | 0.833 | 55.824 | 57.810 | 55.504 | 69.965 | 82.131 |
| quic | deep&#45;32 | portable | 5000 | 53.523 | 61.720 | 6.417 | 1.088 | 60.601 | 63.535 | 59.921 | 75.294 | 86.629 |
| quic | concurrent&#45;32 | legacy | 5000 | 121.820 | 137.851 | 10.427 | 2.645 | 133.736 | 140.906 | 135.789 | 157.965 | 171.880 |
| quic | concurrent&#45;32 | portable | 5000 | 154.037 | 170.508 | 11.571 | 1.610 | 168.620 | 172.518 | 168.003 | 193.259 | 206.959 |
| quic | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 435.606 | 481.377 | 23.690 | 3.547 | 475.175 | 484.195 | 480.479 | 518.462 | 537.919 |
| quic | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 446.750 | 489.202 | 20.553 | 2.129 | 486.639 | 491.334 | 487.957 | 524.636 | 539.148 |
| quic | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 383.504 | 438.345 | 26.964 | 6.911 | 430.667 | 447.643 | 435.518 | 480.447 | 499.545 |
| quic | services&#45;4&#45;deep&#45;16 | portable | 5000 | 385.293 | 437.761 | 24.030 | 6.094 | 432.365 | 448.066 | 435.586 | 479.321 | 498.601 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1705.831 | 1912.385 | 82.535 | 32.106 | 1881.710 | 1959.002 | 1908.385 | 2052.592 | 2091.546 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1766.268 | 1989.372 | 64.839 | 17.300 | 1958.763 | 1999.825 | 1991.784 | 2088.778 | 2138.202 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1656.101 | 2016.273 | 123.488 | 43.748 | 1966.018 | 2069.906 | 2020.028 | 2208.376 | 2275.293 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1588.538 | 2000.279 | 154.305 | 31.065 | 1968.455 | 2052.306 | 2018.444 | 2223.993 | 2284.871 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 3489.077 | 4100.333 | 184.333 | 19.388 | 4074.020 | 4126.550 | 4088.319 | 4410.951 | 4576.451 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3096.320 | 4109.797 | 368.752 | 51.721 | 4058.391 | 4187.599 | 4129.759 | 4669.475 | 4833.720 |

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


