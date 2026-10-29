# Capntproto verification reports

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-29T05:37:24Z · run 40824462192 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40824462192) · commit `15bb774b3d7c` · **failure**

![Cargo test results](docs/reports/cargo-history.svg)

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-29T05:37:24Z · run 40824462192 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40824462192) · commit `15bb774b3d7c` · **failure**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-29T05:37:24Z · run 40824462192 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40824462192) · commit `15bb774b3d7c` · **failure**

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

Latest coverage run: [2026-10-29T05:37:24Z · run 40824462084 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40824462084) · commit `15bb774b3d7c` · **failure**

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-29T05:37:24Z · run 40824462192 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40824462192) · commit `15bb774b3d7c` · **failure**

Measurement evidence: **passed**. Independently validated encrypted benchmark values&#46; Workflow status and diagnostic completion are separate&#59; unavailable values remain blank&#46; Operations include whole concurrent batches and every service hop&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Current TCP fixtures use TLS 1.3, including upstream Rust Cap’n Proto, C++, gRPC over HTTP/2 and WebSockets over WSS; native encrypted transports and standard QUIC are identified separately. Historical data retains its recorded fixture contract. The dedicated campaign also reports recursive data, concurrent calls and independent service chains for gRPC and binary Protobuf WebSockets in its encrypted comparison artifact. Bars compare the recorded workload, rather than universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.


## Exhaustive benchmark tables

Measured run: [2026-10-29T05:37:24Z · run 40824462192 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40824462192) · commit `15bb774b3d7c` · **failure**

All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.

### All encrypted latency comparisons

| Transport | Case | Implementation | Samples | Minimum µs | Mean µs | Sample SD µs | Run mean SD µs | Run mean min µs | Run mean max µs | p50 µs | p95 µs | p99 µs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tls | loopback&#45;0 | cpp | 5000 | 46.337 | 58.407 | 13.514 | 4.769 | 54.244 | 66.209 | 53.463 | 84.219 | 99.070 |
| tls | loopback&#45;0 | grpc | 5000 | 54.640 | 70.595 | 16.020 | 12.530 | 62.912 | 92.737 | 63.665 | 102.719 | 125.273 |
| tls | loopback&#45;0 | legacy | 5000 | 28.940 | 40.306 | 10.664 | 4.432 | 35.736 | 46.413 | 35.568 | 55.939 | 74.035 |
| tls | loopback&#45;0 | portable | 5000 | 31.399 | 42.707 | 11.661 | 9.937 | 37.453 | 60.394 | 36.928 | 62.295 | 82.155 |
| tls | loopback&#45;0 | websocket | 5000 | 27.138 | 41.052 | 10.544 | 8.542 | 32.568 | 49.909 | 44.215 | 56.783 | 70.441 |
| tls | loopback&#45;64 | cpp | 5000 | 45.299 | 72.096 | 18.122 | 13.610 | 56.024 | 86.815 | 78.340 | 98.570 | 119.644 |
| tls | loopback&#45;64 | grpc | 5000 | 53.659 | 74.932 | 19.145 | 14.899 | 63.339 | 92.227 | 64.455 | 108.772 | 130.121 |
| tls | loopback&#45;64 | legacy | 5000 | 29.817 | 38.524 | 9.336 | 4.454 | 35.755 | 46.438 | 35.267 | 53.475 | 70.458 |
| tls | loopback&#45;64 | portable | 5000 | 32.181 | 50.010 | 13.204 | 10.679 | 38.610 | 59.975 | 53.941 | 70.504 | 88.595 |
| tls | loopback&#45;64 | websocket | 5000 | 25.501 | 37.317 | 9.420 | 6.660 | 31.315 | 44.851 | 34.211 | 52.664 | 66.487 |
| tls | loopback&#45;1024 | cpp | 5000 | 49.914 | 69.044 | 17.988 | 13.670 | 57.634 | 91.081 | 59.263 | 101.148 | 121.481 |
| tls | loopback&#45;1024 | grpc | 5000 | 57.388 | 73.028 | 16.155 | 5.988 | 68.464 | 82.815 | 66.651 | 105.114 | 135.294 |
| tls | loopback&#45;1024 | legacy | 5000 | 33.188 | 48.833 | 12.234 | 7.481 | 39.273 | 57.496 | 52.455 | 67.877 | 85.851 |
| tls | loopback&#45;1024 | portable | 5000 | 34.001 | 49.140 | 13.918 | 11.223 | 40.028 | 65.569 | 41.678 | 72.165 | 93.239 |
| tls | loopback&#45;1024 | websocket | 5000 | 24.931 | 44.593 | 10.558 | 4.049 | 40.054 | 48.182 | 43.277 | 60.818 | 75.871 |
| tls | loopback&#45;65536 | cpp | 5000 | 204.921 | 289.221 | 65.736 | 53.844 | 240.863 | 356.684 | 252.220 | 393.200 | 424.219 |
| tls | loopback&#45;65536 | grpc | 5000 | 215.036 | 277.829 | 52.841 | 34.007 | 249.474 | 318.381 | 254.794 | 383.599 | 425.341 |
| tls | loopback&#45;65536 | legacy | 5000 | 148.330 | 172.943 | 16.023 | 1.284 | 171.876 | 175.075 | 168.819 | 201.266 | 221.206 |
| tls | loopback&#45;65536 | portable | 5000 | 155.268 | 216.233 | 46.609 | 47.212 | 181.584 | 272.234 | 192.427 | 290.599 | 314.832 |
| tls | loopback&#45;65536 | websocket | 5000 | 167.268 | 226.697 | 48.860 | 33.852 | 194.078 | 275.138 | 201.080 | 308.628 | 331.176 |
| tls | unary&#45;empty | cpp | 5000 | 47.313 | 55.968 | 7.320 | 1.779 | 53.588 | 57.888 | 53.878 | 70.212 | 84.475 |
| tls | unary&#45;empty | grpc | 5000 | 56.022 | 65.340 | 8.078 | 1.684 | 63.181 | 67.544 | 63.238 | 80.424 | 98.357 |
| tls | unary&#45;empty | legacy | 5000 | 30.056 | 36.843 | 5.778 | 0.761 | 35.745 | 37.560 | 35.479 | 47.385 | 59.020 |
| tls | unary&#45;empty | portable | 5000 | 32.156 | 38.269 | 5.892 | 0.531 | 37.648 | 38.750 | 36.687 | 50.014 | 61.706 |
| tls | unary&#45;empty | websocket | 5000 | 26.957 | 32.985 | 4.888 | 0.624 | 32.297 | 33.856 | 31.969 | 40.842 | 52.346 |
| tls | unary&#45;small | cpp | 5000 | 45.940 | 57.297 | 12.216 | 6.251 | 53.532 | 68.245 | 52.645 | 84.442 | 100.412 |
| tls | unary&#45;small | grpc | 5000 | 55.611 | 73.220 | 20.011 | 19.583 | 63.716 | 108.232 | 63.591 | 112.014 | 145.329 |
| tls | unary&#45;small | legacy | 5000 | 29.626 | 35.983 | 5.334 | 0.561 | 35.011 | 36.367 | 34.569 | 46.929 | 57.215 |
| tls | unary&#45;small | portable | 5000 | 32.136 | 39.441 | 8.384 | 3.741 | 37.314 | 46.103 | 36.432 | 57.740 | 70.005 |
| tls | unary&#45;small | websocket | 5000 | 25.442 | 35.232 | 9.004 | 6.506 | 31.190 | 46.368 | 31.667 | 50.714 | 68.017 |
| tls | unary&#45;1k | cpp | 5000 | 47.704 | 73.485 | 17.683 | 15.685 | 59.795 | 90.952 | 70.154 | 100.835 | 115.546 |
| tls | unary&#45;1k | grpc | 5000 | 58.206 | 78.028 | 19.140 | 12.034 | 69.493 | 98.729 | 69.072 | 115.847 | 137.958 |
| tls | unary&#45;1k | legacy | 5000 | 31.537 | 42.960 | 9.795 | 7.777 | 38.299 | 56.677 | 38.479 | 58.936 | 74.324 |
| tls | unary&#45;1k | portable | 5000 | 34.456 | 50.021 | 13.411 | 9.953 | 40.952 | 63.308 | 43.367 | 71.671 | 89.892 |
| tls | unary&#45;1k | websocket | 5000 | 24.567 | 33.550 | 7.112 | 1.236 | 32.124 | 35.226 | 31.781 | 47.490 | 57.755 |
| tls | fanout&#45;32 | cpp | 5000 | 424.124 | 547.218 | 96.897 | 50.819 | 509.787 | 634.662 | 508.578 | 772.110 | 845.180 |
| tls | fanout&#45;32 | grpc | 5000 | 713.027 | 839.028 | 103.298 | 43.356 | 801.066 | 904.345 | 805.623 | 1100.813 | 1156.947 |
| tls | fanout&#45;32 | legacy | 5000 | 161.021 | 198.254 | 35.426 | 15.251 | 185.092 | 220.687 | 187.562 | 299.683 | 328.110 |
| tls | fanout&#45;32 | portable | 5000 | 191.578 | 246.596 | 53.502 | 47.484 | 222.761 | 331.473 | 226.560 | 374.206 | 398.654 |
| tls | fanout&#45;32 | websocket | 5000 | 193.992 | 270.108 | 54.613 | 31.456 | 235.976 | 307.382 | 244.648 | 368.013 | 392.752 |
| tls | dependent&#45;read | cpp | 5000 | 106.211 | 128.412 | 11.974 | 1.222 | 127.267 | 130.062 | 125.771 | 150.413 | 170.617 |
| tls | dependent&#45;read | grpc | 5000 | 111.211 | 134.876 | 14.174 | 1.585 | 132.773 | 136.510 | 131.238 | 160.352 | 182.209 |
| tls | dependent&#45;read | legacy | 5000 | 68.496 | 81.587 | 9.558 | 2.090 | 79.362 | 83.952 | 79.030 | 99.113 | 117.547 |
| tls | dependent&#45;read | portable | 5000 | 86.917 | 104.755 | 11.092 | 1.498 | 102.441 | 105.976 | 101.681 | 126.274 | 146.116 |
| tls | dependent&#45;read | websocket | 5000 | 48.591 | 62.810 | 8.596 | 0.468 | 62.239 | 63.507 | 61.230 | 78.043 | 90.289 |
| tls | pipelined&#45;read | cpp | 5000 | 77.660 | 94.490 | 9.871 | 1.942 | 91.896 | 96.600 | 92.176 | 112.034 | 125.868 |
| tls | pipelined&#45;read | legacy | 5000 | 41.288 | 49.567 | 9.986 | 0.964 | 48.621 | 50.577 | 47.606 | 62.849 | 74.944 |
| tls | pipelined&#45;read | portable | 5000 | 66.464 | 79.350 | 8.917 | 1.607 | 77.561 | 81.676 | 76.696 | 95.772 | 113.033 |
| tls | write&#45;read | cpp | 5000 | 91.480 | 116.566 | 19.511 | 4.995 | 110.499 | 123.340 | 110.150 | 164.264 | 191.784 |
| tls | write&#45;read | grpc | 5000 | 114.693 | 138.681 | 15.764 | 2.528 | 136.508 | 142.948 | 134.156 | 168.761 | 191.640 |
| tls | write&#45;read | legacy | 5000 | 61.143 | 72.967 | 7.708 | 0.766 | 72.028 | 74.162 | 70.913 | 88.463 | 102.214 |
| tls | write&#45;read | portable | 5000 | 65.110 | 79.916 | 13.602 | 6.043 | 76.529 | 90.643 | 75.368 | 112.800 | 133.389 |
| tls | write&#45;read | websocket | 5000 | 49.223 | 62.626 | 8.688 | 0.857 | 61.684 | 63.659 | 61.076 | 77.853 | 93.138 |
| tls | callback | cpp | 5000 | 101.730 | 125.805 | 22.228 | 13.281 | 118.945 | 149.481 | 118.202 | 182.006 | 203.629 |
| tls | callback | grpc | 5000 | 56.075 | 71.180 | 12.545 | 6.623 | 66.393 | 82.771 | 66.782 | 98.456 | 116.723 |
| tls | callback | legacy | 5000 | 66.511 | 79.348 | 8.978 | 0.464 | 78.742 | 79.932 | 77.023 | 96.979 | 112.340 |
| tls | callback | portable | 5000 | 82.818 | 111.538 | 27.535 | 27.720 | 96.589 | 161.030 | 98.418 | 167.451 | 195.404 |
| tls | callback | websocket | 5000 | 48.037 | 65.201 | 13.617 | 12.461 | 58.806 | 87.440 | 60.163 | 92.931 | 106.846 |
| tls | bulk&#45;64k | cpp | 5000 | 202.631 | 258.696 | 49.014 | 38.854 | 238.776 | 328.089 | 241.321 | 373.447 | 406.166 |
| tls | bulk&#45;64k | grpc | 5000 | 214.229 | 260.597 | 34.955 | 9.682 | 253.387 | 277.282 | 251.556 | 322.631 | 408.448 |
| tls | bulk&#45;64k | legacy | 5000 | 148.436 | 177.500 | 24.597 | 7.676 | 171.924 | 191.025 | 171.582 | 213.797 | 273.830 |
| tls | bulk&#45;64k | portable | 5000 | 156.221 | 197.890 | 39.461 | 34.265 | 177.281 | 258.578 | 181.943 | 280.538 | 314.081 |
| tls | bulk&#45;64k | websocket | 5000 | 167.411 | 196.215 | 19.166 | 2.061 | 193.746 | 198.679 | 192.753 | 222.427 | 245.417 |
| tls | bulk&#45;256k | cpp | 5000 | 807.022 | 920.223 | 74.233 | 18.353 | 907.618 | 952.087 | 909.685 | 984.692 | 1282.419 |
| tls | bulk&#45;256k | grpc | 5000 | 522.995 | 860.830 | 292.281 | 161.894 | 706.142 | 1131.179 | 768.801 | 1325.046 | 1598.288 |
| tls | bulk&#45;256k | legacy | 5000 | 443.808 | 514.660 | 33.137 | 5.332 | 509.140 | 521.151 | 512.384 | 550.862 | 577.251 |
| tls | bulk&#45;256k | portable | 5000 | 463.030 | 529.077 | 45.232 | 10.842 | 518.394 | 547.372 | 522.420 | 571.182 | 721.105 |
| tls | bulk&#45;256k | websocket | 5000 | 997.528 | 1088.318 | 46.973 | 9.964 | 1073.230 | 1098.202 | 1085.287 | 1141.282 | 1192.600 |
| tls | bulk&#45;1m | cpp | 5000 | 3249.975 | 3749.243 | 524.013 | 386.770 | 3498.145 | 4425.410 | 3556.877 | 5103.306 | 5308.544 |
| tls | bulk&#45;1m | grpc | 5000 | 2379.836 | 4810.668 | 873.401 | 874.245 | 3441.661 | 5460.966 | 5060.478 | 6035.682 | 6464.498 |
| tls | bulk&#45;1m | legacy | 5000 | 1577.567 | 1738.903 | 85.151 | 20.656 | 1726.830 | 1775.419 | 1728.514 | 1818.778 | 2201.849 |
| tls | bulk&#45;1m | portable | 5000 | 1600.109 | 1753.183 | 113.396 | 32.058 | 1731.016 | 1808.308 | 1732.038 | 1952.121 | 2274.616 |
| tls | bulk&#45;1m | websocket | 5000 | 4144.969 | 4762.794 | 261.249 | 64.808 | 4683.217 | 4853.371 | 4745.579 | 5167.647 | 5697.955 |
| tls | bulk&#45;4m | cpp | 5000 | 15473.189 | 16864.295 | 920.928 | 188.768 | 16731.190 | 17193.024 | 16714.753 | 17526.105 | 21681.634 |
| tls | bulk&#45;4m | grpc | 5000 | 19324.040 | 22978.777 | 2695.909 | 2645.159 | 20618.281 | 26099.720 | 21339.868 | 28585.073 | 29506.262 |
| tls | bulk&#45;4m | legacy | 5000 | 8601.668 | 9623.554 | 993.563 | 681.466 | 9070.809 | 10719.132 | 9127.795 | 11525.494 | 11775.725 |
| tls | bulk&#45;4m | portable | 5000 | 8634.758 | 9426.065 | 756.817 | 395.558 | 9108.902 | 10043.762 | 9164.189 | 11411.113 | 11728.644 |
| tls | bulk&#45;4m | websocket | 5000 | 27956.073 | 29337.932 | 1542.719 | 223.113 | 29123.736 | 29603.211 | 28935.758 | 34800.628 | 35479.499 |
| tls | deep&#45;8 | cpp | 5000 | 50.374 | 68.667 | 17.595 | 12.199 | 57.247 | 83.358 | 60.202 | 100.697 | 124.366 |
| tls | deep&#45;8 | grpc | 5000 | 65.483 | 89.128 | 22.603 | 18.877 | 76.921 | 121.258 | 77.434 | 131.393 | 155.402 |
| tls | deep&#45;8 | legacy | 5000 | 32.466 | 49.254 | 12.076 | 9.553 | 38.657 | 59.550 | 53.304 | 68.794 | 86.751 |
| tls | deep&#45;8 | portable | 5000 | 33.801 | 43.498 | 10.295 | 4.851 | 40.229 | 51.852 | 39.403 | 64.576 | 78.452 |
| tls | deep&#45;8 | websocket | 5000 | 29.977 | 47.980 | 12.419 | 7.828 | 41.105 | 60.012 | 46.231 | 70.412 | 86.112 |
| tls | deep&#45;32 | cpp | 5000 | 55.313 | 74.657 | 17.791 | 11.481 | 65.938 | 89.981 | 66.024 | 107.175 | 127.184 |
| tls | deep&#45;32 | grpc | 5000 | 82.441 | 135.801 | 34.178 | 29.832 | 96.647 | 161.493 | 148.200 | 184.457 | 212.314 |
| tls | deep&#45;32 | legacy | 5000 | 38.590 | 55.733 | 14.802 | 12.822 | 46.199 | 73.309 | 47.556 | 79.401 | 98.985 |
| tls | deep&#45;32 | portable | 5000 | 42.610 | 65.934 | 16.756 | 15.018 | 49.309 | 79.221 | 71.895 | 90.791 | 113.296 |
| tls | deep&#45;32 | websocket | 5000 | 47.835 | 65.135 | 12.002 | 3.292 | 62.774 | 70.816 | 62.351 | 88.702 | 109.371 |
| tls | concurrent&#45;32 | cpp | 5000 | 480.153 | 595.193 | 81.242 | 72.083 | 534.993 | 696.696 | 564.362 | 743.195 | 793.867 |
| tls | concurrent&#45;32 | grpc | 5000 | 567.276 | 739.509 | 72.790 | 53.840 | 644.801 | 779.329 | 749.745 | 823.439 | 886.746 |
| tls | concurrent&#45;32 | legacy | 5000 | 112.198 | 171.956 | 70.849 | 39.374 | 134.477 | 225.620 | 159.404 | 234.932 | 266.698 |
| tls | concurrent&#45;32 | portable | 5000 | 134.173 | 216.960 | 51.643 | 37.598 | 162.266 | 263.996 | 207.005 | 298.092 | 378.089 |
| tls | concurrent&#45;32 | websocket | 5000 | 105.931 | 200.575 | 41.434 | 29.524 | 159.213 | 228.925 | 202.050 | 268.090 | 299.544 |
| tls | deep&#45;32&#45;concurrent&#45;32 | cpp | 5000 | 632.439 | 929.889 | 104.914 | 69.823 | 818.090 | 982.815 | 938.592 | 1076.765 | 1161.622 |
| tls | deep&#45;32&#45;concurrent&#45;32 | grpc | 5000 | 1469.464 | 1978.312 | 224.817 | 208.577 | 1632.314 | 2197.787 | 1993.803 | 2339.151 | 2430.393 |
| tls | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 459.682 | 541.207 | 52.314 | 32.656 | 510.380 | 596.094 | 527.685 | 638.126 | 677.195 |
| tls | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 466.306 | 588.046 | 78.634 | 45.764 | 538.666 | 635.875 | 563.535 | 720.166 | 815.832 |
| tls | deep&#45;32&#45;concurrent&#45;32 | websocket | 5000 | 820.910 | 1033.396 | 74.165 | 38.636 | 992.325 | 1073.168 | 1042.988 | 1137.805 | 1192.317 |
| tls | services&#45;4&#45;deep&#45;16 | cpp | 5000 | 357.063 | 537.359 | 431.250 | 171.328 | 420.431 | 801.466 | 437.915 | 956.777 | 2898.269 |
| tls | services&#45;4&#45;deep&#45;16 | grpc | 5000 | 510.990 | 818.481 | 8872.725 | 470.752 | 596.036 | 1660.360 | 610.356 | 779.280 | 3171.562 |
| tls | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 257.225 | 354.750 | 278.031 | 73.968 | 311.999 | 484.827 | 318.033 | 399.802 | 1791.844 |
| tls | services&#45;4&#45;deep&#45;16 | portable | 5000 | 287.153 | 452.112 | 400.009 | 121.640 | 362.388 | 593.712 | 374.964 | 526.597 | 3037.526 |
| tls | services&#45;4&#45;deep&#45;16 | websocket | 5000 | 215.146 | 337.209 | 266.675 | 73.340 | 300.447 | 468.214 | 306.153 | 392.539 | 1597.446 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | 5000 | 2776.552 | 4829.977 | 1301.960 | 190.382 | 4649.343 | 5141.068 | 4737.600 | 6938.968 | 8621.919 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | 5000 | 4928.525 | 7968.492 | 1534.513 | 227.802 | 7737.173 | 8238.656 | 7999.675 | 10478.564 | 12102.011 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1575.985 | 2657.680 | 1120.197 | 73.283 | 2559.701 | 2740.457 | 2111.989 | 4856.458 | 6159.460 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1723.519 | 2740.227 | 982.025 | 300.726 | 2486.776 | 3259.970 | 2231.150 | 4845.889 | 5657.524 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | 5000 | 1429.456 | 2978.511 | 2109.235 | 320.434 | 2750.121 | 3538.777 | 2540.850 | 5313.526 | 7048.249 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 2300.649 | 3843.751 | 1370.863 | 851.292 | 2825.893 | 4560.718 | 3201.693 | 6322.852 | 8436.234 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 3919.324 | 6488.936 | 2434.541 | 1508.243 | 4738.564 | 7685.631 | 5811.759 | 10384.486 | 12175.002 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1270.845 | 2172.501 | 837.281 | 610.396 | 1721.655 | 2983.056 | 1793.487 | 4157.586 | 5158.630 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1509.008 | 2255.412 | 799.024 | 480.428 | 1872.507 | 2866.992 | 1993.385 | 4119.649 | 5269.669 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1262.918 | 2507.045 | 1468.861 | 800.636 | 1651.402 | 3293.833 | 1791.124 | 5203.419 | 7794.827 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 4808.219 | 6507.785 | 331.329 | 72.898 | 6414.026 | 6611.372 | 6509.292 | 7038.780 | 7275.671 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 7493.480 | 10106.801 | 749.775 | 144.589 | 9954.572 | 10312.117 | 10134.315 | 11196.685 | 11674.697 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2895.670 | 3990.468 | 249.898 | 167.870 | 3696.427 | 4104.737 | 3995.724 | 4399.861 | 4609.943 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3086.194 | 4084.618 | 290.441 | 149.371 | 3865.245 | 4231.149 | 4120.934 | 4511.015 | 4716.614 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 2424.498 | 3125.986 | 272.503 | 50.182 | 3063.997 | 3197.014 | 3079.840 | 3626.227 | 3937.230 |
| quic | loopback&#45;0 | legacy | 5000 | 41.611 | 60.603 | 15.843 | 10.872 | 48.514 | 75.328 | 54.883 | 85.570 | 103.605 |
| quic | loopback&#45;0 | portable | 5000 | 47.860 | 82.133 | 15.437 | 7.866 | 70.687 | 88.178 | 83.969 | 104.779 | 124.050 |
| quic | loopback&#45;64 | legacy | 5000 | 40.710 | 66.515 | 15.368 | 10.954 | 49.554 | 75.367 | 69.606 | 89.527 | 108.702 |
| quic | loopback&#45;64 | portable | 5000 | 47.621 | 80.875 | 15.608 | 14.345 | 55.235 | 88.131 | 83.769 | 103.237 | 121.380 |
| quic | loopback&#45;1024 | legacy | 5000 | 41.711 | 59.456 | 15.148 | 11.863 | 49.415 | 79.275 | 51.927 | 85.780 | 104.541 |
| quic | loopback&#45;1024 | portable | 5000 | 48.263 | 60.560 | 12.222 | 6.682 | 56.830 | 72.469 | 55.886 | 89.314 | 102.315 |
| quic | loopback&#45;65536 | legacy | 5000 | 191.356 | 268.038 | 60.715 | 50.616 | 222.307 | 344.661 | 234.900 | 360.253 | 384.202 |
| quic | loopback&#45;65536 | portable | 5000 | 196.917 | 256.258 | 53.499 | 37.303 | 227.701 | 300.889 | 231.717 | 360.579 | 387.363 |
| quic | unary&#45;empty | legacy | 5000 | 39.584 | 52.724 | 12.117 | 10.843 | 46.631 | 72.056 | 47.135 | 74.276 | 89.116 |
| quic | unary&#45;empty | portable | 5000 | 44.923 | 60.528 | 15.063 | 13.960 | 53.093 | 85.397 | 53.418 | 88.541 | 106.247 |
| quic | unary&#45;small | legacy | 5000 | 39.955 | 58.595 | 15.285 | 15.047 | 47.098 | 75.167 | 50.508 | 83.757 | 101.051 |
| quic | unary&#45;small | portable | 5000 | 46.913 | 66.560 | 17.882 | 16.120 | 54.351 | 86.223 | 57.051 | 96.210 | 116.839 |
| quic | unary&#45;1k | legacy | 5000 | 42.892 | 50.787 | 6.156 | 0.473 | 50.156 | 51.425 | 49.047 | 63.007 | 74.454 |
| quic | unary&#45;1k | portable | 5000 | 49.313 | 57.142 | 6.788 | 0.243 | 56.876 | 57.537 | 55.165 | 70.604 | 82.964 |
| quic | fanout&#45;32 | legacy | 5000 | 191.944 | 223.882 | 17.985 | 2.119 | 221.382 | 226.829 | 221.186 | 250.745 | 270.625 |
| quic | fanout&#45;32 | portable | 5000 | 228.451 | 265.694 | 26.274 | 5.765 | 260.828 | 275.665 | 261.695 | 293.747 | 410.443 |
| quic | dependent&#45;read | legacy | 5000 | 89.461 | 106.980 | 12.447 | 1.565 | 105.147 | 108.758 | 104.144 | 127.367 | 145.516 |
| quic | dependent&#45;read | portable | 5000 | 120.597 | 145.222 | 13.073 | 1.497 | 143.392 | 147.009 | 142.069 | 169.720 | 187.565 |
| quic | pipelined&#45;read | legacy | 5000 | 53.409 | 71.775 | 17.986 | 17.833 | 62.755 | 103.652 | 62.898 | 107.187 | 124.187 |
| quic | pipelined&#45;read | portable | 5000 | 84.831 | 109.135 | 22.688 | 11.576 | 102.604 | 129.505 | 100.416 | 165.592 | 186.741 |
| quic | write&#45;read | legacy | 5000 | 82.779 | 103.690 | 19.356 | 9.923 | 98.509 | 121.413 | 97.253 | 150.611 | 175.156 |
| quic | write&#45;read | portable | 5000 | 92.540 | 113.591 | 14.952 | 4.570 | 110.219 | 121.410 | 109.663 | 138.762 | 177.196 |
| quic | callback | legacy | 5000 | 86.727 | 110.284 | 18.914 | 6.932 | 104.473 | 122.152 | 104.670 | 151.643 | 179.422 |
| quic | callback | portable | 5000 | 118.896 | 160.637 | 40.102 | 32.512 | 135.629 | 206.726 | 140.998 | 233.449 | 260.713 |
| quic | bulk&#45;64k | legacy | 5000 | 189.175 | 250.470 | 54.333 | 36.622 | 221.092 | 311.072 | 225.571 | 354.074 | 380.875 |
| quic | bulk&#45;64k | portable | 5000 | 198.998 | 282.804 | 63.448 | 41.482 | 229.375 | 332.933 | 251.074 | 370.192 | 397.880 |
| quic | bulk&#45;256k | legacy | 5000 | 486.417 | 601.784 | 877.676 | 34.452 | 576.207 | 655.795 | 553.303 | 795.435 | 833.083 |
| quic | bulk&#45;256k | portable | 5000 | 512.684 | 610.113 | 868.607 | 18.078 | 591.086 | 631.072 | 573.053 | 632.919 | 856.722 |
| quic | bulk&#45;1m | legacy | 5000 | 1875.350 | 2136.546 | 710.229 | 53.014 | 2072.016 | 2195.747 | 2099.038 | 2262.719 | 2943.168 |
| quic | bulk&#45;1m | portable | 5000 | 1859.671 | 2141.549 | 716.911 | 25.073 | 2118.974 | 2178.778 | 2098.869 | 2306.360 | 2937.363 |
| quic | bulk&#45;4m | legacy | 5000 | 10120.575 | 11100.589 | 952.298 | 268.454 | 10841.119 | 11536.235 | 10996.348 | 11601.126 | 15035.862 |
| quic | bulk&#45;4m | portable | 5000 | 10331.625 | 11558.143 | 1429.711 | 274.606 | 11151.310 | 11839.637 | 11305.222 | 14623.728 | 15330.669 |
| quic | deep&#45;8 | legacy | 5000 | 45.054 | 68.750 | 16.707 | 14.369 | 53.013 | 81.648 | 74.018 | 94.207 | 114.345 |
| quic | deep&#45;8 | portable | 5000 | 49.794 | 66.087 | 16.339 | 15.429 | 58.214 | 93.639 | 58.110 | 98.184 | 117.695 |
| quic | deep&#45;32 | legacy | 5000 | 49.641 | 60.013 | 7.149 | 0.813 | 59.158 | 61.354 | 58.057 | 74.961 | 87.499 |
| quic | deep&#45;32 | portable | 5000 | 55.037 | 67.211 | 8.196 | 1.660 | 65.537 | 69.731 | 64.915 | 83.256 | 98.444 |
| quic | concurrent&#45;32 | legacy | 5000 | 122.305 | 165.626 | 42.100 | 33.059 | 142.517 | 222.415 | 146.886 | 256.252 | 280.238 |
| quic | concurrent&#45;32 | portable | 5000 | 155.932 | 182.903 | 15.157 | 2.420 | 180.031 | 186.214 | 180.049 | 208.316 | 222.967 |
| quic | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 435.405 | 508.043 | 37.236 | 29.909 | 493.184 | 561.469 | 497.646 | 578.282 | 616.764 |
| quic | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 465.144 | 554.758 | 43.129 | 30.730 | 526.923 | 594.100 | 544.739 | 625.888 | 660.870 |
| quic | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 406.182 | 484.460 | 30.888 | 4.408 | 479.781 | 491.384 | 481.789 | 533.251 | 560.350 |
| quic | services&#45;4&#45;deep&#45;16 | portable | 5000 | 398.043 | 469.127 | 30.768 | 7.633 | 459.870 | 481.023 | 467.251 | 516.240 | 546.080 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1787.378 | 1998.305 | 69.955 | 6.682 | 1992.910 | 2009.874 | 1995.961 | 2087.320 | 2240.666 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1858.721 | 2067.807 | 96.813 | 25.084 | 2037.255 | 2097.743 | 2059.017 | 2187.585 | 2363.443 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1761.048 | 2149.733 | 138.660 | 37.312 | 2105.612 | 2205.375 | 2149.324 | 2363.990 | 2476.886 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1664.275 | 2153.812 | 186.300 | 65.885 | 2081.628 | 2242.663 | 2179.878 | 2431.110 | 2507.115 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 3784.116 | 4683.434 | 298.095 | 43.663 | 4638.720 | 4745.186 | 4678.854 | 5162.204 | 5447.673 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3375.491 | 4462.998 | 484.329 | 77.025 | 4356.965 | 4563.484 | 4434.852 | 5224.033 | 5469.856 |

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


