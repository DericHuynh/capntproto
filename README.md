# Capntproto verification reports

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-31T01:47:16Z · run 41156999914 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41156999914) · commit `3cdec872355f` · **failure**

![Cargo test results](docs/reports/cargo-history.svg)

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-31T01:47:16Z · run 41156999914 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41156999914) · commit `3cdec872355f` · **failure**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-31T01:47:16Z · run 41156999914 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41156999914) · commit `3cdec872355f` · **failure**

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

Latest coverage run: [2026-10-31T01:47:16Z · run 41156999722 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41156999722) · commit `3cdec872355f` · **failure**

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-31T01:47:16Z · run 41156999914 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41156999914) · commit `3cdec872355f` · **failure**

Measurement evidence: **passed**. Independently validated encrypted benchmark values&#46; Workflow status and diagnostic completion are separate&#59; unavailable values remain blank&#46; Operations include whole concurrent batches and every service hop&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Current TCP fixtures use TLS 1.3, including upstream Rust Cap’n Proto, C++, gRPC over HTTP/2 and WebSockets over WSS; native encrypted transports and standard QUIC are identified separately. Historical data retains its recorded fixture contract. The dedicated campaign also reports recursive data, concurrent calls and independent service chains for gRPC and binary Protobuf WebSockets in its encrypted comparison artifact. Bars compare the recorded workload, rather than universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.


## Exhaustive benchmark tables

Measured run: [2026-10-31T01:47:16Z · run 41156999914 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41156999914) · commit `3cdec872355f` · **failure**

All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.

### All encrypted latency comparisons

| Transport | Case | Implementation | Samples | Minimum µs | Mean µs | Sample SD µs | Run mean SD µs | Run mean min µs | Run mean max µs | p50 µs | p95 µs | p99 µs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tls | loopback&#45;0 | cpp | 5000 | 54.561 | 71.059 | 20.357 | 21.318 | 58.845 | 108.602 | 60.387 | 107.207 | 121.976 |
| tls | loopback&#45;0 | grpc | 5000 | 63.091 | 77.376 | 19.503 | 20.709 | 67.465 | 114.398 | 67.117 | 113.652 | 126.921 |
| tls | loopback&#45;0 | legacy | 5000 | 36.710 | 39.893 | 4.407 | 0.374 | 39.471 | 40.376 | 38.714 | 47.468 | 57.344 |
| tls | loopback&#45;0 | portable | 5000 | 37.341 | 40.952 | 5.528 | 0.633 | 40.366 | 41.928 | 39.557 | 49.870 | 60.197 |
| tls | loopback&#45;0 | websocket | 5000 | 31.515 | 39.279 | 7.814 | 5.804 | 36.462 | 49.656 | 36.183 | 54.032 | 60.548 |
| tls | loopback&#45;64 | cpp | 5000 | 54.297 | 61.065 | 7.510 | 1.443 | 58.732 | 62.331 | 58.759 | 73.648 | 86.159 |
| tls | loopback&#45;64 | grpc | 5000 | 63.364 | 72.571 | 11.764 | 6.207 | 67.677 | 83.292 | 68.251 | 97.337 | 117.399 |
| tls | loopback&#45;64 | legacy | 5000 | 37.259 | 41.353 | 5.012 | 0.823 | 40.357 | 42.341 | 40.133 | 49.510 | 60.159 |
| tls | loopback&#45;64 | portable | 5000 | 38.147 | 42.522 | 5.176 | 0.736 | 41.670 | 43.576 | 41.289 | 50.941 | 63.228 |
| tls | loopback&#45;64 | websocket | 5000 | 30.614 | 35.922 | 5.574 | 0.599 | 35.098 | 36.585 | 34.860 | 43.616 | 53.491 |
| tls | loopback&#45;1024 | cpp | 5000 | 57.000 | 62.664 | 7.029 | 0.883 | 61.510 | 63.859 | 60.287 | 74.094 | 88.169 |
| tls | loopback&#45;1024 | grpc | 5000 | 67.539 | 75.946 | 8.294 | 1.459 | 73.892 | 77.616 | 73.735 | 89.891 | 105.783 |
| tls | loopback&#45;1024 | legacy | 5000 | 39.451 | 43.524 | 5.230 | 0.661 | 42.876 | 44.534 | 42.191 | 51.647 | 61.557 |
| tls | loopback&#45;1024 | portable | 5000 | 40.304 | 44.085 | 4.921 | 0.597 | 43.485 | 44.789 | 42.588 | 51.695 | 63.620 |
| tls | loopback&#45;1024 | websocket | 5000 | 30.365 | 35.725 | 4.812 | 0.696 | 34.770 | 36.384 | 34.895 | 42.501 | 52.000 |
| tls | loopback&#45;65536 | cpp | 5000 | 235.666 | 256.904 | 12.794 | 2.196 | 253.926 | 259.956 | 254.414 | 279.466 | 301.687 |
| tls | loopback&#45;65536 | grpc | 5000 | 241.733 | 271.499 | 21.080 | 4.183 | 267.178 | 276.405 | 265.248 | 316.157 | 346.780 |
| tls | loopback&#45;65536 | legacy | 5000 | 160.798 | 181.792 | 33.990 | 21.424 | 169.263 | 218.957 | 167.151 | 261.424 | 279.105 |
| tls | loopback&#45;65536 | portable | 5000 | 169.237 | 197.464 | 40.087 | 41.530 | 177.572 | 271.718 | 176.985 | 284.994 | 295.070 |
| tls | loopback&#45;65536 | websocket | 5000 | 181.030 | 200.797 | 29.746 | 19.969 | 190.430 | 236.471 | 189.055 | 283.332 | 297.080 |
| tls | unary&#45;empty | cpp | 5000 | 54.400 | 58.817 | 5.103 | 0.816 | 57.880 | 60.118 | 57.137 | 68.953 | 81.140 |
| tls | unary&#45;empty | grpc | 5000 | 63.612 | 68.966 | 7.657 | 1.068 | 67.360 | 70.235 | 67.003 | 79.715 | 92.877 |
| tls | unary&#45;empty | legacy | 5000 | 36.831 | 39.903 | 3.914 | 0.514 | 39.436 | 40.773 | 38.721 | 47.119 | 58.196 |
| tls | unary&#45;empty | portable | 5000 | 37.677 | 41.219 | 4.959 | 0.501 | 40.788 | 41.765 | 40.025 | 49.109 | 60.419 |
| tls | unary&#45;empty | websocket | 5000 | 31.396 | 37.395 | 4.873 | 0.385 | 37.021 | 37.970 | 36.704 | 42.661 | 52.993 |
| tls | unary&#45;small | cpp | 5000 | 54.369 | 59.304 | 6.501 | 0.876 | 58.502 | 60.644 | 57.481 | 70.254 | 86.222 |
| tls | unary&#45;small | grpc | 5000 | 64.262 | 69.701 | 6.526 | 0.960 | 68.375 | 70.693 | 67.778 | 80.051 | 97.099 |
| tls | unary&#45;small | legacy | 5000 | 36.738 | 40.154 | 4.680 | 0.233 | 39.825 | 40.476 | 38.889 | 47.560 | 59.599 |
| tls | unary&#45;small | portable | 5000 | 38.120 | 41.504 | 4.796 | 0.088 | 41.396 | 41.614 | 40.200 | 49.562 | 61.903 |
| tls | unary&#45;small | websocket | 5000 | 30.723 | 35.630 | 4.775 | 0.168 | 35.436 | 35.884 | 34.709 | 41.353 | 54.206 |
| tls | unary&#45;1k | cpp | 5000 | 57.099 | 62.340 | 6.613 | 0.959 | 61.822 | 64.050 | 60.194 | 73.427 | 89.022 |
| tls | unary&#45;1k | grpc | 5000 | 67.692 | 79.334 | 16.985 | 9.928 | 74.750 | 97.090 | 72.648 | 123.992 | 137.551 |
| tls | unary&#45;1k | legacy | 5000 | 39.292 | 42.735 | 4.337 | 0.666 | 41.821 | 43.640 | 41.366 | 50.077 | 60.526 |
| tls | unary&#45;1k | portable | 5000 | 40.129 | 44.037 | 4.941 | 0.685 | 43.285 | 44.796 | 42.675 | 51.794 | 63.418 |
| tls | unary&#45;1k | websocket | 5000 | 30.354 | 35.824 | 4.433 | 0.789 | 34.709 | 36.920 | 34.990 | 42.531 | 53.775 |
| tls | fanout&#45;32 | cpp | 5000 | 434.921 | 612.123 | 223.857 | 238.790 | 472.379 | 1033.772 | 481.203 | 1048.493 | 1160.430 |
| tls | fanout&#45;32 | grpc | 5000 | 813.207 | 881.426 | 114.312 | 49.039 | 848.188 | 964.575 | 850.747 | 1318.279 | 1354.871 |
| tls | fanout&#45;32 | legacy | 5000 | 175.116 | 222.025 | 71.514 | 76.590 | 184.074 | 358.739 | 184.173 | 367.044 | 375.100 |
| tls | fanout&#45;32 | portable | 5000 | 211.667 | 225.330 | 13.032 | 2.241 | 222.388 | 228.507 | 220.478 | 250.919 | 272.962 |
| tls | fanout&#45;32 | websocket | 5000 | 239.038 | 292.178 | 56.460 | 60.321 | 264.389 | 400.072 | 268.196 | 414.499 | 441.063 |
| tls | dependent&#45;read | cpp | 5000 | 125.900 | 158.626 | 48.892 | 49.419 | 135.580 | 247.008 | 134.974 | 267.815 | 277.242 |
| tls | dependent&#45;read | grpc | 5000 | 134.256 | 166.212 | 40.109 | 33.440 | 143.822 | 222.995 | 146.129 | 250.761 | 260.045 |
| tls | dependent&#45;read | legacy | 5000 | 80.924 | 88.403 | 9.833 | 2.325 | 87.223 | 92.545 | 85.445 | 101.952 | 140.779 |
| tls | dependent&#45;read | portable | 5000 | 99.163 | 123.937 | 34.444 | 36.970 | 106.261 | 190.053 | 105.873 | 194.345 | 206.411 |
| tls | dependent&#45;read | websocket | 5000 | 59.855 | 76.003 | 16.019 | 16.274 | 67.946 | 105.095 | 69.327 | 111.006 | 122.888 |
| tls | pipelined&#45;read | cpp | 5000 | 87.874 | 96.933 | 10.147 | 3.100 | 93.466 | 101.200 | 93.786 | 117.461 | 147.122 |
| tls | pipelined&#45;read | legacy | 5000 | 48.902 | 60.302 | 15.339 | 16.224 | 52.312 | 89.303 | 51.970 | 88.990 | 102.869 |
| tls | pipelined&#45;read | portable | 5000 | 73.767 | 84.724 | 17.112 | 8.891 | 79.379 | 100.463 | 78.350 | 139.645 | 151.287 |
| tls | write&#45;read | cpp | 5000 | 111.058 | 121.006 | 8.429 | 0.862 | 120.032 | 122.058 | 118.520 | 136.371 | 153.156 |
| tls | write&#45;read | grpc | 5000 | 133.019 | 161.850 | 38.822 | 39.217 | 142.475 | 231.953 | 143.623 | 249.319 | 260.249 |
| tls | write&#45;read | legacy | 5000 | 75.654 | 82.756 | 7.585 | 1.695 | 81.312 | 85.628 | 80.543 | 95.389 | 108.575 |
| tls | write&#45;read | portable | 5000 | 78.084 | 84.830 | 6.638 | 0.987 | 83.686 | 86.313 | 82.783 | 97.143 | 109.366 |
| tls | write&#45;read | websocket | 5000 | 59.359 | 68.674 | 6.031 | 1.088 | 67.626 | 70.405 | 67.529 | 80.227 | 88.976 |
| tls | callback | cpp | 5000 | 117.235 | 130.312 | 12.773 | 6.754 | 126.342 | 142.317 | 125.683 | 155.079 | 175.214 |
| tls | callback | grpc | 5000 | 70.742 | 77.040 | 5.722 | 1.285 | 75.535 | 78.863 | 75.248 | 88.801 | 99.947 |
| tls | callback | legacy | 5000 | 80.471 | 87.924 | 8.518 | 0.700 | 86.898 | 88.806 | 85.693 | 101.488 | 113.773 |
| tls | callback | portable | 5000 | 95.842 | 116.335 | 29.441 | 27.472 | 103.709 | 165.475 | 102.151 | 178.324 | 195.866 |
| tls | callback | websocket | 5000 | 59.773 | 68.570 | 6.543 | 0.592 | 68.097 | 69.549 | 67.435 | 79.479 | 90.466 |
| tls | bulk&#45;64k | cpp | 5000 | 234.068 | 255.152 | 11.856 | 2.826 | 251.531 | 258.363 | 253.053 | 275.850 | 293.874 |
| tls | bulk&#45;64k | grpc | 5000 | 244.403 | 269.577 | 18.993 | 2.113 | 267.768 | 272.347 | 262.759 | 311.648 | 336.667 |
| tls | bulk&#45;64k | legacy | 5000 | 161.581 | 177.093 | 26.519 | 16.752 | 167.771 | 206.971 | 167.179 | 256.822 | 274.750 |
| tls | bulk&#45;64k | portable | 5000 | 168.936 | 182.007 | 21.952 | 7.949 | 176.744 | 196.013 | 174.092 | 213.186 | 286.712 |
| tls | bulk&#45;64k | websocket | 5000 | 181.503 | 191.484 | 10.874 | 0.796 | 190.789 | 192.833 | 187.873 | 210.856 | 225.474 |
| tls | bulk&#45;256k | cpp | 5000 | 956.330 | 1040.245 | 51.350 | 26.166 | 1015.152 | 1084.098 | 1019.526 | 1136.239 | 1168.491 |
| tls | bulk&#45;256k | grpc | 5000 | 599.493 | 1358.201 | 281.281 | 224.799 | 973.826 | 1517.125 | 1414.380 | 1805.054 | 1895.864 |
| tls | bulk&#45;256k | legacy | 5000 | 469.429 | 549.483 | 90.197 | 62.622 | 498.431 | 647.345 | 501.473 | 715.172 | 728.736 |
| tls | bulk&#45;256k | portable | 5000 | 480.160 | 517.358 | 49.554 | 25.410 | 504.672 | 562.711 | 505.026 | 696.136 | 722.190 |
| tls | bulk&#45;256k | websocket | 5000 | 1131.673 | 1236.901 | 65.908 | 57.729 | 1180.277 | 1308.634 | 1257.545 | 1331.882 | 1415.806 |
| tls | bulk&#45;1m | cpp | 5000 | 3673.470 | 4049.188 | 404.219 | 79.867 | 3924.270 | 4119.467 | 3948.888 | 5358.228 | 5638.977 |
| tls | bulk&#45;1m | grpc | 5000 | 2323.243 | 4339.388 | 1048.878 | 1091.640 | 3566.279 | 6261.517 | 3984.825 | 6501.904 | 7209.631 |
| tls | bulk&#45;1m | legacy | 5000 | 1626.633 | 1854.586 | 232.914 | 186.979 | 1722.125 | 2162.424 | 1693.274 | 2194.909 | 2220.233 |
| tls | bulk&#45;1m | portable | 5000 | 1630.125 | 1910.046 | 252.293 | 225.720 | 1689.654 | 2202.059 | 1710.772 | 2232.127 | 2266.723 |
| tls | bulk&#45;1m | websocket | 5000 | 4524.847 | 4996.507 | 414.940 | 232.798 | 4755.939 | 5301.615 | 4855.911 | 5934.679 | 6168.301 |
| tls | bulk&#45;4m | cpp | 5000 | 16616.719 | 18362.203 | 1931.253 | 808.511 | 17670.640 | 19576.662 | 17788.633 | 24354.839 | 25120.876 |
| tls | bulk&#45;4m | grpc | 5000 | 21598.320 | 27439.039 | 3455.342 | 3264.057 | 22700.864 | 30250.028 | 27996.320 | 32978.553 | 33403.343 |
| tls | bulk&#45;4m | legacy | 5000 | 7951.772 | 9124.115 | 920.153 | 178.096 | 8818.886 | 9254.635 | 8648.622 | 10728.903 | 10885.943 |
| tls | bulk&#45;4m | portable | 5000 | 7975.733 | 9027.613 | 766.712 | 435.809 | 8693.680 | 9619.915 | 8718.863 | 10703.045 | 10889.727 |
| tls | bulk&#45;4m | websocket | 5000 | 28812.061 | 31589.658 | 2793.075 | 654.770 | 30796.795 | 32410.348 | 29898.843 | 36135.299 | 36356.829 |
| tls | deep&#45;8 | cpp | 5000 | 110.674 | 115.459 | 6.546 | 0.430 | 114.977 | 115.891 | 113.051 | 130.823 | 141.518 |
| tls | deep&#45;8 | grpc | 5000 | 136.894 | 143.280 | 8.124 | 0.187 | 143.063 | 143.468 | 140.101 | 159.129 | 173.476 |
| tls | deep&#45;8 | legacy | 5000 | 68.038 | 71.380 | 5.077 | 0.442 | 70.655 | 71.733 | 69.939 | 81.573 | 93.162 |
| tls | deep&#45;8 | portable | 5000 | 71.661 | 76.134 | 5.546 | 0.776 | 75.220 | 76.834 | 74.561 | 88.958 | 101.239 |
| tls | deep&#45;8 | websocket | 5000 | 57.843 | 68.031 | 7.499 | 0.770 | 66.772 | 68.664 | 65.809 | 82.838 | 91.939 |
| tls | deep&#45;32 | cpp | 5000 | 64.220 | 69.953 | 6.821 | 1.103 | 68.513 | 71.561 | 67.970 | 81.038 | 98.649 |
| tls | deep&#45;32 | grpc | 5000 | 92.814 | 114.465 | 33.829 | 29.677 | 99.586 | 167.474 | 99.447 | 194.734 | 210.496 |
| tls | deep&#45;32 | legacy | 5000 | 46.895 | 59.555 | 17.173 | 7.800 | 51.268 | 68.128 | 50.891 | 91.526 | 106.900 |
| tls | deep&#45;32 | portable | 5000 | 47.874 | 52.716 | 5.771 | 0.518 | 52.178 | 53.368 | 51.023 | 61.027 | 79.564 |
| tls | deep&#45;32 | websocket | 5000 | 53.397 | 82.077 | 24.996 | 26.523 | 61.743 | 111.450 | 67.709 | 121.957 | 132.910 |
| tls | concurrent&#45;32 | cpp | 5000 | 492.802 | 1043.218 | 256.277 | 175.297 | 798.934 | 1170.843 | 1162.992 | 1193.050 | 1206.418 |
| tls | concurrent&#45;32 | grpc | 5000 | 603.031 | 956.434 | 172.006 | 163.794 | 672.363 | 1055.026 | 1043.240 | 1069.278 | 1086.296 |
| tls | concurrent&#45;32 | legacy | 5000 | 125.040 | 252.927 | 59.497 | 65.596 | 135.664 | 284.933 | 276.914 | 299.272 | 311.562 |
| tls | concurrent&#45;32 | portable | 5000 | 154.087 | 279.873 | 92.504 | 101.917 | 166.452 | 358.851 | 345.440 | 371.379 | 384.085 |
| tls | concurrent&#45;32 | websocket | 5000 | 117.506 | 236.531 | 50.894 | 45.610 | 174.972 | 270.094 | 252.225 | 304.981 | 324.509 |
| tls | deep&#45;32&#45;concurrent&#45;32 | cpp | 5000 | 643.515 | 1063.173 | 334.904 | 330.947 | 690.927 | 1359.911 | 1345.554 | 1383.379 | 1400.784 |
| tls | deep&#45;32&#45;concurrent&#45;32 | grpc | 5000 | 1481.420 | 1837.337 | 481.279 | 506.006 | 1586.556 | 2741.026 | 1580.243 | 2830.038 | 2861.827 |
| tls | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 422.936 | 530.649 | 166.170 | 183.797 | 443.851 | 859.386 | 447.393 | 864.914 | 875.795 |
| tls | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 443.317 | 573.999 | 168.872 | 164.926 | 461.556 | 832.793 | 466.301 | 840.403 | 851.348 |
| tls | deep&#45;32&#45;concurrent&#45;32 | websocket | 5000 | 666.819 | 735.418 | 42.861 | 4.443 | 730.985 | 741.096 | 727.050 | 799.962 | 946.277 |
| tls | services&#45;4&#45;deep&#45;16 | cpp | 5000 | 472.706 | 518.909 | 20.970 | 10.026 | 508.647 | 532.021 | 517.429 | 551.109 | 570.315 |
| tls | services&#45;4&#45;deep&#45;16 | grpc | 5000 | 620.457 | 667.946 | 23.407 | 1.992 | 665.409 | 669.985 | 666.936 | 692.244 | 716.645 |
| tls | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 345.192 | 376.922 | 15.919 | 3.112 | 374.550 | 381.961 | 375.350 | 401.592 | 419.683 |
| tls | services&#45;4&#45;deep&#45;16 | portable | 5000 | 384.929 | 417.719 | 16.303 | 3.805 | 413.808 | 424.060 | 417.053 | 443.072 | 457.518 |
| tls | services&#45;4&#45;deep&#45;16 | websocket | 5000 | 276.630 | 349.468 | 38.342 | 4.487 | 343.874 | 356.374 | 347.632 | 413.398 | 433.773 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | 5000 | 3230.918 | 4096.779 | 277.778 | 111.222 | 3983.407 | 4226.043 | 4066.713 | 4600.852 | 4838.008 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | 5000 | 5056.707 | 6148.548 | 357.394 | 120.475 | 6041.054 | 6347.763 | 6087.566 | 6778.206 | 7060.681 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1878.161 | 2285.931 | 145.103 | 91.656 | 2150.647 | 2399.100 | 2307.274 | 2495.080 | 2565.895 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1982.549 | 2369.226 | 125.178 | 70.529 | 2262.638 | 2441.186 | 2383.309 | 2578.734 | 2656.104 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | 5000 | 1775.741 | 2008.180 | 70.265 | 11.960 | 1998.456 | 2028.257 | 2005.393 | 2126.625 | 2203.914 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 2813.299 | 3614.124 | 1793.308 | 154.998 | 3498.811 | 3877.613 | 3513.622 | 4244.816 | 4512.546 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 4680.089 | 5971.793 | 578.345 | 110.998 | 5848.433 | 6098.405 | 5980.966 | 6881.261 | 7093.165 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1789.709 | 2362.539 | 148.705 | 40.546 | 2333.066 | 2423.834 | 2356.708 | 2565.172 | 2649.534 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2024.032 | 2423.315 | 109.725 | 28.452 | 2374.499 | 2444.480 | 2417.089 | 2583.019 | 2634.990 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1509.665 | 1811.648 | 126.831 | 35.203 | 1774.383 | 1850.046 | 1802.194 | 2018.688 | 2113.487 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 5115.876 | 7632.903 | 802.134 | 165.695 | 7483.255 | 7863.830 | 7868.555 | 8621.818 | 8947.841 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 8829.956 | 11723.118 | 1509.254 | 372.164 | 11303.595 | 12300.892 | 12117.110 | 13658.573 | 14066.593 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 4026.338 | 4953.478 | 164.410 | 61.179 | 4884.809 | 5016.817 | 4955.923 | 5219.274 | 5396.040 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3873.931 | 4904.142 | 354.769 | 228.109 | 4646.274 | 5216.310 | 4925.964 | 5463.356 | 5683.243 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 2773.783 | 3366.617 | 275.239 | 17.934 | 3341.124 | 3379.849 | 3297.770 | 3909.458 | 4186.405 |
| quic | loopback&#45;0 | legacy | 5000 | 49.114 | 53.406 | 5.431 | 0.828 | 52.549 | 54.514 | 51.941 | 62.272 | 72.756 |
| quic | loopback&#45;0 | portable | 5000 | 53.102 | 57.966 | 5.011 | 1.064 | 57.153 | 59.814 | 56.430 | 67.259 | 76.221 |
| quic | loopback&#45;64 | legacy | 5000 | 49.248 | 54.074 | 7.041 | 1.552 | 52.032 | 55.727 | 52.312 | 63.711 | 87.021 |
| quic | loopback&#45;64 | portable | 5000 | 53.655 | 66.778 | 18.160 | 19.520 | 57.186 | 101.684 | 57.419 | 101.011 | 114.548 |
| quic | loopback&#45;1024 | legacy | 5000 | 50.584 | 62.113 | 15.521 | 16.678 | 53.707 | 91.916 | 53.867 | 91.239 | 103.464 |
| quic | loopback&#45;1024 | portable | 5000 | 56.168 | 76.376 | 22.308 | 22.142 | 59.832 | 105.717 | 61.588 | 111.031 | 120.934 |
| quic | loopback&#45;65536 | legacy | 5000 | 208.551 | 309.679 | 74.971 | 82.738 | 219.190 | 392.609 | 349.283 | 402.902 | 417.304 |
| quic | loopback&#45;65536 | portable | 5000 | 215.358 | 336.046 | 60.507 | 61.012 | 228.339 | 371.972 | 362.756 | 382.078 | 391.562 |
| quic | unary&#45;empty | legacy | 5000 | 49.112 | 53.835 | 5.356 | 0.316 | 53.557 | 54.302 | 52.573 | 62.667 | 73.502 |
| quic | unary&#45;empty | portable | 5000 | 53.883 | 59.190 | 5.337 | 0.982 | 58.240 | 60.657 | 57.796 | 69.162 | 79.404 |
| quic | unary&#45;small | legacy | 5000 | 49.502 | 54.317 | 5.163 | 0.569 | 53.484 | 54.936 | 52.947 | 63.208 | 74.289 |
| quic | unary&#45;small | portable | 5000 | 54.111 | 59.419 | 5.268 | 0.709 | 58.612 | 60.279 | 58.021 | 69.052 | 79.955 |
| quic | unary&#45;1k | legacy | 5000 | 51.075 | 56.018 | 5.198 | 1.070 | 54.308 | 57.112 | 54.628 | 65.508 | 74.244 |
| quic | unary&#45;1k | portable | 5000 | 55.847 | 69.538 | 18.965 | 20.374 | 59.347 | 105.953 | 59.755 | 105.595 | 118.489 |
| quic | fanout&#45;32 | legacy | 5000 | 209.354 | 225.511 | 15.989 | 3.607 | 221.798 | 231.148 | 218.964 | 257.529 | 270.121 |
| quic | fanout&#45;32 | portable | 5000 | 247.293 | 262.007 | 11.863 | 2.984 | 257.835 | 265.423 | 258.808 | 279.250 | 307.342 |
| quic | dependent&#45;read | legacy | 5000 | 107.287 | 124.060 | 24.632 | 16.879 | 114.345 | 154.132 | 114.771 | 188.123 | 204.694 |
| quic | dependent&#45;read | portable | 5000 | 141.315 | 151.121 | 10.494 | 1.223 | 149.368 | 152.642 | 147.647 | 170.218 | 182.693 |
| quic | pipelined&#45;read | legacy | 5000 | 62.457 | 74.036 | 16.840 | 13.774 | 67.629 | 98.673 | 66.906 | 114.102 | 127.197 |
| quic | pipelined&#45;read | portable | 5000 | 97.978 | 113.137 | 25.393 | 17.945 | 103.033 | 145.162 | 102.622 | 179.205 | 195.978 |
| quic | write&#45;read | legacy | 5000 | 101.030 | 118.608 | 24.610 | 19.536 | 108.364 | 153.520 | 109.342 | 178.186 | 192.848 |
| quic | write&#45;read | portable | 5000 | 110.296 | 136.432 | 36.598 | 39.764 | 117.865 | 207.558 | 117.846 | 212.855 | 222.526 |
| quic | callback | legacy | 5000 | 105.790 | 114.226 | 8.180 | 0.950 | 112.955 | 115.350 | 111.818 | 128.370 | 139.909 |
| quic | callback | portable | 5000 | 134.977 | 144.188 | 10.392 | 0.901 | 142.770 | 144.900 | 141.156 | 158.835 | 175.667 |
| quic | bulk&#45;64k | legacy | 5000 | 207.279 | 230.551 | 36.642 | 21.493 | 219.852 | 268.946 | 217.321 | 346.535 | 363.354 |
| quic | bulk&#45;64k | portable | 5000 | 210.521 | 225.464 | 13.454 | 2.433 | 222.748 | 228.094 | 220.945 | 248.442 | 273.578 |
| quic | bulk&#45;256k | legacy | 5000 | 512.010 | 542.024 | 15.009 | 2.439 | 537.976 | 544.409 | 539.994 | 565.705 | 592.249 |
| quic | bulk&#45;256k | portable | 5000 | 539.037 | 563.965 | 16.407 | 2.284 | 560.241 | 566.193 | 560.979 | 592.562 | 619.919 |
| quic | bulk&#45;1m | legacy | 5000 | 1881.389 | 2037.698 | 474.695 | 151.991 | 1938.242 | 2307.095 | 1960.096 | 2812.868 | 2886.731 |
| quic | bulk&#45;1m | portable | 5000 | 1886.600 | 2091.717 | 506.741 | 239.968 | 1964.325 | 2520.198 | 1985.382 | 2905.724 | 3031.539 |
| quic | bulk&#45;4m | legacy | 5000 | 9501.019 | 10218.363 | 686.506 | 107.632 | 10120.312 | 10379.880 | 10143.841 | 10510.816 | 14020.903 |
| quic | bulk&#45;4m | portable | 5000 | 9496.097 | 10591.766 | 1027.374 | 399.683 | 10278.353 | 11285.106 | 10410.768 | 11134.157 | 14547.340 |
| quic | deep&#45;8 | legacy | 5000 | 51.732 | 56.447 | 4.678 | 0.804 | 55.528 | 57.417 | 55.056 | 65.412 | 75.353 |
| quic | deep&#45;8 | portable | 5000 | 56.581 | 61.087 | 5.193 | 1.144 | 59.448 | 62.598 | 59.490 | 70.906 | 82.305 |
| quic | deep&#45;32 | legacy | 5000 | 59.995 | 65.127 | 5.877 | 0.848 | 64.443 | 66.592 | 63.158 | 76.673 | 87.330 |
| quic | deep&#45;32 | portable | 5000 | 63.982 | 69.415 | 5.522 | 0.982 | 68.748 | 71.016 | 67.507 | 80.760 | 90.657 |
| quic | concurrent&#45;32 | legacy | 5000 | 138.856 | 151.350 | 10.099 | 2.155 | 147.991 | 153.708 | 148.021 | 171.314 | 190.314 |
| quic | concurrent&#45;32 | portable | 5000 | 174.478 | 187.972 | 12.047 | 2.943 | 185.137 | 192.595 | 184.483 | 213.360 | 228.600 |
| quic | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 473.350 | 583.770 | 175.044 | 188.925 | 487.446 | 920.765 | 490.960 | 926.104 | 937.551 |
| quic | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 489.334 | 663.134 | 188.709 | 209.573 | 502.864 | 894.683 | 517.878 | 903.456 | 913.241 |
| quic | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 510.392 | 546.146 | 17.047 | 1.987 | 543.999 | 549.099 | 544.668 | 572.285 | 588.500 |
| quic | services&#45;4&#45;deep&#45;16 | portable | 5000 | 499.494 | 537.496 | 20.031 | 10.960 | 525.639 | 551.762 | 534.277 | 574.749 | 599.309 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 2451.357 | 2726.333 | 106.638 | 35.225 | 2674.683 | 2770.217 | 2722.060 | 2892.576 | 2980.315 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 2421.115 | 2771.430 | 126.467 | 67.786 | 2681.491 | 2868.924 | 2768.833 | 2988.592 | 3042.897 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2454.741 | 2855.539 | 181.474 | 51.295 | 2776.971 | 2919.594 | 2810.145 | 3140.397 | 3203.451 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2129.095 | 2635.258 | 205.335 | 78.255 | 2518.546 | 2716.221 | 2703.229 | 2902.278 | 2985.227 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 4912.181 | 5979.507 | 503.737 | 315.817 | 5720.081 | 6490.701 | 5914.159 | 6981.450 | 7297.299 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3836.119 | 5053.230 | 595.572 | 208.591 | 4859.335 | 5369.392 | 5043.600 | 6051.120 | 6467.530 |

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


