# Capntproto verification reports

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-28T23:44:11Z · run 40789388805 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40789388805) · commit `5c39044a1df4` · **failure**

![Cargo test results](docs/reports/cargo-history.svg)

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-28T23:44:11Z · run 40789388805 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40789388805) · commit `5c39044a1df4` · **failure**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-28T23:44:11Z · run 40789388805 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40789388805) · commit `5c39044a1df4` · **failure**

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

Latest coverage run: [2026-10-28T23:44:11Z · run 40789388793 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40789388793) · commit `5c39044a1df4` · **failure**

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-28T23:44:11Z · run 40789388805 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40789388805) · commit `5c39044a1df4` · **failure**

Measurement evidence: **passed**. Independently validated encrypted benchmark values&#46; Workflow status and diagnostic completion are separate&#59; unavailable values remain blank&#46; Operations include whole concurrent batches and every service hop&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Current TCP fixtures use TLS 1.3, including upstream Rust Cap’n Proto, C++, gRPC over HTTP/2 and WebSockets over WSS; native encrypted transports and standard QUIC are identified separately. Historical data retains its recorded fixture contract. The dedicated campaign also reports recursive data, concurrent calls and independent service chains for gRPC and binary Protobuf WebSockets in its encrypted comparison artifact. Bars compare the recorded workload, rather than universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.


## Exhaustive benchmark tables

Measured run: [2026-10-28T23:44:11Z · run 40789388805 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40789388805) · commit `5c39044a1df4` · **failure**

All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.

### All encrypted latency comparisons

| Transport | Case | Implementation | Samples | Minimum µs | Mean µs | Sample SD µs | Run mean SD µs | Run mean min µs | Run mean max µs | p50 µs | p95 µs | p99 µs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tls | loopback&#45;0 | cpp | 5000 | 44.027 | 48.100 | 4.461 | 0.604 | 47.618 | 49.053 | 46.828 | 57.963 | 67.475 |
| tls | loopback&#45;0 | grpc | 5000 | 51.745 | 64.306 | 13.733 | 12.514 | 55.071 | 83.371 | 55.760 | 84.334 | 96.694 |
| tls | loopback&#45;0 | legacy | 5000 | 30.221 | 37.915 | 7.652 | 7.792 | 32.161 | 46.623 | 32.423 | 46.820 | 57.785 |
| tls | loopback&#45;0 | portable | 5000 | 30.623 | 36.357 | 7.095 | 6.557 | 32.672 | 47.993 | 32.841 | 47.795 | 57.076 |
| tls | loopback&#45;0 | websocket | 5000 | 26.777 | 34.067 | 6.789 | 6.657 | 28.618 | 41.397 | 29.477 | 41.671 | 50.861 |
| tls | loopback&#45;64 | cpp | 5000 | 43.947 | 48.004 | 4.608 | 0.825 | 46.853 | 49.115 | 46.673 | 58.350 | 67.616 |
| tls | loopback&#45;64 | grpc | 5000 | 52.276 | 57.727 | 11.092 | 3.273 | 55.668 | 63.505 | 55.006 | 71.501 | 93.088 |
| tls | loopback&#45;64 | legacy | 5000 | 30.084 | 33.064 | 3.901 | 0.412 | 32.478 | 33.421 | 32.089 | 39.523 | 50.718 |
| tls | loopback&#45;64 | portable | 5000 | 30.498 | 32.771 | 3.434 | 0.176 | 32.571 | 33.004 | 31.887 | 38.412 | 49.279 |
| tls | loopback&#45;64 | websocket | 5000 | 24.785 | 27.516 | 3.078 | 0.134 | 27.365 | 27.723 | 26.991 | 32.179 | 41.644 |
| tls | loopback&#45;1024 | cpp | 5000 | 47.536 | 69.967 | 9.768 | 9.588 | 52.972 | 76.428 | 71.935 | 84.285 | 92.884 |
| tls | loopback&#45;1024 | grpc | 5000 | 57.525 | 84.129 | 15.343 | 11.343 | 66.221 | 92.610 | 89.037 | 101.367 | 108.431 |
| tls | loopback&#45;1024 | legacy | 5000 | 31.778 | 43.765 | 6.271 | 5.846 | 33.626 | 48.761 | 44.510 | 51.936 | 62.880 |
| tls | loopback&#45;1024 | portable | 5000 | 32.616 | 45.939 | 6.762 | 6.102 | 35.392 | 51.279 | 46.672 | 55.052 | 65.546 |
| tls | loopback&#45;1024 | websocket | 5000 | 24.916 | 35.887 | 5.674 | 4.513 | 28.255 | 39.474 | 36.125 | 43.387 | 51.495 |
| tls | loopback&#45;65536 | cpp | 5000 | 170.779 | 199.496 | 30.973 | 26.491 | 186.173 | 246.817 | 187.008 | 271.266 | 283.252 |
| tls | loopback&#45;65536 | grpc | 5000 | 174.201 | 192.548 | 14.991 | 0.663 | 191.653 | 193.469 | 187.570 | 226.074 | 247.825 |
| tls | loopback&#45;65536 | legacy | 5000 | 106.156 | 129.891 | 24.010 | 20.281 | 113.093 | 160.331 | 114.546 | 169.899 | 178.731 |
| tls | loopback&#45;65536 | portable | 5000 | 109.756 | 128.116 | 23.144 | 24.410 | 115.536 | 171.727 | 115.557 | 171.755 | 188.386 |
| tls | loopback&#45;65536 | websocket | 5000 | 122.432 | 139.848 | 20.935 | 19.421 | 128.147 | 173.648 | 127.361 | 180.478 | 189.831 |
| tls | unary&#45;empty | cpp | 5000 | 44.137 | 47.512 | 4.233 | 0.801 | 46.310 | 48.353 | 46.266 | 56.717 | 66.724 |
| tls | unary&#45;empty | grpc | 5000 | 52.391 | 56.919 | 4.980 | 1.135 | 55.520 | 58.086 | 55.422 | 66.954 | 77.978 |
| tls | unary&#45;empty | legacy | 5000 | 30.128 | 32.752 | 3.669 | 0.344 | 32.183 | 33.046 | 31.814 | 39.186 | 48.865 |
| tls | unary&#45;empty | portable | 5000 | 30.489 | 32.686 | 3.471 | 0.368 | 32.200 | 33.189 | 31.793 | 38.263 | 49.780 |
| tls | unary&#45;empty | websocket | 5000 | 26.716 | 28.773 | 3.101 | 0.487 | 27.996 | 29.306 | 28.138 | 32.894 | 41.811 |
| tls | unary&#45;small | cpp | 5000 | 44.232 | 48.150 | 4.462 | 0.331 | 47.754 | 48.659 | 46.906 | 57.858 | 67.910 |
| tls | unary&#45;small | grpc | 5000 | 52.896 | 57.193 | 5.049 | 0.763 | 56.444 | 58.251 | 55.673 | 67.477 | 78.617 |
| tls | unary&#45;small | legacy | 5000 | 29.950 | 32.450 | 3.699 | 0.291 | 32.039 | 32.857 | 31.500 | 38.560 | 49.978 |
| tls | unary&#45;small | portable | 5000 | 30.616 | 33.337 | 3.672 | 0.703 | 32.408 | 34.062 | 32.341 | 39.614 | 51.231 |
| tls | unary&#45;small | websocket | 5000 | 25.303 | 28.013 | 3.301 | 0.136 | 27.879 | 28.165 | 27.446 | 32.306 | 41.833 |
| tls | unary&#45;1k | cpp | 5000 | 46.661 | 50.333 | 4.167 | 0.471 | 49.692 | 50.818 | 49.021 | 59.592 | 69.512 |
| tls | unary&#45;1k | grpc | 5000 | 57.983 | 64.074 | 5.564 | 0.786 | 62.831 | 64.936 | 62.664 | 75.690 | 86.523 |
| tls | unary&#45;1k | legacy | 5000 | 31.657 | 34.440 | 3.578 | 0.833 | 33.714 | 35.401 | 33.346 | 41.113 | 50.659 |
| tls | unary&#45;1k | portable | 5000 | 32.462 | 35.466 | 4.167 | 1.190 | 34.644 | 37.394 | 34.096 | 43.909 | 53.340 |
| tls | unary&#45;1k | websocket | 5000 | 25.222 | 28.686 | 3.345 | 0.288 | 28.340 | 28.935 | 28.122 | 33.321 | 42.539 |
| tls | fanout&#45;32 | cpp | 5000 | 365.458 | 429.162 | 72.083 | 32.929 | 401.510 | 485.247 | 402.252 | 622.682 | 692.391 |
| tls | fanout&#45;32 | grpc | 5000 | 636.701 | 707.572 | 100.724 | 49.864 | 670.637 | 791.341 | 675.352 | 992.987 | 1073.397 |
| tls | fanout&#45;32 | legacy | 5000 | 136.134 | 155.446 | 26.522 | 21.610 | 144.498 | 193.977 | 143.809 | 215.825 | 234.845 |
| tls | fanout&#45;32 | portable | 5000 | 163.152 | 182.569 | 29.205 | 18.616 | 172.204 | 215.615 | 171.552 | 264.071 | 284.307 |
| tls | fanout&#45;32 | websocket | 5000 | 182.908 | 206.613 | 16.149 | 1.119 | 205.329 | 208.050 | 201.757 | 235.197 | 249.377 |
| tls | dependent&#45;read | cpp | 5000 | 102.749 | 110.506 | 8.282 | 0.959 | 109.953 | 112.191 | 107.829 | 128.035 | 142.852 |
| tls | dependent&#45;read | grpc | 5000 | 108.967 | 121.448 | 9.032 | 1.924 | 118.928 | 123.869 | 118.898 | 140.557 | 155.477 |
| tls | dependent&#45;read | legacy | 5000 | 65.536 | 80.844 | 15.028 | 14.162 | 69.504 | 96.901 | 71.351 | 102.626 | 119.565 |
| tls | dependent&#45;read | portable | 5000 | 78.559 | 97.287 | 20.984 | 18.522 | 83.516 | 126.260 | 84.435 | 130.184 | 149.444 |
| tls | dependent&#45;read | websocket | 5000 | 48.280 | 60.308 | 10.491 | 9.718 | 55.336 | 77.675 | 56.149 | 82.574 | 89.219 |
| tls | pipelined&#45;read | cpp | 5000 | 73.493 | 96.618 | 24.102 | 24.642 | 77.087 | 127.141 | 79.729 | 132.164 | 146.151 |
| tls | pipelined&#45;read | legacy | 5000 | 39.564 | 51.042 | 11.686 | 9.025 | 42.238 | 60.608 | 49.776 | 61.982 | 79.072 |
| tls | pipelined&#45;read | portable | 5000 | 57.538 | 76.998 | 20.075 | 21.235 | 60.899 | 101.244 | 62.325 | 105.928 | 121.365 |
| tls | write&#45;read | cpp | 5000 | 88.882 | 95.534 | 6.823 | 0.337 | 94.953 | 95.800 | 93.275 | 109.986 | 124.260 |
| tls | write&#45;read | grpc | 5000 | 111.148 | 131.248 | 26.374 | 28.340 | 117.428 | 181.922 | 117.397 | 181.406 | 196.449 |
| tls | write&#45;read | legacy | 5000 | 61.250 | 65.343 | 5.446 | 0.573 | 64.592 | 66.185 | 63.694 | 76.836 | 88.766 |
| tls | write&#45;read | portable | 5000 | 62.393 | 66.329 | 5.392 | 0.341 | 65.956 | 66.888 | 64.603 | 76.999 | 89.474 |
| tls | write&#45;read | websocket | 5000 | 49.038 | 55.727 | 5.076 | 0.880 | 55.115 | 57.181 | 54.804 | 66.203 | 74.732 |
| tls | callback | cpp | 5000 | 95.040 | 113.867 | 24.013 | 25.503 | 101.603 | 159.467 | 101.523 | 158.114 | 178.174 |
| tls | callback | grpc | 5000 | 57.938 | 62.656 | 6.231 | 1.211 | 61.484 | 64.104 | 60.682 | 76.760 | 87.124 |
| tls | callback | legacy | 5000 | 65.363 | 69.683 | 5.078 | 0.612 | 68.959 | 70.337 | 68.007 | 81.057 | 91.613 |
| tls | callback | portable | 5000 | 76.431 | 82.414 | 7.004 | 1.531 | 81.377 | 84.957 | 80.061 | 97.643 | 109.915 |
| tls | callback | websocket | 5000 | 47.576 | 53.701 | 4.200 | 0.331 | 53.231 | 54.109 | 52.911 | 61.332 | 70.881 |
| tls | bulk&#45;64k | cpp | 5000 | 168.405 | 204.231 | 38.794 | 41.527 | 182.587 | 278.387 | 186.054 | 283.625 | 295.760 |
| tls | bulk&#45;64k | grpc | 5000 | 175.039 | 220.988 | 54.011 | 51.689 | 188.748 | 310.153 | 190.010 | 320.047 | 338.232 |
| tls | bulk&#45;64k | legacy | 5000 | 106.322 | 113.114 | 15.560 | 1.464 | 111.690 | 115.050 | 110.686 | 125.572 | 138.024 |
| tls | bulk&#45;64k | portable | 5000 | 109.693 | 130.063 | 27.228 | 22.066 | 115.547 | 166.632 | 114.326 | 182.988 | 195.925 |
| tls | bulk&#45;64k | websocket | 5000 | 122.906 | 128.700 | 7.818 | 0.923 | 127.867 | 129.987 | 125.989 | 142.350 | 156.563 |
| tls | bulk&#45;256k | cpp | 5000 | 664.496 | 777.667 | 114.991 | 124.095 | 719.169 | 999.617 | 724.027 | 1016.259 | 1040.206 |
| tls | bulk&#45;256k | grpc | 5000 | 424.944 | 1077.643 | 193.645 | 116.818 | 893.717 | 1212.888 | 1110.075 | 1298.973 | 1361.845 |
| tls | bulk&#45;256k | legacy | 5000 | 298.841 | 314.349 | 11.536 | 0.841 | 313.246 | 315.602 | 309.716 | 335.376 | 359.696 |
| tls | bulk&#45;256k | portable | 5000 | 310.467 | 323.383 | 15.756 | 1.589 | 321.727 | 325.350 | 318.353 | 343.949 | 385.169 |
| tls | bulk&#45;256k | websocket | 5000 | 741.769 | 783.784 | 32.724 | 17.765 | 771.406 | 814.663 | 774.491 | 856.910 | 871.746 |
| tls | bulk&#45;1m | cpp | 5000 | 2648.056 | 2817.333 | 232.378 | 65.049 | 2757.519 | 2917.851 | 2753.236 | 3275.582 | 3858.311 |
| tls | bulk&#45;1m | grpc | 5000 | 1620.253 | 3156.288 | 1019.948 | 1096.196 | 1693.314 | 4466.662 | 3014.614 | 4771.165 | 4973.455 |
| tls | bulk&#45;1m | legacy | 5000 | 1070.281 | 1187.980 | 131.061 | 66.944 | 1121.584 | 1264.452 | 1122.955 | 1439.615 | 1463.497 |
| tls | bulk&#45;1m | portable | 5000 | 1078.022 | 1158.187 | 106.171 | 46.052 | 1119.271 | 1236.910 | 1120.650 | 1422.919 | 1455.988 |
| tls | bulk&#45;1m | websocket | 5000 | 3259.833 | 3571.935 | 205.074 | 32.535 | 3548.290 | 3626.558 | 3485.453 | 3953.681 | 4043.992 |
| tls | bulk&#45;4m | cpp | 5000 | 11996.416 | 12933.553 | 815.298 | 154.969 | 12753.709 | 13093.967 | 12758.583 | 13696.266 | 16943.774 |
| tls | bulk&#45;4m | grpc | 5000 | 15308.545 | 20180.297 | 1923.920 | 1879.394 | 16825.749 | 21171.390 | 20527.585 | 22652.974 | 23122.266 |
| tls | bulk&#45;4m | legacy | 5000 | 4902.133 | 5342.227 | 381.660 | 74.495 | 5264.383 | 5449.259 | 5213.680 | 6352.529 | 6633.436 |
| tls | bulk&#45;4m | portable | 5000 | 4917.137 | 5494.809 | 516.110 | 217.888 | 5296.397 | 5854.861 | 5293.425 | 6688.573 | 6898.000 |
| tls | bulk&#45;4m | websocket | 5000 | 21261.367 | 22506.538 | 1261.269 | 262.471 | 22189.987 | 22854.436 | 21915.571 | 25028.796 | 25470.225 |
| tls | deep&#45;8 | cpp | 5000 | 47.333 | 56.235 | 10.584 | 10.713 | 50.333 | 75.304 | 50.809 | 74.659 | 87.804 |
| tls | deep&#45;8 | grpc | 5000 | 63.503 | 76.307 | 14.116 | 14.751 | 68.847 | 102.675 | 68.973 | 102.087 | 114.688 |
| tls | deep&#45;8 | legacy | 5000 | 32.175 | 36.391 | 5.961 | 3.592 | 34.366 | 42.769 | 34.135 | 49.610 | 56.670 |
| tls | deep&#45;8 | portable | 5000 | 32.768 | 38.362 | 6.561 | 6.036 | 35.010 | 49.127 | 34.967 | 49.091 | 58.403 |
| tls | deep&#45;8 | websocket | 5000 | 29.021 | 34.710 | 4.252 | 0.435 | 34.349 | 35.456 | 34.049 | 41.493 | 51.940 |
| tls | deep&#45;32 | cpp | 5000 | 52.600 | 59.933 | 9.380 | 5.373 | 56.843 | 69.504 | 56.408 | 83.481 | 90.939 |
| tls | deep&#45;32 | grpc | 5000 | 80.500 | 96.020 | 16.876 | 17.476 | 86.374 | 127.207 | 87.529 | 128.324 | 141.627 |
| tls | deep&#45;32 | legacy | 5000 | 37.913 | 47.059 | 8.348 | 8.234 | 40.872 | 56.222 | 42.163 | 57.100 | 69.146 |
| tls | deep&#45;32 | portable | 5000 | 38.160 | 48.185 | 9.288 | 9.473 | 41.143 | 58.565 | 42.359 | 59.785 | 72.869 |
| tls | deep&#45;32 | websocket | 5000 | 46.858 | 58.476 | 10.027 | 9.669 | 53.701 | 75.759 | 54.383 | 79.173 | 86.945 |
| tls | concurrent&#45;32 | cpp | 5000 | 403.272 | 447.424 | 26.205 | 19.853 | 434.952 | 481.735 | 439.744 | 493.992 | 510.664 |
| tls | concurrent&#45;32 | grpc | 5000 | 502.644 | 591.779 | 54.144 | 41.238 | 527.373 | 625.520 | 611.686 | 647.202 | 667.662 |
| tls | concurrent&#45;32 | legacy | 5000 | 105.347 | 124.556 | 24.421 | 14.318 | 112.283 | 145.990 | 113.307 | 172.807 | 186.654 |
| tls | concurrent&#45;32 | portable | 5000 | 130.927 | 166.269 | 35.427 | 35.865 | 139.184 | 211.620 | 144.000 | 223.154 | 228.831 |
| tls | concurrent&#45;32 | websocket | 5000 | 98.368 | 169.288 | 30.566 | 29.369 | 136.921 | 191.722 | 175.034 | 213.723 | 228.469 |
| tls | deep&#45;32&#45;concurrent&#45;32 | cpp | 5000 | 539.879 | 607.425 | 77.367 | 54.180 | 572.658 | 701.010 | 576.140 | 788.332 | 838.214 |
| tls | deep&#45;32&#45;concurrent&#45;32 | grpc | 5000 | 1297.756 | 1407.365 | 89.272 | 65.153 | 1368.504 | 1521.424 | 1372.591 | 1614.491 | 1651.175 |
| tls | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 312.582 | 333.596 | 32.453 | 4.739 | 329.570 | 341.359 | 329.279 | 355.411 | 417.339 |
| tls | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 337.931 | 364.158 | 37.587 | 5.509 | 358.388 | 371.253 | 357.656 | 401.427 | 487.431 |
| tls | deep&#45;32&#45;concurrent&#45;32 | websocket | 5000 | 621.117 | 695.462 | 71.677 | 49.099 | 670.160 | 783.211 | 673.626 | 899.762 | 932.884 |
| tls | services&#45;4&#45;deep&#45;16 | cpp | 5000 | 323.865 | 367.525 | 29.945 | 2.372 | 364.438 | 370.735 | 360.989 | 408.144 | 437.372 |
| tls | services&#45;4&#45;deep&#45;16 | grpc | 5000 | 431.901 | 473.258 | 24.632 | 1.564 | 471.999 | 475.854 | 470.978 | 510.619 | 530.862 |
| tls | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 231.207 | 260.047 | 20.551 | 4.069 | 257.120 | 267.033 | 253.753 | 295.894 | 314.896 |
| tls | services&#45;4&#45;deep&#45;16 | portable | 5000 | 251.486 | 283.983 | 20.578 | 3.632 | 278.922 | 288.834 | 278.118 | 321.350 | 336.714 |
| tls | services&#45;4&#45;deep&#45;16 | websocket | 5000 | 194.058 | 237.950 | 24.733 | 3.981 | 234.351 | 242.903 | 235.512 | 282.021 | 304.888 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | 5000 | 2340.552 | 2549.846 | 149.784 | 69.283 | 2474.875 | 2630.534 | 2502.239 | 2778.514 | 3064.128 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | 5000 | 4023.933 | 4270.498 | 159.829 | 48.647 | 4229.671 | 4351.560 | 4226.398 | 4558.006 | 4746.169 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1295.922 | 1358.690 | 46.980 | 5.862 | 1352.385 | 1365.616 | 1349.994 | 1409.661 | 1600.492 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1418.456 | 1483.436 | 31.139 | 6.275 | 1476.356 | 1490.495 | 1479.116 | 1525.797 | 1587.469 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | 5000 | 1157.308 | 1264.455 | 51.539 | 8.153 | 1252.385 | 1271.885 | 1260.397 | 1336.604 | 1378.319 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 1959.460 | 2327.933 | 124.109 | 19.467 | 2309.139 | 2360.712 | 2321.381 | 2522.418 | 2658.684 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 3443.020 | 4047.385 | 170.851 | 21.606 | 4019.078 | 4078.857 | 4019.661 | 4356.202 | 4526.318 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1098.048 | 1378.817 | 62.533 | 17.083 | 1361.936 | 1404.175 | 1373.634 | 1455.088 | 1628.416 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1302.516 | 1465.255 | 60.291 | 6.299 | 1455.150 | 1471.468 | 1474.706 | 1546.342 | 1626.165 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1015.863 | 1249.837 | 102.251 | 45.689 | 1190.623 | 1297.074 | 1238.440 | 1427.843 | 1502.192 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 3744.054 | 4841.148 | 184.501 | 109.545 | 4687.923 | 4959.249 | 4840.132 | 5134.210 | 5251.175 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 6449.049 | 8227.264 | 434.925 | 67.081 | 8142.989 | 8320.234 | 8278.494 | 8842.382 | 9041.539 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2701.691 | 2893.269 | 74.404 | 14.412 | 2874.937 | 2909.368 | 2889.231 | 2982.918 | 3141.563 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2484.079 | 3042.870 | 142.573 | 48.621 | 2977.858 | 3103.653 | 3061.307 | 3227.694 | 3299.175 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1977.288 | 2412.304 | 207.356 | 22.942 | 2374.354 | 2432.866 | 2382.015 | 2727.714 | 2968.384 |
| quic | loopback&#45;0 | legacy | 5000 | 38.850 | 49.841 | 9.465 | 9.126 | 42.577 | 59.881 | 44.277 | 60.806 | 72.686 |
| quic | loopback&#45;0 | portable | 5000 | 42.684 | 55.948 | 12.529 | 12.750 | 46.321 | 70.175 | 47.900 | 70.973 | 82.907 |
| quic | loopback&#45;64 | legacy | 5000 | 38.594 | 42.013 | 3.863 | 1.413 | 40.746 | 44.389 | 40.771 | 50.069 | 58.867 |
| quic | loopback&#45;64 | portable | 5000 | 41.912 | 45.330 | 4.599 | 0.775 | 44.150 | 46.019 | 44.018 | 53.950 | 65.172 |
| quic | loopback&#45;1024 | legacy | 5000 | 39.936 | 46.389 | 8.533 | 7.466 | 42.132 | 59.703 | 42.353 | 62.328 | 71.405 |
| quic | loopback&#45;1024 | portable | 5000 | 43.338 | 52.855 | 11.090 | 9.706 | 46.205 | 69.320 | 46.436 | 71.758 | 82.456 |
| quic | loopback&#45;65536 | legacy | 5000 | 143.605 | 176.416 | 38.201 | 36.271 | 151.189 | 233.778 | 151.759 | 243.033 | 252.558 |
| quic | loopback&#45;65536 | portable | 5000 | 147.066 | 165.889 | 29.199 | 15.766 | 154.713 | 192.565 | 153.272 | 242.340 | 258.603 |
| quic | unary&#45;empty | legacy | 5000 | 38.702 | 52.138 | 10.564 | 9.264 | 42.463 | 62.329 | 57.710 | 63.879 | 74.436 |
| quic | unary&#45;empty | portable | 5000 | 42.549 | 59.093 | 10.774 | 11.070 | 46.889 | 67.845 | 65.360 | 73.400 | 81.302 |
| quic | unary&#45;small | legacy | 5000 | 38.498 | 45.699 | 9.442 | 7.102 | 41.510 | 58.338 | 41.972 | 62.285 | 72.831 |
| quic | unary&#45;small | portable | 5000 | 41.855 | 46.180 | 4.501 | 1.323 | 45.125 | 47.815 | 44.900 | 55.035 | 65.672 |
| quic | unary&#45;1k | legacy | 5000 | 40.167 | 43.733 | 4.149 | 0.627 | 43.184 | 44.604 | 42.589 | 51.684 | 60.821 |
| quic | unary&#45;1k | portable | 5000 | 43.575 | 47.190 | 4.168 | 0.499 | 46.510 | 47.654 | 45.986 | 56.154 | 65.594 |
| quic | fanout&#45;32 | legacy | 5000 | 163.255 | 191.798 | 41.537 | 42.836 | 171.634 | 268.396 | 170.920 | 281.769 | 294.923 |
| quic | fanout&#45;32 | portable | 5000 | 192.060 | 234.982 | 57.832 | 46.483 | 201.293 | 296.314 | 200.981 | 340.296 | 351.664 |
| quic | dependent&#45;read | legacy | 5000 | 84.094 | 90.327 | 5.881 | 1.034 | 89.091 | 91.528 | 88.336 | 103.474 | 112.487 |
| quic | dependent&#45;read | portable | 5000 | 106.032 | 113.594 | 7.956 | 0.877 | 112.800 | 115.055 | 110.915 | 128.925 | 145.985 |
| quic | pipelined&#45;read | legacy | 5000 | 49.565 | 58.938 | 11.428 | 11.803 | 52.336 | 80.003 | 53.117 | 79.494 | 92.658 |
| quic | pipelined&#45;read | portable | 5000 | 73.636 | 89.730 | 21.955 | 21.737 | 78.022 | 128.264 | 77.211 | 128.919 | 144.015 |
| quic | write&#45;read | legacy | 5000 | 79.014 | 85.728 | 6.466 | 1.507 | 83.765 | 87.463 | 83.582 | 99.873 | 110.858 |
| quic | write&#45;read | portable | 5000 | 84.870 | 92.529 | 15.564 | 2.490 | 90.520 | 96.607 | 89.812 | 105.862 | 120.183 |
| quic | callback | legacy | 5000 | 83.377 | 89.638 | 6.112 | 1.867 | 87.876 | 92.803 | 87.586 | 102.551 | 113.100 |
| quic | callback | portable | 5000 | 102.418 | 110.576 | 7.976 | 1.175 | 109.434 | 112.061 | 107.971 | 126.107 | 141.777 |
| quic | bulk&#45;64k | legacy | 5000 | 143.182 | 156.401 | 21.552 | 9.277 | 151.612 | 172.961 | 148.553 | 231.053 | 248.503 |
| quic | bulk&#45;64k | portable | 5000 | 147.297 | 191.574 | 45.756 | 47.734 | 155.221 | 248.728 | 160.526 | 260.320 | 269.181 |
| quic | bulk&#45;256k | legacy | 5000 | 364.791 | 393.220 | 37.096 | 16.589 | 384.019 | 422.663 | 382.496 | 439.773 | 564.571 |
| quic | bulk&#45;256k | portable | 5000 | 382.025 | 416.523 | 55.878 | 37.131 | 399.168 | 482.938 | 396.650 | 589.080 | 606.565 |
| quic | bulk&#45;1m | legacy | 5000 | 1383.094 | 1503.599 | 429.734 | 55.845 | 1442.214 | 1578.693 | 1437.849 | 2007.472 | 2072.171 |
| quic | bulk&#45;1m | portable | 5000 | 1383.381 | 1485.241 | 146.817 | 40.069 | 1444.406 | 1543.411 | 1443.122 | 1959.362 | 2019.230 |
| quic | bulk&#45;4m | legacy | 5000 | 6315.378 | 7209.002 | 914.008 | 193.157 | 6993.226 | 7440.705 | 6868.611 | 9183.894 | 9413.078 |
| quic | bulk&#45;4m | portable | 5000 | 6390.391 | 7197.206 | 830.618 | 280.755 | 6958.732 | 7596.435 | 6918.648 | 9086.464 | 9381.595 |
| quic | deep&#45;8 | legacy | 5000 | 40.982 | 52.563 | 10.176 | 10.517 | 44.600 | 64.299 | 46.021 | 65.016 | 77.501 |
| quic | deep&#45;8 | portable | 5000 | 44.190 | 58.108 | 13.506 | 12.315 | 47.372 | 74.061 | 48.989 | 75.430 | 89.594 |
| quic | deep&#45;32 | legacy | 5000 | 47.824 | 56.813 | 10.647 | 10.998 | 51.703 | 76.481 | 51.342 | 76.032 | 87.432 |
| quic | deep&#45;32 | portable | 5000 | 50.655 | 55.884 | 4.993 | 0.821 | 54.775 | 56.903 | 54.391 | 66.401 | 75.394 |
| quic | concurrent&#45;32 | legacy | 5000 | 119.000 | 128.714 | 8.539 | 2.285 | 127.229 | 132.764 | 126.193 | 145.019 | 158.543 |
| quic | concurrent&#45;32 | portable | 5000 | 145.018 | 156.379 | 12.578 | 2.690 | 153.697 | 160.189 | 153.077 | 175.384 | 195.041 |
| quic | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 350.759 | 404.275 | 49.654 | 42.940 | 371.442 | 452.986 | 381.479 | 473.115 | 503.666 |
| quic | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 387.717 | 431.335 | 44.787 | 32.975 | 408.265 | 488.465 | 410.808 | 506.350 | 553.585 |
| quic | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 365.809 | 398.220 | 21.047 | 5.287 | 393.404 | 406.857 | 395.782 | 430.042 | 449.645 |
| quic | services&#45;4&#45;deep&#45;16 | portable | 5000 | 334.314 | 366.509 | 17.452 | 4.962 | 360.426 | 373.671 | 363.725 | 394.196 | 409.546 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1533.849 | 1603.375 | 39.493 | 2.433 | 1599.509 | 1605.801 | 1597.904 | 1657.661 | 1730.481 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1589.766 | 1668.678 | 40.734 | 12.679 | 1649.530 | 1679.923 | 1661.873 | 1738.764 | 1792.233 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1463.245 | 1673.449 | 88.864 | 29.909 | 1641.712 | 1705.048 | 1673.535 | 1801.097 | 1904.457 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1373.476 | 1712.749 | 99.601 | 39.628 | 1658.997 | 1752.789 | 1729.200 | 1849.558 | 1885.897 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 3093.064 | 3501.830 | 158.071 | 22.550 | 3472.978 | 3523.649 | 3530.720 | 3725.842 | 3826.063 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2727.637 | 3523.266 | 292.102 | 25.139 | 3494.459 | 3549.289 | 3574.826 | 3916.724 | 3991.805 |

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


