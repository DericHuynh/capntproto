# Capntproto verification reports

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-30T18:17:59Z · run 41105725615 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41105725615) · commit `55b4b30f01ab` · **failure**

![Cargo test results](docs/reports/cargo-history.svg)

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-30T18:17:59Z · run 41105725615 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41105725615) · commit `55b4b30f01ab` · **failure**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-30T18:17:59Z · run 41105725615 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41105725615) · commit `55b4b30f01ab` · **failure**

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

Latest coverage run: [2026-10-30T19:08:49Z · run 41112813821 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41112813821) · commit `2a47cc7ad579` · **failure**

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-30T18:17:59Z · run 41105725615 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41105725615) · commit `55b4b30f01ab` · **failure**

Measurement evidence: **passed**. Independently validated encrypted benchmark values&#46; Workflow status and diagnostic completion are separate&#59; unavailable values remain blank&#46; Operations include whole concurrent batches and every service hop&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Current TCP fixtures use TLS 1.3, including upstream Rust Cap’n Proto, C++, gRPC over HTTP/2 and WebSockets over WSS; native encrypted transports and standard QUIC are identified separately. Historical data retains its recorded fixture contract. The dedicated campaign also reports recursive data, concurrent calls and independent service chains for gRPC and binary Protobuf WebSockets in its encrypted comparison artifact. Bars compare the recorded workload, rather than universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.


## Exhaustive benchmark tables

Measured run: [2026-10-30T18:17:59Z · run 41105725615 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/41105725615) · commit `55b4b30f01ab` · **failure**

All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.

### All encrypted latency comparisons

| Transport | Case | Implementation | Samples | Minimum µs | Mean µs | Sample SD µs | Run mean SD µs | Run mean min µs | Run mean max µs | p50 µs | p95 µs | p99 µs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tls | loopback&#45;0 | cpp | 5000 | 44.429 | 47.153 | 3.964 | 0.603 | 46.267 | 47.848 | 45.821 | 54.704 | 65.191 |
| tls | loopback&#45;0 | grpc | 5000 | 51.883 | 54.716 | 4.401 | 0.184 | 54.497 | 54.980 | 53.421 | 62.822 | 72.623 |
| tls | loopback&#45;0 | legacy | 5000 | 28.984 | 31.756 | 3.819 | 1.521 | 30.521 | 33.459 | 30.269 | 38.614 | 48.349 |
| tls | loopback&#45;0 | portable | 5000 | 30.394 | 32.573 | 3.600 | 0.805 | 31.953 | 33.949 | 31.578 | 38.304 | 47.107 |
| tls | loopback&#45;0 | websocket | 5000 | 25.920 | 30.486 | 3.192 | 0.285 | 30.138 | 30.858 | 29.961 | 33.750 | 41.105 |
| tls | loopback&#45;64 | cpp | 5000 | 44.165 | 47.737 | 4.342 | 1.387 | 46.543 | 50.066 | 46.183 | 55.407 | 65.734 |
| tls | loopback&#45;64 | grpc | 5000 | 52.542 | 56.262 | 5.011 | 0.986 | 55.052 | 57.368 | 54.230 | 66.192 | 78.204 |
| tls | loopback&#45;64 | legacy | 5000 | 29.278 | 31.048 | 2.804 | 0.182 | 30.754 | 31.173 | 30.291 | 36.058 | 44.334 |
| tls | loopback&#45;64 | portable | 5000 | 30.449 | 32.631 | 3.428 | 0.561 | 31.938 | 33.421 | 31.778 | 38.487 | 46.563 |
| tls | loopback&#45;64 | websocket | 5000 | 23.590 | 28.776 | 3.660 | 1.110 | 28.038 | 30.722 | 28.102 | 33.990 | 43.641 |
| tls | loopback&#45;1024 | cpp | 5000 | 46.704 | 50.767 | 24.315 | 1.501 | 48.733 | 52.766 | 48.889 | 58.990 | 69.372 |
| tls | loopback&#45;1024 | grpc | 5000 | 55.622 | 61.625 | 6.393 | 1.610 | 59.075 | 63.546 | 59.695 | 74.985 | 88.009 |
| tls | loopback&#45;1024 | legacy | 5000 | 30.911 | 33.811 | 3.717 | 0.622 | 32.772 | 34.288 | 32.465 | 40.258 | 50.179 |
| tls | loopback&#45;1024 | portable | 5000 | 31.770 | 35.653 | 4.312 | 0.815 | 34.608 | 36.878 | 34.088 | 44.270 | 54.276 |
| tls | loopback&#45;1024 | websocket | 5000 | 23.342 | 28.488 | 4.465 | 1.177 | 27.215 | 30.048 | 27.552 | 35.426 | 46.108 |
| tls | loopback&#45;65536 | cpp | 5000 | 199.775 | 216.439 | 22.853 | 7.158 | 211.477 | 228.965 | 209.748 | 243.426 | 335.511 |
| tls | loopback&#45;65536 | grpc | 5000 | 202.188 | 227.280 | 16.772 | 2.863 | 223.164 | 230.738 | 221.339 | 264.216 | 284.291 |
| tls | loopback&#45;65536 | legacy | 5000 | 144.544 | 161.906 | 25.250 | 18.706 | 153.239 | 195.366 | 151.085 | 228.924 | 243.304 |
| tls | loopback&#45;65536 | portable | 5000 | 151.113 | 168.785 | 26.079 | 17.684 | 158.500 | 200.302 | 157.564 | 237.389 | 250.989 |
| tls | loopback&#45;65536 | websocket | 5000 | 166.356 | 175.938 | 8.927 | 2.895 | 173.377 | 179.933 | 173.013 | 193.098 | 210.710 |
| tls | unary&#45;empty | cpp | 5000 | 44.779 | 52.691 | 12.231 | 11.173 | 46.977 | 72.588 | 46.517 | 79.022 | 91.358 |
| tls | unary&#45;empty | grpc | 5000 | 52.207 | 62.429 | 14.824 | 15.782 | 54.826 | 90.651 | 54.403 | 90.410 | 100.711 |
| tls | unary&#45;empty | legacy | 5000 | 28.982 | 30.883 | 2.862 | 0.369 | 30.529 | 31.408 | 30.188 | 35.554 | 42.886 |
| tls | unary&#45;empty | portable | 5000 | 30.386 | 33.108 | 3.764 | 1.311 | 32.194 | 35.409 | 31.837 | 39.989 | 49.363 |
| tls | unary&#45;empty | websocket | 5000 | 26.930 | 30.413 | 3.122 | 0.189 | 30.148 | 30.586 | 29.933 | 33.329 | 41.228 |
| tls | unary&#45;small | cpp | 5000 | 45.140 | 48.261 | 4.454 | 0.502 | 47.615 | 48.858 | 46.779 | 56.336 | 67.161 |
| tls | unary&#45;small | grpc | 5000 | 53.342 | 63.272 | 14.234 | 15.293 | 56.276 | 90.627 | 55.510 | 89.858 | 101.939 |
| tls | unary&#45;small | legacy | 5000 | 29.022 | 32.433 | 5.964 | 3.100 | 30.530 | 37.941 | 30.338 | 46.587 | 52.921 |
| tls | unary&#45;small | portable | 5000 | 30.904 | 36.142 | 7.569 | 5.656 | 32.926 | 46.131 | 32.499 | 51.831 | 59.956 |
| tls | unary&#45;small | websocket | 5000 | 24.087 | 30.818 | 5.567 | 4.765 | 28.405 | 39.336 | 29.132 | 41.725 | 47.067 |
| tls | unary&#45;1k | cpp | 5000 | 46.915 | 50.154 | 4.565 | 0.570 | 49.249 | 50.730 | 48.759 | 57.731 | 66.928 |
| tls | unary&#45;1k | grpc | 5000 | 56.469 | 60.451 | 5.274 | 1.184 | 59.497 | 62.460 | 58.421 | 70.706 | 82.070 |
| tls | unary&#45;1k | legacy | 5000 | 31.182 | 34.494 | 4.015 | 1.074 | 33.151 | 36.110 | 32.913 | 41.570 | 51.644 |
| tls | unary&#45;1k | portable | 5000 | 32.577 | 35.483 | 4.136 | 0.681 | 34.690 | 36.456 | 34.248 | 42.282 | 51.729 |
| tls | unary&#45;1k | websocket | 5000 | 23.570 | 28.497 | 3.921 | 1.055 | 27.287 | 29.689 | 27.764 | 34.780 | 44.623 |
| tls | fanout&#45;32 | cpp | 5000 | 393.078 | 447.243 | 49.901 | 18.690 | 422.180 | 474.199 | 428.892 | 534.269 | 680.168 |
| tls | fanout&#45;32 | grpc | 5000 | 668.264 | 709.353 | 43.171 | 6.743 | 702.462 | 718.001 | 697.125 | 783.444 | 947.068 |
| tls | fanout&#45;32 | legacy | 5000 | 155.898 | 167.159 | 9.975 | 3.470 | 161.780 | 171.317 | 164.299 | 185.816 | 204.505 |
| tls | fanout&#45;32 | portable | 5000 | 188.232 | 216.400 | 45.193 | 37.899 | 197.068 | 284.103 | 197.131 | 329.936 | 346.212 |
| tls | fanout&#45;32 | websocket | 5000 | 184.686 | 213.377 | 28.602 | 12.034 | 203.674 | 233.846 | 205.620 | 285.634 | 339.180 |
| tls | dependent&#45;read | cpp | 5000 | 105.449 | 111.744 | 5.943 | 0.896 | 110.625 | 112.707 | 109.660 | 123.189 | 133.175 |
| tls | dependent&#45;read | grpc | 5000 | 110.607 | 118.149 | 9.281 | 2.606 | 115.866 | 122.232 | 114.356 | 135.342 | 157.175 |
| tls | dependent&#45;read | legacy | 5000 | 64.797 | 77.488 | 18.005 | 18.827 | 68.255 | 111.150 | 67.896 | 113.133 | 128.498 |
| tls | dependent&#45;read | portable | 5000 | 84.910 | 97.431 | 21.104 | 16.737 | 89.358 | 127.363 | 88.127 | 149.568 | 163.877 |
| tls | dependent&#45;read | websocket | 5000 | 45.876 | 54.975 | 10.518 | 1.459 | 53.315 | 57.098 | 53.616 | 65.997 | 77.983 |
| tls | pipelined&#45;read | cpp | 5000 | 76.268 | 82.098 | 5.785 | 1.916 | 81.042 | 85.498 | 80.339 | 93.517 | 103.357 |
| tls | pipelined&#45;read | legacy | 5000 | 40.379 | 42.907 | 3.428 | 0.256 | 42.597 | 43.179 | 41.736 | 49.914 | 56.954 |
| tls | pipelined&#45;read | portable | 5000 | 64.126 | 68.903 | 5.936 | 1.998 | 67.377 | 72.136 | 66.552 | 80.147 | 94.140 |
| tls | write&#45;read | cpp | 5000 | 90.492 | 96.147 | 5.289 | 1.144 | 94.967 | 97.673 | 94.286 | 106.175 | 116.981 |
| tls | write&#45;read | grpc | 5000 | 110.453 | 120.221 | 16.904 | 7.402 | 116.226 | 133.439 | 114.687 | 143.710 | 195.821 |
| tls | write&#45;read | legacy | 5000 | 59.433 | 69.491 | 14.317 | 15.347 | 62.241 | 96.931 | 61.603 | 96.501 | 107.076 |
| tls | write&#45;read | portable | 5000 | 62.759 | 66.523 | 4.530 | 0.545 | 65.949 | 67.333 | 65.047 | 74.742 | 83.503 |
| tls | write&#45;read | websocket | 5000 | 45.307 | 58.438 | 13.270 | 12.964 | 52.076 | 81.612 | 53.362 | 87.766 | 99.141 |
| tls | callback | cpp | 5000 | 96.348 | 118.432 | 27.791 | 20.116 | 101.984 | 145.207 | 104.531 | 174.054 | 194.969 |
| tls | callback | grpc | 5000 | 54.782 | 65.398 | 13.237 | 13.334 | 57.985 | 89.084 | 58.536 | 87.980 | 99.196 |
| tls | callback | legacy | 5000 | 64.163 | 80.471 | 19.657 | 18.490 | 67.234 | 109.453 | 68.388 | 113.090 | 126.238 |
| tls | callback | portable | 5000 | 80.043 | 97.269 | 23.034 | 24.453 | 84.128 | 140.947 | 85.190 | 141.056 | 153.610 |
| tls | callback | websocket | 5000 | 46.780 | 65.505 | 13.149 | 12.370 | 54.575 | 81.337 | 60.940 | 87.294 | 98.175 |
| tls | bulk&#45;64k | cpp | 5000 | 197.168 | 223.817 | 41.049 | 27.296 | 209.225 | 272.515 | 208.679 | 323.904 | 335.063 |
| tls | bulk&#45;64k | grpc | 5000 | 202.910 | 231.542 | 28.961 | 10.252 | 225.854 | 249.690 | 222.062 | 306.850 | 340.772 |
| tls | bulk&#45;64k | legacy | 5000 | 144.834 | 154.750 | 14.510 | 4.234 | 151.754 | 162.218 | 150.199 | 171.255 | 229.839 |
| tls | bulk&#45;64k | portable | 5000 | 149.621 | 158.574 | 7.993 | 1.495 | 156.577 | 160.549 | 155.990 | 171.983 | 184.894 |
| tls | bulk&#45;64k | websocket | 5000 | 166.421 | 181.884 | 26.031 | 15.905 | 173.601 | 210.300 | 172.223 | 257.523 | 269.836 |
| tls | bulk&#45;256k | cpp | 5000 | 768.504 | 946.075 | 163.984 | 172.000 | 813.305 | 1135.902 | 831.702 | 1162.042 | 1193.952 |
| tls | bulk&#45;256k | grpc | 5000 | 495.951 | 943.429 | 281.426 | 283.968 | 550.946 | 1334.260 | 881.469 | 1386.427 | 1471.643 |
| tls | bulk&#45;256k | legacy | 5000 | 441.091 | 479.429 | 50.158 | 28.262 | 463.783 | 529.841 | 464.986 | 631.717 | 696.718 |
| tls | bulk&#45;256k | portable | 5000 | 449.179 | 549.456 | 88.434 | 96.890 | 474.191 | 657.286 | 488.996 | 665.522 | 694.942 |
| tls | bulk&#45;256k | websocket | 5000 | 951.355 | 1010.071 | 50.706 | 35.666 | 982.010 | 1071.454 | 990.670 | 1094.112 | 1158.628 |
| tls | bulk&#45;1m | cpp | 5000 | 3004.970 | 3132.232 | 184.043 | 42.654 | 3098.620 | 3189.590 | 3085.416 | 3279.947 | 4125.066 |
| tls | bulk&#45;1m | grpc | 5000 | 2052.433 | 3402.281 | 784.910 | 803.541 | 2871.418 | 4825.011 | 3132.329 | 5245.422 | 5738.259 |
| tls | bulk&#45;1m | legacy | 5000 | 1510.170 | 1584.033 | 113.509 | 57.091 | 1554.424 | 1685.968 | 1552.190 | 1944.673 | 2101.891 |
| tls | bulk&#45;1m | portable | 5000 | 1514.359 | 1656.672 | 181.844 | 193.176 | 1557.316 | 2001.818 | 1561.892 | 2016.634 | 2064.740 |
| tls | bulk&#45;1m | websocket | 5000 | 3730.147 | 4079.058 | 212.464 | 79.238 | 3976.162 | 4180.295 | 4039.696 | 4458.240 | 4923.947 |
| tls | bulk&#45;4m | cpp | 5000 | 13278.305 | 14836.424 | 1414.060 | 753.178 | 14284.395 | 16158.191 | 14404.137 | 18875.394 | 19899.095 |
| tls | bulk&#45;4m | grpc | 5000 | 16647.078 | 21660.378 | 2842.123 | 2601.778 | 18742.800 | 24673.943 | 21873.800 | 26706.251 | 28043.285 |
| tls | bulk&#45;4m | legacy | 5000 | 7297.470 | 8311.072 | 856.904 | 546.959 | 7827.990 | 8988.665 | 7958.158 | 10135.978 | 10535.504 |
| tls | bulk&#45;4m | portable | 5000 | 7272.129 | 8303.544 | 792.220 | 469.200 | 7944.466 | 9104.458 | 8007.173 | 10002.913 | 10465.147 |
| tls | bulk&#45;4m | websocket | 5000 | 24783.404 | 28001.094 | 2633.261 | 1063.364 | 26282.141 | 28937.763 | 26733.923 | 32996.537 | 33802.164 |
| tls | deep&#45;8 | cpp | 5000 | 47.455 | 52.778 | 5.089 | 2.818 | 50.554 | 56.185 | 50.981 | 62.429 | 73.161 |
| tls | deep&#45;8 | grpc | 5000 | 63.169 | 67.452 | 5.077 | 0.600 | 66.801 | 68.275 | 65.661 | 76.847 | 86.882 |
| tls | deep&#45;8 | legacy | 5000 | 31.488 | 34.880 | 4.058 | 1.913 | 33.217 | 37.114 | 33.379 | 42.127 | 50.840 |
| tls | deep&#45;8 | portable | 5000 | 33.165 | 36.250 | 3.908 | 0.594 | 35.362 | 37.010 | 35.094 | 43.125 | 53.300 |
| tls | deep&#45;8 | websocket | 5000 | 27.263 | 36.202 | 5.621 | 0.759 | 35.351 | 37.305 | 35.502 | 45.485 | 51.892 |
| tls | deep&#45;32 | cpp | 5000 | 53.708 | 66.310 | 14.224 | 14.543 | 57.648 | 92.220 | 59.556 | 92.609 | 102.610 |
| tls | deep&#45;32 | grpc | 5000 | 81.214 | 96.900 | 22.249 | 23.970 | 85.404 | 139.762 | 85.174 | 140.061 | 155.432 |
| tls | deep&#45;32 | legacy | 5000 | 38.542 | 47.059 | 10.537 | 10.620 | 41.405 | 65.980 | 41.911 | 68.733 | 77.648 |
| tls | deep&#45;32 | portable | 5000 | 39.581 | 49.300 | 11.583 | 11.763 | 43.601 | 70.337 | 43.949 | 71.271 | 81.704 |
| tls | deep&#45;32 | websocket | 5000 | 45.923 | 54.989 | 6.389 | 1.911 | 53.032 | 57.826 | 53.952 | 65.879 | 76.956 |
| tls | concurrent&#45;32 | cpp | 5000 | 439.091 | 482.915 | 31.119 | 13.919 | 464.195 | 498.181 | 473.434 | 538.354 | 574.746 |
| tls | concurrent&#45;32 | grpc | 5000 | 544.531 | 597.854 | 58.589 | 30.387 | 568.869 | 648.300 | 566.763 | 712.764 | 765.199 |
| tls | concurrent&#45;32 | legacy | 5000 | 104.884 | 133.123 | 31.831 | 30.423 | 115.030 | 186.953 | 116.052 | 191.960 | 201.809 |
| tls | concurrent&#45;32 | portable | 5000 | 133.439 | 164.241 | 42.196 | 45.405 | 140.792 | 245.326 | 143.173 | 250.265 | 274.231 |
| tls | concurrent&#45;32 | websocket | 5000 | 97.059 | 164.793 | 36.657 | 34.847 | 136.727 | 205.061 | 156.035 | 227.491 | 250.886 |
| tls | deep&#45;32&#45;concurrent&#45;32 | cpp | 5000 | 588.559 | 633.488 | 43.403 | 7.403 | 624.101 | 642.555 | 620.314 | 702.321 | 838.103 |
| tls | deep&#45;32&#45;concurrent&#45;32 | grpc | 5000 | 1369.301 | 1454.678 | 58.497 | 2.925 | 1450.821 | 1458.608 | 1442.999 | 1561.365 | 1644.774 |
| tls | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 387.731 | 411.558 | 18.156 | 1.418 | 409.713 | 413.153 | 406.133 | 447.571 | 470.908 |
| tls | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 401.412 | 423.777 | 20.972 | 2.234 | 420.192 | 426.108 | 417.617 | 465.418 | 489.931 |
| tls | deep&#45;32&#45;concurrent&#45;32 | websocket | 5000 | 654.161 | 725.284 | 85.807 | 50.780 | 701.629 | 816.114 | 696.883 | 982.523 | 1051.202 |
| tls | services&#45;4&#45;deep&#45;16 | cpp | 5000 | 335.470 | 369.785 | 19.511 | 5.005 | 362.355 | 376.373 | 366.751 | 405.065 | 430.670 |
| tls | services&#45;4&#45;deep&#45;16 | grpc | 5000 | 482.810 | 529.005 | 25.523 | 5.804 | 521.023 | 534.984 | 523.658 | 581.521 | 612.482 |
| tls | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 242.240 | 269.682 | 17.821 | 3.960 | 263.958 | 273.486 | 264.955 | 301.141 | 326.643 |
| tls | services&#45;4&#45;deep&#45;16 | portable | 5000 | 280.074 | 312.423 | 23.713 | 7.047 | 300.219 | 318.113 | 308.172 | 347.228 | 373.922 |
| tls | services&#45;4&#45;deep&#45;16 | websocket | 5000 | 193.561 | 264.239 | 35.963 | 5.373 | 255.516 | 269.551 | 261.383 | 324.003 | 349.609 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | 5000 | 2275.760 | 2618.554 | 97.209 | 31.567 | 2564.798 | 2645.754 | 2603.756 | 2803.962 | 2940.262 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | 5000 | 4164.812 | 4376.173 | 149.875 | 56.154 | 4309.024 | 4449.020 | 4343.109 | 4652.231 | 4905.803 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1449.960 | 1521.714 | 41.292 | 3.998 | 1517.134 | 1526.283 | 1508.494 | 1607.221 | 1684.622 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1601.618 | 1670.387 | 73.020 | 9.607 | 1657.494 | 1679.243 | 1651.098 | 1795.976 | 1847.326 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | 5000 | 1302.425 | 1470.664 | 72.321 | 12.292 | 1454.244 | 1482.195 | 1461.226 | 1601.944 | 1671.911 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 2048.738 | 2490.959 | 140.472 | 12.331 | 2476.488 | 2505.327 | 2492.579 | 2714.381 | 2862.931 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 3466.728 | 4140.390 | 198.379 | 110.061 | 4040.100 | 4328.475 | 4102.231 | 4483.710 | 4678.715 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1182.924 | 1552.894 | 59.273 | 16.676 | 1536.961 | 1572.215 | 1549.113 | 1649.043 | 1732.970 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1378.291 | 1676.545 | 78.896 | 6.786 | 1667.041 | 1682.101 | 1654.304 | 1817.365 | 1890.754 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1148.881 | 1449.963 | 131.268 | 22.211 | 1423.294 | 1480.270 | 1434.570 | 1684.235 | 1796.356 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 4023.789 | 5209.406 | 278.172 | 121.438 | 5068.974 | 5399.007 | 5146.888 | 5725.881 | 6036.466 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 6568.128 | 8484.087 | 600.481 | 124.416 | 8274.829 | 8585.406 | 8495.239 | 9432.088 | 10068.450 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2686.335 | 3232.890 | 118.463 | 23.560 | 3191.408 | 3247.459 | 3206.362 | 3462.763 | 3617.689 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2711.573 | 3470.501 | 199.769 | 21.760 | 3443.103 | 3501.619 | 3484.435 | 3766.283 | 3913.603 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 2198.774 | 2716.179 | 218.942 | 27.365 | 2694.676 | 2762.724 | 2680.742 | 3131.661 | 3358.820 |
| quic | loopback&#45;0 | legacy | 5000 | 39.415 | 62.887 | 11.957 | 11.841 | 42.019 | 70.992 | 64.674 | 78.035 | 91.328 |
| quic | loopback&#45;0 | portable | 5000 | 43.999 | 60.767 | 15.243 | 13.990 | 48.302 | 76.515 | 52.854 | 80.974 | 94.036 |
| quic | loopback&#45;64 | legacy | 5000 | 39.471 | 47.796 | 10.012 | 10.092 | 42.324 | 65.698 | 42.546 | 65.281 | 75.348 |
| quic | loopback&#45;64 | portable | 5000 | 43.956 | 59.169 | 16.795 | 15.699 | 46.820 | 77.487 | 49.590 | 82.875 | 95.241 |
| quic | loopback&#45;1024 | legacy | 5000 | 41.702 | 51.458 | 10.844 | 10.566 | 44.563 | 70.019 | 46.214 | 70.887 | 82.682 |
| quic | loopback&#45;1024 | portable | 5000 | 45.982 | 54.912 | 11.684 | 7.603 | 49.461 | 68.180 | 50.281 | 80.908 | 91.635 |
| quic | loopback&#45;65536 | legacy | 5000 | 185.600 | 209.199 | 35.369 | 24.382 | 193.423 | 252.289 | 195.020 | 304.483 | 329.237 |
| quic | loopback&#45;65536 | portable | 5000 | 191.495 | 221.422 | 44.673 | 41.558 | 200.526 | 295.704 | 201.427 | 324.031 | 349.469 |
| quic | unary&#45;empty | legacy | 5000 | 39.583 | 42.969 | 4.163 | 0.803 | 42.320 | 44.210 | 41.823 | 49.839 | 60.951 |
| quic | unary&#45;empty | portable | 5000 | 43.825 | 47.847 | 4.701 | 0.359 | 47.342 | 48.325 | 46.558 | 56.533 | 67.990 |
| quic | unary&#45;small | legacy | 5000 | 38.961 | 50.265 | 13.098 | 12.434 | 41.007 | 65.011 | 41.258 | 71.457 | 82.201 |
| quic | unary&#45;small | portable | 5000 | 44.367 | 48.709 | 7.938 | 3.856 | 46.458 | 55.574 | 45.926 | 73.809 | 77.462 |
| quic | unary&#45;1k | legacy | 5000 | 41.677 | 65.024 | 11.632 | 12.012 | 43.631 | 72.320 | 68.328 | 78.778 | 86.323 |
| quic | unary&#45;1k | portable | 5000 | 46.635 | 73.056 | 14.129 | 12.690 | 50.891 | 80.316 | 78.158 | 90.300 | 100.626 |
| quic | fanout&#45;32 | legacy | 5000 | 187.479 | 204.387 | 28.874 | 15.295 | 195.151 | 231.363 | 194.808 | 258.288 | 329.268 |
| quic | fanout&#45;32 | portable | 5000 | 221.882 | 246.268 | 42.473 | 22.933 | 231.447 | 286.774 | 230.940 | 374.332 | 418.759 |
| quic | dependent&#45;read | legacy | 5000 | 87.318 | 94.886 | 11.899 | 2.544 | 92.768 | 98.214 | 91.407 | 112.342 | 137.721 |
| quic | dependent&#45;read | portable | 5000 | 118.510 | 126.886 | 8.727 | 1.918 | 124.677 | 129.941 | 123.657 | 143.482 | 162.829 |
| quic | pipelined&#45;read | legacy | 5000 | 52.684 | 56.186 | 4.941 | 0.520 | 55.417 | 56.715 | 54.746 | 64.455 | 75.228 |
| quic | pipelined&#45;read | portable | 5000 | 84.404 | 97.236 | 22.458 | 18.141 | 88.720 | 129.677 | 87.255 | 153.234 | 169.385 |
| quic | write&#45;read | legacy | 5000 | 80.752 | 85.863 | 5.777 | 1.373 | 84.628 | 87.965 | 83.757 | 97.054 | 111.075 |
| quic | write&#45;read | portable | 5000 | 90.017 | 97.045 | 7.055 | 2.209 | 94.546 | 100.613 | 94.567 | 109.973 | 125.622 |
| quic | callback | legacy | 5000 | 86.571 | 94.413 | 7.872 | 3.806 | 91.074 | 100.977 | 92.172 | 109.199 | 122.587 |
| quic | callback | portable | 5000 | 113.734 | 120.916 | 7.715 | 1.342 | 119.556 | 123.032 | 118.069 | 136.084 | 153.653 |
| quic | bulk&#45;64k | legacy | 5000 | 183.281 | 195.056 | 11.045 | 2.379 | 192.627 | 198.850 | 190.694 | 216.557 | 235.004 |
| quic | bulk&#45;64k | portable | 5000 | 187.884 | 199.598 | 11.395 | 1.922 | 197.588 | 201.932 | 195.165 | 221.346 | 242.335 |
| quic | bulk&#45;256k | legacy | 5000 | 467.277 | 492.010 | 25.530 | 6.116 | 485.854 | 501.412 | 485.329 | 535.402 | 598.544 |
| quic | bulk&#45;256k | portable | 5000 | 486.617 | 513.382 | 22.628 | 3.156 | 508.713 | 517.444 | 507.630 | 553.492 | 590.889 |
| quic | bulk&#45;1m | legacy | 5000 | 1744.121 | 1966.833 | 486.912 | 239.727 | 1832.921 | 2394.381 | 1817.714 | 2564.966 | 2703.336 |
| quic | bulk&#45;1m | portable | 5000 | 1727.302 | 1895.237 | 428.908 | 123.215 | 1824.627 | 2113.241 | 1824.850 | 2516.930 | 2636.625 |
| quic | bulk&#45;4m | legacy | 5000 | 8477.149 | 9696.912 | 1164.178 | 269.294 | 9239.284 | 9893.105 | 9258.472 | 12532.853 | 13145.909 |
| quic | bulk&#45;4m | portable | 5000 | 8610.311 | 9958.710 | 1372.965 | 617.021 | 9415.901 | 10932.652 | 9501.217 | 12750.214 | 13311.582 |
| quic | deep&#45;8 | legacy | 5000 | 42.049 | 51.605 | 11.844 | 10.253 | 46.185 | 69.893 | 46.157 | 71.386 | 82.490 |
| quic | deep&#45;8 | portable | 5000 | 46.116 | 57.009 | 14.268 | 14.586 | 49.516 | 83.039 | 49.499 | 86.521 | 97.539 |
| quic | deep&#45;32 | legacy | 5000 | 49.623 | 78.910 | 13.497 | 13.820 | 54.319 | 87.497 | 82.622 | 94.381 | 105.335 |
| quic | deep&#45;32 | portable | 5000 | 90.293 | 96.114 | 6.581 | 1.541 | 94.915 | 98.730 | 93.615 | 108.440 | 121.364 |
| quic | concurrent&#45;32 | legacy | 5000 | 124.126 | 208.725 | 23.522 | 12.698 | 186.051 | 215.322 | 211.006 | 229.663 | 245.669 |
| quic | concurrent&#45;32 | portable | 5000 | 151.376 | 240.337 | 54.559 | 48.568 | 157.669 | 275.544 | 267.410 | 295.781 | 316.027 |
| quic | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 430.082 | 503.962 | 20.327 | 4.283 | 498.851 | 509.619 | 502.269 | 542.997 | 576.292 |
| quic | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 504.232 | 529.494 | 16.628 | 2.614 | 526.645 | 533.761 | 526.481 | 557.926 | 587.966 |
| quic | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 375.756 | 407.940 | 22.860 | 6.581 | 400.376 | 417.106 | 404.352 | 450.633 | 487.498 |
| quic | services&#45;4&#45;deep&#45;16 | portable | 5000 | 380.410 | 406.211 | 18.109 | 5.814 | 400.841 | 414.812 | 402.558 | 437.219 | 471.703 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1707.845 | 1866.261 | 88.151 | 29.148 | 1820.848 | 1894.982 | 1854.687 | 2003.849 | 2042.450 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1742.311 | 1931.966 | 88.958 | 70.865 | 1829.214 | 1995.947 | 1941.169 | 2056.819 | 2086.166 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1644.857 | 1934.741 | 107.382 | 45.297 | 1856.535 | 1968.444 | 1952.793 | 2088.563 | 2148.947 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1568.113 | 1963.719 | 121.316 | 22.291 | 1939.627 | 1993.263 | 1986.155 | 2129.067 | 2190.907 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 3278.621 | 3948.400 | 198.349 | 31.552 | 3919.890 | 3994.707 | 3968.384 | 4250.094 | 4398.931 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3005.458 | 3839.198 | 283.509 | 107.981 | 3719.359 | 3987.647 | 3804.564 | 4326.993 | 4546.838 |

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


