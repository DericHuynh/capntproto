# Capntproto verification reports

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-29T03:21:35Z · run 40811681878 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40811681878) · commit `10c90c1d254c` · **failure**

![Cargo test results](docs/reports/cargo-history.svg)

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-29T03:21:35Z · run 40811681878 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40811681878) · commit `10c90c1d254c` · **failure**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-29T03:21:35Z · run 40811681878 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40811681878) · commit `10c90c1d254c` · **failure**

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

Latest coverage run: [2026-10-29T01:57:50Z · run 40803524322 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40803524322) · commit `49bc86c9296a` · **failure**

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-29T03:21:35Z · run 40811681878 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40811681878) · commit `10c90c1d254c` · **failure**

Measurement evidence: **passed**. Independently validated encrypted benchmark values&#46; Workflow status and diagnostic completion are separate&#59; unavailable values remain blank&#46; Operations include whole concurrent batches and every service hop&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Current TCP fixtures use TLS 1.3, including upstream Rust Cap’n Proto, C++, gRPC over HTTP/2 and WebSockets over WSS; native encrypted transports and standard QUIC are identified separately. Historical data retains its recorded fixture contract. The dedicated campaign also reports recursive data, concurrent calls and independent service chains for gRPC and binary Protobuf WebSockets in its encrypted comparison artifact. Bars compare the recorded workload, rather than universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.


## Exhaustive benchmark tables

Measured run: [2026-10-29T03:21:35Z · run 40811681878 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40811681878) · commit `10c90c1d254c` · **failure**

All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.

### All encrypted latency comparisons

| Transport | Case | Implementation | Samples | Minimum µs | Mean µs | Sample SD µs | Run mean SD µs | Run mean min µs | Run mean max µs | p50 µs | p95 µs | p99 µs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tls | loopback&#45;0 | cpp | 5000 | 52.323 | 77.635 | 25.894 | 27.629 | 56.397 | 108.045 | 58.492 | 115.511 | 123.495 |
| tls | loopback&#45;0 | grpc | 5000 | 60.860 | 84.224 | 23.529 | 25.519 | 65.505 | 112.857 | 68.015 | 120.523 | 127.547 |
| tls | loopback&#45;0 | legacy | 5000 | 36.350 | 44.501 | 10.753 | 10.582 | 38.646 | 63.231 | 38.060 | 62.546 | 73.150 |
| tls | loopback&#45;0 | portable | 5000 | 36.930 | 50.859 | 14.779 | 15.310 | 39.261 | 67.885 | 40.233 | 69.092 | 82.454 |
| tls | loopback&#45;0 | websocket | 5000 | 32.191 | 44.073 | 9.370 | 7.730 | 36.842 | 54.135 | 37.858 | 56.885 | 65.189 |
| tls | loopback&#45;64 | cpp | 5000 | 97.075 | 107.788 | 6.912 | 0.433 | 107.285 | 108.313 | 105.612 | 120.621 | 124.557 |
| tls | loopback&#45;64 | grpc | 5000 | 62.005 | 94.867 | 24.409 | 26.142 | 65.898 | 114.418 | 110.663 | 124.969 | 130.650 |
| tls | loopback&#45;64 | legacy | 5000 | 36.840 | 58.570 | 10.703 | 10.891 | 39.090 | 63.588 | 62.042 | 71.656 | 78.727 |
| tls | loopback&#45;64 | portable | 5000 | 37.620 | 62.445 | 12.741 | 12.859 | 39.447 | 68.589 | 66.496 | 77.347 | 85.628 |
| tls | loopback&#45;64 | websocket | 5000 | 30.536 | 45.384 | 9.427 | 6.949 | 35.853 | 50.769 | 46.644 | 57.121 | 64.336 |
| tls | loopback&#45;1024 | cpp | 5000 | 54.421 | 80.039 | 26.881 | 29.146 | 58.558 | 112.266 | 60.225 | 119.084 | 127.031 |
| tls | loopback&#45;1024 | grpc | 5000 | 66.742 | 94.365 | 26.369 | 28.762 | 72.991 | 126.056 | 75.382 | 132.693 | 141.128 |
| tls | loopback&#45;1024 | legacy | 5000 | 37.789 | 47.230 | 12.306 | 11.513 | 40.494 | 67.495 | 40.195 | 67.020 | 79.820 |
| tls | loopback&#45;1024 | portable | 5000 | 39.069 | 53.942 | 15.543 | 16.485 | 41.608 | 72.043 | 42.095 | 74.417 | 87.139 |
| tls | loopback&#45;1024 | websocket | 5000 | 29.791 | 41.402 | 10.106 | 10.251 | 33.665 | 52.875 | 35.566 | 59.192 | 65.248 |
| tls | loopback&#45;65536 | cpp | 5000 | 228.749 | 257.631 | 40.888 | 23.126 | 246.084 | 298.958 | 245.645 | 396.720 | 419.196 |
| tls | loopback&#45;65536 | grpc | 5000 | 235.104 | 265.345 | 25.098 | 13.624 | 254.722 | 287.731 | 257.766 | 302.080 | 385.259 |
| tls | loopback&#45;65536 | legacy | 5000 | 156.411 | 168.067 | 7.808 | 1.244 | 165.966 | 169.091 | 165.166 | 181.741 | 193.627 |
| tls | loopback&#45;65536 | portable | 5000 | 164.253 | 174.055 | 8.286 | 1.319 | 172.611 | 175.649 | 170.582 | 191.252 | 202.425 |
| tls | loopback&#45;65536 | websocket | 5000 | 180.683 | 190.755 | 8.504 | 0.754 | 189.624 | 191.716 | 187.493 | 206.405 | 219.227 |
| tls | unary&#45;empty | cpp | 5000 | 52.062 | 56.025 | 4.181 | 0.422 | 55.474 | 56.640 | 54.669 | 64.683 | 74.985 |
| tls | unary&#45;empty | grpc | 5000 | 61.954 | 66.159 | 4.884 | 0.277 | 65.856 | 66.533 | 64.426 | 75.906 | 87.014 |
| tls | unary&#45;empty | legacy | 5000 | 35.715 | 38.573 | 3.597 | 0.332 | 38.307 | 38.939 | 37.487 | 44.973 | 55.379 |
| tls | unary&#45;empty | portable | 5000 | 36.530 | 39.241 | 3.721 | 0.269 | 38.978 | 39.609 | 38.200 | 46.288 | 55.853 |
| tls | unary&#45;empty | websocket | 5000 | 33.517 | 37.030 | 2.957 | 0.297 | 36.667 | 37.458 | 36.488 | 41.810 | 50.371 |
| tls | unary&#45;small | cpp | 5000 | 52.638 | 56.580 | 4.484 | 0.664 | 56.050 | 57.697 | 55.156 | 65.288 | 75.587 |
| tls | unary&#45;small | grpc | 5000 | 62.211 | 67.242 | 6.538 | 1.678 | 66.299 | 70.235 | 64.955 | 79.456 | 95.265 |
| tls | unary&#45;small | legacy | 5000 | 36.150 | 38.518 | 3.116 | 0.435 | 38.138 | 39.128 | 37.658 | 44.044 | 52.183 |
| tls | unary&#45;small | portable | 5000 | 36.967 | 39.581 | 3.808 | 0.297 | 39.203 | 39.929 | 38.464 | 46.799 | 56.063 |
| tls | unary&#45;small | websocket | 5000 | 29.708 | 35.231 | 3.542 | 0.287 | 34.918 | 35.604 | 34.639 | 40.409 | 49.135 |
| tls | unary&#45;1k | cpp | 5000 | 54.883 | 59.343 | 4.780 | 0.424 | 58.956 | 59.961 | 57.669 | 68.188 | 77.605 |
| tls | unary&#45;1k | grpc | 5000 | 67.493 | 82.678 | 21.359 | 17.757 | 73.902 | 114.400 | 72.966 | 126.145 | 138.367 |
| tls | unary&#45;1k | legacy | 5000 | 37.858 | 40.969 | 3.756 | 0.099 | 40.818 | 41.051 | 39.840 | 47.240 | 56.401 |
| tls | unary&#45;1k | portable | 5000 | 38.717 | 48.059 | 12.398 | 13.209 | 41.813 | 71.680 | 41.277 | 71.351 | 83.430 |
| tls | unary&#45;1k | websocket | 5000 | 29.982 | 34.185 | 3.568 | 0.186 | 33.941 | 34.429 | 33.467 | 40.172 | 47.750 |
| tls | fanout&#45;32 | cpp | 5000 | 420.663 | 477.527 | 64.407 | 27.508 | 452.995 | 507.492 | 457.396 | 599.985 | 776.285 |
| tls | fanout&#45;32 | grpc | 5000 | 789.608 | 959.938 | 203.050 | 159.982 | 847.142 | 1196.541 | 845.794 | 1330.353 | 1349.398 |
| tls | fanout&#45;32 | legacy | 5000 | 171.517 | 198.631 | 49.393 | 31.957 | 183.337 | 255.788 | 181.652 | 357.140 | 373.171 |
| tls | fanout&#45;32 | portable | 5000 | 201.913 | 217.141 | 10.274 | 0.956 | 215.523 | 218.071 | 213.596 | 236.090 | 253.301 |
| tls | fanout&#45;32 | websocket | 5000 | 234.853 | 282.535 | 54.908 | 40.860 | 257.839 | 352.829 | 259.516 | 410.820 | 439.306 |
| tls | dependent&#45;read | cpp | 5000 | 121.610 | 157.272 | 53.055 | 58.513 | 130.173 | 261.934 | 129.411 | 268.695 | 276.476 |
| tls | dependent&#45;read | grpc | 5000 | 131.365 | 141.363 | 6.853 | 1.258 | 139.825 | 142.518 | 139.132 | 153.621 | 163.538 |
| tls | dependent&#45;read | legacy | 5000 | 77.988 | 106.248 | 28.877 | 31.280 | 83.198 | 141.691 | 86.211 | 149.670 | 157.183 |
| tls | dependent&#45;read | portable | 5000 | 97.953 | 131.038 | 40.143 | 38.348 | 103.789 | 189.812 | 105.781 | 200.288 | 208.767 |
| tls | dependent&#45;read | websocket | 5000 | 58.902 | 81.449 | 19.330 | 20.037 | 66.543 | 104.323 | 71.068 | 115.920 | 123.491 |
| tls | pipelined&#45;read | cpp | 5000 | 84.736 | 91.412 | 5.574 | 0.436 | 90.763 | 91.962 | 89.499 | 101.429 | 110.870 |
| tls | pipelined&#45;read | legacy | 5000 | 47.307 | 50.750 | 3.604 | 0.381 | 50.379 | 51.327 | 49.532 | 57.747 | 66.128 |
| tls | pipelined&#45;read | portable | 5000 | 72.454 | 77.460 | 5.096 | 0.361 | 77.177 | 77.879 | 75.510 | 88.379 | 97.746 |
| tls | write&#45;read | cpp | 5000 | 108.033 | 114.845 | 6.957 | 0.919 | 114.253 | 116.452 | 112.249 | 128.806 | 139.326 |
| tls | write&#45;read | grpc | 5000 | 133.230 | 142.508 | 7.538 | 1.778 | 140.968 | 145.344 | 140.138 | 156.124 | 167.278 |
| tls | write&#45;read | legacy | 5000 | 73.351 | 88.387 | 20.772 | 22.556 | 77.765 | 128.731 | 77.266 | 128.349 | 142.929 |
| tls | write&#45;read | portable | 5000 | 74.666 | 89.739 | 22.176 | 20.784 | 79.410 | 126.884 | 79.171 | 137.752 | 152.787 |
| tls | write&#45;read | websocket | 5000 | 58.488 | 73.917 | 16.155 | 16.573 | 66.102 | 103.559 | 67.075 | 110.207 | 121.103 |
| tls | callback | cpp | 5000 | 112.559 | 120.812 | 12.874 | 0.981 | 119.864 | 122.406 | 117.921 | 134.196 | 147.423 |
| tls | callback | grpc | 5000 | 66.936 | 72.463 | 4.249 | 0.740 | 71.657 | 73.310 | 71.102 | 81.228 | 90.402 |
| tls | callback | legacy | 5000 | 78.217 | 83.695 | 4.875 | 0.539 | 83.087 | 84.259 | 82.008 | 93.545 | 102.851 |
| tls | callback | portable | 5000 | 93.866 | 100.876 | 6.485 | 0.987 | 99.917 | 102.471 | 98.539 | 114.219 | 126.412 |
| tls | callback | websocket | 5000 | 58.387 | 66.992 | 5.305 | 0.591 | 66.135 | 67.671 | 66.096 | 75.237 | 83.876 |
| tls | bulk&#45;64k | cpp | 5000 | 228.831 | 247.029 | 17.424 | 1.254 | 245.468 | 248.964 | 243.369 | 265.738 | 283.172 |
| tls | bulk&#45;64k | grpc | 5000 | 240.494 | 266.092 | 18.440 | 8.431 | 260.126 | 280.687 | 260.165 | 300.596 | 333.490 |
| tls | bulk&#45;64k | legacy | 5000 | 159.184 | 168.206 | 7.072 | 0.912 | 167.399 | 169.681 | 165.537 | 182.277 | 193.163 |
| tls | bulk&#45;64k | portable | 5000 | 162.788 | 173.183 | 8.260 | 0.598 | 172.600 | 173.948 | 169.670 | 190.814 | 205.568 |
| tls | bulk&#45;64k | websocket | 5000 | 180.820 | 192.134 | 10.021 | 0.851 | 191.303 | 193.408 | 188.668 | 209.401 | 223.510 |
| tls | bulk&#45;256k | cpp | 5000 | 953.345 | 1026.451 | 66.396 | 24.662 | 1013.356 | 1070.387 | 1012.420 | 1082.682 | 1486.729 |
| tls | bulk&#45;256k | grpc | 5000 | 587.797 | 1034.109 | 339.971 | 276.421 | 737.305 | 1423.129 | 1155.733 | 1527.398 | 1608.252 |
| tls | bulk&#45;256k | legacy | 5000 | 481.314 | 562.140 | 89.879 | 83.992 | 505.131 | 698.325 | 512.286 | 710.323 | 725.054 |
| tls | bulk&#45;256k | portable | 5000 | 487.556 | 569.726 | 88.318 | 85.762 | 516.221 | 714.217 | 521.346 | 726.425 | 740.832 |
| tls | bulk&#45;256k | websocket | 5000 | 1159.193 | 1208.756 | 29.079 | 9.147 | 1198.851 | 1220.139 | 1203.198 | 1244.309 | 1342.118 |
| tls | bulk&#45;1m | cpp | 5000 | 3631.340 | 3956.488 | 84.306 | 37.338 | 3902.430 | 4001.344 | 3945.741 | 4095.644 | 4288.720 |
| tls | bulk&#45;1m | grpc | 5000 | 2357.184 | 4241.205 | 1071.010 | 1171.206 | 3541.131 | 6315.691 | 3673.098 | 6513.952 | 6708.744 |
| tls | bulk&#45;1m | legacy | 5000 | 1644.556 | 1746.631 | 156.814 | 111.043 | 1679.971 | 1940.084 | 1686.585 | 2152.243 | 2186.949 |
| tls | bulk&#45;1m | portable | 5000 | 1649.858 | 1750.080 | 164.526 | 129.255 | 1686.353 | 1981.215 | 1691.547 | 2200.500 | 2240.500 |
| tls | bulk&#45;1m | websocket | 5000 | 4696.402 | 5001.984 | 242.804 | 134.383 | 4858.010 | 5212.202 | 4928.011 | 5598.339 | 5799.387 |
| tls | bulk&#45;4m | cpp | 5000 | 16461.943 | 17189.965 | 536.792 | 157.380 | 17026.997 | 17357.513 | 17124.090 | 17742.374 | 18275.287 |
| tls | bulk&#45;4m | grpc | 5000 | 21806.143 | 26871.786 | 3605.910 | 3585.504 | 22866.758 | 29546.976 | 27859.763 | 32544.128 | 32949.656 |
| tls | bulk&#45;4m | legacy | 5000 | 7898.843 | 8510.337 | 617.975 | 269.359 | 8213.680 | 8866.039 | 8326.298 | 10263.775 | 10441.919 |
| tls | bulk&#45;4m | portable | 5000 | 7915.131 | 8365.382 | 353.748 | 102.018 | 8247.345 | 8501.136 | 8313.745 | 8622.557 | 10215.643 |
| tls | bulk&#45;4m | websocket | 5000 | 28948.351 | 30679.040 | 2052.230 | 783.587 | 29848.198 | 31899.263 | 29967.203 | 36156.280 | 36658.621 |
| tls | deep&#45;8 | cpp | 5000 | 55.906 | 58.786 | 3.735 | 0.205 | 58.521 | 59.082 | 57.534 | 67.248 | 72.604 |
| tls | deep&#45;8 | grpc | 5000 | 74.692 | 79.450 | 5.244 | 0.485 | 78.993 | 79.992 | 77.412 | 89.009 | 102.578 |
| tls | deep&#45;8 | legacy | 5000 | 37.801 | 40.867 | 3.103 | 0.401 | 40.358 | 41.168 | 39.950 | 46.718 | 55.677 |
| tls | deep&#45;8 | portable | 5000 | 39.771 | 42.032 | 3.351 | 0.335 | 41.545 | 42.387 | 41.090 | 47.562 | 56.075 |
| tls | deep&#45;8 | websocket | 5000 | 34.148 | 41.867 | 5.275 | 0.647 | 41.333 | 42.994 | 40.978 | 50.886 | 57.735 |
| tls | deep&#45;32 | cpp | 5000 | 62.657 | 65.598 | 3.948 | 0.237 | 65.264 | 65.881 | 64.083 | 74.033 | 81.317 |
| tls | deep&#45;32 | grpc | 5000 | 92.104 | 99.261 | 5.621 | 0.877 | 98.508 | 100.644 | 97.428 | 109.455 | 119.404 |
| tls | deep&#45;32 | legacy | 5000 | 44.652 | 47.860 | 4.024 | 0.255 | 47.562 | 48.108 | 46.564 | 54.680 | 63.974 |
| tls | deep&#45;32 | portable | 5000 | 45.515 | 49.126 | 3.966 | 0.292 | 48.633 | 49.385 | 47.715 | 56.722 | 66.100 |
| tls | deep&#45;32 | websocket | 5000 | 52.361 | 59.424 | 5.256 | 0.212 | 59.235 | 59.658 | 58.526 | 68.518 | 77.398 |
| tls | concurrent&#45;32 | cpp | 5000 | 473.661 | 504.112 | 36.309 | 10.534 | 495.961 | 522.418 | 497.571 | 529.918 | 650.106 |
| tls | concurrent&#45;32 | grpc | 5000 | 584.727 | 619.848 | 30.869 | 2.730 | 616.963 | 623.453 | 613.589 | 644.428 | 771.580 |
| tls | concurrent&#45;32 | legacy | 5000 | 118.309 | 127.757 | 10.051 | 1.079 | 126.972 | 129.600 | 125.582 | 138.256 | 168.884 |
| tls | concurrent&#45;32 | portable | 5000 | 143.460 | 154.876 | 8.964 | 0.783 | 154.112 | 155.904 | 152.289 | 170.545 | 187.628 |
| tls | concurrent&#45;32 | websocket | 5000 | 116.336 | 167.517 | 20.334 | 1.523 | 166.114 | 170.027 | 166.031 | 201.956 | 223.631 |
| tls | deep&#45;32&#45;concurrent&#45;32 | cpp | 5000 | 629.839 | 662.627 | 35.193 | 5.977 | 655.233 | 671.709 | 656.483 | 690.142 | 798.501 |
| tls | deep&#45;32&#45;concurrent&#45;32 | grpc | 5000 | 1459.009 | 1693.319 | 359.843 | 336.543 | 1539.529 | 2295.323 | 1547.043 | 2639.506 | 2791.515 |
| tls | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 390.614 | 416.121 | 25.151 | 3.301 | 413.339 | 421.425 | 410.715 | 439.364 | 489.133 |
| tls | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 422.075 | 446.709 | 19.517 | 2.158 | 444.054 | 449.941 | 443.680 | 464.271 | 540.422 |
| tls | deep&#45;32&#45;concurrent&#45;32 | websocket | 5000 | 660.234 | 740.745 | 95.828 | 40.385 | 720.064 | 812.845 | 720.527 | 815.676 | 1228.749 |
| tls | services&#45;4&#45;deep&#45;16 | cpp | 5000 | 463.373 | 502.570 | 27.042 | 8.608 | 495.502 | 517.582 | 499.469 | 528.815 | 618.787 |
| tls | services&#45;4&#45;deep&#45;16 | grpc | 5000 | 577.596 | 629.331 | 18.641 | 7.137 | 622.552 | 639.618 | 628.552 | 659.177 | 678.013 |
| tls | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 331.741 | 361.163 | 19.697 | 2.069 | 359.494 | 364.298 | 356.634 | 386.582 | 399.943 |
| tls | services&#45;4&#45;deep&#45;16 | portable | 5000 | 371.845 | 396.112 | 15.890 | 2.725 | 392.601 | 398.986 | 392.175 | 422.934 | 435.545 |
| tls | services&#45;4&#45;deep&#45;16 | websocket | 5000 | 259.760 | 332.937 | 37.522 | 1.619 | 330.154 | 334.266 | 331.025 | 394.464 | 413.138 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | 5000 | 3106.964 | 3711.056 | 231.644 | 149.430 | 3607.514 | 3971.861 | 3701.215 | 4065.633 | 4197.703 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | 5000 | 4923.175 | 5699.700 | 295.602 | 78.808 | 5580.920 | 5770.813 | 5741.377 | 6114.111 | 6319.409 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1764.738 | 2035.835 | 132.788 | 76.040 | 1917.760 | 2097.000 | 2033.972 | 2262.345 | 2351.713 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1881.841 | 2095.966 | 141.294 | 63.839 | 2022.028 | 2191.495 | 2056.039 | 2330.824 | 2430.419 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | 5000 | 1616.306 | 1952.925 | 107.651 | 37.076 | 1910.059 | 1992.264 | 1951.822 | 2113.339 | 2185.124 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 2670.532 | 3492.783 | 460.761 | 192.029 | 3207.228 | 3742.375 | 3523.894 | 4104.105 | 4233.664 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 4564.515 | 5650.891 | 418.909 | 125.477 | 5468.756 | 5800.778 | 5684.758 | 6289.926 | 6510.074 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1510.556 | 2066.090 | 117.309 | 65.353 | 2001.809 | 2151.922 | 2034.032 | 2277.191 | 2386.959 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1759.706 | 2249.579 | 120.948 | 44.897 | 2169.936 | 2274.539 | 2257.646 | 2447.690 | 2475.770 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1469.826 | 1776.211 | 132.251 | 55.388 | 1721.905 | 1841.470 | 1767.187 | 1986.049 | 2073.954 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 4984.015 | 6899.062 | 856.559 | 144.790 | 6705.622 | 7089.219 | 7069.095 | 8038.889 | 8318.533 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 8278.120 | 10752.381 | 1197.489 | 170.688 | 10498.948 | 10939.600 | 10898.829 | 12403.257 | 12838.599 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 3888.905 | 4350.375 | 239.067 | 144.228 | 4167.309 | 4569.661 | 4352.481 | 4716.889 | 4832.482 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3299.750 | 4449.882 | 249.688 | 79.784 | 4361.004 | 4574.670 | 4545.709 | 4749.869 | 4881.750 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 2683.180 | 3268.650 | 294.894 | 17.972 | 3241.428 | 3289.710 | 3221.265 | 3712.923 | 3979.502 |
| quic | loopback&#45;0 | legacy | 5000 | 47.416 | 57.225 | 14.867 | 14.165 | 50.595 | 82.556 | 50.056 | 86.587 | 99.501 |
| quic | loopback&#45;0 | portable | 5000 | 53.294 | 74.080 | 22.271 | 24.221 | 55.878 | 101.025 | 56.891 | 106.926 | 114.763 |
| quic | loopback&#45;64 | legacy | 5000 | 48.696 | 61.397 | 17.061 | 10.836 | 51.055 | 75.003 | 50.614 | 88.435 | 101.580 |
| quic | loopback&#45;64 | portable | 5000 | 53.607 | 65.812 | 18.730 | 19.922 | 55.998 | 101.436 | 55.756 | 100.960 | 113.830 |
| quic | loopback&#45;1024 | legacy | 5000 | 51.174 | 87.734 | 14.324 | 10.759 | 68.500 | 92.898 | 90.475 | 103.794 | 109.257 |
| quic | loopback&#45;1024 | portable | 5000 | 55.770 | 95.341 | 20.908 | 14.865 | 75.565 | 106.653 | 103.625 | 117.609 | 123.337 |
| quic | loopback&#45;65536 | legacy | 5000 | 209.840 | 232.344 | 39.324 | 24.684 | 220.096 | 276.469 | 218.649 | 355.722 | 371.125 |
| quic | loopback&#45;65536 | portable | 5000 | 214.089 | 226.544 | 10.174 | 0.765 | 225.999 | 227.867 | 222.977 | 246.839 | 255.156 |
| quic | unary&#45;empty | legacy | 5000 | 47.824 | 50.796 | 4.300 | 0.560 | 49.893 | 51.334 | 49.879 | 57.436 | 66.113 |
| quic | unary&#45;empty | portable | 5000 | 51.811 | 56.200 | 4.248 | 0.385 | 56.009 | 56.888 | 55.106 | 63.589 | 71.988 |
| quic | unary&#45;small | legacy | 5000 | 48.343 | 51.151 | 4.334 | 0.344 | 50.803 | 51.683 | 50.071 | 58.236 | 66.139 |
| quic | unary&#45;small | portable | 5000 | 53.010 | 56.449 | 4.339 | 0.222 | 56.270 | 56.830 | 55.259 | 64.399 | 71.547 |
| quic | unary&#45;1k | legacy | 5000 | 49.895 | 52.905 | 2.859 | 0.112 | 52.768 | 53.073 | 52.016 | 59.335 | 64.743 |
| quic | unary&#45;1k | portable | 5000 | 54.561 | 58.934 | 5.269 | 0.186 | 58.747 | 59.235 | 57.646 | 67.392 | 74.296 |
| quic | fanout&#45;32 | legacy | 5000 | 211.226 | 221.033 | 10.425 | 0.597 | 220.485 | 222.018 | 217.050 | 241.101 | 249.729 |
| quic | fanout&#45;32 | portable | 5000 | 243.299 | 256.702 | 8.728 | 0.330 | 256.355 | 257.228 | 253.545 | 269.164 | 281.323 |
| quic | dependent&#45;read | legacy | 5000 | 103.749 | 126.603 | 32.903 | 35.258 | 110.299 | 189.669 | 109.567 | 196.502 | 206.759 |
| quic | dependent&#45;read | portable | 5000 | 138.251 | 165.150 | 43.047 | 41.817 | 145.731 | 239.942 | 144.311 | 264.450 | 275.014 |
| quic | pipelined&#45;read | legacy | 5000 | 60.932 | 79.331 | 23.144 | 21.673 | 65.261 | 115.717 | 64.965 | 118.119 | 130.938 |
| quic | pipelined&#45;read | portable | 5000 | 94.343 | 127.643 | 38.748 | 37.822 | 100.757 | 181.556 | 100.792 | 191.408 | 199.174 |
| quic | write&#45;read | legacy | 5000 | 97.244 | 103.981 | 6.245 | 0.497 | 103.119 | 104.372 | 102.165 | 113.595 | 124.535 |
| quic | write&#45;read | portable | 5000 | 107.698 | 115.397 | 6.895 | 0.334 | 114.975 | 115.865 | 113.220 | 126.315 | 137.874 |
| quic | callback | legacy | 5000 | 101.761 | 109.532 | 7.674 | 1.067 | 108.402 | 111.152 | 107.204 | 120.400 | 131.697 |
| quic | callback | portable | 5000 | 130.879 | 140.851 | 8.248 | 0.884 | 139.751 | 141.823 | 138.040 | 154.043 | 166.429 |
| quic | bulk&#45;64k | legacy | 5000 | 208.903 | 220.479 | 9.861 | 1.300 | 218.851 | 222.146 | 216.532 | 241.222 | 249.689 |
| quic | bulk&#45;64k | portable | 5000 | 212.232 | 224.880 | 9.884 | 0.622 | 224.386 | 225.943 | 221.144 | 245.636 | 252.895 |
| quic | bulk&#45;256k | legacy | 5000 | 528.936 | 618.601 | 116.541 | 120.716 | 549.311 | 831.834 | 557.490 | 838.795 | 849.730 |
| quic | bulk&#45;256k | portable | 5000 | 548.340 | 632.300 | 119.546 | 67.170 | 571.344 | 734.418 | 575.620 | 877.326 | 889.242 |
| quic | bulk&#45;1m | legacy | 5000 | 1925.993 | 2140.792 | 316.995 | 246.951 | 1982.986 | 2574.782 | 2001.810 | 2862.654 | 2897.781 |
| quic | bulk&#45;1m | portable | 5000 | 1925.996 | 2257.486 | 676.195 | 314.441 | 1987.946 | 2764.109 | 2004.436 | 2875.330 | 2925.994 |
| quic | bulk&#45;4m | legacy | 5000 | 9281.060 | 10028.776 | 914.270 | 189.188 | 9809.322 | 10211.271 | 9832.607 | 13001.226 | 13711.545 |
| quic | bulk&#45;4m | portable | 5000 | 9572.039 | 10482.059 | 1096.675 | 705.579 | 10117.857 | 11741.003 | 10130.708 | 13617.444 | 13901.205 |
| quic | deep&#45;8 | legacy | 5000 | 50.949 | 54.805 | 4.386 | 0.384 | 54.495 | 55.451 | 53.523 | 62.574 | 70.891 |
| quic | deep&#45;8 | portable | 5000 | 56.098 | 60.632 | 3.990 | 0.524 | 60.157 | 61.273 | 59.308 | 69.254 | 76.935 |
| quic | deep&#45;32 | legacy | 5000 | 57.515 | 61.918 | 4.167 | 0.081 | 61.838 | 62.052 | 60.547 | 70.514 | 78.209 |
| quic | deep&#45;32 | portable | 5000 | 63.743 | 68.886 | 5.088 | 0.243 | 68.475 | 69.112 | 67.174 | 78.377 | 87.317 |
| quic | concurrent&#45;32 | legacy | 5000 | 135.413 | 144.498 | 6.681 | 0.892 | 143.437 | 145.529 | 142.234 | 155.309 | 165.710 |
| quic | concurrent&#45;32 | portable | 5000 | 166.729 | 178.783 | 9.017 | 1.182 | 177.243 | 180.287 | 175.231 | 194.605 | 212.436 |
| quic | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 435.396 | 464.210 | 12.629 | 2.978 | 459.561 | 467.165 | 462.507 | 479.966 | 496.642 |
| quic | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 474.835 | 497.081 | 10.505 | 1.806 | 494.557 | 498.918 | 496.133 | 511.014 | 525.958 |
| quic | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 509.935 | 545.791 | 82.016 | 3.070 | 541.191 | 549.450 | 540.441 | 566.075 | 602.267 |
| quic | services&#45;4&#45;deep&#45;16 | portable | 5000 | 503.521 | 534.994 | 62.033 | 3.280 | 531.569 | 540.218 | 531.059 | 556.838 | 582.723 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 2152.756 | 2543.505 | 85.068 | 41.435 | 2503.173 | 2603.734 | 2527.374 | 2705.240 | 2794.256 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 2194.564 | 2561.693 | 103.434 | 53.396 | 2482.003 | 2631.358 | 2556.377 | 2726.206 | 2808.973 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2179.172 | 2576.893 | 150.961 | 28.428 | 2526.731 | 2596.319 | 2558.425 | 2845.331 | 2963.254 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1965.440 | 2475.669 | 199.452 | 101.871 | 2321.543 | 2578.235 | 2542.915 | 2729.194 | 2826.564 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 4009.349 | 5181.886 | 315.367 | 155.851 | 5089.931 | 5456.809 | 5098.571 | 5697.466 | 5915.161 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3702.485 | 4459.288 | 537.261 | 271.289 | 4055.200 | 4783.016 | 4335.435 | 5307.603 | 5620.323 |

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


