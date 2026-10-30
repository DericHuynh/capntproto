# Capntproto verification reports

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-30T16:08:51Z · run 41087208735 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41087208735) · commit `0fd5d6969eed` · **failure**

![Cargo test results](docs/reports/cargo-history.svg)

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-30T16:08:51Z · run 41087208735 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41087208735) · commit `0fd5d6969eed` · **failure**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-30T16:08:51Z · run 41087208735 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41087208735) · commit `0fd5d6969eed` · **failure**

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

Latest coverage run: [2026-10-30T16:08:51Z · run 41087208618 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41087208618) · commit `0fd5d6969eed` · **failure**

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-30T16:08:51Z · run 41087208735 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41087208735) · commit `0fd5d6969eed` · **failure**

Measurement evidence: **passed**. Independently validated encrypted benchmark values&#46; Workflow status and diagnostic completion are separate&#59; unavailable values remain blank&#46; Operations include whole concurrent batches and every service hop&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Current TCP fixtures use TLS 1.3, including upstream Rust Cap’n Proto, C++, gRPC over HTTP/2 and WebSockets over WSS; native encrypted transports and standard QUIC are identified separately. Historical data retains its recorded fixture contract. The dedicated campaign also reports recursive data, concurrent calls and independent service chains for gRPC and binary Protobuf WebSockets in its encrypted comparison artifact. Bars compare the recorded workload, rather than universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.


## Exhaustive benchmark tables

Measured run: [2026-10-30T16:08:51Z · run 41087208735 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41087208735) · commit `0fd5d6969eed` · **failure**

All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.

### All encrypted latency comparisons

| Transport | Case | Implementation | Samples | Minimum µs | Mean µs | Sample SD µs | Run mean SD µs | Run mean min µs | Run mean max µs | p50 µs | p95 µs | p99 µs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tls | loopback&#45;0 | cpp | 5000 | 44.121 | 56.898 | 13.003 | 13.679 | 46.317 | 73.025 | 47.556 | 73.329 | 87.739 |
| tls | loopback&#45;0 | grpc | 5000 | 51.501 | 65.284 | 14.242 | 14.961 | 53.429 | 81.955 | 54.985 | 83.906 | 97.096 |
| tls | loopback&#45;0 | legacy | 5000 | 29.886 | 34.438 | 6.123 | 4.971 | 31.534 | 43.263 | 31.215 | 43.129 | 52.329 |
| tls | loopback&#45;0 | portable | 5000 | 31.207 | 36.293 | 7.446 | 7.019 | 32.834 | 48.839 | 32.433 | 48.270 | 59.018 |
| tls | loopback&#45;0 | websocket | 5000 | 26.651 | 34.837 | 5.978 | 5.909 | 28.386 | 40.789 | 37.492 | 40.693 | 49.462 |
| tls | loopback&#45;64 | cpp | 5000 | 44.367 | 62.264 | 13.286 | 13.990 | 46.651 | 73.735 | 68.735 | 79.569 | 90.284 |
| tls | loopback&#45;64 | grpc | 5000 | 51.979 | 73.271 | 15.440 | 16.097 | 55.517 | 85.292 | 82.822 | 93.477 | 99.857 |
| tls | loopback&#45;64 | legacy | 5000 | 30.165 | 42.069 | 6.335 | 6.045 | 31.735 | 46.616 | 42.460 | 48.843 | 60.569 |
| tls | loopback&#45;64 | portable | 5000 | 31.129 | 38.628 | 8.187 | 8.220 | 32.417 | 49.114 | 32.537 | 48.862 | 58.610 |
| tls | loopback&#45;64 | websocket | 5000 | 24.774 | 35.032 | 5.464 | 4.811 | 26.956 | 38.829 | 35.914 | 41.096 | 49.694 |
| tls | loopback&#45;1024 | cpp | 5000 | 46.134 | 71.377 | 54.241 | 13.563 | 48.921 | 85.642 | 72.767 | 84.935 | 93.936 |
| tls | loopback&#45;1024 | grpc | 5000 | 55.507 | 83.638 | 12.972 | 13.332 | 59.858 | 91.305 | 87.542 | 99.154 | 108.365 |
| tls | loopback&#45;1024 | legacy | 5000 | 32.113 | 41.589 | 7.169 | 7.055 | 33.699 | 48.718 | 44.401 | 49.429 | 60.732 |
| tls | loopback&#45;1024 | portable | 5000 | 33.120 | 43.275 | 7.673 | 7.410 | 34.873 | 48.938 | 47.195 | 50.751 | 64.471 |
| tls | loopback&#45;1024 | websocket | 5000 | 24.939 | 35.411 | 4.945 | 0.594 | 34.711 | 36.305 | 34.676 | 40.478 | 49.382 |
| tls | loopback&#45;65536 | cpp | 5000 | 164.270 | 204.794 | 41.316 | 36.085 | 176.863 | 248.647 | 181.217 | 275.361 | 283.663 |
| tls | loopback&#45;65536 | grpc | 5000 | 170.043 | 183.299 | 12.178 | 1.190 | 181.653 | 184.760 | 178.527 | 209.954 | 229.535 |
| tls | loopback&#45;65536 | legacy | 5000 | 104.208 | 108.745 | 12.025 | 0.674 | 108.001 | 109.645 | 106.315 | 121.067 | 126.086 |
| tls | loopback&#45;65536 | portable | 5000 | 107.170 | 134.812 | 29.163 | 31.924 | 110.319 | 170.110 | 114.996 | 178.805 | 187.229 |
| tls | loopback&#45;65536 | websocket | 5000 | 119.906 | 125.030 | 8.435 | 1.962 | 123.365 | 128.038 | 122.283 | 136.088 | 174.284 |
| tls | unary&#45;empty | cpp | 5000 | 43.664 | 46.702 | 4.378 | 0.935 | 45.789 | 48.230 | 45.485 | 54.668 | 63.953 |
| tls | unary&#45;empty | grpc | 5000 | 51.769 | 55.156 | 6.294 | 0.974 | 54.368 | 56.614 | 53.328 | 65.672 | 75.866 |
| tls | unary&#45;empty | legacy | 5000 | 29.931 | 31.989 | 5.443 | 0.523 | 31.515 | 32.737 | 31.010 | 37.273 | 47.100 |
| tls | unary&#45;empty | portable | 5000 | 30.929 | 32.777 | 3.082 | 0.437 | 32.175 | 33.309 | 31.992 | 37.758 | 47.776 |
| tls | unary&#45;empty | websocket | 5000 | 26.635 | 28.131 | 2.628 | 0.403 | 27.712 | 28.724 | 27.503 | 31.774 | 40.586 |
| tls | unary&#45;small | cpp | 5000 | 43.491 | 46.098 | 4.072 | 0.409 | 45.523 | 46.671 | 44.996 | 54.267 | 64.409 |
| tls | unary&#45;small | grpc | 5000 | 52.369 | 54.774 | 4.377 | 0.315 | 54.351 | 55.078 | 53.625 | 63.686 | 73.139 |
| tls | unary&#45;small | legacy | 5000 | 29.974 | 31.368 | 2.900 | 0.073 | 31.269 | 31.475 | 30.700 | 34.994 | 46.121 |
| tls | unary&#45;small | portable | 5000 | 30.904 | 32.557 | 3.427 | 0.184 | 32.246 | 32.706 | 31.784 | 37.326 | 48.294 |
| tls | unary&#45;small | websocket | 5000 | 24.955 | 27.150 | 3.392 | 0.155 | 27.014 | 27.363 | 26.646 | 30.535 | 40.383 |
| tls | unary&#45;1k | cpp | 5000 | 46.069 | 58.951 | 13.080 | 13.820 | 48.645 | 74.117 | 49.519 | 75.301 | 89.844 |
| tls | unary&#45;1k | grpc | 5000 | 55.427 | 73.226 | 16.733 | 17.856 | 59.456 | 94.809 | 60.927 | 97.254 | 107.784 |
| tls | unary&#45;1k | legacy | 5000 | 31.621 | 41.197 | 7.812 | 7.415 | 33.027 | 48.582 | 44.290 | 49.315 | 60.224 |
| tls | unary&#45;1k | portable | 5000 | 33.130 | 43.882 | 8.448 | 8.526 | 34.581 | 52.121 | 47.680 | 53.113 | 65.385 |
| tls | unary&#45;1k | websocket | 5000 | 24.732 | 31.173 | 5.542 | 4.454 | 27.651 | 36.098 | 28.838 | 39.751 | 46.606 |
| tls | fanout&#45;32 | cpp | 5000 | 362.270 | 401.174 | 38.119 | 7.807 | 387.254 | 405.500 | 386.450 | 477.073 | 519.142 |
| tls | fanout&#45;32 | grpc | 5000 | 635.148 | 672.154 | 57.649 | 16.760 | 659.876 | 701.602 | 661.264 | 690.238 | 1011.276 |
| tls | fanout&#45;32 | legacy | 5000 | 133.161 | 152.028 | 29.247 | 29.178 | 138.378 | 204.217 | 137.467 | 217.855 | 230.447 |
| tls | fanout&#45;32 | portable | 5000 | 164.989 | 171.496 | 9.172 | 0.669 | 170.789 | 172.351 | 168.662 | 183.527 | 191.282 |
| tls | fanout&#45;32 | websocket | 5000 | 177.119 | 198.158 | 15.960 | 1.822 | 195.815 | 200.850 | 193.377 | 224.171 | 232.485 |
| tls | dependent&#45;read | cpp | 5000 | 102.494 | 107.648 | 6.037 | 0.338 | 107.252 | 108.093 | 105.343 | 120.992 | 129.544 |
| tls | dependent&#45;read | grpc | 5000 | 109.937 | 115.906 | 6.516 | 0.846 | 114.927 | 116.913 | 113.705 | 129.450 | 137.906 |
| tls | dependent&#45;read | legacy | 5000 | 66.007 | 82.250 | 17.170 | 18.298 | 68.673 | 102.492 | 69.719 | 108.394 | 119.208 |
| tls | dependent&#45;read | portable | 5000 | 80.036 | 98.032 | 21.561 | 20.514 | 83.553 | 128.120 | 83.209 | 133.455 | 146.708 |
| tls | dependent&#45;read | websocket | 5000 | 48.674 | 58.843 | 10.098 | 9.757 | 53.901 | 76.268 | 55.002 | 80.758 | 86.034 |
| tls | pipelined&#45;read | cpp | 5000 | 73.293 | 77.425 | 4.593 | 0.997 | 76.501 | 78.901 | 75.877 | 88.096 | 96.196 |
| tls | pipelined&#45;read | legacy | 5000 | 39.636 | 42.160 | 3.557 | 0.444 | 41.583 | 42.551 | 41.157 | 49.019 | 58.886 |
| tls | pipelined&#45;read | portable | 5000 | 57.547 | 60.826 | 5.099 | 0.542 | 60.057 | 61.403 | 59.286 | 71.445 | 82.392 |
| tls | write&#45;read | cpp | 5000 | 89.369 | 104.019 | 20.907 | 22.529 | 92.789 | 144.286 | 92.556 | 143.918 | 160.162 |
| tls | write&#45;read | grpc | 5000 | 109.651 | 128.270 | 25.244 | 27.344 | 114.991 | 177.143 | 114.756 | 177.205 | 190.428 |
| tls | write&#45;read | legacy | 5000 | 60.714 | 70.099 | 12.506 | 13.175 | 63.995 | 93.661 | 63.154 | 92.859 | 105.612 |
| tls | write&#45;read | portable | 5000 | 63.208 | 71.735 | 11.477 | 11.874 | 66.082 | 92.969 | 65.428 | 92.503 | 106.811 |
| tls | write&#45;read | websocket | 5000 | 48.542 | 54.251 | 3.957 | 0.311 | 53.961 | 54.653 | 53.668 | 61.549 | 69.856 |
| tls | callback | cpp | 5000 | 95.161 | 121.463 | 28.293 | 29.800 | 98.373 | 154.257 | 102.395 | 162.591 | 170.984 |
| tls | callback | grpc | 5000 | 57.778 | 64.374 | 10.469 | 9.002 | 59.958 | 80.467 | 59.292 | 86.456 | 95.575 |
| tls | callback | legacy | 5000 | 65.882 | 85.590 | 14.744 | 15.625 | 68.214 | 97.789 | 94.391 | 105.295 | 115.329 |
| tls | callback | portable | 5000 | 76.413 | 101.965 | 21.139 | 20.767 | 79.332 | 120.599 | 116.781 | 131.454 | 140.298 |
| tls | callback | websocket | 5000 | 48.704 | 60.528 | 10.811 | 10.888 | 52.268 | 75.589 | 54.112 | 79.540 | 85.767 |
| tls | bulk&#45;64k | cpp | 5000 | 161.210 | 178.830 | 7.942 | 1.611 | 176.493 | 180.444 | 177.911 | 193.134 | 201.900 |
| tls | bulk&#45;64k | grpc | 5000 | 171.826 | 185.350 | 11.296 | 0.485 | 184.770 | 185.883 | 180.917 | 209.680 | 229.412 |
| tls | bulk&#45;64k | legacy | 5000 | 105.032 | 108.607 | 5.249 | 0.156 | 108.416 | 108.829 | 106.526 | 120.977 | 125.974 |
| tls | bulk&#45;64k | portable | 5000 | 106.848 | 110.838 | 5.096 | 0.478 | 110.380 | 111.439 | 108.860 | 122.937 | 126.969 |
| tls | bulk&#45;64k | websocket | 5000 | 121.229 | 125.641 | 5.806 | 0.742 | 124.361 | 126.219 | 123.678 | 136.347 | 143.081 |
| tls | bulk&#45;256k | cpp | 5000 | 654.353 | 712.320 | 64.764 | 28.988 | 697.614 | 764.054 | 698.281 | 758.685 | 1030.982 |
| tls | bulk&#45;256k | grpc | 5000 | 423.356 | 972.211 | 162.131 | 78.308 | 834.260 | 1029.464 | 988.578 | 1235.838 | 1311.168 |
| tls | bulk&#45;256k | legacy | 5000 | 293.911 | 332.415 | 51.677 | 35.965 | 305.348 | 378.977 | 306.748 | 439.159 | 450.792 |
| tls | bulk&#45;256k | portable | 5000 | 303.621 | 314.501 | 11.358 | 0.578 | 313.817 | 315.197 | 310.121 | 331.160 | 344.100 |
| tls | bulk&#45;256k | websocket | 5000 | 739.327 | 780.717 | 32.322 | 23.049 | 763.042 | 819.860 | 769.639 | 852.877 | 865.058 |
| tls | bulk&#45;1m | cpp | 5000 | 2552.852 | 2858.844 | 374.233 | 177.532 | 2699.728 | 3107.265 | 2695.404 | 3781.539 | 3812.595 |
| tls | bulk&#45;1m | grpc | 5000 | 1764.110 | 3572.203 | 756.003 | 778.516 | 2748.457 | 4393.771 | 3669.779 | 4601.385 | 4738.966 |
| tls | bulk&#45;1m | legacy | 5000 | 1036.680 | 1136.541 | 132.972 | 72.881 | 1065.512 | 1214.641 | 1069.241 | 1394.319 | 1416.821 |
| tls | bulk&#45;1m | portable | 5000 | 1043.435 | 1086.540 | 73.245 | 29.381 | 1067.443 | 1138.265 | 1069.938 | 1129.140 | 1413.155 |
| tls | bulk&#45;1m | websocket | 5000 | 3179.149 | 3369.274 | 196.759 | 72.954 | 3308.554 | 3459.234 | 3290.666 | 3815.652 | 3992.463 |
| tls | bulk&#45;4m | cpp | 5000 | 11471.175 | 12242.895 | 988.242 | 433.697 | 11872.726 | 12932.531 | 11918.700 | 14614.824 | 16602.674 |
| tls | bulk&#45;4m | grpc | 5000 | 14427.038 | 18889.222 | 1854.753 | 1894.294 | 15609.839 | 20498.678 | 19194.620 | 21602.752 | 22143.945 |
| tls | bulk&#45;4m | legacy | 5000 | 4727.054 | 4983.236 | 292.143 | 133.804 | 4914.607 | 5222.042 | 4898.112 | 5977.929 | 6177.610 |
| tls | bulk&#45;4m | portable | 5000 | 4712.806 | 5010.376 | 372.996 | 89.785 | 4887.879 | 5131.225 | 4880.045 | 6037.067 | 6192.056 |
| tls | bulk&#45;4m | websocket | 5000 | 20086.492 | 22125.163 | 1500.807 | 677.767 | 21361.699 | 22826.868 | 21292.250 | 24592.559 | 24886.735 |
| tls | deep&#45;8 | cpp | 5000 | 46.810 | 54.441 | 11.163 | 10.782 | 49.076 | 73.702 | 48.624 | 74.799 | 87.228 |
| tls | deep&#45;8 | grpc | 5000 | 62.081 | 65.887 | 5.295 | 0.624 | 65.281 | 66.764 | 64.402 | 75.786 | 84.006 |
| tls | deep&#45;8 | legacy | 5000 | 32.375 | 37.068 | 7.009 | 7.064 | 33.816 | 49.704 | 33.275 | 49.284 | 58.192 |
| tls | deep&#45;8 | portable | 5000 | 33.040 | 38.050 | 6.963 | 6.611 | 34.963 | 49.873 | 34.427 | 49.433 | 58.797 |
| tls | deep&#45;8 | websocket | 5000 | 28.129 | 33.259 | 3.821 | 0.212 | 33.038 | 33.554 | 32.760 | 38.675 | 48.178 |
| tls | deep&#45;32 | cpp | 5000 | 51.583 | 54.486 | 4.337 | 0.355 | 54.166 | 55.026 | 53.231 | 63.097 | 71.376 |
| tls | deep&#45;32 | grpc | 5000 | 78.788 | 83.540 | 5.419 | 0.406 | 82.957 | 84.097 | 82.197 | 94.652 | 102.313 |
| tls | deep&#45;32 | legacy | 5000 | 37.532 | 39.558 | 3.345 | 0.197 | 39.370 | 39.806 | 38.585 | 45.123 | 55.585 |
| tls | deep&#45;32 | portable | 5000 | 38.374 | 40.374 | 3.158 | 0.131 | 40.186 | 40.520 | 39.410 | 46.036 | 55.010 |
| tls | deep&#45;32 | websocket | 5000 | 45.809 | 51.794 | 4.400 | 0.081 | 51.676 | 51.864 | 51.117 | 60.027 | 68.264 |
| tls | concurrent&#45;32 | cpp | 5000 | 395.200 | 431.559 | 23.149 | 8.503 | 419.094 | 441.408 | 427.529 | 469.143 | 525.453 |
| tls | concurrent&#45;32 | grpc | 5000 | 500.139 | 551.283 | 47.748 | 45.887 | 516.269 | 618.596 | 525.181 | 631.792 | 643.022 |
| tls | concurrent&#45;32 | legacy | 5000 | 103.462 | 120.032 | 24.175 | 23.807 | 108.988 | 162.616 | 108.341 | 169.544 | 185.564 |
| tls | concurrent&#45;32 | portable | 5000 | 133.706 | 154.530 | 32.651 | 34.260 | 138.511 | 215.811 | 137.930 | 223.671 | 237.612 |
| tls | concurrent&#45;32 | websocket | 5000 | 91.254 | 142.914 | 27.196 | 26.214 | 130.531 | 189.793 | 134.584 | 200.014 | 218.081 |
| tls | deep&#45;32&#45;concurrent&#45;32 | cpp | 5000 | 534.597 | 612.540 | 94.655 | 103.707 | 562.874 | 798.010 | 567.737 | 810.886 | 829.922 |
| tls | deep&#45;32&#45;concurrent&#45;32 | grpc | 5000 | 1293.805 | 1503.501 | 124.502 | 100.830 | 1359.983 | 1599.349 | 1576.205 | 1639.687 | 1661.597 |
| tls | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 311.107 | 354.013 | 62.060 | 66.870 | 323.475 | 473.631 | 324.440 | 495.522 | 528.998 |
| tls | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 338.307 | 374.910 | 49.037 | 51.120 | 350.992 | 466.347 | 352.664 | 491.435 | 501.489 |
| tls | deep&#45;32&#45;concurrent&#45;32 | websocket | 5000 | 602.318 | 682.116 | 78.273 | 59.206 | 653.111 | 787.910 | 654.741 | 886.548 | 913.627 |
| tls | services&#45;4&#45;deep&#45;16 | cpp | 5000 | 318.441 | 344.264 | 18.257 | 2.746 | 340.694 | 347.464 | 340.811 | 374.400 | 389.446 |
| tls | services&#45;4&#45;deep&#45;16 | grpc | 5000 | 424.371 | 459.081 | 20.309 | 3.705 | 454.040 | 463.781 | 456.128 | 487.863 | 501.599 |
| tls | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 227.626 | 246.794 | 18.077 | 2.632 | 242.928 | 250.314 | 240.937 | 273.928 | 283.272 |
| tls | services&#45;4&#45;deep&#45;16 | portable | 5000 | 254.038 | 272.701 | 16.025 | 2.076 | 271.496 | 276.361 | 266.138 | 302.742 | 315.376 |
| tls | services&#45;4&#45;deep&#45;16 | websocket | 5000 | 187.065 | 224.517 | 22.295 | 1.643 | 222.508 | 226.441 | 222.438 | 264.611 | 282.410 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | 5000 | 2298.996 | 2459.365 | 107.524 | 29.774 | 2425.939 | 2502.708 | 2428.527 | 2656.493 | 2855.672 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | 5000 | 3971.116 | 4162.860 | 121.257 | 23.217 | 4137.954 | 4193.988 | 4126.000 | 4394.444 | 4518.838 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1280.305 | 1335.592 | 28.892 | 1.293 | 1333.997 | 1337.588 | 1332.008 | 1371.392 | 1416.969 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1428.775 | 1501.092 | 33.416 | 0.871 | 1499.909 | 1502.278 | 1497.202 | 1560.088 | 1603.575 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | 5000 | 1167.267 | 1284.827 | 47.877 | 19.689 | 1271.684 | 1319.395 | 1281.792 | 1365.271 | 1424.510 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 1957.360 | 2313.256 | 105.673 | 42.098 | 2248.656 | 2365.549 | 2313.189 | 2491.300 | 2561.807 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 3330.070 | 3944.908 | 162.750 | 85.329 | 3846.657 | 4032.707 | 3940.460 | 4246.816 | 4326.519 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1068.876 | 1370.922 | 48.391 | 18.392 | 1344.727 | 1387.413 | 1369.026 | 1436.782 | 1477.329 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1275.562 | 1536.358 | 66.634 | 27.357 | 1497.923 | 1561.529 | 1530.064 | 1642.896 | 1662.609 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1014.596 | 1259.023 | 103.123 | 70.827 | 1190.406 | 1338.039 | 1245.972 | 1435.640 | 1500.131 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 4044.009 | 4823.915 | 174.215 | 93.958 | 4702.811 | 4948.305 | 4798.892 | 5095.685 | 5211.817 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 6401.707 | 7931.365 | 411.314 | 204.426 | 7768.106 | 8279.731 | 7940.356 | 8604.424 | 8831.690 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2344.713 | 2883.558 | 83.318 | 41.943 | 2848.796 | 2931.106 | 2860.827 | 3020.869 | 3081.636 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2476.161 | 3076.705 | 161.946 | 92.838 | 2999.597 | 3233.054 | 3042.469 | 3393.330 | 3502.369 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1914.617 | 2371.562 | 177.739 | 28.934 | 2326.917 | 2401.173 | 2334.776 | 2720.525 | 2954.557 |
| quic | loopback&#45;0 | legacy | 5000 | 39.067 | 50.094 | 11.492 | 10.953 | 40.599 | 63.181 | 42.133 | 63.561 | 77.753 |
| quic | loopback&#45;0 | portable | 5000 | 42.440 | 58.131 | 12.078 | 12.727 | 44.028 | 67.537 | 65.732 | 70.140 | 84.249 |
| quic | loopback&#45;64 | legacy | 5000 | 39.072 | 56.494 | 9.026 | 8.930 | 40.533 | 60.830 | 59.180 | 67.148 | 79.932 |
| quic | loopback&#45;64 | portable | 5000 | 42.636 | 63.998 | 10.881 | 11.019 | 44.381 | 70.716 | 66.943 | 76.780 | 89.292 |
| quic | loopback&#45;1024 | legacy | 5000 | 40.690 | 50.610 | 10.873 | 11.201 | 42.382 | 62.970 | 42.187 | 63.532 | 77.620 |
| quic | loopback&#45;1024 | portable | 5000 | 44.570 | 56.359 | 12.581 | 13.261 | 46.414 | 70.983 | 46.724 | 71.800 | 86.558 |
| quic | loopback&#45;65536 | legacy | 5000 | 140.817 | 147.516 | 9.152 | 0.558 | 147.018 | 148.327 | 144.759 | 162.254 | 174.760 |
| quic | loopback&#45;65536 | portable | 5000 | 146.380 | 153.600 | 9.207 | 0.413 | 153.164 | 154.146 | 150.015 | 175.636 | 187.038 |
| quic | unary&#45;empty | legacy | 5000 | 38.715 | 40.548 | 3.492 | 0.345 | 40.186 | 41.081 | 39.704 | 45.238 | 58.566 |
| quic | unary&#45;empty | portable | 5000 | 42.401 | 44.429 | 9.054 | 0.303 | 43.937 | 44.655 | 43.325 | 49.763 | 63.812 |
| quic | unary&#45;small | legacy | 5000 | 38.679 | 41.374 | 6.885 | 2.136 | 40.074 | 45.175 | 39.752 | 50.450 | 65.944 |
| quic | unary&#45;small | portable | 5000 | 42.709 | 44.616 | 3.914 | 0.290 | 44.355 | 44.964 | 43.625 | 50.417 | 63.619 |
| quic | unary&#45;1k | legacy | 5000 | 40.623 | 55.099 | 11.147 | 11.720 | 42.329 | 65.664 | 61.149 | 66.807 | 79.846 |
| quic | unary&#45;1k | portable | 5000 | 44.328 | 56.029 | 12.550 | 13.185 | 45.835 | 71.095 | 46.596 | 71.503 | 87.378 |
| quic | fanout&#45;32 | legacy | 5000 | 161.189 | 220.940 | 53.496 | 52.663 | 167.215 | 272.735 | 263.465 | 287.592 | 299.043 |
| quic | fanout&#45;32 | portable | 5000 | 195.231 | 243.634 | 61.535 | 60.184 | 201.340 | 333.293 | 202.874 | 342.398 | 355.902 |
| quic | dependent&#45;read | legacy | 5000 | 84.662 | 100.089 | 21.944 | 21.007 | 87.879 | 137.179 | 87.695 | 136.819 | 154.817 |
| quic | dependent&#45;read | portable | 5000 | 107.759 | 112.348 | 6.566 | 0.823 | 111.279 | 113.529 | 110.197 | 125.254 | 137.564 |
| quic | pipelined&#45;read | legacy | 5000 | 50.070 | 52.548 | 3.952 | 0.251 | 52.294 | 52.830 | 51.474 | 59.718 | 73.192 |
| quic | pipelined&#45;read | portable | 5000 | 73.519 | 77.234 | 5.467 | 0.473 | 76.579 | 77.808 | 75.537 | 88.869 | 102.645 |
| quic | write&#45;read | legacy | 5000 | 79.538 | 104.545 | 22.249 | 21.743 | 82.370 | 129.775 | 118.658 | 134.295 | 147.400 |
| quic | write&#45;read | portable | 5000 | 86.712 | 100.427 | 20.128 | 21.649 | 89.789 | 139.140 | 89.665 | 138.149 | 154.805 |
| quic | callback | legacy | 5000 | 84.222 | 87.708 | 5.019 | 0.654 | 86.915 | 88.530 | 86.067 | 98.726 | 110.577 |
| quic | callback | portable | 5000 | 102.981 | 107.817 | 6.377 | 0.846 | 107.006 | 108.916 | 105.614 | 121.590 | 135.158 |
| quic | bulk&#45;64k | legacy | 5000 | 141.242 | 147.609 | 7.027 | 0.746 | 146.868 | 148.658 | 144.949 | 162.576 | 174.708 |
| quic | bulk&#45;64k | portable | 5000 | 145.165 | 153.093 | 9.445 | 1.089 | 152.045 | 154.711 | 149.630 | 172.517 | 186.132 |
| quic | bulk&#45;256k | legacy | 5000 | 351.720 | 391.041 | 559.954 | 29.208 | 368.580 | 424.059 | 368.005 | 538.557 | 562.402 |
| quic | bulk&#45;256k | portable | 5000 | 372.921 | 394.871 | 395.525 | 12.337 | 386.618 | 415.491 | 381.762 | 416.631 | 497.115 |
| quic | bulk&#45;1m | legacy | 5000 | 1333.691 | 1378.622 | 46.882 | 5.374 | 1375.159 | 1388.116 | 1374.742 | 1406.180 | 1434.544 |
| quic | bulk&#45;1m | portable | 5000 | 1333.016 | 1397.532 | 92.909 | 26.755 | 1382.905 | 1445.179 | 1379.650 | 1427.388 | 1939.445 |
| quic | bulk&#45;4m | legacy | 5000 | 6076.725 | 6548.728 | 580.464 | 109.626 | 6421.278 | 6651.200 | 6444.847 | 7067.766 | 8663.388 |
| quic | bulk&#45;4m | portable | 5000 | 6150.796 | 6682.777 | 654.409 | 151.887 | 6534.966 | 6902.315 | 6530.269 | 8417.501 | 8918.409 |
| quic | deep&#45;8 | legacy | 5000 | 41.537 | 44.068 | 3.887 | 0.387 | 43.732 | 44.671 | 42.980 | 50.474 | 61.829 |
| quic | deep&#45;8 | portable | 5000 | 45.078 | 48.002 | 4.623 | 0.427 | 47.667 | 48.530 | 46.736 | 55.561 | 69.542 |
| quic | deep&#45;32 | legacy | 5000 | 47.529 | 50.564 | 4.191 | 0.392 | 49.921 | 50.885 | 49.373 | 58.999 | 69.747 |
| quic | deep&#45;32 | portable | 5000 | 50.675 | 54.622 | 6.323 | 0.660 | 53.871 | 55.526 | 53.130 | 63.074 | 76.831 |
| quic | concurrent&#45;32 | legacy | 5000 | 117.155 | 124.967 | 6.857 | 0.841 | 123.616 | 125.926 | 122.699 | 137.964 | 150.431 |
| quic | concurrent&#45;32 | portable | 5000 | 148.292 | 156.712 | 8.937 | 1.049 | 155.588 | 158.069 | 153.533 | 174.836 | 187.991 |
| quic | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 349.350 | 367.819 | 15.485 | 2.354 | 365.474 | 371.323 | 364.844 | 389.069 | 407.285 |
| quic | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 392.181 | 409.912 | 16.551 | 1.919 | 407.948 | 412.389 | 406.070 | 431.233 | 455.988 |
| quic | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 364.964 | 392.034 | 17.031 | 3.921 | 387.394 | 397.222 | 387.669 | 422.526 | 436.287 |
| quic | services&#45;4&#45;deep&#45;16 | portable | 5000 | 350.711 | 373.515 | 16.901 | 0.975 | 372.174 | 374.532 | 368.508 | 402.338 | 418.139 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1578.737 | 1682.615 | 46.037 | 31.382 | 1657.943 | 1735.800 | 1673.107 | 1772.393 | 1804.169 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1619.767 | 1767.517 | 41.691 | 3.796 | 1762.746 | 1772.602 | 1765.528 | 1842.398 | 1889.631 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1520.478 | 1771.932 | 112.248 | 4.650 | 1764.748 | 1777.745 | 1768.835 | 1926.040 | 1983.195 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1421.840 | 1804.733 | 110.013 | 17.828 | 1777.329 | 1820.199 | 1832.408 | 1942.369 | 1982.186 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 3020.968 | 3730.233 | 195.626 | 21.204 | 3703.502 | 3758.209 | 3761.077 | 3991.328 | 4046.154 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2804.829 | 3590.249 | 217.855 | 46.146 | 3523.953 | 3638.122 | 3603.170 | 3907.200 | 3959.008 |

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


