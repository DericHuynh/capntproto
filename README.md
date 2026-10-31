# Capntproto verification reports

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-30T21:56:32Z · run 41134678334 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41134678334) · commit `6c29732965e7` · **failure**

![Cargo test results](docs/reports/cargo-history.svg)

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-30T21:56:32Z · run 41134678334 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41134678334) · commit `6c29732965e7` · **failure**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-30T21:56:32Z · run 41134678334 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41134678334) · commit `6c29732965e7` · **failure**

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

Latest coverage run: [2026-10-31T01:47:16Z · run 41156999722 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41156999722) · commit `3cdec872355f` · **failure**

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-30T21:56:32Z · run 41134678334 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41134678334) · commit `6c29732965e7` · **failure**

Measurement evidence: **passed**. Independently validated encrypted benchmark values&#46; Workflow status and diagnostic completion are separate&#59; unavailable values remain blank&#46; Operations include whole concurrent batches and every service hop&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Current TCP fixtures use TLS 1.3, including upstream Rust Cap’n Proto, C++, gRPC over HTTP/2 and WebSockets over WSS; native encrypted transports and standard QUIC are identified separately. Historical data retains its recorded fixture contract. The dedicated campaign also reports recursive data, concurrent calls and independent service chains for gRPC and binary Protobuf WebSockets in its encrypted comparison artifact. Bars compare the recorded workload, rather than universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.


## Exhaustive benchmark tables

Measured run: [2026-10-30T21:56:32Z · run 41134678334 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41134678334) · commit `6c29732965e7` · **failure**

All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.

### All encrypted latency comparisons

| Transport | Case | Implementation | Samples | Minimum µs | Mean µs | Sample SD µs | Run mean SD µs | Run mean min µs | Run mean max µs | p50 µs | p95 µs | p99 µs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tls | loopback&#45;0 | cpp | 5000 | 54.395 | 68.393 | 20.841 | 22.543 | 57.507 | 108.689 | 56.704 | 107.463 | 122.082 |
| tls | loopback&#45;0 | grpc | 5000 | 60.381 | 94.259 | 23.794 | 25.892 | 65.469 | 113.424 | 109.916 | 122.995 | 130.851 |
| tls | loopback&#45;0 | legacy | 5000 | 36.359 | 48.873 | 12.873 | 13.766 | 38.401 | 64.063 | 39.874 | 64.896 | 77.250 |
| tls | loopback&#45;0 | portable | 5000 | 37.616 | 46.141 | 11.929 | 12.585 | 40.101 | 68.640 | 39.684 | 67.679 | 79.955 |
| tls | loopback&#45;0 | websocket | 5000 | 29.930 | 42.666 | 8.729 | 7.523 | 37.429 | 54.159 | 37.298 | 56.002 | 64.284 |
| tls | loopback&#45;64 | cpp | 5000 | 54.618 | 78.438 | 25.569 | 27.984 | 57.676 | 109.108 | 59.811 | 115.748 | 125.627 |
| tls | loopback&#45;64 | grpc | 5000 | 60.754 | 75.743 | 19.474 | 21.062 | 66.173 | 113.418 | 64.939 | 112.680 | 126.643 |
| tls | loopback&#45;64 | legacy | 5000 | 36.275 | 44.174 | 10.704 | 11.013 | 38.812 | 63.858 | 38.038 | 63.052 | 74.283 |
| tls | loopback&#45;64 | portable | 5000 | 37.761 | 51.692 | 14.960 | 15.595 | 40.204 | 69.567 | 41.370 | 71.326 | 83.321 |
| tls | loopback&#45;64 | websocket | 5000 | 29.214 | 41.764 | 87.662 | 8.306 | 35.423 | 51.414 | 36.026 | 52.393 | 59.962 |
| tls | loopback&#45;1024 | cpp | 5000 | 56.513 | 60.685 | 6.772 | 1.040 | 59.583 | 62.392 | 58.561 | 70.362 | 88.166 |
| tls | loopback&#45;1024 | grpc | 5000 | 66.764 | 80.539 | 17.858 | 12.166 | 73.973 | 102.196 | 72.612 | 124.415 | 135.549 |
| tls | loopback&#45;1024 | legacy | 5000 | 38.811 | 52.353 | 13.318 | 14.065 | 41.422 | 67.965 | 43.816 | 69.512 | 82.726 |
| tls | loopback&#45;1024 | portable | 5000 | 39.625 | 55.033 | 15.443 | 16.384 | 42.626 | 73.237 | 44.878 | 75.025 | 89.000 |
| tls | loopback&#45;1024 | websocket | 5000 | 29.664 | 34.710 | 4.229 | 0.449 | 34.013 | 35.223 | 33.883 | 41.880 | 50.330 |
| tls | loopback&#45;65536 | cpp | 5000 | 234.254 | 248.329 | 9.972 | 1.042 | 247.012 | 249.461 | 245.723 | 265.341 | 282.602 |
| tls | loopback&#45;65536 | grpc | 5000 | 229.518 | 258.648 | 14.656 | 3.118 | 255.414 | 263.755 | 254.841 | 288.783 | 305.358 |
| tls | loopback&#45;65536 | legacy | 5000 | 158.928 | 166.657 | 8.934 | 0.802 | 165.798 | 167.710 | 163.593 | 179.598 | 191.130 |
| tls | loopback&#45;65536 | portable | 5000 | 165.959 | 174.080 | 7.938 | 0.857 | 173.106 | 175.304 | 170.702 | 189.407 | 201.568 |
| tls | loopback&#45;65536 | websocket | 5000 | 181.839 | 190.550 | 7.594 | 0.900 | 189.683 | 191.560 | 187.512 | 204.946 | 214.336 |
| tls | unary&#45;empty | cpp | 5000 | 54.319 | 58.299 | 4.802 | 0.500 | 57.750 | 58.899 | 56.524 | 67.511 | 79.502 |
| tls | unary&#45;empty | grpc | 5000 | 60.731 | 66.329 | 5.169 | 0.280 | 65.846 | 66.526 | 64.275 | 76.869 | 87.594 |
| tls | unary&#45;empty | legacy | 5000 | 35.851 | 38.802 | 3.543 | 0.419 | 38.336 | 39.196 | 37.538 | 45.849 | 54.148 |
| tls | unary&#45;empty | portable | 5000 | 37.226 | 40.018 | 3.914 | 0.315 | 39.544 | 40.352 | 38.631 | 47.257 | 56.989 |
| tls | unary&#45;empty | websocket | 5000 | 30.223 | 37.838 | 3.486 | 0.274 | 37.584 | 38.196 | 36.895 | 44.549 | 51.481 |
| tls | unary&#45;small | cpp | 5000 | 54.536 | 58.216 | 4.878 | 0.747 | 57.339 | 59.065 | 56.387 | 68.035 | 79.241 |
| tls | unary&#45;small | grpc | 5000 | 61.697 | 81.031 | 22.794 | 20.387 | 66.195 | 111.313 | 66.082 | 118.004 | 129.480 |
| tls | unary&#45;small | legacy | 5000 | 36.318 | 44.182 | 10.524 | 10.970 | 38.611 | 63.784 | 38.342 | 62.997 | 73.340 |
| tls | unary&#45;small | portable | 5000 | 37.776 | 45.945 | 11.764 | 12.465 | 40.188 | 68.237 | 39.340 | 67.353 | 79.174 |
| tls | unary&#45;small | websocket | 5000 | 29.958 | 38.779 | 6.802 | 6.379 | 35.564 | 50.178 | 36.155 | 52.174 | 58.118 |
| tls | unary&#45;1k | cpp | 5000 | 56.597 | 60.808 | 4.838 | 0.615 | 59.789 | 61.438 | 59.030 | 70.852 | 81.047 |
| tls | unary&#45;1k | grpc | 5000 | 67.453 | 74.615 | 5.586 | 1.216 | 73.673 | 76.549 | 72.891 | 84.671 | 97.450 |
| tls | unary&#45;1k | legacy | 5000 | 38.781 | 41.456 | 3.767 | 0.457 | 40.824 | 41.990 | 40.070 | 48.108 | 58.592 |
| tls | unary&#45;1k | portable | 5000 | 39.831 | 43.145 | 3.932 | 0.348 | 42.835 | 43.719 | 41.680 | 50.357 | 60.994 |
| tls | unary&#45;1k | websocket | 5000 | 30.153 | 34.864 | 4.131 | 0.368 | 34.474 | 35.427 | 33.980 | 41.151 | 51.228 |
| tls | fanout&#45;32 | cpp | 5000 | 433.453 | 465.652 | 41.373 | 6.275 | 458.519 | 475.555 | 459.001 | 486.650 | 682.369 |
| tls | fanout&#45;32 | grpc | 5000 | 803.700 | 852.705 | 59.398 | 8.190 | 846.425 | 866.449 | 841.192 | 914.470 | 1187.403 |
| tls | fanout&#45;32 | legacy | 5000 | 174.742 | 186.498 | 16.691 | 5.068 | 183.761 | 195.480 | 181.378 | 214.809 | 248.021 |
| tls | fanout&#45;32 | portable | 5000 | 207.407 | 218.076 | 10.851 | 0.712 | 217.277 | 219.000 | 214.241 | 241.520 | 255.990 |
| tls | fanout&#45;32 | websocket | 5000 | 236.952 | 267.324 | 30.264 | 12.683 | 258.620 | 289.773 | 260.702 | 297.225 | 415.701 |
| tls | dependent&#45;read | cpp | 5000 | 124.947 | 133.459 | 7.974 | 1.010 | 132.506 | 134.855 | 130.753 | 147.493 | 164.578 |
| tls | dependent&#45;read | grpc | 5000 | 131.658 | 144.494 | 14.490 | 4.855 | 141.122 | 153.025 | 141.197 | 162.334 | 184.148 |
| tls | dependent&#45;read | legacy | 5000 | 79.897 | 85.210 | 5.284 | 0.861 | 83.928 | 86.310 | 83.008 | 95.835 | 104.126 |
| tls | dependent&#45;read | portable | 5000 | 98.680 | 104.336 | 6.147 | 0.465 | 103.602 | 104.811 | 101.776 | 116.320 | 128.368 |
| tls | dependent&#45;read | websocket | 5000 | 58.742 | 67.106 | 5.449 | 0.519 | 66.434 | 67.717 | 66.261 | 77.273 | 84.189 |
| tls | pipelined&#45;read | cpp | 5000 | 88.548 | 116.706 | 45.284 | 49.993 | 93.041 | 206.115 | 93.069 | 208.266 | 222.926 |
| tls | pipelined&#45;read | legacy | 5000 | 48.428 | 61.257 | 17.166 | 16.461 | 51.450 | 89.702 | 50.995 | 90.449 | 103.550 |
| tls | pipelined&#45;read | portable | 5000 | 72.956 | 77.277 | 5.152 | 0.614 | 76.855 | 78.292 | 75.233 | 87.939 | 96.425 |
| tls | write&#45;read | cpp | 5000 | 110.076 | 117.365 | 7.196 | 1.012 | 116.757 | 119.141 | 114.730 | 131.051 | 145.978 |
| tls | write&#45;read | grpc | 5000 | 133.334 | 144.574 | 10.169 | 2.602 | 141.830 | 148.889 | 141.343 | 163.967 | 186.382 |
| tls | write&#45;read | legacy | 5000 | 74.002 | 78.938 | 5.314 | 1.033 | 77.996 | 80.576 | 76.971 | 89.387 | 100.593 |
| tls | write&#45;read | portable | 5000 | 76.787 | 82.652 | 5.669 | 0.941 | 81.522 | 84.052 | 80.430 | 94.466 | 105.687 |
| tls | write&#45;read | websocket | 5000 | 58.292 | 67.432 | 6.273 | 1.781 | 65.749 | 69.990 | 66.151 | 79.225 | 88.989 |
| tls | callback | cpp | 5000 | 116.358 | 142.643 | 42.956 | 41.500 | 122.245 | 216.836 | 122.393 | 242.662 | 253.814 |
| tls | callback | grpc | 5000 | 68.670 | 72.992 | 4.832 | 0.681 | 72.173 | 73.990 | 71.163 | 82.777 | 92.025 |
| tls | callback | legacy | 5000 | 79.376 | 94.380 | 21.452 | 21.048 | 84.032 | 132.008 | 83.814 | 138.371 | 153.913 |
| tls | callback | portable | 5000 | 93.642 | 117.697 | 32.840 | 34.118 | 99.591 | 178.421 | 99.689 | 184.384 | 195.070 |
| tls | callback | websocket | 5000 | 59.155 | 68.906 | 5.591 | 0.294 | 68.492 | 69.237 | 68.100 | 79.045 | 87.077 |
| tls | bulk&#45;64k | cpp | 5000 | 232.383 | 250.272 | 12.541 | 3.585 | 247.853 | 256.595 | 247.503 | 271.225 | 298.193 |
| tls | bulk&#45;64k | grpc | 5000 | 231.995 | 259.445 | 15.658 | 1.371 | 257.804 | 260.687 | 255.156 | 292.121 | 307.303 |
| tls | bulk&#45;64k | legacy | 5000 | 158.984 | 167.010 | 6.851 | 1.593 | 165.455 | 169.597 | 164.546 | 179.299 | 189.132 |
| tls | bulk&#45;64k | portable | 5000 | 164.901 | 172.635 | 7.059 | 0.950 | 171.197 | 173.618 | 169.667 | 185.496 | 197.066 |
| tls | bulk&#45;64k | websocket | 5000 | 181.793 | 192.681 | 9.802 | 1.554 | 190.473 | 194.836 | 190.118 | 206.870 | 225.024 |
| tls | bulk&#45;256k | cpp | 5000 | 952.716 | 1014.654 | 36.713 | 11.364 | 999.925 | 1028.165 | 1007.697 | 1066.740 | 1141.707 |
| tls | bulk&#45;256k | grpc | 5000 | 587.469 | 1280.685 | 231.349 | 138.541 | 1162.120 | 1513.000 | 1298.160 | 1712.263 | 1772.715 |
| tls | bulk&#45;256k | legacy | 5000 | 474.450 | 505.210 | 13.993 | 1.556 | 502.461 | 506.293 | 503.646 | 526.040 | 550.498 |
| tls | bulk&#45;256k | portable | 5000 | 483.711 | 514.762 | 15.014 | 2.934 | 511.017 | 517.224 | 513.276 | 535.425 | 556.706 |
| tls | bulk&#45;256k | websocket | 5000 | 1138.992 | 1196.967 | 50.367 | 47.841 | 1167.029 | 1278.217 | 1174.566 | 1295.243 | 1317.315 |
| tls | bulk&#45;1m | cpp | 5000 | 3680.153 | 3909.646 | 134.423 | 49.058 | 3862.423 | 3991.642 | 3885.380 | 4082.988 | 4409.881 |
| tls | bulk&#45;1m | grpc | 5000 | 2258.017 | 4603.965 | 1739.185 | 1876.645 | 2360.334 | 6628.657 | 3996.319 | 7333.791 | 7619.505 |
| tls | bulk&#45;1m | legacy | 5000 | 1637.694 | 1728.092 | 132.815 | 67.784 | 1683.684 | 1844.835 | 1684.727 | 2128.432 | 2171.189 |
| tls | bulk&#45;1m | portable | 5000 | 1631.744 | 1712.585 | 119.019 | 32.087 | 1683.921 | 1764.234 | 1682.477 | 2141.280 | 2209.851 |
| tls | bulk&#45;1m | websocket | 5000 | 4532.650 | 4789.857 | 250.736 | 146.992 | 4667.062 | 5026.637 | 4707.026 | 5502.002 | 5625.475 |
| tls | bulk&#45;4m | cpp | 5000 | 16321.199 | 17327.457 | 1003.912 | 147.233 | 17181.661 | 17550.587 | 17156.726 | 17912.141 | 24268.589 |
| tls | bulk&#45;4m | grpc | 5000 | 20822.536 | 25783.396 | 3222.241 | 3235.386 | 22066.889 | 28816.966 | 27106.929 | 31918.894 | 32736.596 |
| tls | bulk&#45;4m | legacy | 5000 | 7618.657 | 8213.976 | 445.398 | 142.030 | 8072.946 | 8370.647 | 8125.681 | 8816.121 | 10241.064 |
| tls | bulk&#45;4m | portable | 5000 | 7614.590 | 8232.389 | 393.693 | 64.331 | 8167.798 | 8303.953 | 8166.310 | 8590.531 | 10141.776 |
| tls | bulk&#45;4m | websocket | 5000 | 28220.277 | 30596.318 | 2246.833 | 815.175 | 29506.953 | 31646.163 | 29581.483 | 35549.035 | 36101.364 |
| tls | deep&#45;8 | cpp | 5000 | 56.981 | 79.945 | 25.671 | 22.140 | 61.387 | 104.479 | 63.524 | 121.145 | 131.063 |
| tls | deep&#45;8 | grpc | 5000 | 74.818 | 113.202 | 33.675 | 33.214 | 79.947 | 147.331 | 99.092 | 155.885 | 162.760 |
| tls | deep&#45;8 | legacy | 5000 | 38.867 | 53.182 | 14.778 | 15.973 | 41.173 | 71.011 | 43.126 | 72.612 | 84.380 |
| tls | deep&#45;8 | portable | 5000 | 39.997 | 55.765 | 16.222 | 17.631 | 42.525 | 75.295 | 44.360 | 77.252 | 88.789 |
| tls | deep&#45;8 | websocket | 5000 | 34.089 | 52.322 | 13.033 | 12.786 | 42.111 | 66.478 | 47.967 | 75.595 | 82.416 |
| tls | deep&#45;32 | cpp | 5000 | 63.425 | 93.310 | 30.709 | 33.556 | 67.450 | 130.455 | 73.212 | 137.941 | 145.915 |
| tls | deep&#45;32 | grpc | 5000 | 92.191 | 124.998 | 42.208 | 41.343 | 99.819 | 196.280 | 100.693 | 202.395 | 215.545 |
| tls | deep&#45;32 | legacy | 5000 | 46.586 | 57.699 | 16.773 | 18.168 | 49.252 | 90.195 | 48.421 | 90.023 | 102.396 |
| tls | deep&#45;32 | portable | 5000 | 46.798 | 54.707 | 13.938 | 10.101 | 49.734 | 72.764 | 48.676 | 91.980 | 101.797 |
| tls | deep&#45;32 | websocket | 5000 | 52.409 | 70.631 | 20.969 | 22.030 | 59.648 | 110.003 | 60.948 | 116.259 | 126.182 |
| tls | concurrent&#45;32 | cpp | 5000 | 472.169 | 504.664 | 61.949 | 8.291 | 495.779 | 516.982 | 493.829 | 519.884 | 834.061 |
| tls | concurrent&#45;32 | grpc | 5000 | 593.571 | 707.079 | 162.338 | 174.114 | 621.941 | 1018.327 | 622.106 | 1040.673 | 1053.038 |
| tls | concurrent&#45;32 | legacy | 5000 | 121.329 | 129.575 | 8.538 | 1.489 | 128.016 | 131.840 | 127.160 | 141.836 | 165.632 |
| tls | concurrent&#45;32 | portable | 5000 | 149.914 | 160.346 | 13.514 | 0.839 | 159.663 | 161.658 | 155.891 | 185.596 | 219.015 |
| tls | concurrent&#45;32 | websocket | 5000 | 116.925 | 168.146 | 19.762 | 1.984 | 165.830 | 170.429 | 166.221 | 202.564 | 221.052 |
| tls | deep&#45;32&#45;concurrent&#45;32 | cpp | 5000 | 631.309 | 806.393 | 269.255 | 292.037 | 670.208 | 1328.761 | 667.551 | 1360.622 | 1373.117 |
| tls | deep&#45;32&#45;concurrent&#45;32 | grpc | 5000 | 1467.172 | 1870.690 | 470.346 | 366.345 | 1559.212 | 2278.643 | 1569.518 | 2773.308 | 2814.615 |
| tls | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 429.316 | 549.882 | 173.629 | 170.028 | 451.040 | 845.753 | 451.390 | 871.277 | 881.207 |
| tls | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 431.982 | 531.984 | 146.899 | 159.887 | 456.552 | 817.910 | 458.608 | 835.993 | 846.958 |
| tls | deep&#45;32&#45;concurrent&#45;32 | websocket | 5000 | 671.724 | 804.569 | 172.906 | 99.726 | 729.643 | 954.073 | 730.292 | 1221.935 | 1251.629 |
| tls | services&#45;4&#45;deep&#45;16 | cpp | 5000 | 467.090 | 504.903 | 16.799 | 4.200 | 499.693 | 511.024 | 503.665 | 530.729 | 548.740 |
| tls | services&#45;4&#45;deep&#45;16 | grpc | 5000 | 589.403 | 639.437 | 25.182 | 8.740 | 628.755 | 649.424 | 637.956 | 671.827 | 710.961 |
| tls | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 342.831 | 368.380 | 19.253 | 2.255 | 365.562 | 370.895 | 365.845 | 392.075 | 406.754 |
| tls | services&#45;4&#45;deep&#45;16 | portable | 5000 | 381.040 | 409.134 | 34.519 | 3.054 | 407.203 | 414.383 | 406.675 | 435.426 | 449.957 |
| tls | services&#45;4&#45;deep&#45;16 | websocket | 5000 | 264.048 | 337.229 | 36.720 | 1.475 | 335.857 | 339.560 | 335.001 | 398.662 | 417.550 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | 5000 | 3081.407 | 3688.606 | 265.109 | 70.625 | 3602.166 | 3797.117 | 3697.592 | 4055.272 | 4330.466 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | 5000 | 4850.317 | 5844.463 | 314.296 | 109.447 | 5679.800 | 5928.910 | 5867.948 | 6317.830 | 6590.595 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1818.898 | 2070.387 | 128.941 | 43.757 | 2021.045 | 2121.209 | 2037.681 | 2296.665 | 2412.859 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1855.044 | 2139.570 | 132.648 | 66.825 | 2051.954 | 2213.058 | 2109.328 | 2370.691 | 2490.336 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | 5000 | 1604.637 | 1937.345 | 84.650 | 15.783 | 1923.664 | 1964.387 | 1941.861 | 2066.889 | 2138.182 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 2594.556 | 3436.003 | 445.580 | 119.819 | 3314.380 | 3593.239 | 3430.083 | 4079.977 | 4220.775 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 4401.289 | 5635.765 | 463.516 | 162.697 | 5506.213 | 5852.261 | 5670.814 | 6355.176 | 6639.425 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1531.459 | 2107.533 | 125.405 | 33.240 | 2082.231 | 2165.347 | 2091.016 | 2319.225 | 2430.667 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1861.714 | 2280.654 | 151.511 | 36.190 | 2247.720 | 2342.259 | 2284.082 | 2487.534 | 2532.068 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1456.842 | 1743.629 | 116.819 | 16.826 | 1726.553 | 1766.718 | 1735.463 | 1946.226 | 2038.973 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 5002.303 | 6916.973 | 938.115 | 133.446 | 6768.855 | 7104.335 | 7141.024 | 8087.233 | 8448.161 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 8351.777 | 10686.052 | 1220.189 | 265.112 | 10293.843 | 10943.608 | 10745.968 | 12500.637 | 13120.382 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 3476.365 | 4387.247 | 233.944 | 128.788 | 4269.435 | 4579.338 | 4377.344 | 4768.783 | 4866.125 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3813.889 | 4609.235 | 209.633 | 32.531 | 4569.831 | 4654.837 | 4645.346 | 4868.854 | 5088.865 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 2697.015 | 3235.520 | 235.700 | 24.231 | 3203.689 | 3271.727 | 3185.510 | 3688.027 | 3994.590 |
| quic | loopback&#45;0 | legacy | 5000 | 49.188 | 52.655 | 5.216 | 0.709 | 51.656 | 53.437 | 51.188 | 60.355 | 69.242 |
| quic | loopback&#45;0 | portable | 5000 | 52.846 | 57.350 | 5.332 | 0.760 | 56.574 | 58.221 | 55.559 | 66.302 | 75.905 |
| quic | loopback&#45;64 | legacy | 5000 | 49.196 | 52.482 | 7.155 | 0.541 | 51.763 | 53.116 | 50.904 | 60.756 | 67.954 |
| quic | loopback&#45;64 | portable | 5000 | 53.098 | 57.057 | 4.900 | 0.370 | 56.484 | 57.501 | 55.409 | 65.815 | 75.341 |
| quic | loopback&#45;1024 | legacy | 5000 | 50.938 | 55.139 | 5.266 | 0.744 | 54.026 | 55.857 | 53.532 | 63.993 | 71.492 |
| quic | loopback&#45;1024 | portable | 5000 | 55.669 | 59.280 | 4.709 | 0.360 | 58.816 | 59.647 | 57.588 | 68.036 | 76.638 |
| quic | loopback&#45;65536 | legacy | 5000 | 211.187 | 269.792 | 65.776 | 64.785 | 222.979 | 361.255 | 227.436 | 370.195 | 376.180 |
| quic | loopback&#45;65536 | portable | 5000 | 216.128 | 259.588 | 56.820 | 62.181 | 228.074 | 370.567 | 232.367 | 377.284 | 383.480 |
| quic | unary&#45;empty | legacy | 5000 | 48.849 | 52.461 | 4.360 | 0.374 | 51.892 | 52.865 | 50.757 | 61.248 | 70.014 |
| quic | unary&#45;empty | portable | 5000 | 52.798 | 56.605 | 3.878 | 0.696 | 55.975 | 57.501 | 55.212 | 65.300 | 72.006 |
| quic | unary&#45;small | legacy | 5000 | 49.002 | 52.466 | 3.826 | 0.350 | 51.977 | 52.910 | 51.087 | 60.585 | 67.448 |
| quic | unary&#45;small | portable | 5000 | 53.049 | 56.416 | 4.068 | 0.690 | 55.678 | 57.299 | 55.101 | 64.318 | 71.703 |
| quic | unary&#45;1k | legacy | 5000 | 51.125 | 58.847 | 13.093 | 10.628 | 53.919 | 77.857 | 52.921 | 90.584 | 100.066 |
| quic | unary&#45;1k | portable | 5000 | 55.370 | 67.925 | 18.540 | 20.117 | 58.447 | 103.904 | 57.905 | 103.402 | 116.948 |
| quic | fanout&#45;32 | legacy | 5000 | 209.966 | 275.594 | 86.622 | 60.238 | 221.150 | 366.435 | 224.708 | 430.690 | 439.844 |
| quic | fanout&#45;32 | portable | 5000 | 245.371 | 351.294 | 88.052 | 87.086 | 255.334 | 434.020 | 409.063 | 460.317 | 474.431 |
| quic | dependent&#45;read | legacy | 5000 | 106.976 | 130.810 | 32.605 | 35.596 | 113.607 | 194.466 | 113.884 | 198.030 | 208.742 |
| quic | dependent&#45;read | portable | 5000 | 137.492 | 187.313 | 53.336 | 52.123 | 146.049 | 251.273 | 152.671 | 267.534 | 273.859 |
| quic | pipelined&#45;read | legacy | 5000 | 62.343 | 66.498 | 4.438 | 0.454 | 66.039 | 67.252 | 64.754 | 75.558 | 85.130 |
| quic | pipelined&#45;read | portable | 5000 | 95.435 | 101.466 | 5.702 | 0.512 | 100.796 | 101.911 | 99.160 | 112.825 | 124.060 |
| quic | write&#45;read | legacy | 5000 | 99.481 | 106.518 | 6.989 | 0.674 | 105.703 | 107.017 | 104.165 | 118.522 | 128.702 |
| quic | write&#45;read | portable | 5000 | 109.440 | 142.921 | 42.326 | 39.672 | 115.148 | 203.157 | 116.276 | 215.636 | 222.235 |
| quic | callback | legacy | 5000 | 105.129 | 127.453 | 31.839 | 33.502 | 111.944 | 187.376 | 111.555 | 195.022 | 204.735 |
| quic | callback | portable | 5000 | 133.233 | 162.515 | 42.797 | 46.849 | 141.066 | 246.320 | 140.722 | 252.421 | 260.960 |
| quic | bulk&#45;64k | legacy | 5000 | 209.985 | 223.632 | 12.639 | 1.553 | 222.027 | 225.662 | 220.365 | 243.019 | 272.969 |
| quic | bulk&#45;64k | portable | 5000 | 213.174 | 225.529 | 9.492 | 1.364 | 223.942 | 227.348 | 222.810 | 242.214 | 252.229 |
| quic | bulk&#45;256k | legacy | 5000 | 525.210 | 586.534 | 784.542 | 30.861 | 552.135 | 612.420 | 552.146 | 598.677 | 831.447 |
| quic | bulk&#45;256k | portable | 5000 | 545.564 | 580.899 | 394.016 | 14.071 | 570.461 | 604.374 | 573.549 | 599.671 | 618.852 |
| quic | bulk&#45;1m | legacy | 5000 | 1902.508 | 2013.596 | 804.165 | 35.705 | 1961.683 | 2053.126 | 1971.390 | 2059.827 | 2802.188 |
| quic | bulk&#45;1m | portable | 5000 | 1902.276 | 1995.839 | 420.768 | 28.944 | 1959.806 | 2034.997 | 1963.165 | 2058.832 | 2807.168 |
| quic | bulk&#45;4m | legacy | 5000 | 8965.599 | 9747.065 | 959.566 | 212.135 | 9577.183 | 10085.174 | 9616.782 | 10140.600 | 13336.153 |
| quic | bulk&#45;4m | portable | 5000 | 9098.909 | 9939.177 | 1001.608 | 212.386 | 9769.438 | 10266.029 | 9792.971 | 10397.453 | 13448.661 |
| quic | deep&#45;8 | legacy | 5000 | 51.955 | 55.597 | 4.900 | 0.470 | 55.116 | 56.369 | 54.072 | 63.746 | 73.154 |
| quic | deep&#45;8 | portable | 5000 | 56.551 | 61.685 | 34.441 | 2.534 | 59.543 | 64.863 | 58.737 | 72.416 | 88.528 |
| quic | deep&#45;32 | legacy | 5000 | 60.270 | 75.102 | 22.658 | 24.793 | 63.364 | 119.446 | 62.965 | 119.173 | 132.161 |
| quic | deep&#45;32 | portable | 5000 | 63.871 | 91.679 | 30.337 | 32.044 | 67.711 | 130.451 | 69.300 | 138.670 | 145.421 |
| quic | concurrent&#45;32 | legacy | 5000 | 138.869 | 171.176 | 61.852 | 56.835 | 144.159 | 272.826 | 144.155 | 322.317 | 333.114 |
| quic | concurrent&#45;32 | portable | 5000 | 170.066 | 180.246 | 10.319 | 1.193 | 178.492 | 181.294 | 176.296 | 201.379 | 221.143 |
| quic | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 481.590 | 510.146 | 69.232 | 28.321 | 495.858 | 560.772 | 495.918 | 528.920 | 912.519 |
| quic | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 489.469 | 576.973 | 147.771 | 161.018 | 503.801 | 865.007 | 505.096 | 893.711 | 904.514 |
| quic | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 508.189 | 535.607 | 14.885 | 4.457 | 531.065 | 541.731 | 534.462 | 559.252 | 573.689 |
| quic | services&#45;4&#45;deep&#45;16 | portable | 5000 | 485.228 | 526.158 | 190.091 | 19.795 | 508.015 | 557.578 | 516.812 | 572.216 | 601.648 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 2195.299 | 2570.659 | 120.730 | 60.387 | 2493.800 | 2637.186 | 2555.134 | 2790.253 | 2902.858 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 2214.788 | 2592.575 | 120.646 | 73.148 | 2521.448 | 2706.816 | 2582.036 | 2792.272 | 2903.213 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2145.263 | 2576.806 | 150.621 | 61.142 | 2504.555 | 2649.534 | 2545.454 | 2855.824 | 2988.548 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1966.551 | 2489.588 | 197.850 | 75.327 | 2374.256 | 2561.852 | 2558.407 | 2751.629 | 2834.950 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 3997.501 | 5223.882 | 342.022 | 181.443 | 4981.044 | 5417.636 | 5305.442 | 5700.524 | 5956.456 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3727.071 | 4774.372 | 517.741 | 159.297 | 4646.754 | 5025.197 | 4925.535 | 5508.840 | 5751.715 |

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


