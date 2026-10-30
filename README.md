# Capntproto verification reports

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-30T05:41:05Z · run 41009792478 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41009792478) · commit `9b3e0517100d` · **failure**

![Cargo test results](docs/reports/cargo-history.svg)

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-30T05:41:05Z · run 41009792478 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41009792478) · commit `9b3e0517100d` · **failure**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-30T05:41:05Z · run 41009792478 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41009792478) · commit `9b3e0517100d` · **failure**

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

Latest coverage run: [2026-10-30T05:41:05Z · run 41009792392 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41009792392) · commit `9b3e0517100d` · **cancelled**

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-30T05:41:05Z · run 41009792478 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41009792478) · commit `9b3e0517100d` · **failure**

Measurement evidence: **passed**. Independently validated encrypted benchmark values&#46; Workflow status and diagnostic completion are separate&#59; unavailable values remain blank&#46; Operations include whole concurrent batches and every service hop&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Current TCP fixtures use TLS 1.3, including upstream Rust Cap’n Proto, C++, gRPC over HTTP/2 and WebSockets over WSS; native encrypted transports and standard QUIC are identified separately. Historical data retains its recorded fixture contract. The dedicated campaign also reports recursive data, concurrent calls and independent service chains for gRPC and binary Protobuf WebSockets in its encrypted comparison artifact. Bars compare the recorded workload, rather than universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.


## Exhaustive benchmark tables

Measured run: [2026-10-30T05:41:05Z · run 41009792478 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41009792478) · commit `9b3e0517100d` · **failure**

All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.

### All encrypted latency comparisons

| Transport | Case | Implementation | Samples | Minimum µs | Mean µs | Sample SD µs | Run mean SD µs | Run mean min µs | Run mean max µs | p50 µs | p95 µs | p99 µs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tls | loopback&#45;0 | cpp | 5000 | 49.461 | 55.594 | 4.751 | 0.427 | 55.084 | 56.106 | 54.222 | 64.521 | 76.944 |
| tls | loopback&#45;0 | grpc | 5000 | 60.049 | 64.589 | 5.290 | 0.599 | 63.770 | 65.372 | 62.929 | 74.586 | 86.062 |
| tls | loopback&#45;0 | legacy | 5000 | 35.100 | 37.817 | 3.778 | 0.363 | 37.516 | 38.280 | 36.905 | 42.954 | 54.017 |
| tls | loopback&#45;0 | portable | 5000 | 36.824 | 38.965 | 3.492 | 0.346 | 38.610 | 39.406 | 37.991 | 44.215 | 54.343 |
| tls | loopback&#45;0 | websocket | 5000 | 33.238 | 37.179 | 3.384 | 0.290 | 36.717 | 37.474 | 36.492 | 41.620 | 50.993 |
| tls | loopback&#45;64 | cpp | 5000 | 52.029 | 55.818 | 4.398 | 0.431 | 55.455 | 56.536 | 54.456 | 65.165 | 75.151 |
| tls | loopback&#45;64 | grpc | 5000 | 60.790 | 65.600 | 5.700 | 0.735 | 64.747 | 66.566 | 63.614 | 76.426 | 87.658 |
| tls | loopback&#45;64 | legacy | 5000 | 35.142 | 38.127 | 3.225 | 0.581 | 37.588 | 39.009 | 37.122 | 44.021 | 53.378 |
| tls | loopback&#45;64 | portable | 5000 | 36.720 | 39.399 | 3.941 | 0.309 | 38.982 | 39.769 | 38.385 | 46.083 | 56.356 |
| tls | loopback&#45;64 | websocket | 5000 | 30.441 | 34.811 | 3.578 | 0.120 | 34.682 | 34.956 | 34.130 | 39.428 | 49.703 |
| tls | loopback&#45;1024 | cpp | 5000 | 54.806 | 90.761 | 26.590 | 29.043 | 58.666 | 112.120 | 109.137 | 121.098 | 127.825 |
| tls | loopback&#45;1024 | grpc | 5000 | 66.645 | 105.490 | 31.565 | 28.690 | 72.199 | 126.632 | 123.164 | 135.895 | 147.853 |
| tls | loopback&#45;1024 | legacy | 5000 | 37.754 | 58.256 | 58.546 | 12.936 | 40.502 | 67.761 | 65.375 | 75.660 | 83.512 |
| tls | loopback&#45;1024 | portable | 5000 | 39.173 | 54.134 | 26.201 | 15.712 | 41.922 | 71.357 | 43.686 | 73.809 | 84.370 |
| tls | loopback&#45;1024 | websocket | 5000 | 29.528 | 48.881 | 9.586 | 8.614 | 33.483 | 53.152 | 49.454 | 61.552 | 68.569 |
| tls | loopback&#45;65536 | cpp | 5000 | 224.160 | 244.054 | 9.346 | 1.122 | 242.611 | 245.151 | 241.505 | 260.328 | 273.002 |
| tls | loopback&#45;65536 | grpc | 5000 | 229.387 | 253.909 | 14.547 | 2.958 | 251.723 | 258.875 | 250.271 | 282.890 | 303.380 |
| tls | loopback&#45;65536 | legacy | 5000 | 156.537 | 164.868 | 6.729 | 0.592 | 164.274 | 165.616 | 162.057 | 175.636 | 186.136 |
| tls | loopback&#45;65536 | portable | 5000 | 155.606 | 171.901 | 7.752 | 1.274 | 170.754 | 173.933 | 168.789 | 184.881 | 198.739 |
| tls | loopback&#45;65536 | websocket | 5000 | 179.349 | 199.415 | 30.133 | 21.462 | 189.275 | 237.799 | 186.711 | 282.743 | 299.789 |
| tls | unary&#45;empty | cpp | 5000 | 50.015 | 55.960 | 4.599 | 0.659 | 55.248 | 56.938 | 54.766 | 64.987 | 75.424 |
| tls | unary&#45;empty | grpc | 5000 | 60.642 | 65.328 | 5.276 | 0.263 | 64.898 | 65.613 | 63.612 | 76.162 | 86.557 |
| tls | unary&#45;empty | legacy | 5000 | 35.432 | 38.543 | 4.018 | 1.360 | 37.586 | 40.931 | 37.109 | 46.931 | 55.532 |
| tls | unary&#45;empty | portable | 5000 | 36.571 | 39.397 | 4.497 | 0.351 | 39.064 | 39.962 | 38.339 | 44.956 | 55.639 |
| tls | unary&#45;empty | websocket | 5000 | 32.314 | 37.431 | 3.315 | 0.210 | 37.181 | 37.688 | 36.746 | 41.707 | 50.795 |
| tls | unary&#45;small | cpp | 5000 | 51.376 | 55.922 | 5.162 | 0.450 | 55.409 | 56.617 | 54.574 | 64.443 | 76.792 |
| tls | unary&#45;small | grpc | 5000 | 59.211 | 66.180 | 5.131 | 0.263 | 65.877 | 66.457 | 64.572 | 75.577 | 86.920 |
| tls | unary&#45;small | legacy | 5000 | 35.203 | 37.876 | 3.835 | 0.312 | 37.560 | 38.356 | 36.888 | 42.987 | 53.042 |
| tls | unary&#45;small | portable | 5000 | 36.696 | 39.396 | 3.921 | 0.244 | 39.081 | 39.713 | 38.256 | 46.017 | 56.253 |
| tls | unary&#45;small | websocket | 5000 | 30.605 | 35.171 | 4.173 | 0.219 | 34.789 | 35.337 | 34.398 | 39.794 | 49.682 |
| tls | unary&#45;1k | cpp | 5000 | 54.244 | 79.712 | 26.367 | 28.846 | 58.017 | 111.575 | 60.250 | 118.218 | 125.232 |
| tls | unary&#45;1k | grpc | 5000 | 65.189 | 90.862 | 25.026 | 23.207 | 74.969 | 127.846 | 75.882 | 134.324 | 144.873 |
| tls | unary&#45;1k | legacy | 5000 | 37.430 | 45.847 | 11.062 | 11.762 | 39.853 | 66.872 | 39.591 | 66.248 | 76.040 |
| tls | unary&#45;1k | portable | 5000 | 38.711 | 48.059 | 12.993 | 13.439 | 41.853 | 72.095 | 40.762 | 71.676 | 81.767 |
| tls | unary&#45;1k | websocket | 5000 | 30.129 | 42.013 | 10.850 | 10.688 | 33.962 | 53.890 | 36.235 | 60.263 | 65.365 |
| tls | fanout&#45;32 | cpp | 5000 | 429.329 | 523.851 | 166.214 | 112.578 | 453.918 | 720.742 | 456.195 | 999.244 | 1010.943 |
| tls | fanout&#45;32 | grpc | 5000 | 786.729 | 846.226 | 46.543 | 8.222 | 838.057 | 859.702 | 839.431 | 871.718 | 1028.471 |
| tls | fanout&#45;32 | legacy | 5000 | 162.579 | 182.294 | 10.461 | 2.119 | 179.778 | 185.543 | 178.357 | 201.069 | 224.566 |
| tls | fanout&#45;32 | portable | 5000 | 192.642 | 216.724 | 9.616 | 0.942 | 215.331 | 217.714 | 213.279 | 234.449 | 253.383 |
| tls | fanout&#45;32 | websocket | 5000 | 235.270 | 261.494 | 19.471 | 2.441 | 257.749 | 264.199 | 257.918 | 287.369 | 302.909 |
| tls | dependent&#45;read | cpp | 5000 | 113.568 | 127.734 | 7.894 | 1.360 | 126.018 | 129.319 | 125.083 | 140.600 | 157.303 |
| tls | dependent&#45;read | grpc | 5000 | 129.732 | 141.125 | 8.059 | 2.114 | 139.046 | 144.515 | 138.956 | 153.860 | 169.540 |
| tls | dependent&#45;read | legacy | 5000 | 76.019 | 82.487 | 4.895 | 1.110 | 80.920 | 83.942 | 81.074 | 91.608 | 100.180 |
| tls | dependent&#45;read | portable | 5000 | 96.167 | 102.960 | 6.591 | 0.892 | 101.950 | 104.235 | 100.325 | 116.033 | 128.207 |
| tls | dependent&#45;read | websocket | 5000 | 57.377 | 65.021 | 4.596 | 0.612 | 64.362 | 65.911 | 64.258 | 73.601 | 80.573 |
| tls | pipelined&#45;read | cpp | 5000 | 81.108 | 90.022 | 4.861 | 0.470 | 89.306 | 90.615 | 88.280 | 99.533 | 107.454 |
| tls | pipelined&#45;read | legacy | 5000 | 46.740 | 50.342 | 3.920 | 0.361 | 49.968 | 50.910 | 49.059 | 57.951 | 68.077 |
| tls | pipelined&#45;read | portable | 5000 | 69.566 | 74.957 | 5.285 | 0.594 | 74.230 | 75.729 | 73.013 | 85.646 | 97.824 |
| tls | write&#45;read | cpp | 5000 | 101.588 | 137.218 | 45.738 | 46.575 | 112.026 | 219.789 | 111.562 | 225.529 | 240.581 |
| tls | write&#45;read | grpc | 5000 | 132.672 | 182.214 | 52.933 | 57.705 | 139.776 | 246.324 | 145.410 | 256.758 | 270.377 |
| tls | write&#45;read | legacy | 5000 | 72.036 | 77.477 | 9.371 | 0.979 | 76.674 | 79.129 | 75.505 | 86.215 | 96.350 |
| tls | write&#45;read | portable | 5000 | 74.148 | 91.886 | 24.521 | 26.815 | 79.523 | 139.851 | 78.673 | 139.455 | 154.719 |
| tls | write&#45;read | websocket | 5000 | 58.459 | 80.702 | 20.011 | 21.009 | 64.905 | 103.721 | 68.925 | 116.229 | 125.196 |
| tls | callback | cpp | 5000 | 105.848 | 186.757 | 58.408 | 55.347 | 119.954 | 236.373 | 230.727 | 248.703 | 256.852 |
| tls | callback | grpc | 5000 | 67.411 | 99.629 | 23.064 | 25.234 | 71.682 | 118.523 | 115.208 | 128.313 | 133.609 |
| tls | callback | legacy | 5000 | 77.150 | 105.553 | 28.764 | 31.605 | 82.183 | 140.653 | 83.695 | 149.267 | 156.603 |
| tls | callback | portable | 5000 | 87.624 | 127.913 | 38.353 | 42.294 | 95.467 | 174.553 | 100.209 | 184.883 | 193.125 |
| tls | callback | websocket | 5000 | 59.653 | 86.860 | 15.994 | 12.135 | 67.874 | 97.558 | 92.550 | 107.607 | 116.813 |
| tls | bulk&#45;64k | cpp | 5000 | 218.829 | 245.959 | 22.224 | 6.445 | 241.586 | 257.369 | 240.395 | 263.893 | 397.757 |
| tls | bulk&#45;64k | grpc | 5000 | 231.286 | 271.465 | 51.540 | 34.862 | 253.923 | 333.776 | 253.121 | 429.334 | 457.113 |
| tls | bulk&#45;64k | legacy | 5000 | 150.338 | 165.480 | 7.065 | 0.898 | 164.279 | 166.426 | 162.463 | 177.153 | 189.515 |
| tls | bulk&#45;64k | portable | 5000 | 154.445 | 170.487 | 7.306 | 1.014 | 169.342 | 171.571 | 167.302 | 183.703 | 198.702 |
| tls | bulk&#45;64k | websocket | 5000 | 173.687 | 190.581 | 8.938 | 1.164 | 188.627 | 191.604 | 187.274 | 204.758 | 218.074 |
| tls | bulk&#45;256k | cpp | 5000 | 938.338 | 1022.651 | 73.791 | 28.502 | 997.517 | 1071.328 | 1001.992 | 1118.418 | 1478.114 |
| tls | bulk&#45;256k | grpc | 5000 | 583.116 | 1160.146 | 402.168 | 271.955 | 690.697 | 1363.217 | 1167.676 | 1849.439 | 1955.587 |
| tls | bulk&#45;256k | legacy | 5000 | 469.577 | 523.237 | 65.727 | 34.952 | 497.607 | 571.002 | 499.750 | 697.486 | 709.122 |
| tls | bulk&#45;256k | portable | 5000 | 475.592 | 505.290 | 14.966 | 1.576 | 503.116 | 507.174 | 504.234 | 522.457 | 552.398 |
| tls | bulk&#45;256k | websocket | 5000 | 1145.280 | 1217.331 | 50.673 | 44.523 | 1183.485 | 1294.321 | 1194.353 | 1303.892 | 1336.583 |
| tls | bulk&#45;1m | cpp | 5000 | 3649.294 | 3921.992 | 106.036 | 59.238 | 3860.779 | 3995.412 | 3898.864 | 4088.894 | 4226.524 |
| tls | bulk&#45;1m | grpc | 5000 | 2242.848 | 4268.914 | 1416.067 | 1563.163 | 2306.815 | 5922.282 | 3714.633 | 6281.815 | 6440.552 |
| tls | bulk&#45;1m | legacy | 5000 | 1608.850 | 1670.371 | 96.568 | 44.922 | 1648.078 | 1750.642 | 1648.968 | 1720.631 | 2135.404 |
| tls | bulk&#45;1m | portable | 5000 | 1588.325 | 1667.540 | 75.312 | 22.476 | 1653.699 | 1707.538 | 1654.256 | 1693.577 | 2134.401 |
| tls | bulk&#45;1m | websocket | 5000 | 4618.326 | 4776.039 | 139.452 | 57.848 | 4717.296 | 4862.182 | 4742.887 | 4949.804 | 5546.651 |
| tls | bulk&#45;4m | cpp | 5000 | 15992.202 | 16813.004 | 1214.785 | 266.390 | 16598.481 | 17233.410 | 16531.045 | 17434.740 | 23646.186 |
| tls | bulk&#45;4m | grpc | 5000 | 20884.223 | 27077.151 | 3210.046 | 3086.727 | 21746.112 | 29494.001 | 27099.927 | 31595.046 | 32099.814 |
| tls | bulk&#45;4m | legacy | 5000 | 7508.024 | 8087.895 | 690.120 | 458.679 | 7848.171 | 8907.280 | 7822.355 | 9818.823 | 10182.843 |
| tls | bulk&#45;4m | portable | 5000 | 7525.134 | 8060.478 | 595.450 | 349.088 | 7880.422 | 8683.449 | 7858.389 | 9691.022 | 9922.181 |
| tls | bulk&#45;4m | websocket | 5000 | 27936.490 | 31586.274 | 2763.012 | 1025.396 | 30685.787 | 33244.548 | 32252.254 | 35006.595 | 35500.626 |
| tls | deep&#45;8 | cpp | 5000 | 55.032 | 59.761 | 5.779 | 2.349 | 58.331 | 63.925 | 57.284 | 70.278 | 80.383 |
| tls | deep&#45;8 | grpc | 5000 | 70.166 | 79.194 | 5.049 | 1.313 | 77.798 | 81.306 | 77.034 | 88.987 | 100.092 |
| tls | deep&#45;8 | legacy | 5000 | 37.971 | 40.767 | 3.275 | 0.212 | 40.509 | 41.000 | 39.641 | 47.137 | 55.407 |
| tls | deep&#45;8 | portable | 5000 | 39.088 | 41.946 | 3.602 | 0.428 | 41.529 | 42.620 | 40.800 | 48.863 | 57.959 |
| tls | deep&#45;8 | websocket | 5000 | 33.596 | 47.419 | 11.747 | 10.999 | 41.508 | 67.011 | 43.482 | 72.171 | 82.143 |
| tls | deep&#45;32 | cpp | 5000 | 58.375 | 65.578 | 4.630 | 0.301 | 65.268 | 65.967 | 63.871 | 74.783 | 85.289 |
| tls | deep&#45;32 | grpc | 5000 | 87.920 | 99.549 | 6.472 | 0.803 | 98.581 | 100.641 | 97.275 | 112.298 | 127.663 |
| tls | deep&#45;32 | legacy | 5000 | 43.919 | 47.707 | 4.193 | 0.483 | 47.161 | 48.402 | 46.248 | 56.126 | 65.312 |
| tls | deep&#45;32 | portable | 5000 | 45.521 | 49.079 | 4.322 | 0.299 | 48.695 | 49.522 | 47.425 | 57.814 | 67.890 |
| tls | deep&#45;32 | websocket | 5000 | 50.477 | 59.772 | 5.763 | 0.408 | 59.393 | 60.378 | 58.607 | 70.466 | 80.599 |
| tls | concurrent&#45;32 | cpp | 5000 | 441.651 | 540.746 | 142.968 | 60.236 | 492.704 | 624.274 | 496.116 | 991.105 | 1043.320 |
| tls | concurrent&#45;32 | grpc | 5000 | 552.647 | 680.475 | 144.409 | 143.284 | 611.437 | 936.721 | 616.044 | 1004.006 | 1034.999 |
| tls | concurrent&#45;32 | legacy | 5000 | 115.111 | 136.155 | 37.296 | 19.586 | 125.204 | 171.061 | 125.252 | 271.843 | 289.324 |
| tls | concurrent&#45;32 | portable | 5000 | 145.683 | 207.475 | 84.179 | 82.216 | 155.713 | 344.664 | 155.595 | 354.951 | 367.633 |
| tls | concurrent&#45;32 | websocket | 5000 | 109.072 | 191.306 | 48.500 | 38.044 | 164.157 | 249.628 | 172.869 | 286.251 | 312.098 |
| tls | deep&#45;32&#45;concurrent&#45;32 | cpp | 5000 | 618.713 | 896.135 | 314.057 | 284.554 | 668.805 | 1227.371 | 669.693 | 1367.761 | 1383.840 |
| tls | deep&#45;32&#45;concurrent&#45;32 | grpc | 5000 | 1395.076 | 2036.310 | 511.953 | 473.677 | 1550.299 | 2501.851 | 1891.197 | 2788.519 | 2821.965 |
| tls | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 392.500 | 520.081 | 174.886 | 141.080 | 418.267 | 741.033 | 421.550 | 849.887 | 862.530 |
| tls | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 412.291 | 503.954 | 126.928 | 120.055 | 446.996 | 718.687 | 450.892 | 816.184 | 834.270 |
| tls | deep&#45;32&#45;concurrent&#45;32 | websocket | 5000 | 664.128 | 999.871 | 240.678 | 220.034 | 719.638 | 1207.388 | 1158.286 | 1254.287 | 1279.090 |
| tls | services&#45;4&#45;deep&#45;16 | cpp | 5000 | 462.885 | 498.995 | 18.361 | 2.210 | 497.124 | 502.433 | 498.072 | 522.013 | 539.165 |
| tls | services&#45;4&#45;deep&#45;16 | grpc | 5000 | 562.897 | 626.074 | 40.335 | 7.696 | 618.731 | 638.841 | 625.575 | 652.718 | 668.606 |
| tls | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 314.688 | 359.205 | 13.443 | 2.173 | 356.175 | 361.887 | 355.575 | 382.482 | 393.999 |
| tls | services&#45;4&#45;deep&#45;16 | portable | 5000 | 368.090 | 393.671 | 14.697 | 1.305 | 392.335 | 395.640 | 389.721 | 418.052 | 430.833 |
| tls | services&#45;4&#45;deep&#45;16 | websocket | 5000 | 268.156 | 332.712 | 34.997 | 1.106 | 331.143 | 334.232 | 330.634 | 389.990 | 409.069 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | 5000 | 3038.617 | 3643.558 | 234.463 | 70.426 | 3551.972 | 3723.019 | 3663.690 | 4003.780 | 4199.042 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | 5000 | 4794.596 | 5754.274 | 328.382 | 54.735 | 5677.144 | 5830.251 | 5789.800 | 6241.436 | 6418.493 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1772.076 | 2072.189 | 118.202 | 45.279 | 2025.636 | 2123.438 | 2048.662 | 2278.198 | 2373.809 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1838.025 | 2125.125 | 124.928 | 47.444 | 2062.008 | 2183.987 | 2105.997 | 2333.627 | 2422.643 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | 5000 | 1625.904 | 1940.734 | 87.547 | 23.041 | 1919.846 | 1978.551 | 1940.241 | 2077.810 | 2148.819 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 2651.779 | 3376.740 | 451.273 | 165.300 | 3234.635 | 3580.510 | 3292.414 | 4046.212 | 4184.402 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 4515.432 | 5635.826 | 434.289 | 79.929 | 5506.702 | 5720.993 | 5678.818 | 6273.911 | 6496.044 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1748.240 | 2089.726 | 110.899 | 40.949 | 2024.927 | 2124.986 | 2081.545 | 2264.490 | 2383.052 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1831.588 | 2200.011 | 123.564 | 33.447 | 2174.908 | 2256.666 | 2194.441 | 2423.776 | 2462.778 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1405.189 | 1742.300 | 269.899 | 42.249 | 1719.423 | 1817.526 | 1727.636 | 1953.781 | 2078.391 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 4951.240 | 6763.411 | 907.248 | 138.514 | 6598.889 | 6945.596 | 6910.171 | 7907.403 | 8294.702 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 8147.470 | 10846.537 | 1013.615 | 152.906 | 10676.499 | 11058.880 | 11016.070 | 12225.849 | 12608.254 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 3776.725 | 4190.498 | 229.179 | 146.277 | 4038.443 | 4350.375 | 4099.704 | 4619.887 | 4791.926 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3735.352 | 4435.781 | 212.165 | 111.120 | 4257.494 | 4525.454 | 4510.779 | 4681.094 | 4769.268 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 2569.817 | 3267.750 | 253.569 | 15.544 | 3249.923 | 3287.772 | 3215.513 | 3706.569 | 4038.499 |
| quic | loopback&#45;0 | legacy | 5000 | 47.501 | 58.385 | 15.300 | 16.600 | 50.443 | 88.070 | 50.369 | 87.378 | 99.478 |
| quic | loopback&#45;0 | portable | 5000 | 52.740 | 73.872 | 21.984 | 24.063 | 56.252 | 100.262 | 56.668 | 107.501 | 114.249 |
| quic | loopback&#45;64 | legacy | 5000 | 48.908 | 80.773 | 15.591 | 16.742 | 50.827 | 88.573 | 86.420 | 98.397 | 103.552 |
| quic | loopback&#45;64 | portable | 5000 | 53.909 | 82.889 | 21.324 | 23.254 | 56.906 | 100.708 | 96.856 | 109.347 | 115.098 |
| quic | loopback&#45;1024 | legacy | 5000 | 50.055 | 53.943 | 3.983 | 1.032 | 52.905 | 55.521 | 52.847 | 61.820 | 70.486 |
| quic | loopback&#45;1024 | portable | 5000 | 55.289 | 59.132 | 3.325 | 0.408 | 58.728 | 59.621 | 58.100 | 67.082 | 73.460 |
| quic | loopback&#45;65536 | legacy | 5000 | 197.069 | 217.166 | 8.917 | 0.960 | 215.731 | 218.249 | 213.640 | 235.473 | 245.663 |
| quic | loopback&#45;65536 | portable | 5000 | 212.106 | 226.160 | 8.929 | 1.118 | 224.871 | 227.929 | 222.685 | 244.805 | 254.025 |
| quic | unary&#45;empty | legacy | 5000 | 48.225 | 83.640 | 13.488 | 10.889 | 64.176 | 89.224 | 86.619 | 99.625 | 105.024 |
| quic | unary&#45;empty | portable | 5000 | 90.229 | 100.067 | 5.253 | 0.474 | 99.294 | 100.463 | 98.338 | 112.197 | 116.619 |
| quic | unary&#45;small | legacy | 5000 | 79.662 | 88.527 | 4.760 | 0.267 | 88.284 | 88.984 | 86.857 | 100.066 | 104.960 |
| quic | unary&#45;small | portable | 5000 | 90.707 | 100.226 | 4.538 | 0.324 | 99.900 | 100.688 | 98.511 | 111.417 | 115.932 |
| quic | unary&#45;1k | legacy | 5000 | 49.704 | 53.687 | 3.650 | 0.833 | 52.617 | 54.837 | 52.580 | 61.529 | 69.048 |
| quic | unary&#45;1k | portable | 5000 | 55.582 | 59.212 | 3.391 | 0.166 | 58.972 | 59.390 | 58.148 | 67.030 | 73.720 |
| quic | fanout&#45;32 | legacy | 5000 | 194.737 | 219.886 | 11.028 | 1.067 | 218.228 | 220.821 | 215.604 | 242.299 | 260.207 |
| quic | fanout&#45;32 | portable | 5000 | 230.709 | 255.226 | 8.265 | 0.995 | 253.859 | 256.483 | 252.535 | 266.510 | 279.523 |
| quic | dependent&#45;read | legacy | 5000 | 104.429 | 111.091 | 4.906 | 0.442 | 110.687 | 111.833 | 109.444 | 120.403 | 127.781 |
| quic | dependent&#45;read | portable | 5000 | 131.555 | 145.677 | 6.397 | 1.027 | 144.608 | 146.922 | 143.236 | 157.929 | 169.593 |
| quic | pipelined&#45;read | legacy | 5000 | 61.449 | 65.687 | 3.829 | 0.524 | 65.248 | 66.548 | 64.398 | 74.590 | 81.313 |
| quic | pipelined&#45;read | portable | 5000 | 94.191 | 99.987 | 5.025 | 0.889 | 99.043 | 101.410 | 98.279 | 110.459 | 117.241 |
| quic | write&#45;read | legacy | 5000 | 97.874 | 104.224 | 4.968 | 0.340 | 103.790 | 104.621 | 102.439 | 114.326 | 123.451 |
| quic | write&#45;read | portable | 5000 | 108.856 | 116.488 | 5.601 | 0.796 | 115.426 | 117.462 | 114.686 | 127.391 | 137.756 |
| quic | callback | legacy | 5000 | 99.948 | 110.328 | 5.412 | 0.888 | 109.399 | 111.534 | 108.548 | 120.808 | 130.093 |
| quic | callback | portable | 5000 | 130.157 | 138.788 | 6.665 | 0.571 | 137.963 | 139.482 | 135.996 | 153.194 | 162.918 |
| quic | bulk&#45;64k | legacy | 5000 | 203.112 | 243.460 | 52.010 | 54.922 | 216.564 | 341.491 | 218.104 | 354.066 | 361.581 |
| quic | bulk&#45;64k | portable | 5000 | 209.983 | 225.533 | 23.445 | 7.056 | 221.911 | 238.139 | 219.349 | 243.230 | 371.322 |
| quic | bulk&#45;256k | legacy | 5000 | 502.287 | 558.727 | 66.935 | 35.343 | 541.376 | 621.848 | 542.100 | 815.964 | 833.200 |
| quic | bulk&#45;256k | portable | 5000 | 517.663 | 608.102 | 106.034 | 57.372 | 561.356 | 680.768 | 565.612 | 866.028 | 877.411 |
| quic | bulk&#45;1m | legacy | 5000 | 1879.363 | 2280.537 | 557.534 | 244.197 | 1950.592 | 2604.131 | 1964.643 | 2794.984 | 2841.068 |
| quic | bulk&#45;1m | portable | 5000 | 1895.218 | 2055.692 | 466.213 | 114.761 | 1972.067 | 2239.660 | 1962.363 | 2750.020 | 2803.154 |
| quic | bulk&#45;4m | legacy | 5000 | 8873.087 | 9580.255 | 1042.368 | 348.625 | 9196.479 | 10024.984 | 9233.400 | 12655.386 | 13034.065 |
| quic | bulk&#45;4m | portable | 5000 | 8912.232 | 9925.642 | 1267.555 | 647.366 | 9318.018 | 10712.333 | 9385.425 | 12825.717 | 13248.713 |
| quic | deep&#45;8 | legacy | 5000 | 51.168 | 54.293 | 4.027 | 0.300 | 54.001 | 54.647 | 53.155 | 62.193 | 73.891 |
| quic | deep&#45;8 | portable | 5000 | 56.403 | 60.675 | 4.718 | 0.310 | 60.204 | 60.996 | 59.324 | 70.346 | 82.314 |
| quic | deep&#45;32 | legacy | 5000 | 56.192 | 62.051 | 4.579 | 0.160 | 61.855 | 62.275 | 60.735 | 71.146 | 81.792 |
| quic | deep&#45;32 | portable | 5000 | 63.703 | 68.325 | 4.611 | 0.631 | 67.548 | 68.918 | 66.989 | 78.007 | 89.455 |
| quic | concurrent&#45;32 | legacy | 5000 | 136.574 | 144.467 | 5.785 | 0.637 | 143.861 | 145.507 | 142.331 | 156.115 | 165.434 |
| quic | concurrent&#45;32 | portable | 5000 | 159.470 | 178.913 | 9.349 | 1.473 | 176.743 | 180.494 | 175.516 | 197.228 | 210.060 |
| quic | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 436.583 | 473.201 | 60.792 | 17.068 | 460.573 | 503.208 | 462.075 | 490.663 | 890.897 |
| quic | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 455.159 | 576.700 | 147.755 | 92.954 | 496.565 | 712.970 | 500.231 | 878.788 | 892.391 |
| quic | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 480.885 | 533.340 | 15.741 | 4.029 | 526.907 | 536.794 | 530.980 | 558.777 | 574.449 |
| quic | services&#45;4&#45;deep&#45;16 | portable | 5000 | 465.766 | 513.766 | 39.483 | 12.030 | 501.817 | 528.702 | 512.052 | 543.866 | 559.280 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 2146.013 | 2481.252 | 104.621 | 42.435 | 2424.273 | 2521.006 | 2488.584 | 2644.648 | 2754.683 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 2179.523 | 2485.871 | 122.022 | 70.170 | 2404.943 | 2558.580 | 2492.915 | 2681.984 | 2779.261 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2117.404 | 2525.716 | 152.187 | 32.213 | 2489.517 | 2577.178 | 2511.491 | 2770.418 | 2890.512 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1930.319 | 2423.484 | 195.882 | 87.702 | 2309.026 | 2505.303 | 2490.403 | 2673.453 | 2781.823 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 3847.948 | 5021.294 | 273.952 | 101.784 | 4941.778 | 5142.631 | 5022.857 | 5426.010 | 5615.020 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3597.330 | 4455.971 | 474.615 | 273.966 | 4056.918 | 4717.532 | 4391.616 | 5161.502 | 5450.284 |

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


