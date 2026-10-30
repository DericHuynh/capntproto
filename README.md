# Capntproto verification reports

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-30T07:21:11Z · run 41019558972 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41019558972) · commit `bd2463c95c0a` · **failure**

![Cargo test results](docs/reports/cargo-history.svg)

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-30T07:21:11Z · run 41019558972 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41019558972) · commit `bd2463c95c0a` · **failure**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-30T07:21:11Z · run 41019558972 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41019558972) · commit `bd2463c95c0a` · **failure**

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

Latest coverage run: [2026-10-30T07:21:11Z · run 41019558973 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41019558973) · commit `bd2463c95c0a` · **failure**

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-30T07:21:11Z · run 41019558972 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41019558972) · commit `bd2463c95c0a` · **failure**

Measurement evidence: **passed**. Independently validated encrypted benchmark values&#46; Workflow status and diagnostic completion are separate&#59; unavailable values remain blank&#46; Operations include whole concurrent batches and every service hop&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Current TCP fixtures use TLS 1.3, including upstream Rust Cap’n Proto, C++, gRPC over HTTP/2 and WebSockets over WSS; native encrypted transports and standard QUIC are identified separately. Historical data retains its recorded fixture contract. The dedicated campaign also reports recursive data, concurrent calls and independent service chains for gRPC and binary Protobuf WebSockets in its encrypted comparison artifact. Bars compare the recorded workload, rather than universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.


## Exhaustive benchmark tables

Measured run: [2026-10-30T07:21:11Z · run 41019558972 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41019558972) · commit `bd2463c95c0a` · **failure**

All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.

### All encrypted latency comparisons

| Transport | Case | Implementation | Samples | Minimum µs | Mean µs | Sample SD µs | Run mean SD µs | Run mean min µs | Run mean max µs | p50 µs | p95 µs | p99 µs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tls | loopback&#45;0 | cpp | 5000 | 43.569 | 46.416 | 3.811 | 0.809 | 45.213 | 47.179 | 45.466 | 53.608 | 63.335 |
| tls | loopback&#45;0 | grpc | 5000 | 51.091 | 58.872 | 13.289 | 10.381 | 52.968 | 77.372 | 53.296 | 82.412 | 94.752 |
| tls | loopback&#45;0 | legacy | 5000 | 29.669 | 33.832 | 5.646 | 5.364 | 31.356 | 43.425 | 30.917 | 43.087 | 51.902 |
| tls | loopback&#45;0 | portable | 5000 | 30.643 | 32.408 | 3.122 | 0.373 | 31.940 | 32.738 | 31.677 | 37.479 | 47.611 |
| tls | loopback&#45;0 | websocket | 5000 | 22.003 | 29.904 | 4.592 | 4.305 | 27.609 | 37.552 | 27.520 | 37.451 | 44.696 |
| tls | loopback&#45;64 | cpp | 5000 | 43.748 | 45.601 | 3.618 | 0.148 | 45.373 | 45.787 | 44.663 | 51.729 | 62.245 |
| tls | loopback&#45;64 | grpc | 5000 | 50.301 | 53.685 | 3.673 | 0.297 | 53.302 | 54.044 | 52.678 | 61.322 | 71.143 |
| tls | loopback&#45;64 | legacy | 5000 | 29.609 | 31.214 | 2.759 | 0.216 | 30.860 | 31.395 | 30.614 | 34.374 | 45.706 |
| tls | loopback&#45;64 | portable | 5000 | 30.904 | 32.496 | 2.887 | 0.250 | 32.137 | 32.750 | 31.847 | 36.506 | 47.589 |
| tls | loopback&#45;64 | websocket | 5000 | 21.771 | 26.766 | 2.391 | 0.301 | 26.347 | 27.167 | 26.344 | 29.529 | 38.526 |
| tls | loopback&#45;1024 | cpp | 5000 | 45.421 | 48.216 | 3.771 | 0.301 | 47.871 | 48.544 | 47.139 | 54.483 | 65.489 |
| tls | loopback&#45;1024 | grpc | 5000 | 56.615 | 61.336 | 4.127 | 0.799 | 60.315 | 62.146 | 60.541 | 69.649 | 79.096 |
| tls | loopback&#45;1024 | legacy | 5000 | 30.872 | 33.534 | 3.275 | 0.545 | 32.964 | 34.120 | 32.484 | 39.073 | 49.135 |
| tls | loopback&#45;1024 | portable | 5000 | 32.373 | 34.649 | 33.966 | 1.053 | 34.017 | 36.506 | 33.349 | 38.336 | 49.938 |
| tls | loopback&#45;1024 | websocket | 5000 | 24.427 | 27.591 | 2.751 | 0.258 | 27.174 | 27.781 | 27.185 | 31.079 | 41.048 |
| tls | loopback&#45;65536 | cpp | 5000 | 167.815 | 202.624 | 38.666 | 31.397 | 179.691 | 249.133 | 183.123 | 273.063 | 282.487 |
| tls | loopback&#45;65536 | grpc | 5000 | 169.220 | 187.889 | 23.254 | 2.648 | 184.747 | 190.860 | 183.552 | 214.849 | 235.485 |
| tls | loopback&#45;65536 | legacy | 5000 | 105.833 | 126.528 | 24.093 | 23.526 | 109.347 | 157.786 | 109.777 | 167.529 | 176.327 |
| tls | loopback&#45;65536 | portable | 5000 | 106.686 | 124.248 | 23.557 | 25.639 | 111.903 | 170.097 | 111.456 | 170.236 | 186.184 |
| tls | loopback&#45;65536 | websocket | 5000 | 120.116 | 126.340 | 9.997 | 3.012 | 124.148 | 131.577 | 123.131 | 138.152 | 173.606 |
| tls | unary&#45;empty | cpp | 5000 | 43.762 | 45.941 | 3.556 | 0.353 | 45.743 | 46.568 | 44.946 | 52.480 | 63.735 |
| tls | unary&#45;empty | grpc | 5000 | 51.465 | 54.112 | 4.029 | 0.548 | 53.550 | 55.007 | 52.898 | 61.815 | 73.154 |
| tls | unary&#45;empty | legacy | 5000 | 29.559 | 31.047 | 2.874 | 0.179 | 30.813 | 31.217 | 30.396 | 34.187 | 46.403 |
| tls | unary&#45;empty | portable | 5000 | 30.314 | 32.126 | 2.900 | 0.244 | 31.993 | 32.560 | 31.415 | 36.338 | 48.356 |
| tls | unary&#45;empty | websocket | 5000 | 24.306 | 27.695 | 2.461 | 0.047 | 27.632 | 27.753 | 27.220 | 29.094 | 40.243 |
| tls | unary&#45;small | cpp | 5000 | 43.732 | 46.119 | 3.527 | 0.551 | 45.519 | 46.770 | 45.140 | 53.337 | 62.815 |
| tls | unary&#45;small | grpc | 5000 | 52.495 | 55.119 | 3.983 | 0.552 | 54.664 | 55.960 | 53.919 | 62.790 | 73.807 |
| tls | unary&#45;small | legacy | 5000 | 29.666 | 31.291 | 2.707 | 0.398 | 30.915 | 31.916 | 30.630 | 35.121 | 45.551 |
| tls | unary&#45;small | portable | 5000 | 30.044 | 32.110 | 2.779 | 0.274 | 31.804 | 32.476 | 31.469 | 36.123 | 45.645 |
| tls | unary&#45;small | websocket | 5000 | 23.686 | 27.031 | 2.530 | 0.221 | 26.788 | 27.380 | 26.614 | 29.721 | 39.342 |
| tls | unary&#45;1k | cpp | 5000 | 45.902 | 47.933 | 3.108 | 0.080 | 47.846 | 48.009 | 46.980 | 54.279 | 62.797 |
| tls | unary&#45;1k | grpc | 5000 | 57.218 | 61.077 | 4.525 | 0.927 | 60.129 | 62.231 | 60.102 | 70.181 | 79.091 |
| tls | unary&#45;1k | legacy | 5000 | 31.421 | 33.114 | 2.781 | 0.209 | 32.914 | 33.430 | 32.357 | 36.816 | 46.618 |
| tls | unary&#45;1k | portable | 5000 | 32.646 | 34.424 | 2.999 | 0.188 | 34.165 | 34.657 | 33.549 | 39.483 | 48.986 |
| tls | unary&#45;1k | websocket | 5000 | 24.597 | 27.758 | 2.667 | 0.259 | 27.473 | 28.061 | 27.384 | 31.541 | 40.039 |
| tls | fanout&#45;32 | cpp | 5000 | 363.190 | 404.997 | 40.264 | 2.946 | 401.047 | 408.847 | 388.797 | 480.942 | 510.331 |
| tls | fanout&#45;32 | grpc | 5000 | 633.474 | 664.119 | 20.019 | 3.011 | 659.495 | 667.142 | 661.984 | 683.529 | 700.767 |
| tls | fanout&#45;32 | legacy | 5000 | 133.677 | 140.365 | 8.295 | 1.802 | 138.875 | 143.308 | 137.409 | 152.955 | 182.195 |
| tls | fanout&#45;32 | portable | 5000 | 163.850 | 173.476 | 6.522 | 0.884 | 172.851 | 174.835 | 170.804 | 185.983 | 196.500 |
| tls | fanout&#45;32 | websocket | 5000 | 180.249 | 201.073 | 15.330 | 0.603 | 200.323 | 201.684 | 195.543 | 228.907 | 237.036 |
| tls | dependent&#45;read | cpp | 5000 | 101.663 | 106.692 | 11.666 | 0.472 | 106.235 | 107.346 | 104.500 | 118.914 | 127.410 |
| tls | dependent&#45;read | grpc | 5000 | 108.100 | 115.915 | 4.893 | 0.591 | 115.352 | 116.658 | 114.182 | 126.935 | 134.537 |
| tls | dependent&#45;read | legacy | 5000 | 64.693 | 67.536 | 3.644 | 0.495 | 67.000 | 68.249 | 66.373 | 75.457 | 84.143 |
| tls | dependent&#45;read | portable | 5000 | 79.216 | 82.728 | 4.744 | 0.376 | 82.276 | 83.111 | 81.116 | 93.804 | 101.128 |
| tls | dependent&#45;read | websocket | 5000 | 48.200 | 54.042 | 3.936 | 0.200 | 53.789 | 54.329 | 53.485 | 61.309 | 68.080 |
| tls | pipelined&#45;read | cpp | 5000 | 72.574 | 76.236 | 4.681 | 0.357 | 75.854 | 76.691 | 74.580 | 87.600 | 94.708 |
| tls | pipelined&#45;read | legacy | 5000 | 39.209 | 41.317 | 3.116 | 0.282 | 40.966 | 41.717 | 40.426 | 46.651 | 56.935 |
| tls | pipelined&#45;read | portable | 5000 | 55.865 | 59.610 | 4.538 | 0.659 | 59.129 | 60.764 | 58.114 | 69.392 | 78.578 |
| tls | write&#45;read | cpp | 5000 | 88.646 | 93.097 | 5.083 | 0.971 | 92.067 | 94.699 | 91.246 | 104.977 | 113.650 |
| tls | write&#45;read | grpc | 5000 | 110.577 | 116.520 | 8.285 | 0.520 | 116.019 | 117.320 | 114.513 | 127.603 | 136.481 |
| tls | write&#45;read | legacy | 5000 | 61.016 | 63.897 | 3.685 | 0.325 | 63.441 | 64.330 | 62.708 | 72.047 | 80.282 |
| tls | write&#45;read | portable | 5000 | 61.968 | 65.831 | 12.812 | 0.338 | 65.377 | 66.183 | 64.453 | 74.432 | 83.089 |
| tls | write&#45;read | websocket | 5000 | 48.557 | 54.185 | 7.072 | 0.380 | 53.707 | 54.649 | 53.531 | 61.073 | 69.541 |
| tls | callback | cpp | 5000 | 94.352 | 109.488 | 22.585 | 23.966 | 98.434 | 152.357 | 97.243 | 152.684 | 169.279 |
| tls | callback | grpc | 5000 | 57.658 | 63.034 | 9.230 | 7.040 | 59.488 | 75.617 | 58.982 | 84.859 | 92.677 |
| tls | callback | legacy | 5000 | 65.285 | 68.271 | 3.985 | 0.188 | 68.075 | 68.496 | 66.959 | 77.280 | 86.279 |
| tls | callback | portable | 5000 | 76.065 | 82.208 | 10.523 | 4.549 | 79.340 | 90.226 | 78.895 | 99.524 | 127.329 |
| tls | callback | websocket | 5000 | 48.578 | 52.896 | 3.724 | 0.224 | 52.560 | 53.191 | 52.265 | 59.508 | 68.865 |
| tls | bulk&#45;64k | cpp | 5000 | 166.550 | 198.673 | 35.077 | 37.292 | 179.902 | 265.303 | 182.644 | 270.618 | 281.662 |
| tls | bulk&#45;64k | grpc | 5000 | 172.155 | 186.091 | 12.296 | 1.063 | 184.475 | 187.165 | 181.266 | 212.841 | 233.780 |
| tls | bulk&#45;64k | legacy | 5000 | 105.549 | 118.878 | 21.341 | 21.222 | 109.016 | 156.835 | 108.177 | 162.548 | 178.797 |
| tls | bulk&#45;64k | portable | 5000 | 107.701 | 124.340 | 24.772 | 27.020 | 110.990 | 172.656 | 111.037 | 173.255 | 188.880 |
| tls | bulk&#45;64k | websocket | 5000 | 119.435 | 126.111 | 5.614 | 0.516 | 125.498 | 126.703 | 124.128 | 136.629 | 143.284 |
| tls | bulk&#45;256k | cpp | 5000 | 668.150 | 712.411 | 16.867 | 3.507 | 708.832 | 716.939 | 709.878 | 741.461 | 753.586 |
| tls | bulk&#45;256k | grpc | 5000 | 529.125 | 1039.716 | 149.651 | 106.722 | 890.167 | 1163.984 | 1040.150 | 1274.198 | 1321.856 |
| tls | bulk&#45;256k | legacy | 5000 | 298.379 | 325.719 | 40.591 | 30.744 | 311.063 | 380.704 | 309.881 | 439.841 | 452.249 |
| tls | bulk&#45;256k | portable | 5000 | 306.024 | 336.887 | 45.801 | 37.604 | 319.343 | 404.143 | 318.546 | 459.189 | 467.384 |
| tls | bulk&#45;256k | websocket | 5000 | 745.953 | 817.593 | 37.547 | 34.612 | 771.796 | 853.309 | 831.858 | 865.168 | 879.693 |
| tls | bulk&#45;1m | cpp | 5000 | 2623.359 | 2753.828 | 154.606 | 36.812 | 2725.770 | 2815.752 | 2727.815 | 2846.864 | 3807.794 |
| tls | bulk&#45;1m | grpc | 5000 | 1790.168 | 3257.350 | 873.340 | 856.392 | 2570.036 | 4371.371 | 2667.230 | 4744.382 | 4872.236 |
| tls | bulk&#45;1m | legacy | 5000 | 1045.876 | 1135.410 | 104.215 | 54.281 | 1098.479 | 1220.112 | 1098.968 | 1416.279 | 1444.317 |
| tls | bulk&#45;1m | portable | 5000 | 1050.932 | 1104.342 | 39.816 | 3.442 | 1099.801 | 1109.465 | 1099.884 | 1128.895 | 1194.110 |
| tls | bulk&#45;1m | websocket | 5000 | 3263.842 | 3378.450 | 98.023 | 22.896 | 3351.174 | 3412.959 | 3355.424 | 3496.724 | 3826.153 |
| tls | bulk&#45;4m | cpp | 5000 | 11625.672 | 12260.755 | 854.167 | 173.885 | 12129.541 | 12563.106 | 12025.763 | 13083.321 | 16395.760 |
| tls | bulk&#45;4m | grpc | 5000 | 10978.701 | 18226.239 | 3449.481 | 3709.274 | 11609.123 | 20244.262 | 19304.082 | 21985.110 | 22493.773 |
| tls | bulk&#45;4m | legacy | 5000 | 4816.915 | 5076.437 | 312.185 | 95.913 | 5004.486 | 5239.906 | 4978.275 | 6027.900 | 6193.357 |
| tls | bulk&#45;4m | portable | 5000 | 4769.838 | 4995.755 | 228.061 | 32.803 | 4952.370 | 5031.487 | 4944.180 | 5203.303 | 6151.901 |
| tls | bulk&#45;4m | websocket | 5000 | 20253.245 | 21796.008 | 1274.775 | 277.195 | 21347.025 | 22084.592 | 21207.933 | 24368.314 | 24837.429 |
| tls | deep&#45;8 | cpp | 5000 | 46.469 | 48.822 | 3.806 | 0.318 | 48.553 | 49.328 | 47.682 | 55.713 | 67.215 |
| tls | deep&#45;8 | grpc | 5000 | 62.428 | 66.867 | 3.708 | 0.633 | 66.078 | 67.689 | 66.013 | 74.773 | 83.266 |
| tls | deep&#45;8 | legacy | 5000 | 32.190 | 33.730 | 2.994 | 0.345 | 33.350 | 34.263 | 32.979 | 37.315 | 47.750 |
| tls | deep&#45;8 | portable | 5000 | 33.251 | 35.102 | 3.069 | 0.249 | 34.740 | 35.326 | 34.371 | 39.441 | 49.787 |
| tls | deep&#45;8 | websocket | 5000 | 28.160 | 33.202 | 3.621 | 0.166 | 33.033 | 33.381 | 32.756 | 38.313 | 46.832 |
| tls | deep&#45;32 | cpp | 5000 | 51.776 | 61.023 | 12.668 | 12.861 | 54.098 | 83.873 | 54.206 | 83.738 | 95.395 |
| tls | deep&#45;32 | grpc | 5000 | 80.487 | 85.645 | 4.707 | 0.278 | 85.421 | 86.126 | 84.418 | 96.130 | 104.805 |
| tls | deep&#45;32 | legacy | 5000 | 37.665 | 46.629 | 9.252 | 9.667 | 39.311 | 58.855 | 40.495 | 59.311 | 67.626 |
| tls | deep&#45;32 | portable | 5000 | 38.456 | 47.741 | 9.498 | 9.934 | 40.362 | 59.037 | 41.356 | 59.690 | 72.252 |
| tls | deep&#45;32 | websocket | 5000 | 45.534 | 55.575 | 9.165 | 8.950 | 51.334 | 71.584 | 51.954 | 74.989 | 81.563 |
| tls | concurrent&#45;32 | cpp | 5000 | 403.486 | 455.971 | 32.770 | 25.079 | 424.780 | 483.718 | 453.660 | 506.145 | 519.599 |
| tls | concurrent&#45;32 | grpc | 5000 | 499.460 | 548.159 | 47.783 | 38.274 | 519.643 | 613.075 | 523.889 | 628.951 | 643.234 |
| tls | concurrent&#45;32 | legacy | 5000 | 104.920 | 122.503 | 24.716 | 26.558 | 109.252 | 169.895 | 109.568 | 170.723 | 184.087 |
| tls | concurrent&#45;32 | portable | 5000 | 131.020 | 154.532 | 30.967 | 33.877 | 138.933 | 215.121 | 138.226 | 218.596 | 231.607 |
| tls | concurrent&#45;32 | websocket | 5000 | 93.463 | 156.815 | 33.832 | 33.926 | 131.680 | 197.900 | 144.365 | 214.556 | 230.036 |
| tls | deep&#45;32&#45;concurrent&#45;32 | cpp | 5000 | 533.283 | 675.363 | 111.305 | 76.833 | 564.373 | 773.458 | 737.520 | 825.362 | 843.603 |
| tls | deep&#45;32&#45;concurrent&#45;32 | grpc | 5000 | 1294.310 | 1487.511 | 127.640 | 103.567 | 1353.029 | 1605.054 | 1565.106 | 1636.317 | 1664.707 |
| tls | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 312.684 | 340.513 | 35.726 | 35.921 | 323.084 | 404.748 | 325.216 | 419.674 | 455.806 |
| tls | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 335.345 | 380.631 | 47.606 | 37.223 | 353.048 | 439.629 | 358.124 | 462.125 | 509.025 |
| tls | deep&#45;32&#45;concurrent&#45;32 | websocket | 5000 | 614.655 | 756.709 | 113.887 | 80.728 | 657.820 | 882.720 | 675.305 | 912.833 | 936.973 |
| tls | services&#45;4&#45;deep&#45;16 | cpp | 5000 | 312.141 | 342.484 | 19.192 | 3.010 | 337.519 | 344.738 | 337.727 | 375.381 | 387.368 |
| tls | services&#45;4&#45;deep&#45;16 | grpc | 5000 | 410.309 | 455.455 | 23.164 | 3.625 | 450.036 | 459.930 | 451.482 | 488.745 | 501.846 |
| tls | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 226.065 | 246.218 | 19.025 | 2.648 | 243.836 | 250.688 | 239.958 | 277.589 | 289.001 |
| tls | services&#45;4&#45;deep&#45;16 | portable | 5000 | 247.747 | 271.651 | 22.907 | 1.109 | 270.252 | 272.899 | 264.522 | 303.130 | 315.816 |
| tls | services&#45;4&#45;deep&#45;16 | websocket | 5000 | 187.773 | 225.852 | 22.673 | 1.029 | 224.663 | 227.511 | 224.134 | 265.778 | 283.851 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | 5000 | 2305.786 | 2447.591 | 102.309 | 48.894 | 2388.368 | 2520.330 | 2419.744 | 2642.382 | 2803.445 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | 5000 | 3932.448 | 4242.519 | 144.541 | 82.270 | 4144.585 | 4361.088 | 4277.997 | 4443.240 | 4567.559 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1266.341 | 1325.065 | 20.038 | 6.530 | 1315.630 | 1330.698 | 1324.204 | 1348.891 | 1376.767 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1400.545 | 1475.942 | 71.677 | 11.921 | 1465.159 | 1491.342 | 1469.063 | 1505.191 | 1628.778 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | 5000 | 1141.204 | 1244.399 | 40.690 | 2.916 | 1240.214 | 1247.985 | 1242.591 | 1312.587 | 1342.531 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 1922.464 | 2292.179 | 110.244 | 60.317 | 2231.776 | 2378.398 | 2293.438 | 2447.226 | 2499.364 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 3332.303 | 3984.952 | 163.728 | 88.354 | 3912.205 | 4125.357 | 3955.181 | 4267.449 | 4343.230 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1083.396 | 1356.082 | 43.265 | 13.403 | 1338.185 | 1368.865 | 1356.389 | 1413.946 | 1459.267 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1293.233 | 1465.286 | 68.942 | 14.527 | 1455.081 | 1490.830 | 1462.159 | 1542.161 | 1574.331 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1020.461 | 1267.925 | 105.900 | 39.820 | 1224.316 | 1318.470 | 1269.375 | 1431.076 | 1495.693 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 4106.390 | 4708.050 | 146.216 | 67.700 | 4596.604 | 4768.017 | 4712.446 | 4962.095 | 5049.598 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 6500.683 | 8009.554 | 358.855 | 52.953 | 7966.178 | 8098.915 | 8027.990 | 8529.755 | 8648.441 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2418.161 | 2817.210 | 46.734 | 20.052 | 2785.582 | 2840.147 | 2818.754 | 2863.330 | 2917.546 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2612.351 | 2981.442 | 145.516 | 63.109 | 2872.939 | 3026.715 | 3009.931 | 3191.404 | 3244.247 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1883.817 | 2359.744 | 163.589 | 9.110 | 2350.410 | 2372.880 | 2342.453 | 2651.919 | 2828.969 |
| quic | loopback&#45;0 | legacy | 5000 | 37.407 | 40.261 | 2.955 | 0.374 | 39.830 | 40.591 | 39.585 | 45.749 | 53.707 |
| quic | loopback&#45;0 | portable | 5000 | 41.451 | 44.093 | 2.983 | 0.193 | 43.932 | 44.388 | 43.274 | 50.428 | 58.541 |
| quic | loopback&#45;64 | legacy | 5000 | 38.650 | 40.730 | 3.063 | 0.390 | 40.161 | 41.141 | 39.870 | 46.980 | 55.074 |
| quic | loopback&#45;64 | portable | 5000 | 42.075 | 44.411 | 3.303 | 0.423 | 43.722 | 44.771 | 43.547 | 50.883 | 60.173 |
| quic | loopback&#45;1024 | legacy | 5000 | 39.885 | 42.189 | 2.819 | 0.408 | 41.587 | 42.557 | 41.502 | 47.831 | 55.668 |
| quic | loopback&#45;1024 | portable | 5000 | 43.868 | 46.154 | 3.291 | 0.344 | 45.726 | 46.611 | 45.273 | 52.587 | 62.319 |
| quic | loopback&#45;65536 | legacy | 5000 | 143.607 | 151.060 | 13.177 | 1.239 | 150.102 | 152.575 | 147.955 | 168.569 | 176.937 |
| quic | loopback&#45;65536 | portable | 5000 | 146.382 | 156.097 | 7.300 | 0.544 | 155.400 | 156.668 | 153.200 | 171.376 | 181.580 |
| quic | unary&#45;empty | legacy | 5000 | 37.776 | 40.179 | 3.051 | 0.196 | 39.986 | 40.451 | 39.443 | 45.790 | 54.482 |
| quic | unary&#45;empty | portable | 5000 | 41.551 | 44.145 | 3.155 | 0.286 | 43.789 | 44.532 | 43.273 | 50.315 | 59.206 |
| quic | unary&#45;small | legacy | 5000 | 38.387 | 44.816 | 9.593 | 10.062 | 40.136 | 62.813 | 39.656 | 62.411 | 71.728 |
| quic | unary&#45;small | portable | 5000 | 42.633 | 49.099 | 9.909 | 10.331 | 44.295 | 67.577 | 43.729 | 67.131 | 76.774 |
| quic | unary&#45;1k | legacy | 5000 | 40.335 | 55.253 | 10.311 | 9.825 | 42.169 | 62.406 | 60.859 | 68.211 | 76.236 |
| quic | unary&#45;1k | portable | 5000 | 44.495 | 60.740 | 12.324 | 13.160 | 46.288 | 71.037 | 68.248 | 76.385 | 85.674 |
| quic | fanout&#45;32 | legacy | 5000 | 159.173 | 169.799 | 6.540 | 0.810 | 168.575 | 170.647 | 167.331 | 181.618 | 190.632 |
| quic | fanout&#45;32 | portable | 5000 | 190.771 | 225.011 | 48.239 | 47.496 | 203.376 | 309.972 | 202.300 | 333.730 | 343.643 |
| quic | dependent&#45;read | legacy | 5000 | 83.603 | 87.903 | 4.330 | 0.583 | 86.935 | 88.507 | 86.491 | 98.160 | 105.662 |
| quic | dependent&#45;read | portable | 5000 | 104.804 | 112.210 | 6.094 | 1.194 | 110.719 | 113.807 | 110.185 | 124.079 | 133.874 |
| quic | pipelined&#45;read | legacy | 5000 | 48.383 | 51.926 | 3.490 | 0.294 | 51.545 | 52.364 | 50.976 | 59.055 | 68.917 |
| quic | pipelined&#45;read | portable | 5000 | 73.279 | 77.817 | 4.357 | 0.773 | 77.091 | 78.887 | 76.428 | 87.769 | 96.451 |
| quic | write&#45;read | legacy | 5000 | 77.185 | 86.949 | 19.643 | 10.960 | 81.831 | 106.547 | 80.527 | 120.272 | 129.976 |
| quic | write&#45;read | portable | 5000 | 85.317 | 94.448 | 14.578 | 8.053 | 90.709 | 108.852 | 89.198 | 141.074 | 151.329 |
| quic | callback | legacy | 5000 | 82.955 | 86.765 | 4.242 | 0.546 | 85.929 | 87.266 | 85.414 | 96.662 | 103.381 |
| quic | callback | portable | 5000 | 100.534 | 107.500 | 6.077 | 0.855 | 106.650 | 108.872 | 105.535 | 119.127 | 128.030 |
| quic | bulk&#45;64k | legacy | 5000 | 139.257 | 166.256 | 36.469 | 40.072 | 146.473 | 237.893 | 147.192 | 242.742 | 253.209 |
| quic | bulk&#45;64k | portable | 5000 | 144.321 | 163.915 | 29.336 | 14.664 | 154.168 | 188.910 | 152.577 | 244.880 | 259.726 |
| quic | bulk&#45;256k | legacy | 5000 | 356.226 | 388.747 | 555.826 | 16.431 | 373.649 | 407.348 | 372.978 | 399.812 | 473.960 |
| quic | bulk&#45;256k | portable | 5000 | 373.611 | 411.191 | 55.207 | 35.188 | 394.439 | 474.113 | 392.677 | 593.466 | 613.276 |
| quic | bulk&#45;1m | legacy | 5000 | 1363.173 | 1514.731 | 714.209 | 115.702 | 1409.048 | 1679.305 | 1410.306 | 2005.297 | 2036.179 |
| quic | bulk&#45;1m | portable | 5000 | 1352.737 | 1479.462 | 574.439 | 75.904 | 1415.084 | 1598.872 | 1412.496 | 2001.971 | 2038.615 |
| quic | bulk&#45;4m | legacy | 5000 | 6170.441 | 6677.910 | 604.745 | 117.128 | 6543.744 | 6775.442 | 6489.699 | 8486.607 | 8790.215 |
| quic | bulk&#45;4m | portable | 5000 | 6246.506 | 6598.447 | 320.830 | 77.137 | 6548.622 | 6734.391 | 6538.986 | 6839.582 | 8658.760 |
| quic | deep&#45;8 | legacy | 5000 | 40.291 | 44.606 | 8.351 | 2.315 | 42.769 | 48.370 | 42.199 | 55.442 | 66.729 |
| quic | deep&#45;8 | portable | 5000 | 44.348 | 46.778 | 3.448 | 0.363 | 46.395 | 47.339 | 45.838 | 53.730 | 63.073 |
| quic | deep&#45;32 | legacy | 5000 | 47.375 | 64.771 | 12.999 | 13.916 | 49.399 | 75.792 | 72.759 | 81.438 | 89.779 |
| quic | deep&#45;32 | portable | 5000 | 51.333 | 72.589 | 15.528 | 16.698 | 54.173 | 86.490 | 81.912 | 91.993 | 101.092 |
| quic | concurrent&#45;32 | legacy | 5000 | 116.254 | 133.472 | 25.694 | 22.452 | 122.326 | 173.609 | 122.018 | 193.066 | 205.613 |
| quic | concurrent&#45;32 | portable | 5000 | 142.357 | 154.979 | 6.998 | 1.189 | 153.224 | 156.481 | 152.503 | 169.288 | 179.538 |
| quic | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 344.582 | 378.002 | 29.611 | 20.746 | 366.869 | 415.032 | 369.070 | 450.823 | 472.067 |
| quic | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 382.179 | 405.843 | 13.357 | 2.333 | 403.052 | 408.100 | 404.113 | 423.228 | 433.796 |
| quic | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 358.543 | 387.830 | 13.939 | 4.558 | 382.761 | 394.050 | 385.894 | 411.098 | 423.121 |
| quic | services&#45;4&#45;deep&#45;16 | portable | 5000 | 337.372 | 366.567 | 13.491 | 5.504 | 360.596 | 372.232 | 364.349 | 388.662 | 396.934 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1498.096 | 1581.608 | 60.305 | 8.132 | 1569.052 | 1588.737 | 1569.520 | 1671.171 | 1696.515 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1572.029 | 1669.807 | 42.662 | 7.683 | 1661.528 | 1678.625 | 1662.458 | 1740.636 | 1792.226 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1445.177 | 1655.398 | 79.107 | 17.759 | 1625.234 | 1671.544 | 1653.367 | 1774.415 | 1827.179 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1405.161 | 1711.816 | 186.573 | 17.569 | 1685.425 | 1733.664 | 1727.247 | 1842.028 | 1892.279 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2954.438 | 3452.413 | 139.063 | 14.431 | 3438.646 | 3474.171 | 3481.984 | 3636.503 | 3696.879 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2722.126 | 3491.151 | 281.545 | 36.815 | 3436.425 | 3519.068 | 3583.512 | 3866.099 | 3966.735 |

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


