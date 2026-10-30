# Capntproto verification reports

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-30T19:38:11Z · run 41116858327 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41116858327) · commit `26375a09ff6f` · **failure**

![Cargo test results](docs/reports/cargo-history.svg)

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-30T19:38:11Z · run 41116858327 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41116858327) · commit `26375a09ff6f` · **failure**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-30T19:38:11Z · run 41116858327 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41116858327) · commit `26375a09ff6f` · **failure**

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

Latest coverage run: [2026-10-30T19:38:11Z · run 41116858098 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41116858098) · commit `26375a09ff6f` · **failure**

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-30T19:38:11Z · run 41116858327 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41116858327) · commit `26375a09ff6f` · **failure**

Measurement evidence: **passed**. Independently validated encrypted benchmark values&#46; Workflow status and diagnostic completion are separate&#59; unavailable values remain blank&#46; Operations include whole concurrent batches and every service hop&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Current TCP fixtures use TLS 1.3, including upstream Rust Cap’n Proto, C++, gRPC over HTTP/2 and WebSockets over WSS; native encrypted transports and standard QUIC are identified separately. Historical data retains its recorded fixture contract. The dedicated campaign also reports recursive data, concurrent calls and independent service chains for gRPC and binary Protobuf WebSockets in its encrypted comparison artifact. Bars compare the recorded workload, rather than universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.


## Exhaustive benchmark tables

Measured run: [2026-10-30T19:38:11Z · run 41116858327 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41116858327) · commit `26375a09ff6f` · **failure**

All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.

### All encrypted latency comparisons

| Transport | Case | Implementation | Samples | Minimum µs | Mean µs | Sample SD µs | Run mean SD µs | Run mean min µs | Run mean max µs | p50 µs | p95 µs | p99 µs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tls | loopback&#45;0 | cpp | 5000 | 54.211 | 62.567 | 13.823 | 7.829 | 57.652 | 76.474 | 57.129 | 106.296 | 113.738 |
| tls | loopback&#45;0 | grpc | 5000 | 62.488 | 69.260 | 61.356 | 2.913 | 67.115 | 74.149 | 65.434 | 78.424 | 90.919 |
| tls | loopback&#45;0 | legacy | 5000 | 36.761 | 45.404 | 11.351 | 11.483 | 39.097 | 65.886 | 39.542 | 68.892 | 81.064 |
| tls | loopback&#45;0 | portable | 5000 | 37.558 | 49.170 | 13.464 | 12.522 | 40.075 | 67.714 | 40.913 | 69.543 | 82.984 |
| tls | loopback&#45;0 | websocket | 5000 | 30.489 | 38.925 | 6.256 | 3.683 | 37.114 | 45.507 | 36.744 | 53.317 | 60.691 |
| tls | loopback&#45;64 | cpp | 5000 | 54.487 | 61.577 | 7.457 | 2.710 | 58.964 | 65.879 | 58.564 | 74.822 | 85.705 |
| tls | loopback&#45;64 | grpc | 5000 | 63.415 | 69.524 | 6.380 | 1.818 | 67.724 | 72.580 | 66.853 | 82.053 | 94.057 |
| tls | loopback&#45;64 | legacy | 5000 | 36.681 | 40.755 | 4.743 | 1.228 | 39.606 | 42.198 | 39.391 | 49.173 | 59.254 |
| tls | loopback&#45;64 | portable | 5000 | 37.450 | 41.936 | 4.768 | 1.362 | 40.779 | 44.237 | 40.263 | 50.862 | 59.451 |
| tls | loopback&#45;64 | websocket | 5000 | 26.047 | 35.383 | 3.820 | 0.390 | 34.914 | 35.807 | 34.415 | 41.562 | 50.810 |
| tls | loopback&#45;1024 | cpp | 5000 | 57.112 | 62.211 | 6.074 | 0.773 | 61.127 | 63.307 | 60.056 | 73.221 | 84.828 |
| tls | loopback&#45;1024 | grpc | 5000 | 67.607 | 85.409 | 20.675 | 19.693 | 75.587 | 120.606 | 76.496 | 128.418 | 140.960 |
| tls | loopback&#45;1024 | legacy | 5000 | 38.977 | 42.292 | 3.859 | 0.423 | 41.899 | 42.955 | 40.941 | 49.511 | 58.400 |
| tls | loopback&#45;1024 | portable | 5000 | 39.135 | 43.465 | 4.128 | 0.865 | 42.445 | 44.746 | 41.985 | 51.900 | 59.471 |
| tls | loopback&#45;1024 | websocket | 5000 | 27.410 | 39.234 | 9.282 | 8.418 | 35.053 | 54.277 | 35.405 | 59.022 | 64.166 |
| tls | loopback&#45;65536 | cpp | 5000 | 235.644 | 256.065 | 15.200 | 1.190 | 255.094 | 258.120 | 252.694 | 283.558 | 312.641 |
| tls | loopback&#45;65536 | grpc | 5000 | 230.867 | 275.208 | 48.852 | 29.328 | 258.517 | 327.401 | 259.469 | 425.806 | 462.455 |
| tls | loopback&#45;65536 | legacy | 5000 | 160.206 | 175.192 | 25.296 | 12.958 | 167.639 | 198.290 | 166.374 | 259.105 | 276.849 |
| tls | loopback&#45;65536 | portable | 5000 | 166.760 | 177.718 | 10.653 | 1.399 | 175.533 | 179.431 | 173.810 | 198.950 | 215.852 |
| tls | loopback&#45;65536 | websocket | 5000 | 178.723 | 193.094 | 18.333 | 5.788 | 189.230 | 203.205 | 187.916 | 213.463 | 293.670 |
| tls | unary&#45;empty | cpp | 5000 | 54.396 | 67.605 | 18.482 | 16.034 | 59.664 | 96.204 | 58.909 | 108.641 | 122.704 |
| tls | unary&#45;empty | grpc | 5000 | 63.001 | 69.022 | 5.582 | 0.823 | 68.316 | 69.988 | 67.074 | 80.303 | 90.532 |
| tls | unary&#45;empty | legacy | 5000 | 36.705 | 43.310 | 9.351 | 7.272 | 39.598 | 56.294 | 39.003 | 62.751 | 72.039 |
| tls | unary&#45;empty | portable | 5000 | 37.361 | 41.327 | 4.428 | 0.527 | 40.781 | 42.155 | 39.895 | 49.975 | 60.544 |
| tls | unary&#45;empty | websocket | 5000 | 29.200 | 41.100 | 8.316 | 8.072 | 37.179 | 55.532 | 37.264 | 55.865 | 64.393 |
| tls | unary&#45;small | cpp | 5000 | 54.576 | 70.379 | 21.246 | 22.721 | 59.668 | 111.018 | 59.103 | 110.510 | 124.162 |
| tls | unary&#45;small | grpc | 5000 | 64.279 | 79.107 | 19.946 | 21.330 | 68.084 | 117.075 | 68.235 | 116.816 | 130.161 |
| tls | unary&#45;small | legacy | 5000 | 36.625 | 44.805 | 10.768 | 11.037 | 39.098 | 64.530 | 39.002 | 63.682 | 75.755 |
| tls | unary&#45;small | portable | 5000 | 37.256 | 43.537 | 9.449 | 4.189 | 40.287 | 49.470 | 39.800 | 68.080 | 76.322 |
| tls | unary&#45;small | websocket | 5000 | 28.819 | 38.768 | 7.521 | 7.057 | 35.424 | 51.387 | 35.679 | 53.868 | 61.263 |
| tls | unary&#45;1k | cpp | 5000 | 56.747 | 72.316 | 21.616 | 23.210 | 60.562 | 113.804 | 60.729 | 113.758 | 126.799 |
| tls | unary&#45;1k | grpc | 5000 | 68.527 | 79.560 | 12.516 | 5.178 | 76.047 | 88.698 | 76.074 | 102.085 | 132.893 |
| tls | unary&#45;1k | legacy | 5000 | 38.613 | 47.552 | 10.594 | 10.940 | 41.801 | 67.078 | 42.248 | 66.660 | 77.201 |
| tls | unary&#45;1k | portable | 5000 | 40.277 | 49.165 | 12.345 | 13.041 | 42.970 | 72.480 | 42.434 | 72.217 | 83.998 |
| tls | unary&#45;1k | websocket | 5000 | 30.173 | 35.904 | 4.363 | 0.778 | 35.137 | 36.805 | 35.028 | 43.309 | 53.395 |
| tls | fanout&#45;32 | cpp | 5000 | 440.840 | 502.769 | 80.872 | 20.853 | 480.929 | 527.956 | 477.279 | 679.203 | 830.676 |
| tls | fanout&#45;32 | grpc | 5000 | 813.463 | 1002.892 | 221.042 | 204.722 | 865.098 | 1342.041 | 872.751 | 1364.885 | 1568.296 |
| tls | fanout&#45;32 | legacy | 5000 | 177.216 | 215.174 | 63.673 | 57.862 | 187.674 | 318.656 | 187.236 | 370.989 | 380.688 |
| tls | fanout&#45;32 | portable | 5000 | 210.101 | 232.572 | 35.755 | 15.604 | 223.636 | 260.313 | 221.319 | 277.587 | 408.346 |
| tls | fanout&#45;32 | websocket | 5000 | 245.840 | 302.303 | 55.714 | 58.663 | 274.124 | 407.188 | 279.273 | 422.440 | 448.077 |
| tls | dependent&#45;read | cpp | 5000 | 126.908 | 152.132 | 38.587 | 23.907 | 138.020 | 194.730 | 137.464 | 260.570 | 279.152 |
| tls | dependent&#45;read | grpc | 5000 | 133.535 | 147.453 | 9.959 | 4.218 | 142.796 | 153.179 | 144.787 | 166.770 | 183.302 |
| tls | dependent&#45;read | legacy | 5000 | 79.431 | 87.691 | 6.383 | 0.686 | 86.733 | 88.263 | 85.511 | 100.213 | 110.828 |
| tls | dependent&#45;read | portable | 5000 | 98.834 | 113.835 | 22.490 | 11.645 | 108.289 | 134.662 | 105.877 | 186.654 | 204.269 |
| tls | dependent&#45;read | websocket | 5000 | 59.388 | 70.859 | 9.824 | 4.035 | 68.273 | 77.909 | 68.443 | 91.037 | 111.296 |
| tls | pipelined&#45;read | cpp | 5000 | 87.826 | 97.814 | 11.867 | 5.234 | 93.540 | 106.841 | 93.216 | 125.490 | 140.923 |
| tls | pipelined&#45;read | legacy | 5000 | 48.150 | 53.591 | 5.429 | 1.096 | 52.666 | 55.099 | 51.516 | 64.026 | 74.907 |
| tls | pipelined&#45;read | portable | 5000 | 72.181 | 78.976 | 6.973 | 0.639 | 77.992 | 79.598 | 76.197 | 92.335 | 105.864 |
| tls | write&#45;read | cpp | 5000 | 110.573 | 120.870 | 9.407 | 2.119 | 118.973 | 124.128 | 117.844 | 139.690 | 155.753 |
| tls | write&#45;read | grpc | 5000 | 134.268 | 147.165 | 9.903 | 2.347 | 144.226 | 149.642 | 144.595 | 165.163 | 182.980 |
| tls | write&#45;read | legacy | 5000 | 75.131 | 82.106 | 6.142 | 1.634 | 79.720 | 83.407 | 80.105 | 93.871 | 104.907 |
| tls | write&#45;read | portable | 5000 | 76.157 | 84.441 | 6.640 | 2.200 | 81.796 | 86.973 | 82.535 | 97.540 | 109.800 |
| tls | write&#45;read | websocket | 5000 | 59.333 | 68.975 | 6.170 | 0.304 | 68.543 | 69.313 | 67.862 | 79.742 | 89.438 |
| tls | callback | cpp | 5000 | 117.436 | 128.715 | 9.682 | 1.784 | 126.363 | 131.192 | 125.490 | 147.613 | 163.178 |
| tls | callback | grpc | 5000 | 70.191 | 76.952 | 5.981 | 0.759 | 76.242 | 78.238 | 75.122 | 88.209 | 100.007 |
| tls | callback | legacy | 5000 | 80.527 | 88.405 | 6.904 | 1.878 | 86.029 | 90.547 | 86.405 | 101.746 | 114.731 |
| tls | callback | portable | 5000 | 94.134 | 102.942 | 8.112 | 1.333 | 101.273 | 104.626 | 100.194 | 118.181 | 133.242 |
| tls | callback | websocket | 5000 | 59.126 | 68.548 | 5.645 | 0.567 | 68.130 | 69.511 | 67.448 | 78.650 | 87.346 |
| tls | bulk&#45;64k | cpp | 5000 | 235.621 | 256.706 | 15.661 | 2.978 | 254.224 | 261.237 | 252.785 | 287.138 | 312.805 |
| tls | bulk&#45;64k | grpc | 5000 | 239.676 | 268.384 | 18.172 | 4.473 | 260.596 | 271.101 | 264.368 | 304.087 | 325.256 |
| tls | bulk&#45;64k | legacy | 5000 | 160.882 | 174.221 | 17.142 | 5.669 | 168.559 | 183.588 | 169.540 | 195.482 | 263.368 |
| tls | bulk&#45;64k | portable | 5000 | 166.800 | 177.509 | 9.383 | 0.989 | 176.433 | 178.702 | 174.255 | 196.129 | 208.320 |
| tls | bulk&#45;64k | websocket | 5000 | 180.592 | 196.905 | 21.430 | 7.733 | 191.851 | 210.364 | 190.472 | 228.427 | 296.181 |
| tls | bulk&#45;256k | cpp | 5000 | 961.716 | 1061.792 | 114.916 | 36.513 | 1020.988 | 1099.537 | 1022.139 | 1439.428 | 1501.972 |
| tls | bulk&#45;256k | grpc | 5000 | 585.581 | 1289.458 | 260.536 | 153.962 | 1040.606 | 1460.320 | 1375.783 | 1619.611 | 1833.684 |
| tls | bulk&#45;256k | legacy | 5000 | 473.222 | 543.961 | 75.288 | 51.264 | 501.723 | 629.204 | 510.723 | 706.887 | 734.294 |
| tls | bulk&#45;256k | portable | 5000 | 483.675 | 522.740 | 42.305 | 18.992 | 513.263 | 556.697 | 512.956 | 583.744 | 713.167 |
| tls | bulk&#45;256k | websocket | 5000 | 1135.685 | 1237.634 | 61.580 | 36.893 | 1175.994 | 1271.013 | 1262.766 | 1307.639 | 1338.481 |
| tls | bulk&#45;1m | cpp | 5000 | 3775.492 | 4162.591 | 543.008 | 320.258 | 3924.526 | 4718.156 | 3963.740 | 5744.194 | 5925.045 |
| tls | bulk&#45;1m | grpc | 5000 | 2307.696 | 4931.123 | 1321.031 | 1362.272 | 3891.778 | 6533.192 | 4160.296 | 7187.196 | 7451.216 |
| tls | bulk&#45;1m | legacy | 5000 | 1630.134 | 1740.097 | 163.506 | 101.512 | 1688.154 | 1921.164 | 1690.250 | 2153.580 | 2202.264 |
| tls | bulk&#45;1m | portable | 5000 | 1631.936 | 1725.607 | 114.642 | 28.079 | 1703.508 | 1773.090 | 1696.404 | 2133.871 | 2210.754 |
| tls | bulk&#45;1m | websocket | 5000 | 4602.738 | 5167.203 | 403.578 | 213.840 | 4990.760 | 5515.531 | 5046.266 | 6001.995 | 6224.075 |
| tls | bulk&#45;4m | cpp | 5000 | 16704.901 | 18048.706 | 1308.649 | 169.742 | 17837.067 | 18284.831 | 17797.544 | 18907.603 | 25079.684 |
| tls | bulk&#45;4m | grpc | 5000 | 21690.662 | 27270.448 | 3230.229 | 2915.657 | 23830.174 | 29493.229 | 27907.653 | 32900.235 | 33475.031 |
| tls | bulk&#45;4m | legacy | 5000 | 8105.251 | 8864.770 | 730.937 | 257.403 | 8588.997 | 9209.992 | 8608.755 | 10685.676 | 10992.751 |
| tls | bulk&#45;4m | portable | 5000 | 8068.897 | 9052.551 | 820.965 | 342.543 | 8721.330 | 9544.597 | 8673.477 | 10667.822 | 10945.268 |
| tls | bulk&#45;4m | websocket | 5000 | 28891.071 | 31280.244 | 2503.665 | 501.442 | 30805.650 | 32065.250 | 29956.257 | 35970.877 | 36364.779 |
| tls | deep&#45;8 | cpp | 5000 | 57.392 | 73.977 | 22.270 | 23.555 | 62.430 | 116.075 | 61.891 | 115.457 | 131.114 |
| tls | deep&#45;8 | grpc | 5000 | 75.719 | 95.830 | 26.059 | 26.849 | 82.668 | 143.836 | 82.948 | 147.938 | 161.164 |
| tls | deep&#45;8 | legacy | 5000 | 39.345 | 43.057 | 4.578 | 1.123 | 42.159 | 44.986 | 41.391 | 51.394 | 62.322 |
| tls | deep&#45;8 | portable | 5000 | 40.724 | 50.644 | 13.327 | 13.942 | 43.271 | 75.515 | 43.970 | 74.748 | 88.449 |
| tls | deep&#45;8 | websocket | 5000 | 34.642 | 43.596 | 5.792 | 0.624 | 42.754 | 44.321 | 42.643 | 53.548 | 61.443 |
| tls | deep&#45;32 | cpp | 5000 | 63.873 | 70.393 | 7.240 | 0.853 | 69.135 | 71.374 | 68.037 | 83.451 | 97.678 |
| tls | deep&#45;32 | grpc | 5000 | 93.662 | 104.199 | 9.485 | 1.560 | 101.761 | 105.797 | 101.162 | 122.321 | 141.611 |
| tls | deep&#45;32 | legacy | 5000 | 46.582 | 51.756 | 5.975 | 1.170 | 50.090 | 53.203 | 49.578 | 62.718 | 74.591 |
| tls | deep&#45;32 | portable | 5000 | 47.252 | 52.517 | 6.150 | 1.010 | 51.371 | 53.561 | 50.277 | 64.406 | 78.041 |
| tls | deep&#45;32 | websocket | 5000 | 52.298 | 62.940 | 7.473 | 1.605 | 60.976 | 64.590 | 61.305 | 77.707 | 88.868 |
| tls | concurrent&#45;32 | cpp | 5000 | 476.331 | 563.224 | 150.346 | 105.323 | 510.266 | 751.357 | 509.317 | 991.072 | 1047.725 |
| tls | concurrent&#45;32 | grpc | 5000 | 599.400 | 683.590 | 129.226 | 66.755 | 634.887 | 788.167 | 629.064 | 1013.589 | 1057.464 |
| tls | concurrent&#45;32 | legacy | 5000 | 122.507 | 152.992 | 49.982 | 25.202 | 131.326 | 183.994 | 131.651 | 281.082 | 298.370 |
| tls | concurrent&#45;32 | portable | 5000 | 150.375 | 165.060 | 13.932 | 3.358 | 159.778 | 168.049 | 161.248 | 190.989 | 217.289 |
| tls | concurrent&#45;32 | websocket | 5000 | 113.790 | 178.775 | 28.702 | 5.939 | 173.075 | 188.396 | 174.176 | 225.134 | 292.070 |
| tls | deep&#45;32&#45;concurrent&#45;32 | cpp | 5000 | 632.928 | 849.805 | 312.790 | 271.815 | 685.366 | 1331.256 | 681.034 | 1512.782 | 1565.728 |
| tls | deep&#45;32&#45;concurrent&#45;32 | grpc | 5000 | 1487.310 | 1915.286 | 487.704 | 341.388 | 1578.812 | 2316.839 | 1591.321 | 2805.999 | 2860.148 |
| tls | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 416.132 | 640.122 | 203.145 | 195.491 | 439.243 | 844.522 | 623.119 | 868.897 | 882.001 |
| tls | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 439.400 | 552.993 | 149.053 | 151.005 | 465.608 | 816.521 | 465.648 | 831.657 | 848.013 |
| tls | deep&#45;32&#45;concurrent&#45;32 | websocket | 5000 | 679.645 | 839.359 | 193.779 | 146.138 | 740.789 | 1073.982 | 741.617 | 1236.821 | 1266.653 |
| tls | services&#45;4&#45;deep&#45;16 | cpp | 5000 | 473.596 | 515.502 | 19.472 | 2.959 | 511.328 | 518.482 | 514.035 | 546.660 | 570.895 |
| tls | services&#45;4&#45;deep&#45;16 | grpc | 5000 | 590.070 | 648.182 | 21.827 | 9.891 | 631.399 | 655.957 | 647.577 | 680.142 | 703.859 |
| tls | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 346.489 | 373.357 | 16.361 | 2.219 | 370.401 | 376.542 | 371.421 | 398.681 | 418.217 |
| tls | services&#45;4&#45;deep&#45;16 | portable | 5000 | 382.494 | 409.764 | 16.520 | 2.673 | 406.047 | 413.432 | 407.630 | 436.967 | 453.714 |
| tls | services&#45;4&#45;deep&#45;16 | websocket | 5000 | 268.075 | 346.721 | 38.353 | 2.937 | 344.371 | 351.766 | 345.062 | 409.494 | 429.216 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | 5000 | 3170.400 | 3927.473 | 284.249 | 68.771 | 3824.315 | 3991.702 | 3925.774 | 4408.740 | 4685.317 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | 5000 | 4898.974 | 6019.618 | 337.649 | 79.428 | 5945.768 | 6115.782 | 6006.604 | 6577.031 | 6872.571 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1814.425 | 2170.701 | 150.815 | 43.330 | 2121.704 | 2235.477 | 2159.781 | 2418.474 | 2622.198 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1918.369 | 2193.190 | 124.887 | 13.992 | 2171.721 | 2209.463 | 2187.594 | 2399.242 | 2507.832 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | 5000 | 1634.729 | 1974.744 | 84.736 | 13.963 | 1956.156 | 1990.995 | 1974.610 | 2110.365 | 2185.479 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 2768.469 | 3494.768 | 415.350 | 110.844 | 3349.675 | 3629.380 | 3406.968 | 4140.017 | 4356.737 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 4569.987 | 5821.104 | 531.366 | 130.849 | 5665.404 | 5965.335 | 5828.844 | 6704.516 | 6920.363 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1450.906 | 2170.010 | 169.908 | 69.690 | 2101.021 | 2281.838 | 2150.366 | 2449.191 | 2628.523 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1850.093 | 2279.222 | 140.699 | 80.038 | 2186.228 | 2369.984 | 2278.575 | 2504.821 | 2582.650 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1462.852 | 1753.103 | 113.251 | 5.785 | 1743.785 | 1758.393 | 1744.738 | 1950.368 | 2046.801 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 5190.843 | 7647.091 | 872.164 | 245.483 | 7391.841 | 8009.696 | 7824.419 | 8872.567 | 9394.928 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 8633.049 | 11177.075 | 1446.154 | 289.248 | 10756.731 | 11451.124 | 11365.140 | 13340.264 | 13974.432 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 3671.293 | 4696.999 | 291.625 | 118.231 | 4563.632 | 4832.958 | 4710.042 | 5143.599 | 5381.555 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3376.204 | 4801.855 | 244.683 | 36.497 | 4755.096 | 4838.743 | 4815.841 | 5169.571 | 5377.832 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 2606.254 | 3342.170 | 264.571 | 12.161 | 3327.872 | 3355.850 | 3292.647 | 3843.874 | 4187.545 |
| quic | loopback&#45;0 | legacy | 5000 | 48.909 | 53.547 | 7.393 | 0.602 | 52.793 | 54.467 | 51.915 | 61.874 | 73.960 |
| quic | loopback&#45;0 | portable | 5000 | 53.022 | 58.586 | 7.294 | 0.482 | 58.012 | 59.293 | 56.456 | 69.647 | 97.607 |
| quic | loopback&#45;64 | legacy | 5000 | 47.087 | 60.365 | 15.447 | 16.080 | 52.136 | 89.104 | 52.233 | 88.719 | 100.260 |
| quic | loopback&#45;64 | portable | 5000 | 53.240 | 63.249 | 14.383 | 7.572 | 58.439 | 76.666 | 57.575 | 98.095 | 111.220 |
| quic | loopback&#45;1024 | legacy | 5000 | 51.537 | 58.118 | 9.908 | 4.284 | 55.618 | 65.751 | 54.723 | 90.817 | 96.918 |
| quic | loopback&#45;1024 | portable | 5000 | 55.658 | 65.498 | 15.031 | 10.557 | 59.482 | 84.314 | 59.108 | 103.425 | 115.687 |
| quic | loopback&#45;65536 | legacy | 5000 | 206.817 | 236.249 | 40.465 | 29.313 | 220.034 | 288.494 | 222.280 | 351.290 | 363.890 |
| quic | loopback&#45;65536 | portable | 5000 | 214.018 | 233.847 | 25.603 | 9.028 | 227.517 | 249.545 | 227.297 | 258.973 | 363.925 |
| quic | unary&#45;empty | legacy | 5000 | 44.710 | 54.943 | 6.082 | 1.311 | 53.667 | 56.591 | 53.291 | 65.266 | 78.036 |
| quic | unary&#45;empty | portable | 5000 | 53.396 | 59.319 | 5.465 | 0.872 | 58.406 | 60.370 | 57.661 | 69.092 | 78.747 |
| quic | unary&#45;small | legacy | 5000 | 48.552 | 53.286 | 5.350 | 0.838 | 51.830 | 53.881 | 51.807 | 62.210 | 72.035 |
| quic | unary&#45;small | portable | 5000 | 53.376 | 58.213 | 5.210 | 0.552 | 57.598 | 58.682 | 56.586 | 67.201 | 78.504 |
| quic | unary&#45;1k | legacy | 5000 | 51.421 | 67.937 | 16.864 | 14.553 | 58.932 | 92.774 | 58.795 | 94.545 | 108.099 |
| quic | unary&#45;1k | portable | 5000 | 56.126 | 78.456 | 21.573 | 23.247 | 61.150 | 104.135 | 64.127 | 111.552 | 120.264 |
| quic | fanout&#45;32 | legacy | 5000 | 209.384 | 231.224 | 18.707 | 6.329 | 225.812 | 242.142 | 225.967 | 267.913 | 300.594 |
| quic | fanout&#45;32 | portable | 5000 | 245.754 | 264.828 | 20.004 | 2.570 | 261.021 | 267.762 | 260.090 | 296.511 | 351.927 |
| quic | dependent&#45;read | legacy | 5000 | 107.211 | 115.805 | 8.284 | 1.070 | 114.639 | 117.505 | 113.367 | 129.679 | 144.463 |
| quic | dependent&#45;read | portable | 5000 | 139.544 | 150.629 | 10.130 | 1.133 | 149.563 | 152.555 | 146.873 | 170.398 | 184.163 |
| quic | pipelined&#45;read | legacy | 5000 | 62.795 | 78.810 | 20.291 | 21.344 | 66.926 | 116.844 | 69.210 | 116.982 | 130.923 |
| quic | pipelined&#45;read | portable | 5000 | 95.262 | 125.784 | 35.496 | 32.239 | 103.520 | 182.144 | 106.654 | 189.907 | 198.521 |
| quic | write&#45;read | legacy | 5000 | 100.439 | 110.642 | 8.717 | 1.119 | 108.782 | 111.767 | 108.146 | 125.315 | 138.340 |
| quic | write&#45;read | portable | 5000 | 109.644 | 126.293 | 327.984 | 10.026 | 120.686 | 144.172 | 118.815 | 139.083 | 155.500 |
| quic | callback | legacy | 5000 | 106.180 | 118.251 | 10.227 | 1.590 | 116.000 | 120.375 | 115.879 | 135.232 | 150.494 |
| quic | callback | portable | 5000 | 133.423 | 161.632 | 35.663 | 30.484 | 146.273 | 216.088 | 146.870 | 247.028 | 257.929 |
| quic | bulk&#45;64k | legacy | 5000 | 208.107 | 240.438 | 41.674 | 30.405 | 224.460 | 294.585 | 225.438 | 353.548 | 365.857 |
| quic | bulk&#45;64k | portable | 5000 | 212.044 | 246.030 | 41.920 | 30.892 | 227.378 | 301.028 | 230.935 | 356.066 | 369.393 |
| quic | bulk&#45;256k | legacy | 5000 | 512.393 | 567.283 | 558.952 | 21.945 | 553.414 | 606.171 | 551.339 | 588.513 | 653.392 |
| quic | bulk&#45;256k | portable | 5000 | 539.068 | 602.351 | 869.780 | 30.907 | 572.422 | 651.719 | 570.995 | 606.699 | 653.805 |
| quic | bulk&#45;1m | legacy | 5000 | 1926.196 | 2096.862 | 225.264 | 70.037 | 2053.550 | 2220.661 | 2031.034 | 2830.627 | 2950.936 |
| quic | bulk&#45;1m | portable | 5000 | 1913.281 | 2169.584 | 636.195 | 138.896 | 2045.826 | 2379.450 | 2051.386 | 2921.332 | 3269.525 |
| quic | bulk&#45;4m | legacy | 5000 | 9407.879 | 10364.567 | 1171.557 | 157.762 | 10170.771 | 10549.440 | 10144.627 | 12184.006 | 14294.407 |
| quic | bulk&#45;4m | portable | 5000 | 9582.477 | 10874.603 | 1554.316 | 576.382 | 10546.756 | 11901.658 | 10370.481 | 14361.548 | 15128.094 |
| quic | deep&#45;8 | legacy | 5000 | 52.485 | 59.872 | 11.235 | 5.476 | 56.918 | 69.598 | 55.801 | 94.086 | 103.672 |
| quic | deep&#45;8 | portable | 5000 | 56.844 | 70.282 | 18.055 | 17.779 | 61.440 | 102.068 | 61.338 | 106.569 | 120.873 |
| quic | deep&#45;32 | legacy | 5000 | 60.544 | 76.736 | 21.502 | 23.156 | 65.123 | 118.074 | 65.607 | 118.210 | 131.224 |
| quic | deep&#45;32 | portable | 5000 | 64.739 | 83.810 | 23.108 | 24.891 | 69.695 | 128.117 | 73.714 | 130.403 | 141.938 |
| quic | concurrent&#45;32 | legacy | 5000 | 139.429 | 158.218 | 38.353 | 19.799 | 146.154 | 193.190 | 147.297 | 299.380 | 326.837 |
| quic | concurrent&#45;32 | portable | 5000 | 171.482 | 227.181 | 86.012 | 90.106 | 182.637 | 388.129 | 182.986 | 397.946 | 410.189 |
| quic | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 470.739 | 745.422 | 200.458 | 75.716 | 659.609 | 850.590 | 899.290 | 947.039 | 962.372 |
| quic | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 485.546 | 653.235 | 172.682 | 137.338 | 508.555 | 871.919 | 525.035 | 897.980 | 923.713 |
| quic | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 511.869 | 546.459 | 17.516 | 3.852 | 541.044 | 550.921 | 545.085 | 573.000 | 596.325 |
| quic | services&#45;4&#45;deep&#45;16 | portable | 5000 | 485.604 | 526.980 | 18.630 | 9.563 | 516.246 | 537.682 | 524.808 | 557.644 | 578.924 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 2193.960 | 2608.666 | 152.509 | 56.789 | 2540.672 | 2685.443 | 2596.209 | 2857.542 | 3034.845 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 2195.079 | 2603.176 | 126.283 | 59.780 | 2543.765 | 2686.861 | 2597.187 | 2809.655 | 2873.487 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2197.017 | 2602.463 | 164.818 | 42.560 | 2553.752 | 2654.398 | 2578.349 | 2894.679 | 3020.515 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1975.384 | 2499.214 | 228.832 | 43.260 | 2458.045 | 2564.584 | 2582.376 | 2767.394 | 2885.675 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 4275.525 | 5304.763 | 399.872 | 96.272 | 5214.763 | 5443.469 | 5213.401 | 6038.915 | 6381.704 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3707.740 | 4686.585 | 590.978 | 224.092 | 4435.231 | 4977.173 | 4649.485 | 5637.352 | 6018.203 |

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


