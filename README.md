# Capntproto verification reports

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-30T15:03:29Z · run 41077167423 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41077167423) · commit `c06dbdcd33f6` · **failure**

![Cargo test results](docs/reports/cargo-history.svg)

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-30T15:03:29Z · run 41077167423 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41077167423) · commit `c06dbdcd33f6` · **failure**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-30T15:03:29Z · run 41077167423 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41077167423) · commit `c06dbdcd33f6` · **failure**

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

Latest coverage run: [2026-10-30T15:03:29Z · run 41077167389 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41077167389) · commit `c06dbdcd33f6` · **cancelled**

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-30T15:03:29Z · run 41077167423 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41077167423) · commit `c06dbdcd33f6` · **failure**

Measurement evidence: **passed**. Independently validated encrypted benchmark values&#46; Workflow status and diagnostic completion are separate&#59; unavailable values remain blank&#46; Operations include whole concurrent batches and every service hop&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Current TCP fixtures use TLS 1.3, including upstream Rust Cap’n Proto, C++, gRPC over HTTP/2 and WebSockets over WSS; native encrypted transports and standard QUIC are identified separately. Historical data retains its recorded fixture contract. The dedicated campaign also reports recursive data, concurrent calls and independent service chains for gRPC and binary Protobuf WebSockets in its encrypted comparison artifact. Bars compare the recorded workload, rather than universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.


## Exhaustive benchmark tables

Measured run: [2026-10-30T15:03:29Z · run 41077167423 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41077167423) · commit `c06dbdcd33f6` · **failure**

All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.

### All encrypted latency comparisons

| Transport | Case | Implementation | Samples | Minimum µs | Mean µs | Sample SD µs | Run mean SD µs | Run mean min µs | Run mean max µs | p50 µs | p95 µs | p99 µs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tls | loopback&#45;0 | cpp | 5000 | 53.866 | 72.026 | 20.795 | 20.117 | 61.178 | 107.876 | 65.359 | 111.160 | 126.081 |
| tls | loopback&#45;0 | grpc | 5000 | 62.098 | 80.141 | 21.193 | 20.554 | 69.416 | 116.856 | 70.621 | 119.543 | 138.553 |
| tls | loopback&#45;0 | legacy | 5000 | 32.992 | 55.611 | 22.981 | 13.726 | 39.461 | 65.759 | 62.456 | 76.287 | 88.373 |
| tls | loopback&#45;0 | portable | 5000 | 34.327 | 47.264 | 13.008 | 12.362 | 41.007 | 69.355 | 40.842 | 68.785 | 84.605 |
| tls | loopback&#45;0 | websocket | 5000 | 31.028 | 45.502 | 10.673 | 9.970 | 38.036 | 56.510 | 39.228 | 61.677 | 73.289 |
| tls | loopback&#45;64 | cpp | 5000 | 53.578 | 71.019 | 21.642 | 21.990 | 59.889 | 110.316 | 61.006 | 111.800 | 127.630 |
| tls | loopback&#45;64 | grpc | 5000 | 60.456 | 80.337 | 21.748 | 21.621 | 69.247 | 118.965 | 70.817 | 121.347 | 141.301 |
| tls | loopback&#45;64 | legacy | 5000 | 32.788 | 45.875 | 11.739 | 11.222 | 40.106 | 65.927 | 40.173 | 65.231 | 82.191 |
| tls | loopback&#45;64 | portable | 5000 | 33.502 | 47.260 | 13.121 | 12.609 | 40.884 | 69.797 | 40.731 | 68.945 | 85.154 |
| tls | loopback&#45;64 | websocket | 5000 | 26.532 | 35.944 | 6.332 | 0.154 | 35.746 | 36.161 | 34.778 | 44.676 | 57.193 |
| tls | loopback&#45;1024 | cpp | 5000 | 55.534 | 89.272 | 26.812 | 25.095 | 65.234 | 115.740 | 83.309 | 128.383 | 144.706 |
| tls | loopback&#45;1024 | grpc | 5000 | 67.530 | 90.216 | 24.243 | 22.621 | 78.637 | 130.555 | 78.416 | 134.946 | 154.369 |
| tls | loopback&#45;1024 | legacy | 5000 | 38.385 | 62.577 | 14.587 | 11.774 | 43.365 | 70.745 | 66.787 | 83.110 | 98.691 |
| tls | loopback&#45;1024 | portable | 5000 | 35.203 | 57.387 | 16.698 | 16.173 | 45.129 | 75.907 | 49.402 | 83.269 | 100.203 |
| tls | loopback&#45;1024 | websocket | 5000 | 29.889 | 47.221 | 12.019 | 10.603 | 35.354 | 55.367 | 48.659 | 64.464 | 76.874 |
| tls | loopback&#45;65536 | cpp | 5000 | 231.372 | 297.394 | 64.092 | 43.700 | 268.376 | 372.198 | 275.596 | 431.743 | 469.489 |
| tls | loopback&#45;65536 | grpc | 5000 | 238.069 | 309.054 | 65.724 | 48.816 | 271.674 | 381.385 | 280.211 | 442.906 | 480.407 |
| tls | loopback&#45;65536 | legacy | 5000 | 161.034 | 188.382 | 34.220 | 25.119 | 176.415 | 233.307 | 175.647 | 273.454 | 301.829 |
| tls | loopback&#45;65536 | portable | 5000 | 167.938 | 186.361 | 15.542 | 0.801 | 185.565 | 187.583 | 182.460 | 214.718 | 239.472 |
| tls | loopback&#45;65536 | websocket | 5000 | 184.269 | 209.481 | 30.390 | 16.143 | 199.968 | 238.255 | 199.434 | 288.707 | 318.613 |
| tls | unary&#45;empty | cpp | 5000 | 54.160 | 82.199 | 25.318 | 26.034 | 62.639 | 110.879 | 70.520 | 121.845 | 135.708 |
| tls | unary&#45;empty | grpc | 5000 | 62.691 | 87.016 | 24.905 | 20.856 | 71.861 | 118.825 | 73.847 | 128.536 | 148.640 |
| tls | unary&#45;empty | legacy | 5000 | 34.822 | 51.004 | 13.733 | 13.213 | 40.463 | 66.575 | 43.358 | 72.430 | 86.402 |
| tls | unary&#45;empty | portable | 5000 | 37.646 | 53.217 | 15.127 | 15.535 | 41.255 | 70.280 | 43.932 | 77.449 | 91.326 |
| tls | unary&#45;empty | websocket | 5000 | 29.810 | 45.991 | 11.356 | 10.639 | 38.024 | 57.714 | 39.766 | 63.118 | 75.665 |
| tls | unary&#45;small | cpp | 5000 | 53.857 | 81.809 | 25.795 | 26.894 | 60.674 | 111.258 | 68.880 | 122.139 | 138.600 |
| tls | unary&#45;small | grpc | 5000 | 62.113 | 94.172 | 27.767 | 24.882 | 71.279 | 121.728 | 84.639 | 133.762 | 152.554 |
| tls | unary&#45;small | legacy | 5000 | 32.185 | 51.036 | 14.297 | 13.511 | 40.143 | 66.067 | 42.865 | 71.957 | 85.156 |
| tls | unary&#45;small | portable | 5000 | 37.867 | 58.615 | 17.548 | 13.235 | 42.050 | 70.846 | 66.768 | 82.545 | 95.754 |
| tls | unary&#45;small | websocket | 5000 | 27.343 | 38.190 | 7.784 | 3.603 | 36.165 | 44.572 | 35.663 | 53.900 | 64.543 |
| tls | unary&#45;1k | cpp | 5000 | 56.571 | 75.367 | 22.797 | 22.983 | 64.570 | 116.474 | 67.483 | 118.707 | 134.512 |
| tls | unary&#45;1k | grpc | 5000 | 67.048 | 100.879 | 29.599 | 29.934 | 77.626 | 134.390 | 86.049 | 146.758 | 169.888 |
| tls | unary&#45;1k | legacy | 5000 | 34.558 | 54.264 | 14.708 | 14.301 | 43.190 | 69.965 | 46.008 | 77.288 | 90.680 |
| tls | unary&#45;1k | portable | 5000 | 37.731 | 56.715 | 16.308 | 16.533 | 44.387 | 75.763 | 47.517 | 83.480 | 97.912 |
| tls | unary&#45;1k | websocket | 5000 | 29.134 | 36.421 | 5.307 | 0.670 | 35.794 | 37.350 | 35.298 | 45.276 | 54.602 |
| tls | fanout&#45;32 | cpp | 5000 | 443.268 | 649.476 | 213.866 | 65.749 | 570.038 | 733.992 | 524.816 | 1043.923 | 1090.471 |
| tls | fanout&#45;32 | grpc | 5000 | 808.337 | 1003.774 | 185.406 | 61.005 | 924.287 | 1072.103 | 924.367 | 1367.035 | 1423.975 |
| tls | fanout&#45;32 | legacy | 5000 | 175.934 | 224.271 | 60.970 | 49.169 | 199.388 | 312.116 | 200.853 | 372.784 | 398.139 |
| tls | fanout&#45;32 | portable | 5000 | 209.792 | 248.303 | 41.551 | 14.114 | 237.329 | 272.976 | 236.327 | 318.008 | 416.229 |
| tls | fanout&#45;32 | websocket | 5000 | 235.611 | 282.376 | 41.740 | 21.291 | 269.033 | 319.937 | 271.369 | 390.453 | 439.192 |
| tls | dependent&#45;read | cpp | 5000 | 124.870 | 168.502 | 51.213 | 27.942 | 145.875 | 208.876 | 148.962 | 274.654 | 293.956 |
| tls | dependent&#45;read | grpc | 5000 | 134.038 | 153.495 | 19.245 | 2.677 | 149.496 | 156.907 | 148.508 | 184.618 | 241.797 |
| tls | dependent&#45;read | legacy | 5000 | 75.861 | 88.952 | 10.054 | 1.144 | 87.206 | 89.840 | 86.437 | 106.110 | 125.816 |
| tls | dependent&#45;read | portable | 5000 | 97.933 | 137.084 | 40.306 | 32.886 | 110.652 | 177.412 | 116.496 | 207.411 | 231.600 |
| tls | dependent&#45;read | websocket | 5000 | 58.290 | 79.056 | 18.131 | 15.866 | 70.763 | 107.368 | 72.813 | 116.353 | 131.535 |
| tls | pipelined&#45;read | cpp | 5000 | 87.980 | 105.377 | 21.722 | 4.937 | 101.184 | 113.911 | 94.548 | 150.391 | 172.628 |
| tls | pipelined&#45;read | legacy | 5000 | 48.411 | 59.855 | 15.662 | 12.294 | 53.847 | 81.836 | 53.601 | 90.264 | 110.197 |
| tls | pipelined&#45;read | portable | 5000 | 73.373 | 119.070 | 35.123 | 32.502 | 83.305 | 148.231 | 139.111 | 167.361 | 190.878 |
| tls | write&#45;read | cpp | 5000 | 109.044 | 135.405 | 32.174 | 12.517 | 123.202 | 155.910 | 125.597 | 213.981 | 242.322 |
| tls | write&#45;read | grpc | 5000 | 133.829 | 161.538 | 34.199 | 22.415 | 149.391 | 201.570 | 149.349 | 247.529 | 273.235 |
| tls | write&#45;read | legacy | 5000 | 72.454 | 82.545 | 9.131 | 1.133 | 81.597 | 83.985 | 80.620 | 97.644 | 111.290 |
| tls | write&#45;read | portable | 5000 | 74.070 | 96.722 | 25.633 | 26.735 | 83.164 | 144.516 | 83.830 | 148.189 | 167.803 |
| tls | write&#45;read | websocket | 5000 | 58.102 | 70.705 | 8.152 | 0.885 | 69.655 | 71.670 | 69.638 | 85.004 | 94.978 |
| tls | callback | cpp | 5000 | 116.418 | 168.649 | 54.479 | 49.913 | 132.162 | 238.264 | 143.550 | 254.179 | 279.070 |
| tls | callback | grpc | 5000 | 69.000 | 76.662 | 7.769 | 0.485 | 76.136 | 77.361 | 73.982 | 90.597 | 103.912 |
| tls | callback | legacy | 5000 | 77.458 | 88.735 | 9.425 | 0.962 | 87.552 | 89.859 | 86.692 | 105.342 | 120.711 |
| tls | callback | portable | 5000 | 91.937 | 105.399 | 11.845 | 1.438 | 103.635 | 107.515 | 102.075 | 126.993 | 147.637 |
| tls | callback | websocket | 5000 | 56.678 | 69.459 | 7.082 | 0.499 | 69.040 | 70.286 | 68.202 | 81.499 | 91.848 |
| tls | bulk&#45;64k | cpp | 5000 | 231.214 | 265.858 | 27.737 | 3.251 | 261.256 | 269.833 | 258.941 | 312.417 | 342.717 |
| tls | bulk&#45;64k | grpc | 5000 | 238.071 | 275.231 | 23.120 | 4.952 | 268.716 | 280.509 | 271.017 | 316.822 | 350.334 |
| tls | bulk&#45;64k | legacy | 5000 | 161.797 | 178.771 | 14.225 | 1.504 | 177.174 | 180.575 | 175.056 | 204.161 | 225.377 |
| tls | bulk&#45;64k | portable | 5000 | 167.866 | 201.114 | 38.766 | 33.438 | 185.640 | 260.923 | 186.271 | 290.824 | 309.999 |
| tls | bulk&#45;64k | websocket | 5000 | 184.235 | 201.967 | 16.324 | 1.914 | 200.426 | 205.069 | 198.039 | 229.142 | 267.333 |
| tls | bulk&#45;256k | cpp | 5000 | 961.348 | 1205.951 | 196.380 | 33.851 | 1172.878 | 1253.071 | 1121.815 | 1536.758 | 1591.027 |
| tls | bulk&#45;256k | grpc | 5000 | 614.820 | 1567.198 | 267.907 | 77.200 | 1444.878 | 1645.305 | 1584.509 | 1934.604 | 2005.284 |
| tls | bulk&#45;256k | legacy | 5000 | 479.309 | 563.965 | 76.893 | 60.293 | 522.004 | 668.440 | 530.317 | 718.712 | 754.984 |
| tls | bulk&#45;256k | portable | 5000 | 493.140 | 540.571 | 26.659 | 3.140 | 535.877 | 543.960 | 536.265 | 585.025 | 627.336 |
| tls | bulk&#45;256k | websocket | 5000 | 1177.535 | 1270.909 | 57.523 | 15.427 | 1248.932 | 1286.434 | 1251.296 | 1369.063 | 1413.081 |
| tls | bulk&#45;1m | cpp | 5000 | 3751.689 | 4445.298 | 617.265 | 157.503 | 4250.095 | 4593.624 | 4199.907 | 5781.333 | 5918.195 |
| tls | bulk&#45;1m | grpc | 5000 | 2286.352 | 5910.713 | 1720.907 | 1812.145 | 2693.442 | 6921.626 | 6355.461 | 7790.571 | 8132.332 |
| tls | bulk&#45;1m | legacy | 5000 | 1640.659 | 1922.181 | 226.312 | 29.680 | 1888.975 | 1961.995 | 1783.808 | 2254.988 | 2331.688 |
| tls | bulk&#45;1m | portable | 5000 | 1640.928 | 1923.970 | 240.641 | 41.838 | 1862.391 | 1972.719 | 1778.052 | 2285.974 | 2439.052 |
| tls | bulk&#45;1m | websocket | 5000 | 4762.823 | 5362.440 | 400.382 | 111.258 | 5262.611 | 5553.067 | 5200.486 | 6068.150 | 6360.889 |
| tls | bulk&#45;4m | cpp | 5000 | 16776.245 | 19357.056 | 2527.346 | 421.963 | 18808.698 | 19966.047 | 18321.472 | 25094.203 | 25800.822 |
| tls | bulk&#45;4m | grpc | 5000 | 21991.891 | 29481.180 | 3242.782 | 2894.219 | 24322.074 | 31032.111 | 29559.120 | 34075.958 | 35084.443 |
| tls | bulk&#45;4m | legacy | 5000 | 7978.600 | 9068.004 | 852.684 | 138.709 | 8879.911 | 9203.995 | 8663.634 | 10608.690 | 10833.201 |
| tls | bulk&#45;4m | portable | 5000 | 8014.701 | 9151.216 | 905.135 | 181.901 | 8848.908 | 9281.978 | 8668.694 | 10624.422 | 10924.264 |
| tls | bulk&#45;4m | websocket | 5000 | 29755.059 | 32793.883 | 2539.754 | 467.466 | 32347.001 | 33588.263 | 31423.359 | 37299.869 | 38013.152 |
| tls | deep&#45;8 | cpp | 5000 | 56.489 | 64.326 | 9.568 | 2.043 | 61.852 | 67.385 | 59.070 | 83.086 | 95.892 |
| tls | deep&#45;8 | grpc | 5000 | 75.787 | 86.996 | 11.581 | 2.430 | 84.416 | 89.820 | 83.998 | 109.030 | 124.116 |
| tls | deep&#45;8 | legacy | 5000 | 34.921 | 49.733 | 13.291 | 13.010 | 42.534 | 72.885 | 43.707 | 72.616 | 89.313 |
| tls | deep&#45;8 | portable | 5000 | 38.836 | 44.439 | 6.125 | 1.055 | 42.956 | 45.648 | 42.092 | 55.514 | 67.071 |
| tls | deep&#45;8 | websocket | 5000 | 34.320 | 46.845 | 8.205 | 1.607 | 45.421 | 49.584 | 45.623 | 60.423 | 71.789 |
| tls | deep&#45;32 | cpp | 5000 | 62.503 | 84.658 | 26.060 | 26.712 | 69.573 | 132.030 | 73.747 | 134.578 | 148.976 |
| tls | deep&#45;32 | grpc | 5000 | 92.642 | 141.640 | 45.795 | 44.065 | 108.301 | 201.155 | 118.945 | 213.742 | 233.800 |
| tls | deep&#45;32 | legacy | 5000 | 45.368 | 52.626 | 7.832 | 1.097 | 51.031 | 53.856 | 50.280 | 67.319 | 80.183 |
| tls | deep&#45;32 | portable | 5000 | 42.910 | 61.221 | 19.219 | 19.715 | 51.432 | 96.456 | 52.221 | 98.401 | 114.752 |
| tls | deep&#45;32 | websocket | 5000 | 52.506 | 68.765 | 12.686 | 2.319 | 65.232 | 71.462 | 66.448 | 90.532 | 117.825 |
| tls | concurrent&#45;32 | cpp | 5000 | 480.703 | 856.966 | 286.899 | 67.211 | 790.190 | 961.212 | 856.183 | 1216.915 | 1257.083 |
| tls | concurrent&#45;32 | grpc | 5000 | 600.301 | 842.088 | 172.913 | 62.154 | 733.593 | 887.849 | 827.332 | 1081.655 | 1121.496 |
| tls | concurrent&#45;32 | legacy | 5000 | 122.456 | 146.943 | 24.887 | 4.905 | 140.222 | 152.033 | 136.218 | 199.746 | 219.810 |
| tls | concurrent&#45;32 | portable | 5000 | 149.854 | 180.656 | 30.304 | 5.696 | 172.652 | 187.944 | 168.080 | 244.267 | 267.387 |
| tls | concurrent&#45;32 | websocket | 5000 | 116.158 | 184.784 | 26.392 | 2.285 | 182.173 | 187.874 | 182.502 | 231.427 | 254.612 |
| tls | deep&#45;32&#45;concurrent&#45;32 | cpp | 5000 | 634.504 | 1049.645 | 301.234 | 85.962 | 944.897 | 1149.692 | 1056.171 | 1421.740 | 1468.151 |
| tls | deep&#45;32&#45;concurrent&#45;32 | grpc | 5000 | 1511.072 | 2222.724 | 468.448 | 115.931 | 2089.235 | 2375.406 | 2197.158 | 2868.123 | 2940.907 |
| tls | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 400.212 | 635.805 | 184.441 | 43.422 | 601.701 | 695.558 | 598.637 | 882.078 | 915.342 |
| tls | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 428.864 | 675.580 | 168.309 | 13.911 | 657.226 | 695.027 | 695.840 | 871.861 | 906.632 |
| tls | deep&#45;32&#45;concurrent&#45;32 | websocket | 5000 | 672.381 | 998.662 | 223.890 | 128.654 | 829.930 | 1112.365 | 966.332 | 1287.736 | 1345.441 |
| tls | services&#45;4&#45;deep&#45;16 | cpp | 5000 | 466.631 | 518.991 | 47.340 | 3.778 | 514.707 | 524.676 | 513.383 | 567.045 | 615.791 |
| tls | services&#45;4&#45;deep&#45;16 | grpc | 5000 | 585.224 | 658.425 | 30.665 | 3.157 | 653.603 | 661.748 | 653.999 | 713.015 | 756.758 |
| tls | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 342.813 | 378.428 | 24.556 | 3.325 | 374.078 | 382.203 | 374.788 | 421.812 | 463.375 |
| tls | services&#45;4&#45;deep&#45;16 | portable | 5000 | 376.952 | 417.768 | 26.242 | 1.250 | 415.997 | 419.412 | 413.468 | 464.776 | 509.430 |
| tls | services&#45;4&#45;deep&#45;16 | websocket | 5000 | 267.936 | 351.420 | 61.333 | 5.331 | 345.333 | 359.701 | 348.531 | 419.199 | 457.916 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | 5000 | 2882.740 | 4227.340 | 304.362 | 43.970 | 4163.731 | 4275.773 | 4205.807 | 4745.386 | 4936.331 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | 5000 | 5148.918 | 6526.457 | 395.520 | 20.761 | 6501.506 | 6555.473 | 6528.186 | 7144.111 | 7375.618 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1857.765 | 2408.647 | 149.315 | 9.242 | 2393.234 | 2415.227 | 2422.955 | 2623.773 | 2684.864 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 2009.162 | 2497.183 | 140.181 | 15.968 | 2480.972 | 2518.646 | 2506.482 | 2699.777 | 2773.973 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | 5000 | 1748.395 | 2019.470 | 81.949 | 6.426 | 2013.291 | 2027.749 | 2014.863 | 2158.208 | 2227.970 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 2799.837 | 3741.607 | 438.999 | 34.304 | 3694.856 | 3791.167 | 3714.827 | 4473.078 | 4636.302 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 4634.444 | 6084.443 | 536.231 | 42.696 | 6019.189 | 6126.017 | 6128.575 | 6882.107 | 7105.457 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1616.578 | 2463.513 | 162.636 | 8.253 | 2450.069 | 2470.989 | 2475.302 | 2679.831 | 2768.567 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1829.031 | 2466.935 | 123.350 | 6.841 | 2462.297 | 2478.844 | 2490.306 | 2635.933 | 2707.548 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1470.727 | 1822.818 | 134.603 | 6.355 | 1814.696 | 1828.365 | 1813.739 | 2053.881 | 2172.691 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 5122.791 | 7514.625 | 865.211 | 64.058 | 7425.079 | 7579.482 | 7639.639 | 8731.447 | 9064.983 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 8481.981 | 11561.445 | 1346.271 | 113.689 | 11474.452 | 11760.671 | 11738.331 | 13458.704 | 13919.627 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 3001.691 | 4982.504 | 235.352 | 39.444 | 4941.860 | 5023.356 | 4988.880 | 5347.650 | 5509.750 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3409.602 | 4920.646 | 248.110 | 10.630 | 4909.509 | 4935.724 | 4959.845 | 5247.489 | 5358.879 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 2755.592 | 3420.140 | 262.605 | 9.630 | 3405.483 | 3431.523 | 3375.479 | 3897.884 | 4182.941 |
| quic | loopback&#45;0 | legacy | 5000 | 49.128 | 68.465 | 19.824 | 20.511 | 53.130 | 91.505 | 57.096 | 98.931 | 115.486 |
| quic | loopback&#45;0 | portable | 5000 | 53.988 | 68.557 | 19.444 | 19.617 | 58.682 | 103.612 | 58.660 | 103.682 | 122.011 |
| quic | loopback&#45;64 | legacy | 5000 | 49.486 | 69.478 | 19.023 | 19.781 | 54.598 | 91.754 | 58.608 | 98.664 | 111.927 |
| quic | loopback&#45;64 | portable | 5000 | 54.162 | 65.251 | 13.359 | 5.633 | 61.194 | 74.857 | 60.234 | 98.780 | 112.548 |
| quic | loopback&#45;1024 | legacy | 5000 | 51.918 | 73.151 | 20.994 | 20.988 | 57.615 | 97.119 | 61.006 | 105.127 | 124.823 |
| quic | loopback&#45;1024 | portable | 5000 | 57.711 | 75.648 | 21.034 | 19.730 | 64.688 | 110.534 | 64.905 | 114.818 | 132.366 |
| quic | loopback&#45;65536 | legacy | 5000 | 212.908 | 249.779 | 43.645 | 28.267 | 233.681 | 300.076 | 234.795 | 365.313 | 397.796 |
| quic | loopback&#45;65536 | portable | 5000 | 222.678 | 251.470 | 27.163 | 7.976 | 242.547 | 263.570 | 245.118 | 291.617 | 376.642 |
| quic | unary&#45;empty | legacy | 5000 | 47.397 | 55.108 | 12.923 | 1.009 | 53.782 | 56.014 | 52.531 | 67.995 | 81.384 |
| quic | unary&#45;empty | portable | 5000 | 52.084 | 60.415 | 8.489 | 0.991 | 59.571 | 62.046 | 57.386 | 75.496 | 87.978 |
| quic | unary&#45;small | legacy | 5000 | 46.360 | 54.757 | 7.689 | 0.525 | 53.997 | 55.222 | 52.299 | 67.287 | 80.657 |
| quic | unary&#45;small | portable | 5000 | 54.320 | 66.031 | 18.134 | 12.944 | 59.560 | 89.156 | 58.246 | 102.681 | 123.014 |
| quic | unary&#45;1k | legacy | 5000 | 51.421 | 59.994 | 12.504 | 5.729 | 57.059 | 70.233 | 55.903 | 91.422 | 105.073 |
| quic | unary&#45;1k | portable | 5000 | 54.968 | 64.388 | 9.933 | 1.001 | 63.024 | 65.680 | 61.649 | 79.973 | 93.807 |
| quic | fanout&#45;32 | legacy | 5000 | 211.792 | 253.637 | 45.134 | 12.066 | 244.325 | 271.436 | 241.032 | 366.419 | 427.316 |
| quic | fanout&#45;32 | portable | 5000 | 249.315 | 295.545 | 44.120 | 11.527 | 283.762 | 309.324 | 281.833 | 375.948 | 455.346 |
| quic | dependent&#45;read | legacy | 5000 | 106.177 | 131.420 | 32.638 | 29.595 | 116.296 | 184.263 | 117.620 | 203.610 | 229.360 |
| quic | dependent&#45;read | portable | 5000 | 141.715 | 173.360 | 38.412 | 27.213 | 159.757 | 221.973 | 159.822 | 269.318 | 299.676 |
| quic | pipelined&#45;read | legacy | 5000 | 63.359 | 73.112 | 14.068 | 5.064 | 70.393 | 82.143 | 67.825 | 113.398 | 128.695 |
| quic | pipelined&#45;read | portable | 5000 | 97.683 | 110.863 | 13.063 | 0.275 | 110.538 | 111.267 | 107.505 | 133.317 | 154.764 |
| quic | write&#45;read | legacy | 5000 | 99.388 | 115.648 | 20.776 | 8.546 | 110.743 | 130.887 | 109.392 | 178.119 | 197.062 |
| quic | write&#45;read | portable | 5000 | 111.841 | 145.918 | 41.477 | 39.601 | 122.208 | 215.163 | 124.857 | 224.366 | 251.327 |
| quic | callback | legacy | 5000 | 106.816 | 141.351 | 37.955 | 30.809 | 117.363 | 178.672 | 122.294 | 207.591 | 238.321 |
| quic | callback | portable | 5000 | 134.146 | 162.708 | 36.314 | 26.194 | 148.418 | 209.370 | 149.714 | 252.465 | 280.261 |
| quic | bulk&#45;64k | legacy | 5000 | 212.006 | 236.827 | 23.086 | 6.316 | 232.871 | 248.062 | 231.524 | 270.006 | 354.031 |
| quic | bulk&#45;64k | portable | 5000 | 218.978 | 241.547 | 17.461 | 2.066 | 239.052 | 243.760 | 237.689 | 271.982 | 297.926 |
| quic | bulk&#45;256k | legacy | 5000 | 534.744 | 597.425 | 545.272 | 32.193 | 574.297 | 649.424 | 574.489 | 645.854 | 869.263 |
| quic | bulk&#45;256k | portable | 5000 | 557.886 | 657.314 | 781.246 | 28.731 | 609.242 | 685.083 | 614.057 | 895.444 | 999.689 |
| quic | bulk&#45;1m | legacy | 5000 | 1937.401 | 2237.355 | 513.242 | 160.029 | 2067.199 | 2416.288 | 2079.949 | 2910.842 | 2985.822 |
| quic | bulk&#45;1m | portable | 5000 | 1950.619 | 2340.734 | 779.122 | 179.508 | 2091.697 | 2597.659 | 2120.565 | 2938.238 | 3123.051 |
| quic | bulk&#45;4m | legacy | 5000 | 9424.271 | 11364.640 | 1778.588 | 86.596 | 11311.414 | 11517.253 | 10467.092 | 14096.512 | 14404.439 |
| quic | bulk&#45;4m | portable | 5000 | 9643.945 | 11597.476 | 1771.343 | 153.507 | 11416.337 | 11822.099 | 10733.297 | 14220.136 | 14646.196 |
| quic | deep&#45;8 | legacy | 5000 | 49.651 | 59.292 | 9.276 | 1.801 | 57.656 | 61.778 | 56.917 | 74.905 | 92.018 |
| quic | deep&#45;8 | portable | 5000 | 57.108 | 64.678 | 7.780 | 1.505 | 62.760 | 66.754 | 62.635 | 79.701 | 93.634 |
| quic | deep&#45;32 | legacy | 5000 | 59.803 | 85.163 | 27.341 | 24.889 | 66.514 | 120.825 | 70.687 | 129.774 | 154.673 |
| quic | deep&#45;32 | portable | 5000 | 66.071 | 75.819 | 10.019 | 1.748 | 74.255 | 78.636 | 73.544 | 95.065 | 110.297 |
| quic | concurrent&#45;32 | legacy | 5000 | 140.834 | 170.574 | 27.529 | 8.842 | 162.686 | 185.774 | 160.672 | 225.905 | 248.245 |
| quic | concurrent&#45;32 | portable | 5000 | 174.229 | 224.372 | 62.410 | 43.434 | 200.885 | 301.758 | 199.922 | 386.332 | 431.673 |
| quic | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 450.670 | 679.849 | 192.003 | 45.465 | 611.422 | 738.617 | 626.999 | 945.370 | 1006.781 |
| quic | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 488.082 | 688.168 | 171.521 | 31.635 | 634.363 | 712.382 | 629.119 | 929.208 | 962.190 |
| quic | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 510.412 | 556.231 | 26.381 | 1.304 | 554.695 | 557.642 | 551.311 | 604.829 | 642.583 |
| quic | services&#45;4&#45;deep&#45;16 | portable | 5000 | 482.997 | 540.932 | 51.490 | 11.243 | 521.748 | 550.122 | 536.048 | 591.540 | 622.495 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 2201.119 | 2753.595 | 158.639 | 22.023 | 2737.777 | 2790.612 | 2752.920 | 2938.915 | 3093.325 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 2270.456 | 2802.752 | 135.136 | 47.857 | 2759.675 | 2869.295 | 2801.147 | 3029.654 | 3144.823 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2222.205 | 2753.732 | 199.601 | 7.542 | 2745.538 | 2764.792 | 2694.703 | 3107.722 | 3223.674 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2011.471 | 2570.102 | 230.371 | 21.668 | 2556.766 | 2608.179 | 2644.564 | 2879.318 | 3058.157 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 4083.965 | 5509.032 | 425.979 | 122.246 | 5420.290 | 5724.352 | 5430.242 | 6228.161 | 6614.195 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3801.852 | 4906.747 | 573.202 | 68.850 | 4793.500 | 4962.469 | 5009.392 | 5807.700 | 6157.565 |

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


