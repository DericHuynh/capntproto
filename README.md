# Capntproto verification reports

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-30T17:41:48Z · run 41100872709 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41100872709) · commit `230da39503e5` · **failure**

![Cargo test results](docs/reports/cargo-history.svg)

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-30T17:41:48Z · run 41100872709 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41100872709) · commit `230da39503e5` · **failure**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-30T17:41:48Z · run 41100872709 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41100872709) · commit `230da39503e5` · **failure**

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

Latest coverage run: [2026-10-30T17:41:48Z · run 41100872534 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41100872534) · commit `230da39503e5` · **failure**

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-30T17:41:48Z · run 41100872709 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41100872709) · commit `230da39503e5` · **failure**

Measurement evidence: **passed**. Independently validated encrypted benchmark values&#46; Workflow status and diagnostic completion are separate&#59; unavailable values remain blank&#46; Operations include whole concurrent batches and every service hop&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Current TCP fixtures use TLS 1.3, including upstream Rust Cap’n Proto, C++, gRPC over HTTP/2 and WebSockets over WSS; native encrypted transports and standard QUIC are identified separately. Historical data retains its recorded fixture contract. The dedicated campaign also reports recursive data, concurrent calls and independent service chains for gRPC and binary Protobuf WebSockets in its encrypted comparison artifact. Bars compare the recorded workload, rather than universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.


## Exhaustive benchmark tables

Measured run: [2026-10-30T17:41:48Z · run 41100872709 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41100872709) · commit `230da39503e5` · **failure**

All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.

### All encrypted latency comparisons

| Transport | Case | Implementation | Samples | Minimum µs | Mean µs | Sample SD µs | Run mean SD µs | Run mean min µs | Run mean max µs | p50 µs | p95 µs | p99 µs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tls | loopback&#45;0 | cpp | 5000 | 53.203 | 65.963 | 21.125 | 23.112 | 55.209 | 107.303 | 54.793 | 105.878 | 121.472 |
| tls | loopback&#45;0 | grpc | 5000 | 62.102 | 74.555 | 21.097 | 21.261 | 64.386 | 112.576 | 63.587 | 111.635 | 126.200 |
| tls | loopback&#45;0 | legacy | 5000 | 35.765 | 37.800 | 3.519 | 0.434 | 37.200 | 38.229 | 37.074 | 42.556 | 54.257 |
| tls | loopback&#45;0 | portable | 5000 | 37.290 | 44.889 | 42.555 | 12.380 | 38.962 | 67.027 | 38.566 | 66.515 | 76.834 |
| tls | loopback&#45;0 | websocket | 5000 | 33.667 | 36.369 | 3.019 | 0.195 | 36.154 | 36.619 | 35.833 | 40.375 | 50.455 |
| tls | loopback&#45;64 | cpp | 5000 | 52.775 | 67.432 | 21.632 | 22.003 | 55.006 | 106.543 | 54.702 | 105.947 | 119.879 |
| tls | loopback&#45;64 | grpc | 5000 | 62.139 | 74.439 | 19.646 | 21.353 | 64.619 | 112.633 | 63.698 | 111.779 | 123.322 |
| tls | loopback&#45;64 | legacy | 5000 | 35.999 | 43.408 | 10.541 | 10.867 | 37.876 | 62.753 | 37.292 | 62.100 | 71.048 |
| tls | loopback&#45;64 | portable | 5000 | 37.418 | 48.438 | 13.749 | 12.871 | 39.062 | 67.889 | 39.069 | 68.047 | 82.648 |
| tls | loopback&#45;64 | websocket | 5000 | 30.240 | 34.306 | 3.179 | 0.187 | 34.078 | 34.494 | 33.851 | 38.085 | 47.828 |
| tls | loopback&#45;1024 | cpp | 5000 | 55.447 | 79.263 | 25.863 | 28.472 | 57.942 | 110.486 | 59.884 | 116.962 | 124.990 |
| tls | loopback&#45;1024 | grpc | 5000 | 66.341 | 92.162 | 27.310 | 28.203 | 70.430 | 125.046 | 72.601 | 132.649 | 138.762 |
| tls | loopback&#45;1024 | legacy | 5000 | 37.964 | 45.523 | 11.246 | 12.053 | 39.655 | 67.070 | 39.464 | 66.547 | 75.382 |
| tls | loopback&#45;1024 | portable | 5000 | 39.540 | 53.956 | 15.295 | 16.522 | 41.357 | 72.747 | 42.214 | 74.439 | 85.917 |
| tls | loopback&#45;1024 | websocket | 5000 | 29.457 | 37.089 | 8.572 | 8.649 | 33.126 | 52.559 | 33.359 | 56.372 | 61.615 |
| tls | loopback&#45;65536 | cpp | 5000 | 227.161 | 241.980 | 8.624 | 0.910 | 240.996 | 242.936 | 239.934 | 257.068 | 272.650 |
| tls | loopback&#45;65536 | grpc | 5000 | 233.366 | 253.302 | 13.771 | 2.489 | 251.157 | 256.270 | 249.771 | 279.924 | 302.586 |
| tls | loopback&#45;65536 | legacy | 5000 | 158.010 | 164.375 | 6.240 | 1.087 | 162.726 | 165.677 | 161.958 | 175.232 | 183.506 |
| tls | loopback&#45;65536 | portable | 5000 | 164.893 | 170.281 | 6.010 | 0.511 | 169.717 | 170.963 | 167.587 | 181.697 | 189.455 |
| tls | loopback&#45;65536 | websocket | 5000 | 179.764 | 187.834 | 6.898 | 1.344 | 186.561 | 189.879 | 184.998 | 201.384 | 210.545 |
| tls | unary&#45;empty | cpp | 5000 | 52.993 | 57.310 | 6.192 | 2.131 | 55.607 | 60.998 | 54.650 | 67.725 | 79.666 |
| tls | unary&#45;empty | grpc | 5000 | 61.674 | 65.060 | 4.609 | 0.327 | 64.571 | 65.370 | 63.505 | 74.845 | 84.648 |
| tls | unary&#45;empty | legacy | 5000 | 35.913 | 37.977 | 3.214 | 0.128 | 37.777 | 38.093 | 37.046 | 43.103 | 52.213 |
| tls | unary&#45;empty | portable | 5000 | 37.223 | 39.463 | 3.316 | 0.320 | 39.099 | 39.960 | 38.501 | 45.240 | 54.439 |
| tls | unary&#45;empty | websocket | 5000 | 34.099 | 36.899 | 2.381 | 0.388 | 36.256 | 37.224 | 36.457 | 40.857 | 48.160 |
| tls | unary&#45;small | cpp | 5000 | 53.156 | 55.959 | 5.039 | 0.394 | 55.431 | 56.413 | 54.581 | 64.803 | 76.202 |
| tls | unary&#45;small | grpc | 5000 | 62.811 | 66.166 | 5.355 | 0.426 | 65.726 | 66.761 | 64.470 | 76.386 | 87.783 |
| tls | unary&#45;small | legacy | 5000 | 35.688 | 37.786 | 3.696 | 0.588 | 37.248 | 38.739 | 36.853 | 42.914 | 52.733 |
| tls | unary&#45;small | portable | 5000 | 37.485 | 39.535 | 3.410 | 0.185 | 39.319 | 39.799 | 38.424 | 45.523 | 56.447 |
| tls | unary&#45;small | websocket | 5000 | 30.896 | 34.972 | 4.062 | 0.205 | 34.831 | 35.324 | 34.233 | 39.805 | 49.891 |
| tls | unary&#45;1k | cpp | 5000 | 55.095 | 58.492 | 4.798 | 0.481 | 57.795 | 59.020 | 57.013 | 67.127 | 79.628 |
| tls | unary&#45;1k | grpc | 5000 | 66.754 | 72.783 | 5.878 | 1.618 | 70.952 | 75.209 | 71.768 | 83.863 | 96.886 |
| tls | unary&#45;1k | legacy | 5000 | 38.190 | 40.569 | 4.247 | 0.493 | 39.976 | 41.277 | 39.322 | 46.557 | 56.289 |
| tls | unary&#45;1k | portable | 5000 | 39.791 | 42.173 | 3.857 | 0.210 | 41.916 | 42.388 | 40.974 | 48.021 | 58.376 |
| tls | unary&#45;1k | websocket | 5000 | 29.726 | 34.189 | 4.243 | 0.521 | 33.694 | 35.057 | 33.366 | 40.166 | 49.472 |
| tls | fanout&#45;32 | cpp | 5000 | 433.536 | 472.667 | 47.991 | 18.835 | 453.102 | 493.032 | 456.478 | 585.290 | 660.805 |
| tls | fanout&#45;32 | grpc | 5000 | 796.221 | 833.340 | 18.217 | 4.582 | 828.375 | 838.170 | 831.251 | 857.932 | 887.507 |
| tls | fanout&#45;32 | legacy | 5000 | 172.739 | 181.540 | 9.923 | 1.859 | 179.421 | 183.081 | 177.932 | 201.323 | 221.968 |
| tls | fanout&#45;32 | portable | 5000 | 208.213 | 218.696 | 23.528 | 3.454 | 216.565 | 224.707 | 213.408 | 240.040 | 276.932 |
| tls | fanout&#45;32 | websocket | 5000 | 235.009 | 276.283 | 50.400 | 43.301 | 253.876 | 353.634 | 256.752 | 399.667 | 430.871 |
| tls | dependent&#45;read | cpp | 5000 | 122.258 | 127.663 | 5.999 | 0.750 | 126.873 | 128.788 | 125.296 | 138.656 | 148.419 |
| tls | dependent&#45;read | grpc | 5000 | 129.831 | 138.833 | 7.012 | 2.311 | 135.074 | 141.156 | 137.366 | 150.848 | 161.340 |
| tls | dependent&#45;read | legacy | 5000 | 77.188 | 82.726 | 4.220 | 0.477 | 82.232 | 83.431 | 81.196 | 91.771 | 100.607 |
| tls | dependent&#45;read | portable | 5000 | 97.616 | 102.376 | 5.599 | 0.603 | 101.618 | 103.053 | 100.227 | 113.883 | 124.644 |
| tls | dependent&#45;read | websocket | 5000 | 58.179 | 65.110 | 4.743 | 0.180 | 64.963 | 65.396 | 64.176 | 73.729 | 81.720 |
| tls | pipelined&#45;read | cpp | 5000 | 86.384 | 90.234 | 5.219 | 0.427 | 89.687 | 90.656 | 88.411 | 100.219 | 111.865 |
| tls | pipelined&#45;read | legacy | 5000 | 47.564 | 50.329 | 4.188 | 0.283 | 50.035 | 50.771 | 48.960 | 58.644 | 69.689 |
| tls | pipelined&#45;read | portable | 5000 | 70.833 | 75.214 | 6.478 | 0.330 | 74.996 | 75.795 | 72.868 | 88.316 | 100.651 |
| tls | write&#45;read | cpp | 5000 | 107.269 | 114.466 | 19.436 | 3.485 | 112.461 | 120.562 | 111.731 | 125.368 | 138.004 |
| tls | write&#45;read | grpc | 5000 | 131.397 | 140.040 | 6.254 | 1.174 | 138.757 | 141.508 | 137.946 | 152.339 | 161.997 |
| tls | write&#45;read | legacy | 5000 | 74.075 | 77.983 | 4.251 | 0.529 | 77.535 | 78.783 | 76.315 | 86.356 | 95.950 |
| tls | write&#45;read | portable | 5000 | 76.801 | 80.944 | 4.551 | 0.236 | 80.574 | 81.173 | 79.178 | 90.243 | 99.026 |
| tls | write&#45;read | websocket | 5000 | 57.731 | 65.520 | 4.961 | 0.582 | 65.060 | 66.437 | 64.657 | 74.177 | 82.413 |
| tls | callback | cpp | 5000 | 113.183 | 118.250 | 6.416 | 1.402 | 116.984 | 120.432 | 116.030 | 129.151 | 141.825 |
| tls | callback | grpc | 5000 | 68.567 | 71.964 | 3.926 | 0.763 | 71.141 | 73.083 | 70.812 | 79.701 | 88.900 |
| tls | callback | legacy | 5000 | 78.611 | 82.086 | 4.342 | 0.310 | 81.679 | 82.526 | 80.596 | 91.214 | 99.661 |
| tls | callback | portable | 5000 | 93.491 | 98.634 | 6.820 | 1.020 | 97.824 | 100.197 | 96.341 | 110.751 | 120.844 |
| tls | callback | websocket | 5000 | 59.218 | 66.922 | 4.942 | 0.151 | 66.676 | 67.045 | 66.344 | 74.644 | 82.273 |
| tls | bulk&#45;64k | cpp | 5000 | 225.753 | 240.090 | 8.775 | 0.738 | 238.886 | 240.912 | 236.925 | 256.390 | 269.438 |
| tls | bulk&#45;64k | grpc | 5000 | 235.618 | 254.190 | 13.414 | 1.552 | 252.735 | 256.820 | 250.538 | 282.660 | 303.994 |
| tls | bulk&#45;64k | legacy | 5000 | 158.724 | 164.371 | 6.045 | 0.531 | 163.768 | 165.067 | 161.724 | 175.983 | 185.657 |
| tls | bulk&#45;64k | portable | 5000 | 164.984 | 171.006 | 17.954 | 0.971 | 170.041 | 172.464 | 167.576 | 183.398 | 195.627 |
| tls | bulk&#45;64k | websocket | 5000 | 182.288 | 189.293 | 8.990 | 0.578 | 188.482 | 189.935 | 185.629 | 206.256 | 218.448 |
| tls | bulk&#45;256k | cpp | 5000 | 951.375 | 997.477 | 25.718 | 19.852 | 984.814 | 1032.570 | 989.287 | 1045.656 | 1066.399 |
| tls | bulk&#45;256k | grpc | 5000 | 574.201 | 1208.490 | 351.782 | 330.181 | 742.015 | 1670.444 | 1163.527 | 1835.611 | 1884.611 |
| tls | bulk&#45;256k | legacy | 5000 | 466.592 | 493.721 | 12.039 | 3.294 | 488.109 | 496.305 | 492.744 | 509.543 | 525.815 |
| tls | bulk&#45;256k | portable | 5000 | 478.644 | 503.828 | 13.414 | 5.771 | 500.646 | 514.073 | 503.216 | 522.142 | 535.369 |
| tls | bulk&#45;256k | websocket | 5000 | 1142.342 | 1176.637 | 36.798 | 22.298 | 1165.476 | 1216.469 | 1166.033 | 1281.613 | 1300.781 |
| tls | bulk&#45;1m | cpp | 5000 | 3585.904 | 3838.870 | 96.833 | 53.290 | 3761.640 | 3898.058 | 3832.628 | 3979.433 | 4248.791 |
| tls | bulk&#45;1m | grpc | 5000 | 2229.121 | 3785.689 | 959.657 | 975.039 | 2286.769 | 4952.012 | 3926.186 | 5135.223 | 6327.959 |
| tls | bulk&#45;1m | legacy | 5000 | 1599.061 | 1641.926 | 20.185 | 7.354 | 1634.579 | 1653.622 | 1639.580 | 1666.486 | 1710.559 |
| tls | bulk&#45;1m | portable | 5000 | 1601.692 | 1645.737 | 30.347 | 2.019 | 1642.586 | 1647.643 | 1642.938 | 1668.789 | 1703.026 |
| tls | bulk&#45;1m | websocket | 5000 | 4586.692 | 4800.570 | 188.733 | 82.542 | 4706.470 | 4921.506 | 4749.726 | 5401.915 | 5535.676 |
| tls | bulk&#45;4m | cpp | 5000 | 15985.775 | 16953.841 | 1025.590 | 94.646 | 16847.546 | 17092.589 | 16760.353 | 17662.904 | 23934.034 |
| tls | bulk&#45;4m | grpc | 5000 | 21054.188 | 26580.644 | 2982.362 | 2810.846 | 22492.668 | 29124.886 | 26995.968 | 31594.168 | 32115.475 |
| tls | bulk&#45;4m | legacy | 5000 | 7529.000 | 8420.389 | 834.959 | 503.491 | 7893.951 | 9007.300 | 8074.111 | 10218.427 | 10420.120 |
| tls | bulk&#45;4m | portable | 5000 | 7529.307 | 8203.794 | 559.206 | 341.811 | 7950.498 | 8794.114 | 8057.240 | 9823.577 | 10073.404 |
| tls | bulk&#45;4m | websocket | 5000 | 27729.045 | 30412.968 | 2488.031 | 1152.336 | 29394.834 | 32346.634 | 29131.852 | 34945.850 | 35301.432 |
| tls | deep&#45;8 | cpp | 5000 | 55.860 | 66.167 | 19.637 | 17.121 | 58.003 | 96.788 | 57.536 | 111.724 | 125.319 |
| tls | deep&#45;8 | grpc | 5000 | 74.527 | 77.666 | 4.285 | 0.354 | 77.214 | 78.009 | 76.238 | 86.413 | 97.097 |
| tls | deep&#45;8 | legacy | 5000 | 38.822 | 46.228 | 12.013 | 13.010 | 40.176 | 69.499 | 39.788 | 68.732 | 80.041 |
| tls | deep&#45;8 | portable | 5000 | 37.726 | 48.529 | 13.270 | 14.386 | 41.575 | 74.254 | 41.479 | 73.415 | 84.786 |
| tls | deep&#45;8 | websocket | 5000 | 34.123 | 41.147 | 4.600 | 0.223 | 40.832 | 41.362 | 40.411 | 49.238 | 55.440 |
| tls | deep&#45;32 | cpp | 5000 | 62.513 | 66.004 | 5.036 | 0.843 | 65.137 | 67.411 | 64.133 | 76.102 | 87.432 |
| tls | deep&#45;32 | grpc | 5000 | 91.975 | 99.398 | 8.271 | 1.316 | 98.122 | 101.618 | 97.091 | 115.574 | 132.135 |
| tls | deep&#45;32 | legacy | 5000 | 45.274 | 47.957 | 3.289 | 0.504 | 47.652 | 48.850 | 46.804 | 53.825 | 62.993 |
| tls | deep&#45;32 | portable | 5000 | 46.414 | 49.013 | 3.935 | 0.128 | 48.922 | 49.227 | 47.630 | 56.284 | 67.623 |
| tls | deep&#45;32 | websocket | 5000 | 51.740 | 58.817 | 4.969 | 0.472 | 58.394 | 59.613 | 57.991 | 67.235 | 76.693 |
| tls | concurrent&#45;32 | cpp | 5000 | 470.787 | 503.023 | 45.241 | 8.971 | 488.773 | 513.044 | 497.502 | 522.344 | 696.320 |
| tls | concurrent&#45;32 | grpc | 5000 | 594.149 | 619.546 | 35.438 | 3.755 | 615.273 | 624.780 | 611.613 | 644.449 | 810.941 |
| tls | concurrent&#45;32 | legacy | 5000 | 120.390 | 127.343 | 6.425 | 1.004 | 126.517 | 128.963 | 125.655 | 138.837 | 152.195 |
| tls | concurrent&#45;32 | portable | 5000 | 150.987 | 161.697 | 11.494 | 2.881 | 158.487 | 165.364 | 158.469 | 185.206 | 205.510 |
| tls | concurrent&#45;32 | websocket | 5000 | 105.467 | 163.715 | 17.501 | 0.633 | 162.953 | 164.354 | 163.108 | 193.107 | 207.358 |
| tls | deep&#45;32&#45;concurrent&#45;32 | cpp | 5000 | 631.953 | 787.515 | 252.070 | 268.493 | 661.120 | 1267.749 | 665.353 | 1345.325 | 1361.260 |
| tls | deep&#45;32&#45;concurrent&#45;32 | grpc | 5000 | 1476.987 | 1650.857 | 304.527 | 219.756 | 1542.065 | 2043.640 | 1547.254 | 2506.284 | 2781.894 |
| tls | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 410.455 | 450.987 | 81.891 | 42.625 | 430.110 | 527.105 | 428.852 | 638.369 | 809.366 |
| tls | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 424.489 | 450.567 | 24.716 | 3.682 | 447.672 | 455.637 | 446.973 | 466.977 | 639.249 |
| tls | deep&#45;32&#45;concurrent&#45;32 | websocket | 5000 | 664.072 | 720.515 | 37.860 | 4.352 | 713.831 | 724.739 | 714.203 | 749.060 | 939.898 |
| tls | services&#45;4&#45;deep&#45;16 | cpp | 5000 | 453.225 | 491.385 | 18.729 | 4.670 | 483.685 | 495.219 | 489.886 | 521.940 | 534.518 |
| tls | services&#45;4&#45;deep&#45;16 | grpc | 5000 | 584.413 | 627.207 | 18.365 | 6.497 | 620.706 | 636.654 | 626.977 | 657.035 | 672.171 |
| tls | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 333.886 | 362.749 | 38.905 | 5.786 | 357.562 | 372.380 | 356.858 | 389.509 | 427.486 |
| tls | services&#45;4&#45;deep&#45;16 | portable | 5000 | 375.365 | 396.486 | 19.084 | 1.588 | 394.883 | 398.499 | 391.840 | 423.944 | 438.137 |
| tls | services&#45;4&#45;deep&#45;16 | websocket | 5000 | 263.005 | 329.008 | 35.244 | 2.072 | 326.205 | 330.884 | 326.832 | 387.337 | 406.504 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | 5000 | 3007.054 | 3595.004 | 208.875 | 52.797 | 3503.828 | 3637.872 | 3579.657 | 3925.854 | 4135.421 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | 5000 | 4724.599 | 5593.978 | 344.540 | 134.957 | 5405.910 | 5761.863 | 5654.077 | 6078.562 | 6296.556 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1793.727 | 2100.478 | 126.360 | 21.380 | 2070.613 | 2127.983 | 2088.823 | 2290.863 | 2460.509 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1900.342 | 2144.294 | 126.321 | 32.599 | 2102.484 | 2182.581 | 2135.748 | 2344.591 | 2455.397 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | 5000 | 1647.132 | 1944.367 | 89.186 | 28.670 | 1900.593 | 1969.938 | 1942.989 | 2094.304 | 2162.848 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 2626.838 | 3332.463 | 440.768 | 207.290 | 3169.513 | 3679.961 | 3195.313 | 4042.561 | 4155.177 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 4528.172 | 5649.228 | 453.360 | 214.275 | 5373.745 | 5968.567 | 5703.321 | 6335.766 | 6506.236 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1634.565 | 2038.735 | 96.545 | 45.710 | 2003.953 | 2114.946 | 2012.529 | 2234.294 | 2341.036 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1912.826 | 2162.942 | 112.258 | 55.388 | 2071.546 | 2216.102 | 2166.854 | 2338.610 | 2408.523 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1423.545 | 1731.538 | 143.883 | 49.893 | 1686.309 | 1815.614 | 1720.401 | 1935.034 | 2041.801 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 4983.721 | 6797.209 | 788.688 | 93.818 | 6696.211 | 6894.796 | 6919.246 | 7891.838 | 8143.370 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 8160.480 | 10577.936 | 1098.831 | 357.809 | 9984.287 | 10922.610 | 10715.944 | 12197.118 | 12588.821 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 3828.866 | 4220.792 | 223.196 | 125.017 | 4067.266 | 4343.236 | 4159.611 | 4625.815 | 4772.997 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3739.711 | 4550.890 | 167.208 | 49.037 | 4508.472 | 4619.000 | 4584.908 | 4737.163 | 4838.616 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 2693.365 | 3242.955 | 232.500 | 30.583 | 3199.327 | 3276.339 | 3207.456 | 3654.540 | 3891.823 |
| quic | loopback&#45;0 | legacy | 5000 | 47.839 | 50.635 | 3.693 | 0.649 | 49.580 | 51.302 | 49.756 | 56.443 | 66.111 |
| quic | loopback&#45;0 | portable | 5000 | 53.757 | 56.753 | 3.728 | 0.574 | 56.007 | 57.411 | 55.731 | 64.074 | 73.079 |
| quic | loopback&#45;64 | legacy | 5000 | 48.707 | 51.245 | 3.649 | 0.450 | 50.789 | 51.860 | 50.211 | 57.696 | 65.643 |
| quic | loopback&#45;64 | portable | 5000 | 54.119 | 56.920 | 4.177 | 0.374 | 56.434 | 57.395 | 55.696 | 64.737 | 75.125 |
| quic | loopback&#45;1024 | legacy | 5000 | 50.715 | 74.246 | 19.885 | 19.690 | 53.034 | 91.918 | 89.282 | 99.119 | 107.023 |
| quic | loopback&#45;1024 | portable | 5000 | 56.941 | 77.993 | 23.067 | 25.247 | 59.142 | 105.776 | 60.114 | 111.636 | 119.339 |
| quic | loopback&#45;65536 | legacy | 5000 | 204.353 | 235.815 | 49.770 | 31.400 | 214.974 | 286.095 | 212.403 | 352.714 | 363.659 |
| quic | loopback&#45;65536 | portable | 5000 | 212.859 | 277.177 | 67.329 | 74.476 | 221.930 | 358.852 | 229.754 | 370.308 | 378.307 |
| quic | unary&#45;empty | legacy | 5000 | 48.369 | 51.040 | 4.143 | 0.282 | 50.677 | 51.283 | 49.956 | 58.041 | 67.015 |
| quic | unary&#45;empty | portable | 5000 | 53.973 | 56.823 | 3.731 | 0.375 | 56.413 | 57.295 | 55.672 | 64.825 | 72.973 |
| quic | unary&#45;small | legacy | 5000 | 48.145 | 50.667 | 4.115 | 0.599 | 49.866 | 51.550 | 49.678 | 57.150 | 65.490 |
| quic | unary&#45;small | portable | 5000 | 54.002 | 56.819 | 3.411 | 0.503 | 56.188 | 57.393 | 55.848 | 64.101 | 71.277 |
| quic | unary&#45;1k | legacy | 5000 | 50.007 | 52.621 | 3.116 | 0.341 | 52.149 | 53.002 | 51.676 | 60.035 | 65.880 |
| quic | unary&#45;1k | portable | 5000 | 56.047 | 59.035 | 3.654 | 0.430 | 58.469 | 59.677 | 57.933 | 66.827 | 74.100 |
| quic | fanout&#45;32 | legacy | 5000 | 205.715 | 251.565 | 73.337 | 80.722 | 213.967 | 395.950 | 213.813 | 410.195 | 434.655 |
| quic | fanout&#45;32 | portable | 5000 | 244.624 | 253.768 | 9.280 | 0.815 | 252.690 | 254.497 | 250.647 | 267.703 | 280.116 |
| quic | dependent&#45;read | legacy | 5000 | 104.681 | 109.455 | 6.576 | 0.513 | 108.581 | 109.779 | 107.464 | 120.273 | 132.094 |
| quic | dependent&#45;read | portable | 5000 | 137.471 | 144.399 | 7.489 | 0.681 | 143.476 | 145.188 | 141.880 | 157.673 | 170.099 |
| quic | pipelined&#45;read | legacy | 5000 | 61.860 | 70.168 | 15.816 | 10.937 | 64.670 | 89.720 | 63.906 | 113.071 | 125.186 |
| quic | pipelined&#45;read | portable | 5000 | 93.977 | 114.664 | 32.123 | 35.137 | 97.906 | 177.508 | 97.541 | 177.392 | 194.092 |
| quic | write&#45;read | legacy | 5000 | 99.103 | 154.863 | 35.297 | 33.564 | 104.296 | 178.692 | 173.889 | 190.871 | 200.390 |
| quic | write&#45;read | portable | 5000 | 111.606 | 170.652 | 44.298 | 48.617 | 116.734 | 206.739 | 200.783 | 219.871 | 228.922 |
| quic | callback | legacy | 5000 | 103.633 | 109.765 | 7.168 | 0.851 | 109.017 | 111.032 | 107.399 | 122.488 | 136.701 |
| quic | callback | portable | 5000 | 130.919 | 138.795 | 8.377 | 1.109 | 136.981 | 139.942 | 136.103 | 155.329 | 167.845 |
| quic | bulk&#45;64k | legacy | 5000 | 204.806 | 215.393 | 9.583 | 1.155 | 213.923 | 216.709 | 211.702 | 236.458 | 248.150 |
| quic | bulk&#45;64k | portable | 5000 | 211.235 | 221.542 | 9.922 | 0.743 | 220.406 | 222.395 | 217.547 | 241.642 | 255.366 |
| quic | bulk&#45;256k | legacy | 5000 | 516.231 | 534.243 | 10.268 | 1.129 | 532.946 | 535.935 | 532.409 | 549.937 | 563.451 |
| quic | bulk&#45;256k | portable | 5000 | 534.425 | 556.097 | 14.002 | 3.939 | 552.975 | 562.685 | 554.048 | 578.743 | 600.352 |
| quic | bulk&#45;1m | legacy | 5000 | 1865.150 | 1933.329 | 38.378 | 11.499 | 1921.396 | 1952.414 | 1927.780 | 1975.773 | 2067.651 |
| quic | bulk&#45;1m | portable | 5000 | 1859.967 | 1955.170 | 685.694 | 42.332 | 1917.268 | 2023.477 | 1923.760 | 1984.853 | 2716.103 |
| quic | bulk&#45;4m | legacy | 5000 | 8807.512 | 9411.930 | 325.042 | 110.193 | 9266.569 | 9570.545 | 9383.610 | 9783.200 | 10067.912 |
| quic | bulk&#45;4m | portable | 5000 | 8915.900 | 9650.208 | 374.756 | 88.643 | 9509.865 | 9748.784 | 9616.064 | 10109.468 | 10450.812 |
| quic | deep&#45;8 | legacy | 5000 | 51.672 | 86.716 | 17.032 | 18.201 | 54.169 | 95.328 | 93.020 | 105.530 | 110.542 |
| quic | deep&#45;8 | portable | 5000 | 57.442 | 89.739 | 23.270 | 25.398 | 61.063 | 108.550 | 105.606 | 118.144 | 122.924 |
| quic | deep&#45;32 | legacy | 5000 | 59.906 | 74.538 | 21.803 | 23.799 | 63.079 | 117.089 | 63.022 | 116.569 | 129.386 |
| quic | deep&#45;32 | portable | 5000 | 65.804 | 92.982 | 29.208 | 31.294 | 69.581 | 129.161 | 71.480 | 137.406 | 143.480 |
| quic | concurrent&#45;32 | legacy | 5000 | 137.296 | 186.103 | 75.875 | 75.190 | 140.972 | 316.067 | 141.730 | 323.767 | 329.430 |
| quic | concurrent&#45;32 | portable | 5000 | 172.260 | 265.308 | 103.587 | 115.305 | 178.481 | 392.152 | 187.014 | 403.004 | 407.846 |
| quic | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 465.390 | 730.485 | 200.289 | 172.635 | 477.627 | 871.663 | 882.570 | 921.704 | 932.878 |
| quic | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 480.809 | 678.068 | 188.470 | 185.170 | 491.912 | 864.754 | 568.510 | 890.212 | 896.776 |
| quic | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 502.578 | 529.680 | 12.334 | 4.510 | 522.087 | 533.742 | 529.592 | 549.498 | 557.618 |
| quic | services&#45;4&#45;deep&#45;16 | portable | 5000 | 489.408 | 516.382 | 13.431 | 8.400 | 505.523 | 527.170 | 515.688 | 537.378 | 547.012 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 2137.326 | 2467.698 | 102.350 | 27.418 | 2419.171 | 2483.672 | 2478.682 | 2604.978 | 2682.623 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 2165.835 | 2496.074 | 102.830 | 59.436 | 2408.501 | 2546.012 | 2507.747 | 2664.904 | 2722.944 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2060.003 | 2443.493 | 143.169 | 11.929 | 2434.674 | 2464.346 | 2434.221 | 2684.272 | 2800.474 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1880.383 | 2432.879 | 199.728 | 19.481 | 2406.388 | 2456.683 | 2498.658 | 2714.544 | 2787.673 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 3820.337 | 5010.237 | 272.284 | 98.109 | 4873.503 | 5145.786 | 5047.642 | 5403.073 | 5559.666 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3614.115 | 4349.491 | 491.032 | 297.924 | 4016.977 | 4750.910 | 4282.592 | 5099.979 | 5428.378 |

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


