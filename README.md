# Capntproto verification reports

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-31T02:24:25Z · run 41159861239 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41159861239) · commit `5e29c3ceb203` · **failure**

![Cargo test results](docs/reports/cargo-history.svg)

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-31T02:24:25Z · run 41159861239 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41159861239) · commit `5e29c3ceb203` · **failure**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-31T02:24:25Z · run 41159861239 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41159861239) · commit `5e29c3ceb203` · **failure**

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

Latest coverage run: [2026-10-31T05:05:02Z · run 41171141966 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41171141966) · commit `256241c2801c` · **failure**

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-31T02:24:25Z · run 41159861239 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41159861239) · commit `5e29c3ceb203` · **failure**

Measurement evidence: **passed**. Independently validated encrypted benchmark values&#46; Workflow status and diagnostic completion are separate&#59; unavailable values remain blank&#46; Operations include whole concurrent batches and every service hop&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Current TCP fixtures use TLS 1.3, including upstream Rust Cap’n Proto, C++, gRPC over HTTP/2 and WebSockets over WSS; native encrypted transports and standard QUIC are identified separately. Historical data retains its recorded fixture contract. The dedicated campaign also reports recursive data, concurrent calls and independent service chains for gRPC and binary Protobuf WebSockets in its encrypted comparison artifact. Bars compare the recorded workload, rather than universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.


## Exhaustive benchmark tables

Measured run: [2026-10-31T02:24:25Z · run 41159861239 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41159861239) · commit `5e29c3ceb203` · **failure**

All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.

### All encrypted latency comparisons

| Transport | Case | Implementation | Samples | Minimum µs | Mean µs | Sample SD µs | Run mean SD µs | Run mean min µs | Run mean max µs | p50 µs | p95 µs | p99 µs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tls | loopback&#45;0 | cpp | 5000 | 53.521 | 79.608 | 25.292 | 25.678 | 58.007 | 107.760 | 61.581 | 114.787 | 122.909 |
| tls | loopback&#45;0 | grpc | 5000 | 61.952 | 78.874 | 21.785 | 20.258 | 65.176 | 113.671 | 65.050 | 114.261 | 127.259 |
| tls | loopback&#45;0 | legacy | 5000 | 36.396 | 49.238 | 12.765 | 13.381 | 38.355 | 63.910 | 40.513 | 64.792 | 77.241 |
| tls | loopback&#45;0 | portable | 5000 | 37.273 | 52.139 | 28.276 | 14.973 | 39.582 | 68.775 | 41.058 | 70.385 | 83.587 |
| tls | loopback&#45;0 | websocket | 5000 | 28.595 | 43.987 | 12.238 | 9.854 | 36.553 | 55.331 | 37.635 | 57.049 | 65.313 |
| tls | loopback&#45;64 | cpp | 5000 | 53.857 | 63.131 | 15.384 | 9.663 | 57.347 | 80.341 | 57.277 | 106.262 | 117.141 |
| tls | loopback&#45;64 | grpc | 5000 | 62.582 | 67.554 | 6.112 | 0.542 | 66.797 | 68.122 | 65.658 | 77.579 | 89.999 |
| tls | loopback&#45;64 | legacy | 5000 | 36.586 | 44.333 | 10.848 | 11.331 | 38.882 | 64.587 | 38.476 | 63.847 | 75.101 |
| tls | loopback&#45;64 | portable | 5000 | 37.567 | 50.681 | 14.496 | 14.010 | 39.980 | 68.957 | 41.197 | 69.973 | 82.810 |
| tls | loopback&#45;64 | websocket | 5000 | 30.530 | 35.316 | 4.656 | 0.124 | 35.109 | 35.424 | 34.403 | 40.978 | 49.661 |
| tls | loopback&#45;1024 | cpp | 5000 | 56.067 | 61.345 | 6.035 | 1.300 | 59.345 | 62.799 | 59.497 | 71.961 | 84.843 |
| tls | loopback&#45;1024 | grpc | 5000 | 66.981 | 83.466 | 20.677 | 19.503 | 73.564 | 118.322 | 74.801 | 127.511 | 139.662 |
| tls | loopback&#45;1024 | legacy | 5000 | 38.408 | 41.689 | 4.742 | 0.285 | 41.299 | 42.031 | 40.240 | 49.173 | 58.557 |
| tls | loopback&#45;1024 | portable | 5000 | 39.888 | 43.464 | 5.416 | 0.896 | 42.503 | 44.584 | 42.034 | 51.162 | 61.073 |
| tls | loopback&#45;1024 | websocket | 5000 | 30.276 | 34.879 | 4.307 | 0.191 | 34.603 | 35.091 | 33.921 | 42.001 | 50.992 |
| tls | loopback&#45;65536 | cpp | 5000 | 234.832 | 299.746 | 75.723 | 72.057 | 250.052 | 414.357 | 252.697 | 426.047 | 441.711 |
| tls | loopback&#45;65536 | grpc | 5000 | 242.004 | 297.505 | 68.146 | 33.882 | 264.426 | 347.616 | 265.995 | 455.217 | 483.563 |
| tls | loopback&#45;65536 | legacy | 5000 | 160.291 | 187.699 | 41.339 | 45.225 | 166.994 | 268.596 | 165.860 | 275.143 | 282.490 |
| tls | loopback&#45;65536 | portable | 5000 | 169.266 | 216.088 | 51.335 | 52.498 | 176.525 | 282.424 | 182.150 | 293.727 | 300.070 |
| tls | loopback&#45;65536 | websocket | 5000 | 179.846 | 189.803 | 9.589 | 1.232 | 188.336 | 191.487 | 186.603 | 206.434 | 218.283 |
| tls | unary&#45;empty | cpp | 5000 | 53.947 | 57.899 | 5.399 | 1.185 | 56.571 | 59.623 | 56.198 | 68.201 | 80.488 |
| tls | unary&#45;empty | grpc | 5000 | 62.324 | 67.240 | 6.050 | 0.862 | 66.412 | 68.590 | 65.115 | 78.652 | 90.753 |
| tls | unary&#45;empty | legacy | 5000 | 36.192 | 39.619 | 3.973 | 0.794 | 38.487 | 40.344 | 38.381 | 47.233 | 57.643 |
| tls | unary&#45;empty | portable | 5000 | 37.616 | 40.774 | 4.112 | 0.477 | 40.303 | 41.457 | 39.369 | 48.534 | 57.524 |
| tls | unary&#45;empty | websocket | 5000 | 29.677 | 37.036 | 4.205 | 0.385 | 36.656 | 37.484 | 36.234 | 42.438 | 53.506 |
| tls | unary&#45;small | cpp | 5000 | 53.878 | 58.509 | 5.102 | 1.035 | 57.266 | 59.640 | 56.797 | 68.603 | 78.853 |
| tls | unary&#45;small | grpc | 5000 | 63.399 | 67.942 | 4.967 | 0.445 | 67.274 | 68.497 | 66.153 | 77.569 | 89.005 |
| tls | unary&#45;small | legacy | 5000 | 32.202 | 39.010 | 3.706 | 0.744 | 37.774 | 39.770 | 37.731 | 45.786 | 55.522 |
| tls | unary&#45;small | portable | 5000 | 37.400 | 40.414 | 4.080 | 0.303 | 40.162 | 40.929 | 39.107 | 47.481 | 56.108 |
| tls | unary&#45;small | websocket | 5000 | 29.206 | 35.943 | 4.263 | 0.374 | 35.490 | 36.275 | 35.242 | 42.803 | 52.387 |
| tls | unary&#45;1k | cpp | 5000 | 56.177 | 60.796 | 5.655 | 0.535 | 60.324 | 61.712 | 58.674 | 70.884 | 84.890 |
| tls | unary&#45;1k | grpc | 5000 | 67.288 | 75.351 | 6.152 | 0.549 | 74.650 | 75.959 | 73.614 | 86.727 | 99.914 |
| tls | unary&#45;1k | legacy | 5000 | 38.616 | 41.997 | 4.012 | 0.702 | 41.232 | 43.124 | 40.571 | 49.011 | 59.148 |
| tls | unary&#45;1k | portable | 5000 | 39.915 | 43.137 | 3.975 | 0.459 | 42.376 | 43.550 | 41.705 | 50.537 | 59.829 |
| tls | unary&#45;1k | websocket | 5000 | 29.925 | 34.880 | 3.825 | 0.241 | 34.641 | 35.220 | 34.079 | 41.398 | 49.820 |
| tls | fanout&#45;32 | cpp | 5000 | 432.416 | 619.768 | 221.275 | 228.303 | 474.510 | 1025.423 | 485.205 | 1024.970 | 1157.266 |
| tls | fanout&#45;32 | grpc | 5000 | 814.564 | 856.788 | 26.963 | 4.625 | 851.772 | 861.697 | 851.846 | 890.091 | 994.262 |
| tls | fanout&#45;32 | legacy | 5000 | 173.871 | 206.483 | 59.700 | 50.403 | 182.819 | 296.609 | 181.806 | 363.825 | 371.510 |
| tls | fanout&#45;32 | portable | 5000 | 212.392 | 255.148 | 67.538 | 66.898 | 224.866 | 374.818 | 223.651 | 411.424 | 435.491 |
| tls | fanout&#45;32 | websocket | 5000 | 243.156 | 278.273 | 38.645 | 23.569 | 265.233 | 320.251 | 266.812 | 387.520 | 428.299 |
| tls | dependent&#45;read | cpp | 5000 | 124.168 | 147.602 | 41.528 | 31.671 | 131.141 | 204.160 | 131.789 | 259.956 | 276.084 |
| tls | dependent&#45;read | grpc | 5000 | 132.235 | 143.540 | 7.712 | 1.780 | 141.135 | 145.517 | 141.425 | 157.827 | 169.928 |
| tls | dependent&#45;read | legacy | 5000 | 79.121 | 90.548 | 19.028 | 11.204 | 83.986 | 110.343 | 83.962 | 139.878 | 152.665 |
| tls | dependent&#45;read | portable | 5000 | 98.643 | 108.190 | 15.849 | 5.636 | 104.243 | 117.912 | 103.678 | 123.269 | 193.237 |
| tls | dependent&#45;read | websocket | 5000 | 58.665 | 71.020 | 13.462 | 9.805 | 66.100 | 88.548 | 66.749 | 104.124 | 120.698 |
| tls | pipelined&#45;read | cpp | 5000 | 86.980 | 99.880 | 27.061 | 14.276 | 90.648 | 125.001 | 91.671 | 200.288 | 215.161 |
| tls | pipelined&#45;read | legacy | 5000 | 48.188 | 57.733 | 14.767 | 9.837 | 50.676 | 73.879 | 50.761 | 89.292 | 102.853 |
| tls | pipelined&#45;read | portable | 5000 | 73.926 | 92.922 | 28.287 | 30.840 | 78.079 | 148.074 | 77.619 | 149.668 | 163.140 |
| tls | write&#45;read | cpp | 5000 | 108.818 | 116.606 | 10.297 | 1.302 | 115.482 | 118.300 | 113.988 | 130.801 | 143.563 |
| tls | write&#45;read | grpc | 5000 | 133.491 | 164.027 | 43.064 | 27.993 | 142.241 | 209.105 | 142.805 | 255.865 | 263.666 |
| tls | write&#45;read | legacy | 5000 | 74.135 | 89.796 | 21.316 | 22.957 | 78.640 | 130.820 | 78.725 | 131.135 | 144.312 |
| tls | write&#45;read | portable | 5000 | 76.989 | 85.650 | 14.027 | 6.463 | 81.122 | 97.052 | 81.414 | 105.211 | 145.927 |
| tls | write&#45;read | websocket | 5000 | 59.001 | 77.755 | 18.526 | 15.936 | 66.380 | 102.152 | 68.892 | 114.391 | 124.181 |
| tls | callback | cpp | 5000 | 116.042 | 123.887 | 7.629 | 1.048 | 122.522 | 124.819 | 121.301 | 138.459 | 153.987 |
| tls | callback | grpc | 5000 | 68.841 | 75.038 | 7.594 | 3.293 | 71.492 | 80.434 | 72.673 | 88.387 | 107.270 |
| tls | callback | legacy | 5000 | 78.518 | 83.934 | 13.456 | 1.823 | 81.954 | 86.100 | 81.749 | 93.919 | 104.457 |
| tls | callback | portable | 5000 | 94.447 | 100.434 | 6.508 | 1.109 | 98.586 | 101.329 | 98.272 | 113.026 | 123.503 |
| tls | callback | websocket | 5000 | 59.328 | 67.467 | 5.313 | 0.470 | 67.150 | 68.290 | 66.842 | 76.140 | 82.899 |
| tls | bulk&#45;64k | cpp | 5000 | 231.579 | 248.058 | 10.984 | 1.642 | 246.104 | 250.147 | 245.158 | 266.913 | 287.594 |
| tls | bulk&#45;64k | grpc | 5000 | 248.681 | 274.504 | 19.114 | 1.352 | 272.910 | 276.359 | 268.439 | 315.718 | 344.664 |
| tls | bulk&#45;64k | legacy | 5000 | 161.399 | 169.138 | 7.143 | 0.727 | 168.455 | 170.175 | 166.302 | 181.827 | 191.766 |
| tls | bulk&#45;64k | portable | 5000 | 169.151 | 179.185 | 17.963 | 1.717 | 177.893 | 182.168 | 174.839 | 197.030 | 209.088 |
| tls | bulk&#45;64k | websocket | 5000 | 181.255 | 192.060 | 10.089 | 1.066 | 190.444 | 192.933 | 188.697 | 206.725 | 218.489 |
| tls | bulk&#45;256k | cpp | 5000 | 952.724 | 1026.355 | 41.055 | 8.597 | 1015.507 | 1039.102 | 1014.659 | 1105.254 | 1189.310 |
| tls | bulk&#45;256k | grpc | 5000 | 602.478 | 1307.772 | 287.050 | 100.676 | 1208.697 | 1445.999 | 1235.307 | 1853.992 | 1970.704 |
| tls | bulk&#45;256k | legacy | 5000 | 476.944 | 507.486 | 17.294 | 1.523 | 505.221 | 508.965 | 505.224 | 530.120 | 562.009 |
| tls | bulk&#45;256k | portable | 5000 | 488.768 | 554.164 | 78.183 | 85.828 | 514.182 | 707.679 | 518.929 | 718.939 | 735.877 |
| tls | bulk&#45;256k | websocket | 5000 | 1150.269 | 1203.464 | 42.996 | 31.020 | 1180.555 | 1257.180 | 1189.532 | 1302.319 | 1331.965 |
| tls | bulk&#45;1m | cpp | 5000 | 3625.552 | 3937.345 | 159.172 | 54.079 | 3882.553 | 4023.243 | 3921.158 | 4084.002 | 4358.723 |
| tls | bulk&#45;1m | grpc | 5000 | 2288.387 | 4016.865 | 1237.951 | 1341.309 | 2359.814 | 6108.030 | 3916.068 | 6264.012 | 6569.477 |
| tls | bulk&#45;1m | legacy | 5000 | 1633.958 | 1724.874 | 132.330 | 42.677 | 1684.342 | 1790.492 | 1685.038 | 2144.544 | 2189.045 |
| tls | bulk&#45;1m | portable | 5000 | 1639.556 | 1738.740 | 142.493 | 66.746 | 1686.944 | 1831.998 | 1692.173 | 2161.463 | 2199.146 |
| tls | bulk&#45;1m | websocket | 5000 | 4548.837 | 4971.401 | 304.710 | 103.508 | 4884.022 | 5128.627 | 4878.277 | 5690.255 | 5903.407 |
| tls | bulk&#45;4m | cpp | 5000 | 16459.643 | 17701.426 | 1111.233 | 66.315 | 17606.621 | 17769.011 | 17550.187 | 18402.442 | 24650.901 |
| tls | bulk&#45;4m | grpc | 5000 | 21454.237 | 26546.308 | 3279.582 | 3246.361 | 22848.170 | 29291.919 | 27603.872 | 32529.421 | 33145.594 |
| tls | bulk&#45;4m | legacy | 5000 | 7954.696 | 8712.719 | 695.632 | 249.826 | 8426.414 | 9100.012 | 8491.106 | 10473.378 | 10679.717 |
| tls | bulk&#45;4m | portable | 5000 | 7999.459 | 8687.310 | 631.083 | 221.937 | 8461.948 | 8982.633 | 8523.679 | 10453.132 | 10797.734 |
| tls | bulk&#45;4m | websocket | 5000 | 28772.367 | 31112.652 | 2565.565 | 801.831 | 30198.740 | 31998.383 | 29839.370 | 36165.842 | 36407.175 |
| tls | deep&#45;8 | cpp | 5000 | 56.681 | 61.478 | 5.129 | 1.051 | 59.831 | 62.375 | 59.825 | 71.765 | 81.585 |
| tls | deep&#45;8 | grpc | 5000 | 74.630 | 81.491 | 7.480 | 1.498 | 80.109 | 83.514 | 79.213 | 93.755 | 106.400 |
| tls | deep&#45;8 | legacy | 5000 | 39.069 | 42.256 | 3.902 | 0.605 | 41.745 | 43.047 | 40.890 | 49.876 | 59.299 |
| tls | deep&#45;8 | portable | 5000 | 39.762 | 44.177 | 7.824 | 2.926 | 42.158 | 49.251 | 41.755 | 56.023 | 77.180 |
| tls | deep&#45;8 | websocket | 5000 | 34.562 | 43.391 | 5.657 | 0.297 | 43.060 | 43.773 | 42.356 | 53.344 | 60.702 |
| tls | deep&#45;32 | cpp | 5000 | 62.690 | 68.015 | 5.453 | 1.015 | 66.959 | 69.196 | 66.267 | 78.485 | 89.124 |
| tls | deep&#45;32 | grpc | 5000 | 91.857 | 101.698 | 7.229 | 2.040 | 98.412 | 103.977 | 99.941 | 114.084 | 131.402 |
| tls | deep&#45;32 | legacy | 5000 | 45.967 | 50.159 | 4.943 | 1.193 | 48.969 | 52.157 | 48.401 | 59.962 | 70.319 |
| tls | deep&#45;32 | portable | 5000 | 46.405 | 50.432 | 4.271 | 0.573 | 49.877 | 51.320 | 48.855 | 59.545 | 67.913 |
| tls | deep&#45;32 | websocket | 5000 | 52.393 | 62.433 | 7.132 | 1.348 | 60.960 | 64.636 | 61.051 | 75.392 | 86.606 |
| tls | concurrent&#45;32 | cpp | 5000 | 476.210 | 598.637 | 198.396 | 189.061 | 503.595 | 936.301 | 510.278 | 1069.033 | 1169.616 |
| tls | concurrent&#45;32 | grpc | 5000 | 599.469 | 649.745 | 89.747 | 39.574 | 628.978 | 720.322 | 622.940 | 923.485 | 1051.071 |
| tls | concurrent&#45;32 | legacy | 5000 | 122.651 | 133.143 | 9.461 | 1.224 | 131.608 | 134.250 | 130.322 | 151.134 | 170.672 |
| tls | concurrent&#45;32 | portable | 5000 | 151.664 | 166.987 | 12.209 | 5.428 | 161.066 | 175.249 | 163.581 | 195.517 | 210.314 |
| tls | concurrent&#45;32 | websocket | 5000 | 112.866 | 177.137 | 31.719 | 14.972 | 166.273 | 203.472 | 171.070 | 255.148 | 290.159 |
| tls | deep&#45;32&#45;concurrent&#45;32 | cpp | 5000 | 633.058 | 785.718 | 234.417 | 214.827 | 678.100 | 1168.707 | 671.217 | 1330.391 | 1374.782 |
| tls | deep&#45;32&#45;concurrent&#45;32 | grpc | 5000 | 1491.764 | 1629.433 | 225.410 | 97.950 | 1579.051 | 1804.386 | 1571.196 | 2051.792 | 2813.015 |
| tls | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 419.272 | 459.118 | 72.564 | 26.143 | 445.026 | 505.728 | 442.325 | 564.411 | 859.898 |
| tls | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 440.167 | 473.712 | 65.482 | 24.348 | 457.612 | 516.516 | 459.288 | 554.131 | 823.445 |
| tls | deep&#45;32&#45;concurrent&#45;32 | websocket | 5000 | 668.392 | 876.176 | 221.136 | 211.797 | 729.260 | 1207.415 | 737.936 | 1242.316 | 1276.035 |
| tls | services&#45;4&#45;deep&#45;16 | cpp | 5000 | 458.966 | 505.679 | 17.570 | 3.393 | 500.157 | 508.648 | 503.144 | 535.329 | 556.347 |
| tls | services&#45;4&#45;deep&#45;16 | grpc | 5000 | 600.160 | 644.277 | 19.789 | 8.140 | 637.199 | 657.817 | 642.370 | 676.866 | 696.825 |
| tls | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 347.070 | 372.536 | 16.733 | 4.126 | 368.094 | 377.438 | 369.045 | 402.151 | 420.017 |
| tls | services&#45;4&#45;deep&#45;16 | portable | 5000 | 388.961 | 416.423 | 19.297 | 4.147 | 412.166 | 422.597 | 413.131 | 447.998 | 468.440 |
| tls | services&#45;4&#45;deep&#45;16 | websocket | 5000 | 266.793 | 343.305 | 58.728 | 3.535 | 339.091 | 348.073 | 339.774 | 406.240 | 428.864 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | 5000 | 3103.497 | 3808.264 | 250.033 | 96.193 | 3692.860 | 3937.929 | 3800.471 | 4232.329 | 4424.723 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | 5000 | 4835.927 | 5878.004 | 320.559 | 119.832 | 5687.517 | 5990.178 | 5918.935 | 6369.329 | 6639.159 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1821.106 | 2109.586 | 130.950 | 63.184 | 2001.394 | 2156.275 | 2089.164 | 2335.543 | 2447.091 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1936.093 | 2192.466 | 124.980 | 45.638 | 2138.813 | 2243.966 | 2175.750 | 2409.144 | 2515.199 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | 5000 | 1621.219 | 1979.088 | 87.512 | 6.232 | 1973.246 | 1989.617 | 1978.845 | 2123.550 | 2205.286 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 2613.327 | 3421.337 | 430.482 | 76.194 | 3349.436 | 3540.296 | 3279.675 | 4067.213 | 4218.180 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 4553.561 | 5654.091 | 484.838 | 243.805 | 5290.251 | 5924.553 | 5701.501 | 6409.358 | 6699.705 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1477.212 | 2139.356 | 130.763 | 63.906 | 2038.998 | 2205.863 | 2125.257 | 2355.420 | 2449.117 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1864.401 | 2281.170 | 169.973 | 59.647 | 2244.990 | 2386.402 | 2276.253 | 2491.231 | 2565.317 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1435.655 | 1778.784 | 132.260 | 58.994 | 1719.640 | 1859.587 | 1769.432 | 2012.397 | 2117.479 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 5037.988 | 7096.216 | 911.895 | 174.782 | 6792.694 | 7212.630 | 7290.091 | 8284.838 | 8595.884 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 8327.508 | 10922.699 | 1199.804 | 186.205 | 10695.296 | 11103.439 | 11077.492 | 12671.268 | 13112.402 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 3290.756 | 4466.212 | 231.985 | 57.839 | 4363.990 | 4502.084 | 4452.577 | 4844.440 | 5038.212 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3819.341 | 4666.134 | 247.617 | 153.220 | 4501.129 | 4891.720 | 4712.426 | 5006.311 | 5147.251 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 2677.006 | 3313.006 | 256.162 | 32.721 | 3280.871 | 3362.144 | 3256.471 | 3801.227 | 4075.146 |
| quic | loopback&#45;0 | legacy | 5000 | 48.916 | 59.646 | 15.160 | 16.281 | 51.563 | 88.741 | 51.642 | 88.048 | 99.938 |
| quic | loopback&#45;0 | portable | 5000 | 53.392 | 65.373 | 18.109 | 19.574 | 56.453 | 100.387 | 55.696 | 100.035 | 112.428 |
| quic | loopback&#45;64 | legacy | 5000 | 48.554 | 52.327 | 3.923 | 1.461 | 51.050 | 54.825 | 51.107 | 59.912 | 68.760 |
| quic | loopback&#45;64 | portable | 5000 | 53.245 | 56.862 | 4.153 | 0.763 | 55.620 | 57.653 | 55.497 | 65.012 | 76.694 |
| quic | loopback&#45;1024 | legacy | 5000 | 51.076 | 65.623 | 17.611 | 15.243 | 55.247 | 89.641 | 55.165 | 94.123 | 106.623 |
| quic | loopback&#45;1024 | portable | 5000 | 56.245 | 60.778 | 4.507 | 0.673 | 59.671 | 61.399 | 59.392 | 69.992 | 80.536 |
| quic | loopback&#45;65536 | legacy | 5000 | 209.551 | 222.669 | 9.891 | 1.514 | 221.020 | 225.047 | 219.811 | 239.899 | 252.390 |
| quic | loopback&#45;65536 | portable | 5000 | 218.355 | 231.131 | 9.929 | 0.966 | 229.584 | 232.240 | 227.858 | 249.721 | 260.193 |
| quic | unary&#45;empty | legacy | 5000 | 48.377 | 62.381 | 17.089 | 13.483 | 51.686 | 81.458 | 51.767 | 89.466 | 102.159 |
| quic | unary&#45;empty | portable | 5000 | 53.351 | 75.137 | 21.389 | 22.883 | 57.266 | 100.482 | 60.459 | 107.185 | 113.659 |
| quic | unary&#45;small | legacy | 5000 | 48.772 | 76.615 | 17.192 | 16.111 | 53.599 | 88.608 | 85.729 | 97.435 | 103.569 |
| quic | unary&#45;small | portable | 5000 | 54.038 | 83.814 | 21.354 | 23.292 | 57.912 | 101.114 | 98.084 | 109.780 | 115.723 |
| quic | unary&#45;1k | legacy | 5000 | 51.032 | 68.615 | 18.772 | 15.475 | 54.441 | 92.373 | 55.727 | 96.438 | 107.166 |
| quic | unary&#45;1k | portable | 5000 | 56.159 | 82.918 | 23.847 | 23.296 | 59.915 | 107.894 | 70.147 | 116.318 | 124.295 |
| quic | fanout&#45;32 | legacy | 5000 | 209.797 | 222.264 | 11.872 | 0.608 | 221.622 | 223.227 | 218.197 | 246.588 | 261.211 |
| quic | fanout&#45;32 | portable | 5000 | 250.711 | 263.702 | 10.276 | 2.055 | 260.927 | 265.808 | 260.829 | 279.042 | 299.019 |
| quic | dependent&#45;read | legacy | 5000 | 106.473 | 113.131 | 6.071 | 1.964 | 111.084 | 116.334 | 111.046 | 124.669 | 136.262 |
| quic | dependent&#45;read | portable | 5000 | 139.271 | 148.923 | 8.633 | 0.959 | 147.677 | 150.029 | 145.804 | 165.770 | 179.080 |
| quic | pipelined&#45;read | legacy | 5000 | 62.107 | 67.024 | 4.844 | 1.079 | 65.772 | 68.075 | 65.524 | 76.645 | 87.343 |
| quic | pipelined&#45;read | portable | 5000 | 96.988 | 103.015 | 6.597 | 1.162 | 101.444 | 104.157 | 100.673 | 115.051 | 129.552 |
| quic | write&#45;read | legacy | 5000 | 100.030 | 139.067 | 37.290 | 38.518 | 105.357 | 181.510 | 113.857 | 191.076 | 196.152 |
| quic | write&#45;read | portable | 5000 | 109.828 | 172.403 | 45.527 | 50.037 | 115.044 | 209.341 | 203.049 | 221.822 | 227.458 |
| quic | callback | legacy | 5000 | 104.743 | 123.192 | 27.723 | 24.258 | 110.900 | 166.484 | 111.217 | 188.370 | 203.294 |
| quic | callback | portable | 5000 | 133.901 | 165.032 | 43.219 | 47.410 | 142.870 | 249.830 | 142.991 | 256.901 | 263.733 |
| quic | bulk&#45;64k | legacy | 5000 | 211.252 | 250.936 | 56.011 | 61.771 | 222.605 | 361.429 | 224.178 | 368.227 | 376.344 |
| quic | bulk&#45;64k | portable | 5000 | 213.069 | 250.420 | 54.915 | 54.392 | 224.316 | 347.696 | 225.997 | 376.936 | 384.847 |
| quic | bulk&#45;256k | legacy | 5000 | 533.729 | 631.628 | 570.177 | 97.842 | 557.493 | 760.969 | 561.442 | 859.758 | 873.493 |
| quic | bulk&#45;256k | portable | 5000 | 554.255 | 640.746 | 409.435 | 69.739 | 576.440 | 746.737 | 579.618 | 901.019 | 913.873 |
| quic | bulk&#45;1m | legacy | 5000 | 1909.364 | 2034.735 | 430.628 | 58.212 | 1993.997 | 2136.774 | 1994.753 | 2111.801 | 2872.175 |
| quic | bulk&#45;1m | portable | 5000 | 1913.711 | 2015.312 | 565.687 | 26.334 | 1986.157 | 2052.079 | 1986.585 | 2068.504 | 2762.866 |
| quic | bulk&#45;4m | legacy | 5000 | 9335.238 | 10283.148 | 1183.371 | 144.345 | 10089.931 | 10446.942 | 10015.794 | 13663.784 | 14162.157 |
| quic | bulk&#45;4m | portable | 5000 | 9533.634 | 10584.587 | 1314.332 | 412.906 | 10153.482 | 11048.159 | 10249.576 | 13857.537 | 14385.013 |
| quic | deep&#45;8 | legacy | 5000 | 52.302 | 55.144 | 4.258 | 0.541 | 54.714 | 56.060 | 53.691 | 63.196 | 73.928 |
| quic | deep&#45;8 | portable | 5000 | 56.396 | 59.221 | 4.434 | 0.315 | 58.942 | 59.720 | 57.728 | 67.774 | 79.199 |
| quic | deep&#45;32 | legacy | 5000 | 57.582 | 64.224 | 4.926 | 0.560 | 63.545 | 64.834 | 62.633 | 73.529 | 86.657 |
| quic | deep&#45;32 | portable | 5000 | 64.328 | 69.193 | 6.127 | 0.344 | 68.695 | 69.556 | 67.332 | 80.194 | 94.525 |
| quic | concurrent&#45;32 | legacy | 5000 | 137.630 | 163.425 | 52.825 | 39.392 | 144.506 | 233.873 | 144.460 | 317.343 | 334.434 |
| quic | concurrent&#45;32 | portable | 5000 | 171.797 | 186.159 | 11.724 | 4.036 | 181.745 | 190.064 | 183.347 | 208.768 | 229.691 |
| quic | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 474.805 | 524.931 | 104.201 | 58.289 | 494.248 | 628.982 | 495.171 | 906.895 | 923.276 |
| quic | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 488.911 | 529.122 | 82.943 | 27.545 | 506.712 | 566.599 | 506.777 | 716.678 | 895.763 |
| quic | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 513.532 | 545.776 | 18.614 | 1.605 | 544.324 | 548.249 | 543.606 | 573.543 | 605.013 |
| quic | services&#45;4&#45;deep&#45;16 | portable | 5000 | 497.132 | 537.044 | 18.007 | 12.788 | 519.805 | 546.873 | 535.862 | 565.896 | 579.953 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 2195.350 | 2562.099 | 112.981 | 27.705 | 2521.071 | 2597.893 | 2559.056 | 2747.907 | 2840.329 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 2221.471 | 2609.067 | 108.685 | 31.764 | 2573.937 | 2643.660 | 2612.420 | 2775.506 | 2853.986 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2185.954 | 2633.036 | 154.606 | 38.616 | 2585.319 | 2679.844 | 2615.830 | 2887.197 | 3003.748 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1929.593 | 2506.461 | 216.537 | 87.735 | 2364.458 | 2581.449 | 2596.052 | 2758.839 | 2882.459 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 4273.151 | 5314.718 | 337.611 | 125.216 | 5168.932 | 5509.629 | 5326.630 | 5875.498 | 6123.633 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3746.932 | 4620.170 | 511.288 | 320.912 | 4111.971 | 4998.403 | 4666.024 | 5331.231 | 5606.345 |

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


