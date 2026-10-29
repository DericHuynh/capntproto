# Capntproto verification reports

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-29T01:57:50Z · run 40803524321 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40803524321) · commit `49bc86c9296a` · **failure**

![Cargo test results](docs/reports/cargo-history.svg)

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-29T01:57:50Z · run 40803524321 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40803524321) · commit `49bc86c9296a` · **failure**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-29T01:57:50Z · run 40803524321 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40803524321) · commit `49bc86c9296a` · **failure**

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

Latest coverage run: [2026-10-29T01:57:50Z · run 40803524322 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40803524322) · commit `49bc86c9296a` · **failure**

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-29T01:57:50Z · run 40803524321 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40803524321) · commit `49bc86c9296a` · **failure**

Measurement evidence: **passed**. Independently validated encrypted benchmark values&#46; Workflow status and diagnostic completion are separate&#59; unavailable values remain blank&#46; Operations include whole concurrent batches and every service hop&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Current TCP fixtures use TLS 1.3, including upstream Rust Cap’n Proto, C++, gRPC over HTTP/2 and WebSockets over WSS; native encrypted transports and standard QUIC are identified separately. Historical data retains its recorded fixture contract. The dedicated campaign also reports recursive data, concurrent calls and independent service chains for gRPC and binary Protobuf WebSockets in its encrypted comparison artifact. Bars compare the recorded workload, rather than universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.


## Exhaustive benchmark tables

Measured run: [2026-10-29T01:57:50Z · run 40803524321 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40803524321) · commit `49bc86c9296a` · **failure**

All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.

### All encrypted latency comparisons

| Transport | Case | Implementation | Samples | Minimum µs | Mean µs | Sample SD µs | Run mean SD µs | Run mean min µs | Run mean max µs | p50 µs | p95 µs | p99 µs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tls | loopback&#45;0 | cpp | 5000 | 53.546 | 95.515 | 24.583 | 20.245 | 63.104 | 113.155 | 106.057 | 126.355 | 147.091 |
| tls | loopback&#45;0 | grpc | 5000 | 62.231 | 89.117 | 23.424 | 22.110 | 71.518 | 115.825 | 78.679 | 125.271 | 142.379 |
| tls | loopback&#45;0 | legacy | 5000 | 33.195 | 49.781 | 13.317 | 12.381 | 40.647 | 63.485 | 42.593 | 70.223 | 83.538 |
| tls | loopback&#45;0 | portable | 5000 | 33.779 | 57.846 | 29.020 | 13.512 | 42.252 | 73.622 | 65.601 | 81.530 | 99.235 |
| tls | loopback&#45;0 | websocket | 5000 | 29.358 | 47.247 | 58.535 | 12.344 | 37.908 | 65.516 | 40.976 | 62.057 | 77.491 |
| tls | loopback&#45;64 | cpp | 5000 | 54.334 | 94.192 | 25.399 | 24.953 | 65.125 | 113.475 | 105.902 | 128.489 | 151.482 |
| tls | loopback&#45;64 | grpc | 5000 | 62.779 | 74.131 | 9.666 | 0.768 | 73.119 | 75.142 | 72.058 | 92.090 | 107.387 |
| tls | loopback&#45;64 | legacy | 5000 | 33.595 | 42.683 | 7.169 | 1.890 | 40.749 | 45.660 | 40.486 | 59.232 | 69.437 |
| tls | loopback&#45;64 | portable | 5000 | 37.380 | 46.386 | 11.343 | 8.493 | 42.008 | 61.567 | 41.270 | 67.428 | 81.365 |
| tls | loopback&#45;64 | websocket | 5000 | 28.576 | 36.991 | 7.033 | 1.794 | 35.837 | 40.179 | 34.838 | 53.039 | 60.910 |
| tls | loopback&#45;1024 | cpp | 5000 | 56.326 | 82.230 | 21.506 | 12.206 | 71.483 | 95.987 | 76.965 | 121.376 | 140.093 |
| tls | loopback&#45;1024 | grpc | 5000 | 67.448 | 83.158 | 11.500 | 1.247 | 81.123 | 84.414 | 80.061 | 105.031 | 123.291 |
| tls | loopback&#45;1024 | legacy | 5000 | 38.338 | 48.351 | 10.762 | 6.984 | 43.966 | 60.754 | 44.359 | 68.080 | 85.354 |
| tls | loopback&#45;1024 | portable | 5000 | 36.486 | 46.500 | 7.181 | 0.548 | 46.096 | 47.405 | 44.666 | 59.306 | 73.607 |
| tls | loopback&#45;1024 | websocket | 5000 | 26.588 | 40.292 | 10.117 | 8.929 | 35.965 | 56.261 | 36.405 | 61.706 | 71.311 |
| tls | loopback&#45;65536 | cpp | 5000 | 232.743 | 311.246 | 62.378 | 38.046 | 281.909 | 369.599 | 292.535 | 433.810 | 470.714 |
| tls | loopback&#45;65536 | grpc | 5000 | 236.365 | 297.528 | 53.556 | 35.897 | 274.133 | 361.226 | 280.213 | 428.423 | 480.849 |
| tls | loopback&#45;65536 | legacy | 5000 | 159.930 | 205.516 | 42.790 | 34.990 | 178.219 | 251.803 | 184.859 | 279.855 | 304.320 |
| tls | loopback&#45;65536 | portable | 5000 | 165.186 | 226.142 | 48.561 | 51.121 | 187.613 | 284.494 | 202.210 | 299.624 | 323.619 |
| tls | loopback&#45;65536 | websocket | 5000 | 180.684 | 235.236 | 47.583 | 43.493 | 201.797 | 299.793 | 211.638 | 314.064 | 339.472 |
| tls | unary&#45;empty | cpp | 5000 | 54.242 | 86.534 | 25.185 | 21.439 | 63.723 | 111.488 | 80.287 | 125.299 | 145.546 |
| tls | unary&#45;empty | grpc | 5000 | 63.139 | 92.583 | 25.609 | 23.037 | 72.723 | 119.583 | 81.956 | 134.280 | 154.824 |
| tls | unary&#45;empty | legacy | 5000 | 36.296 | 52.233 | 13.490 | 10.071 | 41.262 | 66.235 | 46.975 | 73.919 | 91.904 |
| tls | unary&#45;empty | portable | 5000 | 35.727 | 60.004 | 15.731 | 15.061 | 43.126 | 71.943 | 66.104 | 84.878 | 102.757 |
| tls | unary&#45;empty | websocket | 5000 | 28.679 | 44.245 | 10.639 | 8.308 | 38.182 | 57.109 | 38.620 | 62.182 | 76.219 |
| tls | unary&#45;small | cpp | 5000 | 54.351 | 87.731 | 24.950 | 14.974 | 68.723 | 110.522 | 87.388 | 122.460 | 138.695 |
| tls | unary&#45;small | grpc | 5000 | 63.882 | 109.628 | 23.125 | 11.122 | 97.692 | 122.894 | 114.116 | 141.204 | 165.413 |
| tls | unary&#45;small | legacy | 5000 | 36.183 | 57.325 | 13.605 | 10.355 | 42.192 | 65.932 | 61.797 | 77.000 | 95.641 |
| tls | unary&#45;small | portable | 5000 | 37.539 | 57.966 | 15.040 | 13.203 | 43.712 | 70.005 | 65.727 | 80.411 | 94.546 |
| tls | unary&#45;small | websocket | 5000 | 28.675 | 45.868 | 9.677 | 8.363 | 35.943 | 53.069 | 47.552 | 59.868 | 71.474 |
| tls | unary&#45;1k | cpp | 5000 | 55.773 | 74.239 | 16.415 | 8.043 | 68.310 | 88.257 | 70.420 | 110.873 | 131.822 |
| tls | unary&#45;1k | grpc | 5000 | 64.417 | 96.394 | 25.247 | 21.882 | 81.919 | 131.797 | 86.003 | 139.092 | 159.432 |
| tls | unary&#45;1k | legacy | 5000 | 38.683 | 57.539 | 13.933 | 12.151 | 44.568 | 70.068 | 64.417 | 78.843 | 91.196 |
| tls | unary&#45;1k | portable | 5000 | 38.235 | 46.364 | 6.511 | 0.631 | 45.505 | 47.011 | 44.788 | 57.765 | 69.198 |
| tls | unary&#45;1k | websocket | 5000 | 28.411 | 43.688 | 10.923 | 8.311 | 36.734 | 54.534 | 39.591 | 63.043 | 73.915 |
| tls | fanout&#45;32 | cpp | 5000 | 445.357 | 726.921 | 218.255 | 88.840 | 605.330 | 844.529 | 694.884 | 1065.663 | 1121.589 |
| tls | fanout&#45;32 | grpc | 5000 | 810.193 | 1107.818 | 202.431 | 34.797 | 1075.383 | 1146.581 | 1067.658 | 1410.449 | 1520.925 |
| tls | fanout&#45;32 | legacy | 5000 | 174.870 | 230.431 | 51.835 | 25.424 | 215.299 | 275.576 | 217.740 | 365.461 | 385.227 |
| tls | fanout&#45;32 | portable | 5000 | 208.048 | 255.146 | 34.943 | 4.854 | 248.819 | 261.003 | 251.241 | 312.799 | 365.547 |
| tls | fanout&#45;32 | websocket | 5000 | 237.823 | 319.340 | 66.403 | 26.720 | 282.413 | 353.226 | 289.848 | 438.980 | 478.499 |
| tls | dependent&#45;read | cpp | 5000 | 125.659 | 192.616 | 52.313 | 31.142 | 159.252 | 230.765 | 175.761 | 281.988 | 307.348 |
| tls | dependent&#45;read | grpc | 5000 | 133.109 | 186.937 | 49.060 | 37.515 | 159.273 | 233.735 | 165.798 | 272.893 | 308.508 |
| tls | dependent&#45;read | legacy | 5000 | 77.160 | 111.653 | 27.492 | 22.177 | 91.778 | 141.855 | 98.567 | 155.884 | 174.108 |
| tls | dependent&#45;read | portable | 5000 | 99.207 | 116.248 | 21.905 | 2.740 | 113.535 | 120.523 | 112.082 | 143.856 | 173.074 |
| tls | dependent&#45;read | websocket | 5000 | 58.231 | 72.997 | 9.196 | 0.230 | 72.812 | 73.388 | 71.701 | 89.013 | 101.042 |
| tls | pipelined&#45;read | cpp | 5000 | 87.316 | 139.041 | 45.175 | 43.076 | 117.026 | 215.993 | 124.262 | 222.951 | 251.902 |
| tls | pipelined&#45;read | legacy | 5000 | 48.240 | 62.709 | 15.418 | 8.129 | 56.343 | 76.081 | 57.535 | 92.398 | 117.533 |
| tls | pipelined&#45;read | portable | 5000 | 73.213 | 88.393 | 15.426 | 4.579 | 83.943 | 95.833 | 84.155 | 116.218 | 152.448 |
| tls | write&#45;read | cpp | 5000 | 109.811 | 159.095 | 42.785 | 25.054 | 130.155 | 187.004 | 144.110 | 233.571 | 253.186 |
| tls | write&#45;read | grpc | 5000 | 134.508 | 168.223 | 33.125 | 20.407 | 156.341 | 204.522 | 159.446 | 248.448 | 285.754 |
| tls | write&#45;read | legacy | 5000 | 72.487 | 85.681 | 9.628 | 1.697 | 84.282 | 87.656 | 83.528 | 102.634 | 119.266 |
| tls | write&#45;read | portable | 5000 | 77.374 | 109.072 | 30.463 | 25.320 | 88.792 | 147.208 | 92.569 | 160.139 | 186.383 |
| tls | write&#45;read | websocket | 5000 | 57.922 | 79.683 | 17.603 | 16.170 | 71.311 | 108.559 | 73.779 | 115.771 | 136.696 |
| tls | callback | cpp | 5000 | 116.223 | 168.578 | 47.567 | 22.190 | 141.854 | 193.515 | 150.530 | 254.294 | 282.217 |
| tls | callback | grpc | 5000 | 68.959 | 79.271 | 10.342 | 0.862 | 78.096 | 80.491 | 76.352 | 98.730 | 119.934 |
| tls | callback | legacy | 5000 | 79.453 | 107.732 | 27.793 | 24.744 | 89.809 | 144.838 | 94.002 | 156.338 | 179.145 |
| tls | callback | portable | 5000 | 96.189 | 138.996 | 37.591 | 29.237 | 110.678 | 172.650 | 121.448 | 196.890 | 220.871 |
| tls | callback | websocket | 5000 | 58.502 | 79.587 | 17.989 | 14.841 | 68.884 | 102.950 | 71.494 | 110.866 | 130.732 |
| tls | bulk&#45;64k | cpp | 5000 | 232.967 | 295.623 | 44.831 | 22.540 | 275.908 | 333.447 | 288.009 | 401.979 | 442.058 |
| tls | bulk&#45;64k | grpc | 5000 | 239.815 | 291.412 | 40.789 | 18.961 | 279.164 | 324.823 | 280.652 | 397.517 | 447.703 |
| tls | bulk&#45;64k | legacy | 5000 | 160.570 | 216.855 | 48.603 | 32.340 | 178.832 | 256.319 | 192.972 | 292.431 | 320.310 |
| tls | bulk&#45;64k | portable | 5000 | 165.050 | 197.989 | 35.024 | 15.670 | 185.412 | 220.593 | 185.137 | 280.129 | 300.975 |
| tls | bulk&#45;64k | websocket | 5000 | 182.857 | 246.165 | 50.686 | 42.266 | 203.600 | 289.916 | 224.254 | 318.550 | 345.916 |
| tls | bulk&#45;256k | cpp | 5000 | 957.242 | 1204.097 | 171.528 | 66.893 | 1096.550 | 1259.989 | 1141.920 | 1520.559 | 1583.862 |
| tls | bulk&#45;256k | grpc | 5000 | 596.231 | 1533.746 | 264.660 | 67.809 | 1497.264 | 1654.850 | 1523.849 | 1848.959 | 1938.544 |
| tls | bulk&#45;256k | legacy | 5000 | 469.546 | 526.476 | 35.996 | 7.025 | 522.142 | 538.636 | 519.291 | 582.922 | 693.996 |
| tls | bulk&#45;256k | portable | 5000 | 484.953 | 552.012 | 52.717 | 27.087 | 531.489 | 595.157 | 537.208 | 694.637 | 728.064 |
| tls | bulk&#45;256k | websocket | 5000 | 1146.521 | 1244.420 | 59.564 | 15.909 | 1223.886 | 1262.991 | 1227.444 | 1353.104 | 1402.902 |
| tls | bulk&#45;1m | cpp | 5000 | 3580.717 | 4296.100 | 486.640 | 95.480 | 4174.641 | 4398.392 | 4151.650 | 5597.280 | 5764.013 |
| tls | bulk&#45;1m | grpc | 5000 | 2285.953 | 4342.095 | 1382.145 | 1433.422 | 2629.633 | 6598.863 | 4024.842 | 6992.953 | 7748.671 |
| tls | bulk&#45;1m | legacy | 5000 | 1600.487 | 1870.593 | 221.560 | 48.196 | 1815.068 | 1926.435 | 1741.942 | 2239.583 | 2311.975 |
| tls | bulk&#45;1m | portable | 5000 | 1625.472 | 1842.129 | 215.121 | 69.736 | 1778.136 | 1927.973 | 1732.243 | 2240.941 | 2318.004 |
| tls | bulk&#45;1m | websocket | 5000 | 4604.768 | 5158.798 | 369.890 | 101.366 | 5059.762 | 5316.218 | 5019.187 | 5855.124 | 6054.676 |
| tls | bulk&#45;4m | cpp | 5000 | 16751.555 | 19230.810 | 1902.327 | 422.000 | 18500.473 | 19559.557 | 18699.307 | 24882.506 | 25576.861 |
| tls | bulk&#45;4m | grpc | 5000 | 27477.665 | 30279.213 | 1909.347 | 46.808 | 30241.807 | 30345.928 | 29342.407 | 33630.803 | 34259.526 |
| tls | bulk&#45;4m | legacy | 5000 | 8185.788 | 9123.844 | 858.190 | 260.571 | 8830.327 | 9447.874 | 8677.978 | 10735.502 | 11007.338 |
| tls | bulk&#45;4m | portable | 5000 | 8245.354 | 9206.265 | 872.353 | 130.440 | 8990.377 | 9319.544 | 8786.716 | 10902.848 | 11254.745 |
| tls | bulk&#45;4m | websocket | 5000 | 29650.290 | 31827.146 | 2319.797 | 219.696 | 31541.182 | 32114.110 | 30559.847 | 36195.090 | 36580.107 |
| tls | deep&#45;8 | cpp | 5000 | 56.592 | 83.707 | 22.589 | 19.733 | 69.713 | 117.801 | 78.777 | 123.135 | 139.384 |
| tls | deep&#45;8 | grpc | 5000 | 75.448 | 110.286 | 29.930 | 22.209 | 90.362 | 137.960 | 98.661 | 156.918 | 178.702 |
| tls | deep&#45;8 | legacy | 5000 | 39.311 | 66.888 | 11.438 | 9.105 | 50.607 | 71.255 | 68.544 | 82.541 | 94.382 |
| tls | deep&#45;8 | portable | 5000 | 40.238 | 75.173 | 10.256 | 2.747 | 70.319 | 76.960 | 73.148 | 90.741 | 107.010 |
| tls | deep&#45;8 | websocket | 5000 | 34.611 | 54.330 | 13.344 | 11.341 | 45.506 | 67.765 | 51.912 | 78.797 | 91.235 |
| tls | deep&#45;32 | cpp | 5000 | 62.829 | 82.299 | 20.903 | 11.024 | 74.604 | 101.517 | 77.669 | 129.138 | 147.346 |
| tls | deep&#45;32 | grpc | 5000 | 91.279 | 119.977 | 26.534 | 9.460 | 111.460 | 136.130 | 114.252 | 190.421 | 213.315 |
| tls | deep&#45;32 | legacy | 5000 | 45.998 | 61.617 | 16.966 | 16.966 | 52.595 | 91.895 | 55.204 | 92.062 | 105.547 |
| tls | deep&#45;32 | portable | 5000 | 45.983 | 60.452 | 16.092 | 11.588 | 54.018 | 81.122 | 54.512 | 92.817 | 107.058 |
| tls | deep&#45;32 | websocket | 5000 | 52.540 | 81.474 | 23.324 | 18.507 | 67.927 | 109.781 | 72.971 | 124.161 | 139.872 |
| tls | concurrent&#45;32 | cpp | 5000 | 479.200 | 845.434 | 275.726 | 65.977 | 763.609 | 944.812 | 836.722 | 1218.150 | 1257.822 |
| tls | concurrent&#45;32 | grpc | 5000 | 597.592 | 860.689 | 175.429 | 73.886 | 731.821 | 916.952 | 866.769 | 1082.506 | 1134.709 |
| tls | concurrent&#45;32 | legacy | 5000 | 121.290 | 198.109 | 66.358 | 42.729 | 147.959 | 231.690 | 168.668 | 297.321 | 320.087 |
| tls | concurrent&#45;32 | portable | 5000 | 148.893 | 200.580 | 58.577 | 37.668 | 176.919 | 267.543 | 185.967 | 350.672 | 378.873 |
| tls | concurrent&#45;32 | websocket | 5000 | 113.943 | 209.357 | 46.636 | 26.695 | 185.849 | 243.791 | 197.673 | 295.897 | 325.022 |
| tls | deep&#45;32&#45;concurrent&#45;32 | cpp | 5000 | 636.941 | 1013.728 | 278.057 | 96.949 | 921.846 | 1159.131 | 965.991 | 1416.026 | 1468.187 |
| tls | deep&#45;32&#45;concurrent&#45;32 | grpc | 5000 | 1489.062 | 2161.959 | 437.142 | 51.106 | 2124.531 | 2251.370 | 2088.361 | 2858.967 | 2931.138 |
| tls | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 423.024 | 664.663 | 173.713 | 62.498 | 554.345 | 707.200 | 655.653 | 890.377 | 918.173 |
| tls | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 426.038 | 610.258 | 149.566 | 29.239 | 579.781 | 651.868 | 565.631 | 839.820 | 874.529 |
| tls | deep&#45;32&#45;concurrent&#45;32 | websocket | 5000 | 682.072 | 997.313 | 200.473 | 50.451 | 944.132 | 1051.402 | 957.313 | 1284.141 | 1333.017 |
| tls | services&#45;4&#45;deep&#45;16 | cpp | 5000 | 471.364 | 523.522 | 26.459 | 2.189 | 520.409 | 526.502 | 519.603 | 572.114 | 610.185 |
| tls | services&#45;4&#45;deep&#45;16 | grpc | 5000 | 592.602 | 659.402 | 30.146 | 6.685 | 654.042 | 670.784 | 655.679 | 711.544 | 751.561 |
| tls | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 341.578 | 380.700 | 24.213 | 5.367 | 376.105 | 389.932 | 376.863 | 424.057 | 457.121 |
| tls | services&#45;4&#45;deep&#45;16 | portable | 5000 | 375.359 | 414.394 | 25.117 | 2.545 | 412.296 | 418.623 | 410.478 | 457.528 | 494.205 |
| tls | services&#45;4&#45;deep&#45;16 | websocket | 5000 | 269.151 | 351.720 | 40.676 | 5.624 | 344.237 | 360.087 | 349.180 | 418.981 | 453.374 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | 5000 | 3256.779 | 4391.235 | 331.101 | 49.944 | 4321.458 | 4453.154 | 4399.773 | 4899.182 | 5022.297 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | 5000 | 5345.520 | 6752.881 | 371.030 | 54.041 | 6662.483 | 6800.327 | 6766.784 | 7345.145 | 7555.808 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1914.467 | 2478.010 | 134.752 | 12.010 | 2471.092 | 2499.260 | 2490.050 | 2661.297 | 2719.437 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1971.196 | 2508.386 | 129.557 | 9.674 | 2499.839 | 2519.646 | 2521.010 | 2686.312 | 2739.233 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | 5000 | 1746.896 | 2025.635 | 157.749 | 9.516 | 2014.897 | 2035.832 | 2020.048 | 2155.853 | 2222.304 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 2784.164 | 3754.919 | 440.679 | 37.856 | 3703.610 | 3804.158 | 3734.673 | 4500.041 | 4668.616 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 4570.523 | 6076.740 | 579.693 | 70.103 | 5966.327 | 6155.895 | 6096.597 | 6973.444 | 7205.996 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1586.563 | 2464.755 | 156.471 | 25.933 | 2449.425 | 2510.715 | 2481.285 | 2679.735 | 2743.659 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1847.241 | 2408.656 | 118.364 | 14.311 | 2394.735 | 2432.734 | 2425.430 | 2575.958 | 2642.873 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1494.937 | 1814.392 | 131.227 | 8.272 | 1801.978 | 1823.936 | 1806.668 | 2036.363 | 2151.568 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 5290.198 | 7655.988 | 936.697 | 60.795 | 7579.362 | 7723.751 | 7763.984 | 9012.244 | 9338.535 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 8398.480 | 11740.046 | 1545.955 | 143.771 | 11558.508 | 11934.854 | 11955.437 | 13877.266 | 14326.259 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 3454.100 | 5108.519 | 264.003 | 49.768 | 5058.961 | 5175.795 | 5110.308 | 5507.174 | 5721.945 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3216.737 | 4955.571 | 263.856 | 58.664 | 4872.115 | 5023.647 | 4987.910 | 5311.386 | 5597.303 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 2831.164 | 3436.879 | 254.891 | 14.866 | 3421.434 | 3460.645 | 3402.326 | 3916.923 | 4166.157 |
| quic | loopback&#45;0 | legacy | 5000 | 48.871 | 62.831 | 15.472 | 15.582 | 53.749 | 90.603 | 55.752 | 90.733 | 105.288 |
| quic | loopback&#45;0 | portable | 5000 | 54.659 | 77.180 | 22.746 | 18.444 | 62.115 | 106.534 | 64.425 | 116.507 | 137.307 |
| quic | loopback&#45;64 | legacy | 5000 | 49.162 | 68.021 | 17.878 | 16.421 | 55.499 | 90.372 | 58.068 | 98.250 | 112.027 |
| quic | loopback&#45;64 | portable | 5000 | 55.080 | 74.877 | 20.740 | 17.404 | 62.335 | 104.784 | 64.003 | 111.975 | 128.447 |
| quic | loopback&#45;1024 | legacy | 5000 | 51.116 | 80.569 | 20.427 | 12.996 | 65.970 | 95.108 | 90.145 | 107.102 | 122.589 |
| quic | loopback&#45;1024 | portable | 5000 | 57.380 | 89.718 | 23.394 | 23.008 | 63.653 | 109.348 | 103.239 | 121.122 | 139.541 |
| quic | loopback&#45;65536 | legacy | 5000 | 210.100 | 266.425 | 54.079 | 39.893 | 234.862 | 334.684 | 241.666 | 365.475 | 386.889 |
| quic | loopback&#45;65536 | portable | 5000 | 217.066 | 295.448 | 63.729 | 36.219 | 246.035 | 340.665 | 259.892 | 385.969 | 408.956 |
| quic | unary&#45;empty | legacy | 5000 | 48.754 | 62.970 | 15.813 | 10.679 | 55.122 | 80.609 | 56.415 | 91.863 | 107.422 |
| quic | unary&#45;empty | portable | 5000 | 54.846 | 86.540 | 21.117 | 21.971 | 62.294 | 102.690 | 97.926 | 114.103 | 132.130 |
| quic | unary&#45;small | legacy | 5000 | 48.828 | 62.952 | 15.205 | 15.000 | 55.201 | 89.741 | 56.758 | 90.353 | 105.687 |
| quic | unary&#45;small | portable | 5000 | 54.773 | 62.539 | 7.167 | 1.059 | 61.315 | 63.933 | 60.519 | 76.134 | 86.427 |
| quic | unary&#45;1k | legacy | 5000 | 51.303 | 60.339 | 9.887 | 2.902 | 57.738 | 65.300 | 57.575 | 79.807 | 100.728 |
| quic | unary&#45;1k | portable | 5000 | 57.288 | 83.052 | 23.425 | 24.375 | 65.090 | 111.336 | 70.242 | 120.423 | 138.816 |
| quic | fanout&#45;32 | legacy | 5000 | 211.094 | 266.310 | 43.906 | 15.110 | 257.498 | 292.774 | 258.626 | 369.605 | 426.188 |
| quic | fanout&#45;32 | portable | 5000 | 247.370 | 303.643 | 36.482 | 2.452 | 300.592 | 307.379 | 299.911 | 364.075 | 419.923 |
| quic | dependent&#45;read | legacy | 5000 | 104.380 | 136.497 | 32.662 | 20.478 | 121.169 | 167.404 | 123.143 | 204.702 | 233.377 |
| quic | dependent&#45;read | portable | 5000 | 141.331 | 229.942 | 60.041 | 62.520 | 160.861 | 296.033 | 255.008 | 311.766 | 345.272 |
| quic | pipelined&#45;read | legacy | 5000 | 62.179 | 73.497 | 8.521 | 1.183 | 71.823 | 75.128 | 71.288 | 89.362 | 102.265 |
| quic | pipelined&#45;read | portable | 5000 | 97.109 | 112.291 | 12.203 | 1.355 | 109.998 | 113.334 | 109.283 | 135.316 | 153.284 |
| quic | write&#45;read | legacy | 5000 | 100.115 | 136.022 | 33.185 | 27.728 | 115.464 | 183.713 | 119.562 | 193.843 | 219.461 |
| quic | write&#45;read | portable | 5000 | 112.313 | 128.944 | 12.631 | 1.536 | 127.319 | 131.249 | 126.077 | 152.323 | 172.684 |
| quic | callback | legacy | 5000 | 106.003 | 144.504 | 37.985 | 35.311 | 118.323 | 199.573 | 124.944 | 212.371 | 236.802 |
| quic | callback | portable | 5000 | 136.386 | 176.842 | 41.944 | 35.217 | 154.854 | 237.854 | 159.529 | 260.034 | 286.204 |
| quic | bulk&#45;64k | legacy | 5000 | 209.121 | 249.714 | 42.111 | 20.980 | 234.539 | 280.619 | 235.528 | 355.293 | 376.442 |
| quic | bulk&#45;64k | portable | 5000 | 214.701 | 264.681 | 52.542 | 30.623 | 239.927 | 305.215 | 242.482 | 372.580 | 395.977 |
| quic | bulk&#45;256k | legacy | 5000 | 517.699 | 596.707 | 675.890 | 19.290 | 572.530 | 619.037 | 572.743 | 632.292 | 845.602 |
| quic | bulk&#45;256k | portable | 5000 | 541.361 | 610.133 | 392.817 | 8.916 | 603.295 | 625.601 | 601.686 | 654.481 | 678.907 |
| quic | bulk&#45;1m | legacy | 5000 | 1892.290 | 2406.416 | 696.060 | 98.726 | 2244.422 | 2512.375 | 2160.856 | 3188.353 | 3309.309 |
| quic | bulk&#45;1m | portable | 5000 | 1900.541 | 2241.088 | 633.860 | 72.549 | 2154.105 | 2327.020 | 2116.547 | 2889.727 | 3158.841 |
| quic | bulk&#45;4m | legacy | 5000 | 9730.384 | 11431.195 | 1713.114 | 159.714 | 11233.763 | 11630.572 | 10588.478 | 14242.450 | 14922.835 |
| quic | bulk&#45;4m | portable | 5000 | 9822.245 | 11562.939 | 1783.502 | 154.005 | 11343.868 | 11740.242 | 10719.685 | 14324.335 | 14993.668 |
| quic | deep&#45;8 | legacy | 5000 | 51.919 | 71.047 | 19.434 | 15.718 | 58.624 | 93.829 | 61.648 | 105.539 | 122.076 |
| quic | deep&#45;8 | portable | 5000 | 57.574 | 71.728 | 16.390 | 11.081 | 65.947 | 91.485 | 66.175 | 109.392 | 133.109 |
| quic | deep&#45;32 | legacy | 5000 | 60.125 | 107.385 | 24.759 | 22.148 | 69.842 | 122.308 | 115.372 | 137.943 | 159.450 |
| quic | deep&#45;32 | portable | 5000 | 66.159 | 122.662 | 25.121 | 23.917 | 79.913 | 134.266 | 126.712 | 153.561 | 178.061 |
| quic | concurrent&#45;32 | legacy | 5000 | 137.973 | 212.951 | 69.476 | 66.523 | 168.491 | 329.571 | 187.512 | 340.421 | 366.835 |
| quic | concurrent&#45;32 | portable | 5000 | 170.358 | 225.729 | 44.519 | 17.955 | 211.201 | 256.466 | 222.894 | 291.596 | 398.559 |
| quic | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 474.049 | 789.174 | 167.471 | 28.162 | 754.761 | 827.351 | 896.608 | 951.093 | 985.809 |
| quic | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 484.836 | 739.761 | 155.623 | 17.705 | 711.059 | 758.885 | 777.252 | 916.326 | 953.716 |
| quic | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 512.084 | 569.827 | 35.708 | 23.077 | 557.228 | 611.004 | 562.715 | 638.001 | 679.353 |
| quic | services&#45;4&#45;deep&#45;16 | portable | 5000 | 482.146 | 553.637 | 40.100 | 26.561 | 530.975 | 596.694 | 545.584 | 627.437 | 671.637 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 2214.438 | 2929.478 | 172.959 | 42.083 | 2884.967 | 2984.479 | 2923.882 | 3214.634 | 3338.009 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 2279.876 | 2928.461 | 151.153 | 58.691 | 2890.314 | 3032.075 | 2932.609 | 3177.251 | 3284.075 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2318.833 | 2909.524 | 234.955 | 72.069 | 2832.113 | 3003.445 | 2878.762 | 3328.331 | 3489.933 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2083.608 | 2658.546 | 262.732 | 102.777 | 2533.228 | 2794.552 | 2674.936 | 3048.821 | 3235.612 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 4476.436 | 6057.181 | 494.439 | 108.816 | 5905.834 | 6187.987 | 6011.382 | 6902.900 | 7197.448 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3932.208 | 5252.753 | 610.607 | 61.731 | 5184.627 | 5316.161 | 5314.697 | 6165.943 | 6568.844 |

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


