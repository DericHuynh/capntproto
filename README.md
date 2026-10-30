# Capntproto verification reports

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-29T15:34:51Z · run 40903006590 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40903006590) · commit `217c993f771b` · **failure**

![Cargo test results](docs/reports/cargo-history.svg)

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-29T15:34:51Z · run 40903006590 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40903006590) · commit `217c993f771b` · **failure**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-29T15:34:51Z · run 40903006590 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40903006590) · commit `217c993f771b` · **failure**

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

Latest coverage run: [2026-10-30T04:34:37Z · run 41003663019 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41003663019) · commit `08a5a53b1332` · **cancelled**

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-29T15:34:51Z · run 40903006590 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40903006590) · commit `217c993f771b` · **failure**

Measurement evidence: **passed**. Independently validated encrypted benchmark values&#46; Workflow status and diagnostic completion are separate&#59; unavailable values remain blank&#46; Operations include whole concurrent batches and every service hop&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Current TCP fixtures use TLS 1.3, including upstream Rust Cap’n Proto, C++, gRPC over HTTP/2 and WebSockets over WSS; native encrypted transports and standard QUIC are identified separately. Historical data retains its recorded fixture contract. The dedicated campaign also reports recursive data, concurrent calls and independent service chains for gRPC and binary Protobuf WebSockets in its encrypted comparison artifact. Bars compare the recorded workload, rather than universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.


## Exhaustive benchmark tables

Measured run: [2026-10-29T15:34:51Z · run 40903006590 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40903006590) · commit `217c993f771b` · **failure**

All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.

### All encrypted latency comparisons

| Transport | Case | Implementation | Samples | Minimum µs | Mean µs | Sample SD µs | Run mean SD µs | Run mean min µs | Run mean max µs | p50 µs | p95 µs | p99 µs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tls | loopback&#45;0 | cpp | 5000 | 53.184 | 72.112 | 21.813 | 20.964 | 59.134 | 109.227 | 64.810 | 111.059 | 128.451 |
| tls | loopback&#45;0 | grpc | 5000 | 62.349 | 78.210 | 19.429 | 19.847 | 67.686 | 113.635 | 70.153 | 112.673 | 128.999 |
| tls | loopback&#45;0 | legacy | 5000 | 36.605 | 45.351 | 10.990 | 10.912 | 39.732 | 64.829 | 40.239 | 63.690 | 78.282 |
| tls | loopback&#45;0 | portable | 5000 | 37.553 | 53.050 | 14.136 | 14.122 | 41.558 | 68.543 | 45.448 | 74.060 | 91.221 |
| tls | loopback&#45;0 | websocket | 5000 | 31.644 | 41.587 | 32.453 | 7.918 | 36.828 | 55.341 | 37.050 | 55.694 | 65.786 |
| tls | loopback&#45;64 | cpp | 5000 | 54.741 | 85.616 | 25.474 | 23.493 | 63.507 | 111.153 | 80.514 | 121.787 | 136.802 |
| tls | loopback&#45;64 | grpc | 5000 | 61.206 | 80.504 | 25.001 | 20.519 | 67.660 | 116.119 | 69.626 | 119.461 | 136.907 |
| tls | loopback&#45;64 | legacy | 5000 | 36.833 | 45.337 | 11.389 | 11.216 | 39.956 | 65.395 | 39.964 | 63.880 | 80.249 |
| tls | loopback&#45;64 | portable | 5000 | 35.031 | 52.410 | 14.367 | 14.157 | 41.572 | 67.962 | 43.167 | 73.373 | 88.142 |
| tls | loopback&#45;64 | websocket | 5000 | 30.462 | 38.721 | 8.093 | 6.911 | 35.006 | 51.040 | 35.358 | 53.856 | 63.082 |
| tls | loopback&#45;1024 | cpp | 5000 | 56.613 | 100.018 | 22.975 | 18.864 | 67.599 | 113.700 | 109.243 | 127.972 | 144.953 |
| tls | loopback&#45;1024 | grpc | 5000 | 66.502 | 105.454 | 27.862 | 28.893 | 73.286 | 127.429 | 118.301 | 140.626 | 162.900 |
| tls | loopback&#45;1024 | legacy | 5000 | 38.443 | 57.234 | 14.507 | 11.352 | 43.186 | 69.062 | 65.643 | 76.641 | 93.166 |
| tls | loopback&#45;1024 | portable | 5000 | 40.189 | 67.429 | 13.558 | 12.683 | 44.750 | 73.463 | 69.971 | 85.715 | 103.542 |
| tls | loopback&#45;1024 | websocket | 5000 | 29.450 | 46.447 | 11.586 | 10.538 | 34.809 | 54.294 | 48.702 | 63.274 | 74.999 |
| tls | loopback&#45;65536 | cpp | 5000 | 228.304 | 289.620 | 61.714 | 53.647 | 262.058 | 385.513 | 269.646 | 422.699 | 454.597 |
| tls | loopback&#45;65536 | grpc | 5000 | 234.866 | 291.792 | 63.028 | 54.341 | 264.159 | 388.917 | 265.772 | 434.893 | 468.617 |
| tls | loopback&#45;65536 | legacy | 5000 | 160.330 | 204.607 | 44.826 | 43.548 | 173.224 | 265.192 | 179.040 | 275.122 | 292.807 |
| tls | loopback&#45;65536 | portable | 5000 | 165.028 | 179.406 | 11.902 | 1.252 | 178.281 | 181.399 | 176.111 | 202.457 | 217.262 |
| tls | loopback&#45;65536 | websocket | 5000 | 180.948 | 213.045 | 41.048 | 43.966 | 191.843 | 291.661 | 193.678 | 297.284 | 312.064 |
| tls | unary&#45;empty | cpp | 5000 | 54.372 | 80.298 | 25.813 | 27.151 | 59.181 | 110.194 | 66.914 | 119.652 | 135.895 |
| tls | unary&#45;empty | grpc | 5000 | 63.000 | 89.602 | 25.429 | 24.596 | 67.045 | 116.190 | 76.797 | 126.354 | 141.440 |
| tls | unary&#45;empty | legacy | 5000 | 37.237 | 59.180 | 11.769 | 9.945 | 41.819 | 65.315 | 62.481 | 75.283 | 89.794 |
| tls | unary&#45;empty | portable | 5000 | 33.719 | 52.091 | 14.830 | 14.750 | 41.073 | 68.445 | 42.292 | 74.045 | 88.461 |
| tls | unary&#45;empty | websocket | 5000 | 29.610 | 46.595 | 10.612 | 9.294 | 37.213 | 55.977 | 52.590 | 60.482 | 72.734 |
| tls | unary&#45;small | cpp | 5000 | 53.703 | 68.969 | 20.449 | 9.195 | 58.530 | 83.109 | 58.064 | 108.030 | 127.072 |
| tls | unary&#45;small | grpc | 5000 | 63.009 | 83.035 | 23.025 | 21.044 | 69.949 | 119.226 | 71.885 | 123.747 | 143.149 |
| tls | unary&#45;small | legacy | 5000 | 35.274 | 51.785 | 13.544 | 12.360 | 40.226 | 65.290 | 45.760 | 70.923 | 84.944 |
| tls | unary&#45;small | portable | 5000 | 37.243 | 53.211 | 14.925 | 14.467 | 41.846 | 69.072 | 43.778 | 74.131 | 89.961 |
| tls | unary&#45;small | websocket | 5000 | 30.842 | 45.416 | 10.225 | 9.186 | 35.347 | 52.420 | 47.906 | 59.262 | 75.228 |
| tls | unary&#45;1k | cpp | 5000 | 57.183 | 94.498 | 24.840 | 20.581 | 66.690 | 115.100 | 109.882 | 126.172 | 140.764 |
| tls | unary&#45;1k | grpc | 5000 | 66.492 | 80.881 | 17.310 | 9.747 | 75.265 | 98.263 | 74.651 | 125.751 | 139.831 |
| tls | unary&#45;1k | legacy | 5000 | 38.045 | 57.501 | 14.491 | 11.118 | 43.181 | 68.152 | 65.407 | 77.506 | 91.006 |
| tls | unary&#45;1k | portable | 5000 | 38.209 | 61.004 | 15.798 | 13.793 | 43.850 | 72.620 | 69.370 | 82.236 | 95.669 |
| tls | unary&#45;1k | websocket | 5000 | 29.257 | 39.771 | 10.344 | 5.950 | 35.126 | 49.770 | 35.581 | 60.934 | 72.260 |
| tls | fanout&#45;32 | cpp | 5000 | 435.115 | 573.112 | 182.117 | 89.297 | 489.839 | 721.099 | 475.382 | 1015.592 | 1127.505 |
| tls | fanout&#45;32 | grpc | 5000 | 802.722 | 1031.997 | 202.529 | 113.006 | 922.292 | 1169.089 | 977.587 | 1354.723 | 1420.332 |
| tls | fanout&#45;32 | legacy | 5000 | 174.573 | 201.754 | 24.318 | 8.565 | 191.363 | 213.734 | 194.738 | 249.573 | 268.767 |
| tls | fanout&#45;32 | portable | 5000 | 207.099 | 275.043 | 80.981 | 45.175 | 231.863 | 330.692 | 244.595 | 445.783 | 466.716 |
| tls | fanout&#45;32 | websocket | 5000 | 235.546 | 276.371 | 43.128 | 18.927 | 263.953 | 309.114 | 263.399 | 388.047 | 436.322 |
| tls | dependent&#45;read | cpp | 5000 | 124.072 | 140.289 | 16.228 | 2.024 | 137.296 | 142.253 | 132.133 | 169.021 | 189.487 |
| tls | dependent&#45;read | grpc | 5000 | 131.194 | 149.072 | 22.478 | 8.470 | 144.395 | 164.112 | 144.969 | 183.895 | 246.180 |
| tls | dependent&#45;read | legacy | 5000 | 78.355 | 108.830 | 29.230 | 30.821 | 85.120 | 142.730 | 90.407 | 153.672 | 172.224 |
| tls | dependent&#45;read | portable | 5000 | 98.558 | 139.143 | 41.623 | 42.030 | 107.611 | 188.554 | 113.139 | 204.360 | 225.305 |
| tls | dependent&#45;read | websocket | 5000 | 58.978 | 75.302 | 16.793 | 6.680 | 69.914 | 85.735 | 70.576 | 108.479 | 132.638 |
| tls | pipelined&#45;read | cpp | 5000 | 88.204 | 127.366 | 44.864 | 44.573 | 101.643 | 206.653 | 115.654 | 209.769 | 230.697 |
| tls | pipelined&#45;read | legacy | 5000 | 48.479 | 59.773 | 14.856 | 13.953 | 52.681 | 84.710 | 53.543 | 87.872 | 103.458 |
| tls | pipelined&#45;read | portable | 5000 | 72.750 | 117.630 | 32.560 | 32.824 | 81.053 | 143.741 | 137.418 | 159.154 | 177.268 |
| tls | write&#45;read | cpp | 5000 | 110.324 | 165.651 | 50.569 | 45.258 | 123.420 | 219.871 | 139.577 | 234.781 | 257.774 |
| tls | write&#45;read | grpc | 5000 | 129.379 | 182.489 | 52.133 | 35.735 | 144.464 | 239.874 | 151.628 | 264.976 | 292.082 |
| tls | write&#45;read | legacy | 5000 | 75.560 | 114.956 | 25.801 | 24.371 | 82.643 | 133.043 | 127.240 | 146.831 | 167.870 |
| tls | write&#45;read | portable | 5000 | 77.376 | 110.498 | 30.235 | 22.294 | 84.191 | 141.728 | 94.691 | 154.005 | 173.626 |
| tls | write&#45;read | websocket | 5000 | 59.371 | 79.642 | 19.121 | 16.061 | 68.551 | 105.857 | 71.272 | 117.849 | 129.436 |
| tls | callback | cpp | 5000 | 116.207 | 205.117 | 54.836 | 51.974 | 125.910 | 241.980 | 231.453 | 261.671 | 288.654 |
| tls | callback | grpc | 5000 | 70.113 | 94.209 | 26.665 | 24.733 | 75.178 | 122.630 | 79.186 | 130.382 | 149.889 |
| tls | callback | legacy | 5000 | 80.334 | 111.209 | 29.281 | 28.466 | 86.948 | 142.488 | 96.102 | 154.251 | 175.884 |
| tls | callback | portable | 5000 | 95.095 | 165.195 | 32.530 | 21.864 | 127.067 | 179.666 | 172.568 | 202.556 | 236.133 |
| tls | callback | websocket | 5000 | 58.370 | 76.155 | 16.413 | 10.397 | 68.586 | 90.601 | 68.847 | 106.626 | 121.644 |
| tls | bulk&#45;64k | cpp | 5000 | 228.610 | 262.889 | 29.853 | 7.296 | 250.655 | 268.390 | 262.643 | 302.392 | 330.989 |
| tls | bulk&#45;64k | grpc | 5000 | 236.122 | 313.521 | 78.561 | 72.217 | 264.016 | 432.263 | 271.780 | 455.494 | 486.131 |
| tls | bulk&#45;64k | legacy | 5000 | 160.827 | 197.146 | 42.560 | 33.413 | 172.643 | 241.468 | 174.454 | 274.263 | 293.111 |
| tls | bulk&#45;64k | portable | 5000 | 164.442 | 196.557 | 40.731 | 43.104 | 174.935 | 273.623 | 177.388 | 280.518 | 297.726 |
| tls | bulk&#45;64k | websocket | 5000 | 182.646 | 210.706 | 38.489 | 34.146 | 195.163 | 271.784 | 193.289 | 295.863 | 316.866 |
| tls | bulk&#45;256k | cpp | 5000 | 945.563 | 1107.612 | 134.050 | 54.090 | 1047.367 | 1183.487 | 1076.005 | 1459.406 | 1522.935 |
| tls | bulk&#45;256k | grpc | 5000 | 588.455 | 1405.029 | 294.420 | 116.797 | 1230.240 | 1556.258 | 1446.892 | 1839.034 | 1942.349 |
| tls | bulk&#45;256k | legacy | 5000 | 468.629 | 554.513 | 90.055 | 86.784 | 506.199 | 707.001 | 510.581 | 716.935 | 757.984 |
| tls | bulk&#45;256k | portable | 5000 | 478.973 | 526.987 | 52.224 | 15.847 | 515.561 | 550.765 | 513.065 | 697.735 | 730.932 |
| tls | bulk&#45;256k | websocket | 5000 | 1142.168 | 1240.750 | 67.564 | 58.031 | 1178.286 | 1311.335 | 1208.913 | 1344.058 | 1394.723 |
| tls | bulk&#45;1m | cpp | 5000 | 3648.244 | 4110.955 | 352.098 | 95.970 | 3966.663 | 4235.297 | 4020.854 | 4521.822 | 5795.069 |
| tls | bulk&#45;1m | grpc | 5000 | 2364.745 | 4641.158 | 1029.931 | 1028.186 | 3788.617 | 6165.938 | 4263.052 | 6767.557 | 7064.333 |
| tls | bulk&#45;1m | legacy | 5000 | 1614.281 | 1722.470 | 133.019 | 41.641 | 1681.153 | 1766.028 | 1681.973 | 2137.516 | 2220.053 |
| tls | bulk&#45;1m | portable | 5000 | 1615.603 | 1713.569 | 114.188 | 29.597 | 1683.399 | 1749.639 | 1683.442 | 2060.933 | 2204.753 |
| tls | bulk&#45;1m | websocket | 5000 | 4636.429 | 5260.451 | 453.933 | 216.376 | 4955.819 | 5517.774 | 5074.372 | 5992.307 | 6221.571 |
| tls | bulk&#45;4m | cpp | 5000 | 16439.427 | 18580.685 | 1987.397 | 267.995 | 18317.122 | 19018.817 | 17933.556 | 24339.388 | 25091.475 |
| tls | bulk&#45;4m | grpc | 5000 | 22060.150 | 27821.476 | 3243.091 | 2949.268 | 24474.731 | 30664.287 | 28326.367 | 33312.022 | 33903.728 |
| tls | bulk&#45;4m | legacy | 5000 | 8023.192 | 9082.453 | 994.326 | 492.477 | 8560.228 | 9766.898 | 8621.468 | 11072.761 | 11343.440 |
| tls | bulk&#45;4m | portable | 5000 | 8115.930 | 8805.829 | 620.909 | 156.081 | 8627.104 | 9010.501 | 8620.023 | 10545.188 | 10996.136 |
| tls | bulk&#45;4m | websocket | 5000 | 29371.669 | 32273.967 | 2710.520 | 720.920 | 31268.723 | 33030.406 | 30836.316 | 37044.404 | 37330.244 |
| tls | deep&#45;8 | cpp | 5000 | 56.621 | 75.870 | 21.706 | 22.433 | 62.271 | 115.708 | 69.163 | 115.319 | 129.709 |
| tls | deep&#45;8 | grpc | 5000 | 75.090 | 95.789 | 26.131 | 27.217 | 82.171 | 144.356 | 83.643 | 146.940 | 158.364 |
| tls | deep&#45;8 | legacy | 5000 | 39.293 | 48.717 | 11.880 | 12.071 | 42.912 | 70.300 | 43.515 | 69.548 | 81.304 |
| tls | deep&#45;8 | portable | 5000 | 40.150 | 57.044 | 15.881 | 16.319 | 44.695 | 75.169 | 47.659 | 80.581 | 93.004 |
| tls | deep&#45;8 | websocket | 5000 | 34.724 | 48.180 | 10.497 | 7.267 | 44.700 | 61.168 | 45.621 | 68.087 | 82.933 |
| tls | deep&#45;32 | cpp | 5000 | 63.273 | 93.665 | 30.471 | 31.298 | 70.067 | 131.184 | 78.249 | 140.851 | 155.200 |
| tls | deep&#45;32 | grpc | 5000 | 91.837 | 104.180 | 12.205 | 2.518 | 100.733 | 106.568 | 98.382 | 128.811 | 141.011 |
| tls | deep&#45;32 | legacy | 5000 | 45.683 | 59.786 | 15.530 | 14.924 | 51.184 | 86.384 | 54.469 | 88.766 | 102.270 |
| tls | deep&#45;32 | portable | 5000 | 46.467 | 52.866 | 6.367 | 0.834 | 51.928 | 54.040 | 51.460 | 64.054 | 77.098 |
| tls | deep&#45;32 | websocket | 5000 | 52.902 | 66.290 | 9.194 | 2.814 | 63.860 | 70.786 | 65.015 | 83.658 | 94.122 |
| tls | concurrent&#45;32 | cpp | 5000 | 479.168 | 706.718 | 283.432 | 159.006 | 539.131 | 916.232 | 514.852 | 1187.540 | 1214.849 |
| tls | concurrent&#45;32 | grpc | 5000 | 599.950 | 785.527 | 174.822 | 148.500 | 675.365 | 1043.997 | 720.733 | 1058.425 | 1093.522 |
| tls | concurrent&#45;32 | legacy | 5000 | 120.889 | 176.737 | 67.099 | 44.884 | 130.784 | 233.113 | 136.259 | 291.605 | 306.545 |
| tls | concurrent&#45;32 | portable | 5000 | 147.628 | 216.694 | 80.262 | 30.958 | 180.347 | 249.847 | 181.770 | 355.059 | 376.467 |
| tls | concurrent&#45;32 | websocket | 5000 | 112.245 | 236.633 | 49.220 | 35.289 | 186.124 | 266.955 | 249.965 | 302.817 | 325.128 |
| tls | deep&#45;32&#45;concurrent&#45;32 | cpp | 5000 | 634.494 | 955.621 | 312.528 | 194.225 | 709.270 | 1171.902 | 774.414 | 1396.701 | 1438.127 |
| tls | deep&#45;32&#45;concurrent&#45;32 | grpc | 5000 | 1480.346 | 1800.106 | 386.164 | 174.632 | 1612.934 | 2079.659 | 1595.633 | 2793.167 | 2855.579 |
| tls | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 394.235 | 553.427 | 185.315 | 155.429 | 438.128 | 794.522 | 437.819 | 864.843 | 885.135 |
| tls | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 424.012 | 530.294 | 125.572 | 68.930 | 480.044 | 647.828 | 463.095 | 822.010 | 847.136 |
| tls | deep&#45;32&#45;concurrent&#45;32 | websocket | 5000 | 674.165 | 909.989 | 217.365 | 100.959 | 761.992 | 1028.742 | 783.158 | 1257.322 | 1289.367 |
| tls | services&#45;4&#45;deep&#45;16 | cpp | 5000 | 469.173 | 515.005 | 27.515 | 7.052 | 508.002 | 524.879 | 511.620 | 553.501 | 588.562 |
| tls | services&#45;4&#45;deep&#45;16 | grpc | 5000 | 607.583 | 660.605 | 23.256 | 2.705 | 657.476 | 663.391 | 658.506 | 697.621 | 731.462 |
| tls | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 335.999 | 367.563 | 18.337 | 0.935 | 366.305 | 368.465 | 365.029 | 396.132 | 420.288 |
| tls | services&#45;4&#45;deep&#45;16 | portable | 5000 | 372.428 | 406.576 | 18.536 | 2.308 | 402.964 | 409.246 | 405.136 | 435.977 | 460.837 |
| tls | services&#45;4&#45;deep&#45;16 | websocket | 5000 | 268.292 | 341.395 | 40.472 | 2.256 | 339.140 | 343.887 | 339.239 | 403.111 | 427.965 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | 5000 | 3136.795 | 4200.466 | 321.968 | 154.708 | 4025.756 | 4375.065 | 4180.661 | 4782.682 | 4933.555 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | 5000 | 5054.887 | 6352.988 | 414.080 | 40.404 | 6312.747 | 6409.623 | 6318.946 | 7033.442 | 7473.033 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1882.983 | 2349.847 | 156.551 | 58.108 | 2273.832 | 2410.102 | 2338.648 | 2581.249 | 2659.913 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1934.336 | 2429.813 | 131.595 | 54.516 | 2365.636 | 2481.892 | 2422.606 | 2616.840 | 2741.101 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | 5000 | 1778.514 | 2023.600 | 84.536 | 18.173 | 2003.137 | 2038.059 | 2014.604 | 2160.173 | 2255.249 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 2820.997 | 3598.366 | 410.663 | 98.773 | 3450.863 | 3707.586 | 3485.672 | 4284.261 | 4569.628 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 4610.808 | 6125.795 | 547.364 | 144.932 | 5970.245 | 6349.726 | 6152.178 | 6918.259 | 7167.216 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1714.799 | 2443.189 | 144.454 | 55.927 | 2382.724 | 2503.003 | 2454.838 | 2640.070 | 2716.573 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1884.508 | 2341.553 | 120.330 | 8.456 | 2331.381 | 2354.402 | 2310.453 | 2535.998 | 2595.622 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1512.519 | 1818.606 | 130.648 | 25.390 | 1775.997 | 1843.562 | 1807.741 | 2032.298 | 2135.943 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 5093.653 | 7548.414 | 836.968 | 79.792 | 7462.825 | 7654.118 | 7674.614 | 8803.711 | 9180.700 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 8618.695 | 11924.968 | 1405.050 | 280.663 | 11654.567 | 12295.597 | 12293.062 | 13730.984 | 14227.584 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 3669.288 | 4960.313 | 265.119 | 68.263 | 4882.079 | 5031.233 | 4920.082 | 5394.247 | 5660.799 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3332.563 | 4719.508 | 315.507 | 77.765 | 4601.491 | 4798.121 | 4731.368 | 5194.182 | 5402.089 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 2794.934 | 3418.077 | 267.094 | 27.652 | 3393.637 | 3463.579 | 3374.613 | 3911.431 | 4195.323 |
| quic | loopback&#45;0 | legacy | 5000 | 49.022 | 53.525 | 5.705 | 0.707 | 52.633 | 54.616 | 51.529 | 63.832 | 74.927 |
| quic | loopback&#45;0 | portable | 5000 | 52.938 | 58.893 | 7.455 | 0.974 | 57.867 | 60.471 | 56.220 | 72.041 | 87.447 |
| quic | loopback&#45;64 | legacy | 5000 | 48.951 | 62.360 | 16.849 | 15.707 | 53.768 | 90.332 | 54.008 | 89.957 | 107.976 |
| quic | loopback&#45;64 | portable | 5000 | 53.819 | 63.216 | 12.420 | 5.304 | 59.450 | 72.388 | 59.322 | 97.634 | 108.138 |
| quic | loopback&#45;1024 | legacy | 5000 | 51.267 | 56.930 | 5.112 | 0.915 | 56.285 | 58.516 | 55.961 | 66.586 | 76.728 |
| quic | loopback&#45;1024 | portable | 5000 | 56.413 | 72.769 | 19.631 | 17.845 | 63.458 | 104.557 | 63.197 | 107.772 | 122.863 |
| quic | loopback&#45;65536 | legacy | 5000 | 206.379 | 225.906 | 14.428 | 1.669 | 223.014 | 227.327 | 223.186 | 249.025 | 263.550 |
| quic | loopback&#45;65536 | portable | 5000 | 213.999 | 232.998 | 13.402 | 0.965 | 231.575 | 234.102 | 230.642 | 255.276 | 267.666 |
| quic | unary&#45;empty | legacy | 5000 | 49.033 | 54.563 | 6.336 | 0.260 | 54.163 | 54.873 | 53.411 | 65.248 | 75.179 |
| quic | unary&#45;empty | portable | 5000 | 53.868 | 59.927 | 6.730 | 1.779 | 58.775 | 62.949 | 58.851 | 70.401 | 81.448 |
| quic | unary&#45;small | legacy | 5000 | 47.951 | 75.346 | 18.341 | 19.046 | 54.249 | 90.158 | 85.638 | 99.529 | 114.693 |
| quic | unary&#45;small | portable | 5000 | 54.220 | 81.712 | 21.539 | 20.394 | 59.024 | 100.841 | 96.054 | 111.538 | 124.240 |
| quic | unary&#45;1k | legacy | 5000 | 51.136 | 64.137 | 15.994 | 16.414 | 55.622 | 93.468 | 56.390 | 92.862 | 108.310 |
| quic | unary&#45;1k | portable | 5000 | 56.172 | 62.646 | 5.614 | 1.155 | 61.166 | 64.115 | 61.492 | 73.726 | 84.551 |
| quic | fanout&#45;32 | legacy | 5000 | 208.316 | 244.937 | 53.293 | 28.764 | 226.702 | 295.834 | 227.331 | 417.220 | 449.634 |
| quic | fanout&#45;32 | portable | 5000 | 245.269 | 296.609 | 70.667 | 41.386 | 267.294 | 358.101 | 266.163 | 463.028 | 487.046 |
| quic | dependent&#45;read | legacy | 5000 | 106.229 | 143.318 | 39.102 | 26.727 | 118.421 | 178.527 | 120.785 | 207.219 | 225.876 |
| quic | dependent&#45;read | portable | 5000 | 139.207 | 185.844 | 51.795 | 47.672 | 150.583 | 259.732 | 155.999 | 272.625 | 293.168 |
| quic | pipelined&#45;read | legacy | 5000 | 62.205 | 78.050 | 21.873 | 22.866 | 66.828 | 118.932 | 66.406 | 118.353 | 134.684 |
| quic | pipelined&#45;read | portable | 5000 | 96.258 | 129.029 | 37.309 | 24.085 | 105.278 | 161.702 | 108.080 | 193.637 | 214.103 |
| quic | write&#45;read | legacy | 5000 | 100.677 | 154.361 | 37.288 | 32.788 | 107.391 | 182.060 | 174.077 | 196.560 | 218.077 |
| quic | write&#45;read | portable | 5000 | 110.936 | 161.326 | 45.828 | 34.855 | 120.259 | 208.886 | 135.189 | 223.228 | 242.998 |
| quic | callback | legacy | 5000 | 105.398 | 118.443 | 17.460 | 5.998 | 113.926 | 128.789 | 113.412 | 141.942 | 193.150 |
| quic | callback | portable | 5000 | 134.347 | 157.757 | 30.830 | 15.163 | 146.570 | 183.214 | 147.595 | 240.596 | 263.035 |
| quic | bulk&#45;64k | legacy | 5000 | 207.481 | 286.981 | 65.727 | 43.683 | 225.926 | 327.074 | 258.768 | 370.358 | 396.720 |
| quic | bulk&#45;64k | portable | 5000 | 212.452 | 252.668 | 51.563 | 49.751 | 229.757 | 341.657 | 229.239 | 365.551 | 386.748 |
| quic | bulk&#45;256k | legacy | 5000 | 519.637 | 556.683 | 32.865 | 2.980 | 554.088 | 560.641 | 549.766 | 595.266 | 705.652 |
| quic | bulk&#45;256k | portable | 5000 | 534.681 | 616.788 | 688.805 | 54.316 | 573.905 | 693.497 | 577.396 | 855.521 | 890.676 |
| quic | bulk&#45;1m | legacy | 5000 | 1873.662 | 2211.721 | 539.904 | 132.194 | 1977.165 | 2298.234 | 2016.296 | 2842.919 | 2930.689 |
| quic | bulk&#45;1m | portable | 5000 | 1877.242 | 2186.329 | 761.824 | 125.241 | 2036.664 | 2295.845 | 2006.125 | 2821.786 | 2911.689 |
| quic | bulk&#45;4m | legacy | 5000 | 9541.788 | 10510.565 | 1315.581 | 293.990 | 10256.716 | 11008.248 | 10179.091 | 13735.271 | 14211.411 |
| quic | bulk&#45;4m | portable | 5000 | 9625.920 | 10676.107 | 1527.789 | 176.557 | 10463.081 | 10850.422 | 10356.871 | 13679.075 | 14182.977 |
| quic | deep&#45;8 | legacy | 5000 | 49.867 | 65.998 | 17.522 | 17.611 | 55.908 | 97.109 | 57.813 | 96.624 | 114.000 |
| quic | deep&#45;8 | portable | 5000 | 57.410 | 93.295 | 24.541 | 18.900 | 63.154 | 109.418 | 104.984 | 122.912 | 149.010 |
| quic | deep&#45;32 | legacy | 5000 | 56.624 | 85.476 | 26.476 | 11.047 | 69.825 | 95.763 | 71.061 | 127.586 | 143.366 |
| quic | deep&#45;32 | portable | 5000 | 64.929 | 91.453 | 28.865 | 24.783 | 71.443 | 131.953 | 75.921 | 139.850 | 158.917 |
| quic | concurrent&#45;32 | legacy | 5000 | 138.027 | 190.200 | 48.592 | 27.993 | 166.774 | 236.238 | 180.714 | 313.734 | 341.168 |
| quic | concurrent&#45;32 | portable | 5000 | 169.411 | 209.174 | 32.323 | 7.884 | 198.195 | 216.982 | 211.937 | 265.589 | 284.244 |
| quic | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 437.838 | 666.844 | 188.280 | 95.209 | 520.308 | 780.081 | 622.225 | 920.909 | 956.943 |
| quic | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 480.724 | 780.674 | 149.578 | 111.553 | 625.175 | 891.519 | 870.253 | 918.966 | 971.187 |
| quic | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 500.315 | 551.426 | 28.597 | 4.409 | 544.496 | 555.794 | 547.313 | 596.263 | 653.318 |
| quic | services&#45;4&#45;deep&#45;16 | portable | 5000 | 488.800 | 535.777 | 30.981 | 7.906 | 526.192 | 547.361 | 529.679 | 583.236 | 634.324 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 2380.387 | 2753.708 | 117.922 | 21.405 | 2727.449 | 2783.794 | 2774.493 | 2892.665 | 3005.665 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 2401.371 | 2744.678 | 91.103 | 10.863 | 2729.303 | 2756.535 | 2758.676 | 2860.411 | 2939.653 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2418.092 | 2848.847 | 197.819 | 46.712 | 2770.065 | 2884.923 | 2824.905 | 3122.898 | 3188.367 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2063.668 | 2595.628 | 210.220 | 29.811 | 2573.003 | 2643.255 | 2658.398 | 2864.783 | 3014.261 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 4733.632 | 5694.272 | 402.182 | 99.519 | 5591.051 | 5839.696 | 5641.801 | 6359.401 | 6671.337 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3720.047 | 4941.102 | 599.950 | 222.703 | 4608.249 | 5139.525 | 4948.779 | 5924.086 | 6206.428 |

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


