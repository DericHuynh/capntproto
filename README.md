# Capntproto verification reports

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-28T16:38:24Z · run 40727941577 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40727941577) · commit `2f0e2ccb4611` · **failure**

![Cargo test results](docs/reports/cargo-history.svg)

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-28T16:38:24Z · run 40727941577 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40727941577) · commit `2f0e2ccb4611` · **failure**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-28T16:38:24Z · run 40727941577 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40727941577) · commit `2f0e2ccb4611` · **failure**

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

Latest coverage run: [2026-10-28T15:00:10Z · run 40711514398 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40711514398) · commit `d45a9285b0db` · **failure**

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-28T16:38:24Z · run 40727941577 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40727941577) · commit `2f0e2ccb4611` · **failure**

Measurement evidence: **passed**. Independently validated encrypted benchmark values&#46; Workflow status and diagnostic completion are separate&#59; unavailable values remain blank&#46; Operations include whole concurrent batches and every service hop&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Current TCP fixtures use TLS 1.3, including upstream Rust Cap’n Proto, C++, gRPC over HTTP/2 and WebSockets over WSS; native encrypted transports and standard QUIC are identified separately. Historical data retains its recorded fixture contract. The dedicated campaign also reports recursive data, concurrent calls and independent service chains for gRPC and binary Protobuf WebSockets in its encrypted comparison artifact. Bars compare the recorded workload, rather than universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.


## Exhaustive benchmark tables

Measured run: [2026-10-28T16:38:24Z · run 40727941577 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40727941577) · commit `2f0e2ccb4611` · **failure**

All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.

### All encrypted latency comparisons

| Transport | Case | Implementation | Samples | Minimum µs | Mean µs | Sample SD µs | Run mean SD µs | Run mean min µs | Run mean max µs | p50 µs | p95 µs | p99 µs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tls | loopback&#45;0 | cpp | 5000 | 44.421 | 57.376 | 13.362 | 13.004 | 50.684 | 80.618 | 51.015 | 82.192 | 96.042 |
| tls | loopback&#45;0 | grpc | 5000 | 52.400 | 64.314 | 13.445 | 8.217 | 58.948 | 78.845 | 59.534 | 95.938 | 107.871 |
| tls | loopback&#45;0 | legacy | 5000 | 29.436 | 37.274 | 9.093 | 7.571 | 33.410 | 50.784 | 33.398 | 51.826 | 63.686 |
| tls | loopback&#45;0 | portable | 5000 | 30.573 | 39.012 | 9.540 | 8.690 | 34.981 | 54.555 | 34.637 | 55.691 | 68.756 |
| tls | loopback&#45;0 | websocket | 5000 | 23.152 | 31.825 | 4.171 | 0.401 | 31.348 | 32.434 | 31.027 | 39.877 | 49.859 |
| tls | loopback&#45;64 | cpp | 5000 | 44.790 | 51.958 | 5.405 | 0.683 | 51.072 | 52.846 | 50.785 | 63.698 | 73.147 |
| tls | loopback&#45;64 | grpc | 5000 | 52.350 | 65.780 | 14.355 | 11.064 | 59.383 | 85.513 | 60.058 | 96.638 | 109.249 |
| tls | loopback&#45;64 | legacy | 5000 | 29.829 | 34.517 | 4.386 | 0.631 | 33.711 | 35.216 | 33.545 | 43.137 | 53.226 |
| tls | loopback&#45;64 | portable | 5000 | 30.241 | 35.113 | 5.388 | 1.033 | 34.102 | 36.830 | 34.060 | 44.367 | 54.123 |
| tls | loopback&#45;64 | websocket | 5000 | 24.029 | 30.222 | 4.687 | 0.237 | 29.926 | 30.435 | 29.338 | 38.927 | 48.032 |
| tls | loopback&#45;1024 | cpp | 5000 | 46.403 | 53.631 | 7.129 | 1.445 | 52.008 | 54.945 | 52.332 | 66.414 | 77.814 |
| tls | loopback&#45;1024 | grpc | 5000 | 56.137 | 65.206 | 7.498 | 1.068 | 63.725 | 66.266 | 63.164 | 80.860 | 93.230 |
| tls | loopback&#45;1024 | legacy | 5000 | 31.498 | 37.002 | 5.088 | 0.529 | 36.244 | 37.517 | 36.115 | 46.819 | 56.351 |
| tls | loopback&#45;1024 | portable | 5000 | 32.647 | 37.970 | 5.151 | 0.533 | 37.441 | 38.753 | 36.873 | 47.593 | 58.474 |
| tls | loopback&#45;1024 | websocket | 5000 | 23.640 | 31.467 | 7.631 | 3.096 | 29.201 | 36.063 | 28.969 | 45.837 | 56.932 |
| tls | loopback&#45;65536 | cpp | 5000 | 199.268 | 228.991 | 21.888 | 5.015 | 220.571 | 234.038 | 225.745 | 259.861 | 338.137 |
| tls | loopback&#45;65536 | grpc | 5000 | 202.527 | 238.806 | 18.859 | 3.722 | 235.890 | 245.199 | 235.854 | 272.812 | 304.608 |
| tls | loopback&#45;65536 | legacy | 5000 | 146.341 | 192.204 | 41.778 | 40.237 | 162.595 | 247.348 | 168.126 | 261.541 | 277.195 |
| tls | loopback&#45;65536 | portable | 5000 | 151.385 | 185.427 | 35.091 | 32.958 | 170.324 | 244.383 | 170.500 | 263.355 | 282.045 |
| tls | loopback&#45;65536 | websocket | 5000 | 164.824 | 206.040 | 41.633 | 29.579 | 184.495 | 249.090 | 186.779 | 290.016 | 312.562 |
| tls | unary&#45;empty | cpp | 5000 | 44.444 | 56.982 | 13.573 | 13.630 | 50.471 | 81.354 | 50.655 | 82.556 | 95.570 |
| tls | unary&#45;empty | grpc | 5000 | 52.455 | 74.654 | 19.101 | 19.868 | 59.332 | 96.519 | 62.404 | 104.543 | 117.187 |
| tls | unary&#45;empty | legacy | 5000 | 29.339 | 34.278 | 4.873 | 1.019 | 32.614 | 35.180 | 33.203 | 44.420 | 53.239 |
| tls | unary&#45;empty | portable | 5000 | 30.536 | 37.829 | 8.981 | 5.174 | 34.324 | 46.980 | 34.575 | 55.682 | 67.799 |
| tls | unary&#45;empty | websocket | 5000 | 25.595 | 34.312 | 6.908 | 4.478 | 32.067 | 42.313 | 31.730 | 48.081 | 56.890 |
| tls | unary&#45;small | cpp | 5000 | 44.950 | 52.317 | 5.814 | 1.134 | 50.636 | 53.648 | 51.050 | 64.087 | 72.790 |
| tls | unary&#45;small | grpc | 5000 | 53.511 | 61.452 | 6.382 | 0.872 | 60.520 | 62.681 | 59.815 | 74.978 | 86.806 |
| tls | unary&#45;small | legacy | 5000 | 29.663 | 34.374 | 4.648 | 0.450 | 33.690 | 34.792 | 33.168 | 43.330 | 53.957 |
| tls | unary&#45;small | portable | 5000 | 30.579 | 37.350 | 7.296 | 3.093 | 34.920 | 42.766 | 35.012 | 53.466 | 61.272 |
| tls | unary&#45;small | websocket | 5000 | 24.325 | 30.625 | 4.427 | 0.161 | 30.419 | 30.768 | 29.880 | 39.390 | 48.762 |
| tls | unary&#45;1k | cpp | 5000 | 46.883 | 53.900 | 5.791 | 0.442 | 53.371 | 54.477 | 52.291 | 66.466 | 76.720 |
| tls | unary&#45;1k | grpc | 5000 | 56.855 | 65.852 | 6.910 | 0.967 | 64.539 | 66.955 | 64.133 | 80.607 | 91.350 |
| tls | unary&#45;1k | legacy | 5000 | 31.026 | 36.501 | 4.795 | 0.435 | 36.031 | 36.931 | 35.335 | 46.117 | 55.207 |
| tls | unary&#45;1k | portable | 5000 | 32.631 | 37.966 | 4.753 | 0.550 | 37.048 | 38.484 | 36.719 | 48.222 | 58.122 |
| tls | unary&#45;1k | websocket | 5000 | 23.998 | 30.205 | 4.581 | 0.661 | 29.450 | 30.807 | 29.158 | 38.318 | 47.285 |
| tls | fanout&#45;32 | cpp | 5000 | 398.626 | 476.410 | 58.194 | 22.417 | 452.383 | 502.101 | 462.286 | 577.996 | 741.838 |
| tls | fanout&#45;32 | grpc | 5000 | 673.430 | 765.749 | 50.048 | 10.610 | 756.092 | 783.917 | 761.242 | 815.326 | 1029.150 |
| tls | fanout&#45;32 | legacy | 5000 | 156.656 | 178.176 | 17.517 | 1.994 | 175.996 | 181.066 | 175.859 | 201.799 | 275.432 |
| tls | fanout&#45;32 | portable | 5000 | 184.569 | 212.216 | 22.985 | 8.372 | 205.093 | 226.626 | 208.772 | 236.901 | 347.032 |
| tls | fanout&#45;32 | websocket | 5000 | 188.030 | 223.227 | 17.044 | 4.552 | 217.803 | 228.708 | 220.988 | 253.371 | 271.990 |
| tls | dependent&#45;read | cpp | 5000 | 104.870 | 122.098 | 10.016 | 1.557 | 120.050 | 124.165 | 120.261 | 142.321 | 154.020 |
| tls | dependent&#45;read | grpc | 5000 | 110.967 | 127.930 | 9.943 | 1.795 | 125.406 | 130.071 | 125.976 | 148.104 | 160.921 |
| tls | dependent&#45;read | legacy | 5000 | 65.226 | 75.308 | 6.915 | 1.430 | 73.072 | 77.017 | 73.836 | 89.935 | 100.419 |
| tls | dependent&#45;read | portable | 5000 | 82.530 | 96.829 | 10.141 | 2.671 | 94.735 | 101.228 | 94.213 | 118.321 | 132.921 |
| tls | dependent&#45;read | websocket | 5000 | 46.414 | 58.469 | 6.844 | 0.970 | 57.462 | 59.959 | 57.196 | 71.164 | 81.539 |
| tls | pipelined&#45;read | cpp | 5000 | 76.259 | 99.610 | 23.711 | 24.077 | 87.535 | 142.619 | 88.728 | 146.627 | 165.365 |
| tls | pipelined&#45;read | legacy | 5000 | 40.849 | 62.295 | 14.450 | 13.111 | 46.801 | 72.653 | 66.558 | 83.620 | 97.979 |
| tls | pipelined&#45;read | portable | 5000 | 61.226 | 90.447 | 26.655 | 26.982 | 69.756 | 120.649 | 74.701 | 130.317 | 148.166 |
| tls | write&#45;read | cpp | 5000 | 90.744 | 108.161 | 17.679 | 9.350 | 102.852 | 124.839 | 102.835 | 156.180 | 174.685 |
| tls | write&#45;read | grpc | 5000 | 111.261 | 127.403 | 9.862 | 2.169 | 125.729 | 131.182 | 125.395 | 147.520 | 160.392 |
| tls | write&#45;read | legacy | 5000 | 59.980 | 76.440 | 15.902 | 15.899 | 67.412 | 104.743 | 69.304 | 107.388 | 122.053 |
| tls | write&#45;read | portable | 5000 | 62.543 | 71.512 | 6.773 | 0.630 | 70.543 | 72.050 | 69.567 | 85.406 | 96.593 |
| tls | write&#45;read | websocket | 5000 | 46.209 | 57.421 | 7.331 | 1.402 | 55.564 | 59.437 | 56.058 | 69.989 | 79.283 |
| tls | callback | cpp | 5000 | 96.472 | 110.896 | 14.785 | 2.256 | 108.587 | 113.378 | 109.267 | 129.389 | 142.971 |
| tls | callback | grpc | 5000 | 54.886 | 63.365 | 6.537 | 1.559 | 61.410 | 64.942 | 62.124 | 77.068 | 88.083 |
| tls | callback | legacy | 5000 | 64.669 | 74.420 | 7.200 | 0.812 | 73.653 | 75.317 | 72.789 | 88.936 | 102.649 |
| tls | callback | portable | 5000 | 80.909 | 92.168 | 9.112 | 1.463 | 90.595 | 93.860 | 90.107 | 109.429 | 123.921 |
| tls | callback | websocket | 5000 | 46.563 | 57.730 | 6.738 | 0.585 | 56.901 | 58.398 | 56.134 | 70.273 | 81.464 |
| tls | bulk&#45;64k | cpp | 5000 | 198.723 | 225.245 | 14.480 | 2.080 | 221.970 | 227.195 | 224.066 | 253.002 | 269.228 |
| tls | bulk&#45;64k | grpc | 5000 | 205.743 | 248.518 | 34.224 | 21.030 | 236.155 | 285.901 | 236.911 | 333.069 | 366.434 |
| tls | bulk&#45;64k | legacy | 5000 | 147.114 | 165.953 | 10.551 | 1.480 | 163.779 | 167.831 | 164.843 | 186.958 | 200.792 |
| tls | bulk&#45;64k | portable | 5000 | 150.995 | 173.699 | 21.381 | 8.543 | 166.756 | 188.308 | 169.123 | 209.561 | 265.021 |
| tls | bulk&#45;64k | websocket | 5000 | 164.181 | 211.489 | 44.493 | 34.898 | 184.172 | 251.291 | 188.845 | 292.152 | 312.846 |
| tls | bulk&#45;256k | cpp | 5000 | 770.972 | 886.550 | 96.131 | 38.524 | 856.240 | 943.141 | 863.937 | 1177.102 | 1252.361 |
| tls | bulk&#45;256k | grpc | 5000 | 827.734 | 1115.408 | 214.084 | 119.291 | 986.818 | 1275.935 | 998.246 | 1485.061 | 1535.328 |
| tls | bulk&#45;256k | legacy | 5000 | 453.415 | 533.026 | 75.778 | 81.272 | 492.285 | 678.262 | 502.761 | 700.341 | 718.799 |
| tls | bulk&#45;256k | portable | 5000 | 449.775 | 505.837 | 32.425 | 9.120 | 498.437 | 521.462 | 501.576 | 543.841 | 676.898 |
| tls | bulk&#45;256k | websocket | 5000 | 946.486 | 1052.169 | 53.595 | 10.130 | 1044.790 | 1068.870 | 1041.346 | 1155.618 | 1185.813 |
| tls | bulk&#45;1m | cpp | 5000 | 3059.371 | 3475.382 | 422.852 | 353.950 | 3293.189 | 4107.766 | 3326.197 | 4575.038 | 4737.888 |
| tls | bulk&#45;1m | grpc | 5000 | 2351.167 | 4555.195 | 788.980 | 752.912 | 3213.689 | 4995.623 | 4661.428 | 5678.145 | 6077.937 |
| tls | bulk&#45;1m | legacy | 5000 | 1524.923 | 1685.989 | 120.869 | 39.156 | 1648.569 | 1742.422 | 1658.163 | 2049.690 | 2135.117 |
| tls | bulk&#45;1m | portable | 5000 | 1533.440 | 1668.421 | 71.971 | 10.207 | 1654.283 | 1680.425 | 1663.763 | 1736.188 | 2063.642 |
| tls | bulk&#45;1m | websocket | 5000 | 3827.482 | 4399.509 | 315.557 | 163.699 | 4253.451 | 4676.077 | 4320.309 | 5096.410 | 5491.082 |
| tls | bulk&#45;4m | cpp | 5000 | 14096.234 | 15490.630 | 762.435 | 192.208 | 15349.239 | 15823.597 | 15393.174 | 16137.546 | 20115.523 |
| tls | bulk&#45;4m | grpc | 5000 | 18184.795 | 22701.096 | 2745.534 | 2592.929 | 19627.590 | 25078.297 | 23142.558 | 27332.383 | 27881.194 |
| tls | bulk&#45;4m | legacy | 5000 | 7813.585 | 8565.679 | 368.229 | 20.306 | 8536.476 | 8587.151 | 8527.085 | 8897.365 | 10521.206 |
| tls | bulk&#45;4m | portable | 5000 | 7776.793 | 8722.898 | 494.926 | 192.880 | 8526.276 | 9041.300 | 8631.968 | 10213.352 | 10705.018 |
| tls | bulk&#45;4m | websocket | 5000 | 26485.710 | 28312.719 | 1957.419 | 409.440 | 27806.955 | 28819.026 | 27640.607 | 33479.911 | 33871.997 |
| tls | deep&#45;8 | cpp | 5000 | 47.632 | 56.806 | 9.790 | 4.739 | 53.747 | 65.216 | 53.896 | 82.400 | 93.319 |
| tls | deep&#45;8 | grpc | 5000 | 63.768 | 73.115 | 7.517 | 0.739 | 72.156 | 74.204 | 71.489 | 88.188 | 101.610 |
| tls | deep&#45;8 | legacy | 5000 | 31.403 | 36.899 | 5.439 | 0.639 | 35.819 | 37.373 | 35.757 | 46.786 | 56.105 |
| tls | deep&#45;8 | portable | 5000 | 32.516 | 42.243 | 10.234 | 10.033 | 37.383 | 60.185 | 37.273 | 61.579 | 71.821 |
| tls | deep&#45;8 | websocket | 5000 | 28.090 | 38.515 | 6.434 | 0.615 | 37.862 | 39.302 | 37.297 | 50.267 | 59.222 |
| tls | deep&#45;32 | cpp | 5000 | 53.398 | 61.105 | 6.734 | 1.583 | 58.786 | 63.133 | 59.898 | 74.464 | 87.554 |
| tls | deep&#45;32 | grpc | 5000 | 80.838 | 92.098 | 8.428 | 1.636 | 89.418 | 93.802 | 89.798 | 109.221 | 122.589 |
| tls | deep&#45;32 | legacy | 5000 | 38.828 | 44.375 | 5.017 | 0.884 | 42.920 | 45.237 | 42.808 | 55.139 | 63.635 |
| tls | deep&#45;32 | portable | 5000 | 38.877 | 44.949 | 6.104 | 0.349 | 44.600 | 45.445 | 43.625 | 56.474 | 66.188 |
| tls | deep&#45;32 | websocket | 5000 | 46.408 | 57.995 | 7.129 | 1.564 | 56.221 | 60.408 | 56.533 | 71.551 | 81.763 |
| tls | concurrent&#45;32 | cpp | 5000 | 444.569 | 512.204 | 32.019 | 16.347 | 500.143 | 540.227 | 509.121 | 576.528 | 601.565 |
| tls | concurrent&#45;32 | grpc | 5000 | 548.902 | 663.970 | 61.533 | 43.349 | 616.356 | 734.932 | 645.407 | 758.325 | 777.828 |
| tls | concurrent&#45;32 | legacy | 5000 | 107.305 | 136.849 | 30.516 | 28.734 | 122.370 | 188.212 | 124.374 | 207.243 | 222.636 |
| tls | concurrent&#45;32 | portable | 5000 | 132.406 | 162.257 | 32.698 | 23.543 | 149.715 | 204.301 | 151.714 | 250.580 | 271.615 |
| tls | concurrent&#45;32 | websocket | 5000 | 102.757 | 178.595 | 40.502 | 38.727 | 148.625 | 223.914 | 167.901 | 248.063 | 274.070 |
| tls | deep&#45;32&#45;concurrent&#45;32 | cpp | 5000 | 593.058 | 803.278 | 142.965 | 119.795 | 676.911 | 939.093 | 725.227 | 1024.868 | 1061.344 |
| tls | deep&#45;32&#45;concurrent&#45;32 | grpc | 5000 | 1368.407 | 1595.280 | 123.378 | 113.925 | 1525.279 | 1795.981 | 1565.032 | 1852.042 | 1906.774 |
| tls | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 396.740 | 461.291 | 35.922 | 30.568 | 442.543 | 515.613 | 454.841 | 532.544 | 564.321 |
| tls | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 400.728 | 478.592 | 38.192 | 25.094 | 457.098 | 508.413 | 471.619 | 546.344 | 574.094 |
| tls | deep&#45;32&#45;concurrent&#45;32 | websocket | 5000 | 656.317 | 826.962 | 133.784 | 88.378 | 756.142 | 961.535 | 767.221 | 1078.771 | 1113.006 |
| tls | services&#45;4&#45;deep&#45;16 | cpp | 5000 | 334.984 | 400.447 | 22.980 | 7.319 | 387.361 | 403.968 | 400.214 | 436.947 | 456.000 |
| tls | services&#45;4&#45;deep&#45;16 | grpc | 5000 | 491.088 | 563.004 | 27.468 | 5.158 | 553.909 | 566.094 | 562.495 | 608.535 | 629.639 |
| tls | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 254.329 | 296.846 | 21.243 | 4.916 | 288.995 | 301.876 | 295.783 | 328.223 | 342.651 |
| tls | services&#45;4&#45;deep&#45;16 | portable | 5000 | 275.951 | 328.450 | 21.059 | 5.045 | 320.972 | 333.763 | 327.920 | 363.469 | 379.244 |
| tls | services&#45;4&#45;deep&#45;16 | websocket | 5000 | 201.376 | 284.285 | 37.280 | 2.797 | 280.021 | 287.439 | 281.179 | 349.119 | 372.921 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | 5000 | 2498.493 | 2783.018 | 97.769 | 27.813 | 2754.212 | 2825.677 | 2777.081 | 2937.354 | 3053.191 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | 5000 | 4264.194 | 4699.951 | 167.994 | 63.435 | 4637.131 | 4805.801 | 4684.440 | 4984.795 | 5131.601 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1482.140 | 1639.942 | 75.126 | 11.413 | 1628.513 | 1656.336 | 1640.176 | 1717.482 | 1861.286 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1562.499 | 1728.411 | 76.827 | 7.202 | 1719.929 | 1737.864 | 1728.803 | 1809.452 | 1868.770 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | 5000 | 1334.838 | 1597.646 | 92.139 | 36.371 | 1548.879 | 1640.291 | 1590.786 | 1756.543 | 1832.679 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 2111.285 | 2655.698 | 124.018 | 26.890 | 2613.792 | 2682.450 | 2668.822 | 2828.574 | 2928.323 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 3667.948 | 4496.524 | 233.806 | 67.020 | 4384.286 | 4548.106 | 4478.593 | 4890.679 | 5023.581 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1199.910 | 1676.032 | 88.369 | 15.959 | 1654.778 | 1691.814 | 1676.447 | 1767.808 | 1801.902 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1369.135 | 1754.787 | 93.800 | 19.798 | 1731.542 | 1775.280 | 1751.508 | 1921.903 | 1971.544 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1179.513 | 1560.582 | 140.814 | 55.654 | 1486.841 | 1609.180 | 1548.412 | 1802.675 | 1902.046 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 4377.848 | 5530.025 | 246.307 | 75.536 | 5442.177 | 5610.775 | 5501.198 | 5977.869 | 6189.983 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 7005.689 | 9087.963 | 611.431 | 118.106 | 8953.367 | 9258.487 | 9165.635 | 9965.471 | 10257.327 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 3111.110 | 3466.008 | 122.554 | 55.626 | 3393.251 | 3521.156 | 3462.972 | 3658.441 | 3780.647 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3019.311 | 3564.702 | 242.556 | 122.754 | 3423.918 | 3688.946 | 3563.789 | 3932.547 | 4052.125 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 2289.793 | 2905.355 | 224.968 | 26.905 | 2872.593 | 2934.472 | 2873.003 | 3331.987 | 3571.243 |
| quic | loopback&#45;0 | legacy | 5000 | 39.190 | 55.281 | 13.946 | 13.383 | 45.055 | 70.172 | 46.982 | 76.905 | 91.508 |
| quic | loopback&#45;0 | portable | 5000 | 43.188 | 50.414 | 6.265 | 1.031 | 49.219 | 51.879 | 49.014 | 62.935 | 76.174 |
| quic | loopback&#45;64 | legacy | 5000 | 39.979 | 60.301 | 13.519 | 12.387 | 45.528 | 70.073 | 64.485 | 78.845 | 91.389 |
| quic | loopback&#45;64 | portable | 5000 | 43.074 | 67.307 | 16.781 | 16.750 | 48.170 | 79.890 | 73.566 | 90.692 | 105.990 |
| quic | loopback&#45;1024 | legacy | 5000 | 41.785 | 64.621 | 14.332 | 13.219 | 48.647 | 75.146 | 69.165 | 84.854 | 100.257 |
| quic | loopback&#45;1024 | portable | 5000 | 46.421 | 72.528 | 17.748 | 17.956 | 52.459 | 85.954 | 80.910 | 97.289 | 112.413 |
| quic | loopback&#45;65536 | legacy | 5000 | 187.616 | 216.266 | 28.801 | 11.421 | 209.458 | 236.356 | 208.445 | 274.114 | 335.316 |
| quic | loopback&#45;65536 | portable | 5000 | 191.733 | 214.356 | 14.719 | 2.578 | 211.665 | 218.019 | 211.916 | 244.813 | 260.451 |
| quic | unary&#45;empty | legacy | 5000 | 39.504 | 45.554 | 5.736 | 1.415 | 44.112 | 47.496 | 44.120 | 57.480 | 68.414 |
| quic | unary&#45;empty | portable | 5000 | 43.148 | 49.366 | 6.089 | 0.590 | 48.656 | 50.264 | 47.782 | 62.126 | 73.612 |
| quic | unary&#45;small | legacy | 5000 | 39.377 | 45.740 | 6.260 | 0.795 | 44.712 | 46.837 | 44.389 | 57.362 | 68.749 |
| quic | unary&#45;small | portable | 5000 | 43.531 | 50.094 | 6.175 | 0.742 | 49.181 | 51.097 | 48.567 | 62.619 | 73.763 |
| quic | unary&#45;1k | legacy | 5000 | 41.233 | 47.686 | 6.904 | 1.337 | 46.032 | 49.688 | 46.068 | 59.714 | 72.541 |
| quic | unary&#45;1k | portable | 5000 | 45.992 | 51.980 | 5.802 | 0.911 | 50.909 | 53.347 | 50.253 | 64.119 | 76.857 |
| quic | fanout&#45;32 | legacy | 5000 | 187.196 | 210.959 | 16.401 | 2.116 | 208.993 | 213.693 | 209.185 | 241.115 | 259.162 |
| quic | fanout&#45;32 | portable | 5000 | 215.612 | 241.976 | 17.743 | 1.413 | 240.128 | 243.268 | 239.320 | 270.818 | 291.255 |
| quic | dependent&#45;read | legacy | 5000 | 87.494 | 101.931 | 9.915 | 1.427 | 100.762 | 103.678 | 99.878 | 119.935 | 139.729 |
| quic | dependent&#45;read | portable | 5000 | 115.787 | 132.723 | 12.211 | 1.255 | 130.590 | 133.917 | 130.497 | 156.096 | 179.649 |
| quic | pipelined&#45;read | legacy | 5000 | 52.399 | 60.577 | 7.244 | 1.119 | 58.900 | 61.771 | 58.848 | 74.113 | 90.911 |
| quic | pipelined&#45;read | portable | 5000 | 81.752 | 93.146 | 9.204 | 0.925 | 91.703 | 94.095 | 91.305 | 109.846 | 129.288 |
| quic | write&#45;read | legacy | 5000 | 82.546 | 95.827 | 9.344 | 1.108 | 94.619 | 97.167 | 93.879 | 112.464 | 132.983 |
| quic | write&#45;read | portable | 5000 | 89.906 | 103.350 | 9.443 | 0.488 | 102.786 | 103.847 | 101.217 | 120.401 | 140.232 |
| quic | callback | legacy | 5000 | 86.415 | 100.666 | 10.969 | 1.231 | 98.948 | 102.351 | 98.376 | 118.367 | 140.979 |
| quic | callback | portable | 5000 | 110.452 | 132.481 | 24.614 | 13.461 | 124.877 | 156.367 | 124.740 | 193.807 | 217.358 |
| quic | bulk&#45;64k | legacy | 5000 | 187.422 | 216.230 | 28.264 | 9.947 | 207.779 | 233.501 | 208.992 | 261.630 | 339.602 |
| quic | bulk&#45;64k | portable | 5000 | 188.873 | 222.720 | 35.265 | 20.580 | 212.305 | 259.482 | 211.259 | 318.939 | 350.552 |
| quic | bulk&#45;256k | legacy | 5000 | 479.895 | 528.058 | 22.341 | 5.190 | 522.661 | 536.256 | 526.481 | 565.813 | 584.926 |
| quic | bulk&#45;256k | portable | 5000 | 498.120 | 549.299 | 22.009 | 4.124 | 544.397 | 554.631 | 547.551 | 587.396 | 604.714 |
| quic | bulk&#45;1m | legacy | 5000 | 1765.097 | 2008.400 | 469.061 | 111.187 | 1926.505 | 2183.171 | 1944.714 | 2688.598 | 2857.757 |
| quic | bulk&#45;1m | portable | 5000 | 1777.891 | 2068.181 | 482.374 | 175.876 | 1934.213 | 2357.255 | 1959.893 | 2725.265 | 2824.978 |
| quic | bulk&#45;4m | legacy | 5000 | 9122.576 | 10242.599 | 939.229 | 267.433 | 9887.991 | 10520.093 | 10002.391 | 13259.089 | 13739.463 |
| quic | bulk&#45;4m | portable | 5000 | 9423.430 | 10767.126 | 1129.728 | 641.113 | 10324.275 | 11897.357 | 10381.449 | 13634.927 | 14020.045 |
| quic | deep&#45;8 | legacy | 5000 | 42.757 | 50.963 | 6.565 | 0.985 | 49.809 | 51.909 | 49.344 | 64.023 | 76.071 |
| quic | deep&#45;8 | portable | 5000 | 46.905 | 55.306 | 6.146 | 0.908 | 54.011 | 56.257 | 53.946 | 68.114 | 78.767 |
| quic | deep&#45;32 | legacy | 5000 | 50.656 | 66.269 | 15.193 | 15.469 | 57.969 | 93.816 | 59.212 | 95.241 | 108.014 |
| quic | deep&#45;32 | portable | 5000 | 54.194 | 70.762 | 17.552 | 17.942 | 61.879 | 102.813 | 62.212 | 104.812 | 118.208 |
| quic | concurrent&#45;32 | legacy | 5000 | 122.796 | 142.457 | 16.867 | 4.074 | 137.830 | 148.961 | 139.578 | 164.527 | 229.932 |
| quic | concurrent&#45;32 | portable | 5000 | 150.294 | 225.329 | 63.692 | 65.008 | 171.108 | 297.719 | 184.608 | 313.546 | 329.157 |
| quic | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 436.191 | 516.541 | 36.782 | 30.983 | 488.403 | 552.566 | 511.205 | 581.808 | 598.142 |
| quic | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 451.989 | 535.464 | 40.379 | 32.266 | 493.821 | 561.931 | 540.118 | 597.462 | 616.134 |
| quic | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 387.640 | 454.946 | 27.458 | 5.584 | 446.915 | 459.636 | 452.639 | 496.144 | 519.021 |
| quic | services&#45;4&#45;deep&#45;16 | portable | 5000 | 376.008 | 436.215 | 27.819 | 10.954 | 421.952 | 445.832 | 435.489 | 476.067 | 497.159 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1766.313 | 1975.614 | 66.592 | 3.817 | 1971.862 | 1979.785 | 1982.040 | 2065.705 | 2103.160 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1741.294 | 1986.472 | 62.517 | 10.305 | 1974.501 | 1998.924 | 1991.115 | 2079.407 | 2133.645 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1697.073 | 2038.024 | 136.461 | 36.571 | 2005.152 | 2096.061 | 2026.299 | 2248.778 | 2327.274 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1540.873 | 2026.122 | 150.985 | 39.864 | 1985.606 | 2075.657 | 2032.810 | 2255.363 | 2325.294 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 3607.930 | 4184.694 | 202.982 | 35.261 | 4141.547 | 4225.045 | 4157.014 | 4551.468 | 4725.298 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2985.048 | 4077.010 | 334.027 | 116.467 | 3919.632 | 4235.470 | 4053.976 | 4623.413 | 4754.117 |

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


