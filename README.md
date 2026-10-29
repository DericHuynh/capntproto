# Capntproto verification reports

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-29T14:25:31Z · run 40891322090 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40891322090) · commit `9ba02f88e0f9` · **failure**

![Cargo test results](docs/reports/cargo-history.svg)

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-29T14:25:31Z · run 40891322090 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40891322090) · commit `9ba02f88e0f9` · **failure**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-29T14:25:31Z · run 40891322090 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40891322090) · commit `9ba02f88e0f9` · **failure**

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

Latest coverage run: [2026-10-29T05:37:24Z · run 40824462084 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40824462084) · commit `15bb774b3d7c` · **failure**

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-29T14:25:31Z · run 40891322090 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40891322090) · commit `9ba02f88e0f9` · **failure**

Measurement evidence: **passed**. Independently validated encrypted benchmark values&#46; Workflow status and diagnostic completion are separate&#59; unavailable values remain blank&#46; Operations include whole concurrent batches and every service hop&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Current TCP fixtures use TLS 1.3, including upstream Rust Cap’n Proto, C++, gRPC over HTTP/2 and WebSockets over WSS; native encrypted transports and standard QUIC are identified separately. Historical data retains its recorded fixture contract. The dedicated campaign also reports recursive data, concurrent calls and independent service chains for gRPC and binary Protobuf WebSockets in its encrypted comparison artifact. Bars compare the recorded workload, rather than universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.


## Exhaustive benchmark tables

Measured run: [2026-10-29T14:25:31Z · run 40891322090 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40891322090) · commit `9ba02f88e0f9` · **failure**

All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.

### All encrypted latency comparisons

| Transport | Case | Implementation | Samples | Minimum µs | Mean µs | Sample SD µs | Run mean SD µs | Run mean min µs | Run mean max µs | p50 µs | p95 µs | p99 µs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tls | loopback&#45;0 | cpp | 5000 | 44.153 | 46.972 | 4.616 | 0.479 | 46.317 | 47.567 | 45.802 | 54.197 | 63.155 |
| tls | loopback&#45;0 | grpc | 5000 | 51.517 | 56.160 | 50.574 | 1.759 | 54.868 | 59.053 | 53.840 | 64.911 | 75.278 |
| tls | loopback&#45;0 | legacy | 5000 | 29.210 | 31.313 | 2.844 | 0.094 | 31.147 | 31.368 | 30.585 | 36.594 | 44.790 |
| tls | loopback&#45;0 | portable | 5000 | 30.315 | 32.737 | 4.724 | 0.315 | 32.298 | 33.034 | 31.870 | 38.566 | 47.306 |
| tls | loopback&#45;0 | websocket | 5000 | 26.856 | 30.089 | 3.250 | 0.167 | 29.817 | 30.248 | 29.675 | 33.959 | 41.591 |
| tls | loopback&#45;64 | cpp | 5000 | 44.156 | 47.180 | 20.264 | 0.890 | 46.138 | 48.397 | 45.845 | 53.833 | 62.771 |
| tls | loopback&#45;64 | grpc | 5000 | 52.322 | 55.849 | 7.151 | 1.189 | 54.631 | 57.790 | 54.126 | 64.060 | 75.612 |
| tls | loopback&#45;64 | legacy | 5000 | 29.196 | 31.279 | 3.351 | 0.160 | 31.004 | 31.383 | 30.371 | 36.874 | 46.798 |
| tls | loopback&#45;64 | portable | 5000 | 30.240 | 32.578 | 3.726 | 0.458 | 31.975 | 33.099 | 31.596 | 39.000 | 47.949 |
| tls | loopback&#45;64 | websocket | 5000 | 24.074 | 28.557 | 3.885 | 0.458 | 28.103 | 29.198 | 27.954 | 33.660 | 44.107 |
| tls | loopback&#45;1024 | cpp | 5000 | 46.531 | 49.577 | 3.940 | 0.546 | 48.889 | 50.141 | 48.073 | 57.147 | 66.576 |
| tls | loopback&#45;1024 | grpc | 5000 | 55.303 | 60.400 | 5.653 | 1.269 | 59.203 | 62.565 | 58.937 | 70.890 | 83.362 |
| tls | loopback&#45;1024 | legacy | 5000 | 31.257 | 33.524 | 3.179 | 0.320 | 33.203 | 34.049 | 32.576 | 38.290 | 47.452 |
| tls | loopback&#45;1024 | portable | 5000 | 32.518 | 35.252 | 3.605 | 0.438 | 34.671 | 35.835 | 34.074 | 41.653 | 51.482 |
| tls | loopback&#45;1024 | websocket | 5000 | 22.950 | 27.265 | 3.811 | 0.582 | 26.512 | 28.069 | 26.481 | 33.138 | 42.576 |
| tls | loopback&#45;65536 | cpp | 5000 | 197.788 | 210.462 | 10.206 | 0.936 | 208.960 | 211.449 | 206.959 | 229.391 | 247.148 |
| tls | loopback&#45;65536 | grpc | 5000 | 206.689 | 235.410 | 33.073 | 7.010 | 227.183 | 243.588 | 228.250 | 273.916 | 307.684 |
| tls | loopback&#45;65536 | legacy | 5000 | 143.366 | 151.567 | 7.755 | 1.486 | 149.720 | 152.924 | 148.500 | 166.887 | 181.394 |
| tls | loopback&#45;65536 | portable | 5000 | 147.789 | 157.234 | 9.430 | 2.983 | 154.870 | 162.245 | 153.403 | 175.720 | 191.226 |
| tls | loopback&#45;65536 | websocket | 5000 | 161.751 | 171.639 | 9.027 | 2.078 | 169.069 | 174.839 | 168.776 | 187.758 | 205.198 |
| tls | unary&#45;empty | cpp | 5000 | 44.490 | 47.793 | 4.241 | 0.759 | 46.930 | 48.933 | 46.438 | 55.677 | 65.134 |
| tls | unary&#45;empty | grpc | 5000 | 52.632 | 56.583 | 4.789 | 0.824 | 55.443 | 57.537 | 54.972 | 65.349 | 76.916 |
| tls | unary&#45;empty | legacy | 5000 | 29.001 | 31.204 | 2.969 | 0.323 | 30.734 | 31.575 | 30.422 | 36.832 | 44.529 |
| tls | unary&#45;empty | portable | 5000 | 30.139 | 32.449 | 3.132 | 0.434 | 32.036 | 33.074 | 31.629 | 38.328 | 46.555 |
| tls | unary&#45;empty | websocket | 5000 | 26.652 | 30.525 | 3.375 | 0.176 | 30.347 | 30.751 | 30.022 | 35.521 | 43.549 |
| tls | unary&#45;small | cpp | 5000 | 43.923 | 46.949 | 3.717 | 0.605 | 46.057 | 47.571 | 45.782 | 54.324 | 62.307 |
| tls | unary&#45;small | grpc | 5000 | 53.299 | 56.038 | 3.857 | 0.423 | 55.542 | 56.615 | 54.823 | 63.333 | 73.251 |
| tls | unary&#45;small | legacy | 5000 | 29.226 | 31.758 | 3.511 | 0.349 | 31.358 | 32.138 | 30.728 | 38.768 | 48.402 |
| tls | unary&#45;small | portable | 5000 | 30.199 | 34.454 | 6.784 | 3.973 | 32.168 | 41.538 | 31.618 | 51.299 | 55.496 |
| tls | unary&#45;small | websocket | 5000 | 24.590 | 28.877 | 3.219 | 0.290 | 28.629 | 29.356 | 28.377 | 33.789 | 41.742 |
| tls | unary&#45;1k | cpp | 5000 | 46.594 | 49.748 | 4.415 | 0.934 | 48.909 | 51.194 | 48.319 | 57.516 | 67.091 |
| tls | unary&#45;1k | grpc | 5000 | 55.907 | 60.484 | 5.593 | 0.948 | 59.663 | 61.755 | 58.631 | 70.220 | 81.836 |
| tls | unary&#45;1k | legacy | 5000 | 31.301 | 34.124 | 3.849 | 0.554 | 33.441 | 34.742 | 32.873 | 40.878 | 51.180 |
| tls | unary&#45;1k | portable | 5000 | 32.794 | 35.369 | 3.751 | 0.592 | 34.886 | 36.242 | 34.288 | 40.867 | 52.348 |
| tls | unary&#45;1k | websocket | 5000 | 23.757 | 27.685 | 3.716 | 0.212 | 27.490 | 28.039 | 26.845 | 33.230 | 43.006 |
| tls | fanout&#45;32 | cpp | 5000 | 391.482 | 518.435 | 133.056 | 125.790 | 430.335 | 723.168 | 444.967 | 756.843 | 815.727 |
| tls | fanout&#45;32 | grpc | 5000 | 661.030 | 774.484 | 133.202 | 130.301 | 697.977 | 1002.492 | 701.332 | 1038.467 | 1094.001 |
| tls | fanout&#45;32 | legacy | 5000 | 155.459 | 183.478 | 42.670 | 46.104 | 161.080 | 265.882 | 162.280 | 273.960 | 290.125 |
| tls | fanout&#45;32 | portable | 5000 | 186.223 | 223.993 | 56.044 | 58.021 | 193.610 | 327.205 | 196.692 | 332.962 | 340.084 |
| tls | fanout&#45;32 | websocket | 5000 | 187.203 | 211.685 | 17.365 | 4.977 | 205.146 | 217.278 | 208.074 | 241.049 | 270.594 |
| tls | dependent&#45;read | cpp | 5000 | 104.837 | 157.005 | 35.755 | 38.347 | 113.106 | 192.170 | 175.192 | 203.136 | 217.130 |
| tls | dependent&#45;read | grpc | 5000 | 110.340 | 165.258 | 38.881 | 41.249 | 116.773 | 197.730 | 185.946 | 214.297 | 228.751 |
| tls | dependent&#45;read | legacy | 5000 | 65.537 | 102.369 | 17.792 | 15.908 | 74.305 | 111.243 | 106.528 | 123.606 | 136.844 |
| tls | dependent&#45;read | portable | 5000 | 83.627 | 129.061 | 31.225 | 33.533 | 88.123 | 154.567 | 146.837 | 165.842 | 180.096 |
| tls | dependent&#45;read | websocket | 5000 | 45.920 | 76.291 | 14.779 | 11.480 | 55.949 | 83.761 | 76.354 | 98.257 | 108.057 |
| tls | pipelined&#45;read | cpp | 5000 | 76.826 | 111.318 | 28.768 | 27.330 | 83.016 | 139.060 | 132.260 | 149.119 | 163.059 |
| tls | pipelined&#45;read | legacy | 5000 | 40.380 | 64.175 | 11.035 | 10.925 | 44.691 | 70.332 | 66.797 | 78.490 | 86.528 |
| tls | pipelined&#45;read | portable | 5000 | 62.885 | 86.827 | 24.267 | 26.364 | 66.570 | 115.802 | 69.810 | 123.145 | 130.582 |
| tls | write&#45;read | cpp | 5000 | 90.024 | 120.177 | 30.096 | 32.598 | 94.778 | 157.912 | 100.288 | 165.163 | 183.548 |
| tls | write&#45;read | grpc | 5000 | 110.423 | 136.207 | 33.253 | 27.434 | 115.753 | 176.589 | 116.443 | 197.063 | 207.327 |
| tls | write&#45;read | legacy | 5000 | 59.574 | 84.833 | 18.796 | 17.989 | 63.361 | 101.634 | 95.975 | 109.718 | 123.812 |
| tls | write&#45;read | portable | 5000 | 63.149 | 96.288 | 20.123 | 18.154 | 69.427 | 110.789 | 105.345 | 119.886 | 132.280 |
| tls | write&#45;read | websocket | 5000 | 45.916 | 70.927 | 16.692 | 16.175 | 52.848 | 83.316 | 72.976 | 97.605 | 108.611 |
| tls | callback | cpp | 5000 | 96.226 | 116.762 | 26.957 | 28.656 | 102.236 | 167.957 | 103.067 | 173.288 | 190.225 |
| tls | callback | grpc | 5000 | 54.768 | 60.189 | 6.445 | 2.847 | 57.910 | 65.005 | 57.997 | 70.779 | 82.401 |
| tls | callback | legacy | 5000 | 64.089 | 69.675 | 5.679 | 1.591 | 68.769 | 72.507 | 67.445 | 80.625 | 92.908 |
| tls | callback | portable | 5000 | 80.486 | 98.049 | 22.924 | 24.346 | 84.471 | 141.358 | 85.950 | 144.500 | 154.818 |
| tls | callback | websocket | 5000 | 45.945 | 58.715 | 11.271 | 7.190 | 54.546 | 71.316 | 55.222 | 81.663 | 92.715 |
| tls | bulk&#45;64k | cpp | 5000 | 195.379 | 224.619 | 37.778 | 27.577 | 209.639 | 273.759 | 210.039 | 324.218 | 335.042 |
| tls | bulk&#45;64k | grpc | 5000 | 208.075 | 231.060 | 17.211 | 1.225 | 229.395 | 232.007 | 224.894 | 269.500 | 289.400 |
| tls | bulk&#45;64k | legacy | 5000 | 141.546 | 153.337 | 9.023 | 2.750 | 149.560 | 157.326 | 149.436 | 171.441 | 185.741 |
| tls | bulk&#45;64k | portable | 5000 | 148.030 | 158.226 | 14.665 | 2.409 | 155.300 | 161.439 | 153.407 | 178.546 | 202.001 |
| tls | bulk&#45;64k | websocket | 5000 | 163.587 | 175.772 | 13.393 | 3.869 | 171.348 | 180.305 | 170.451 | 200.233 | 223.536 |
| tls | bulk&#45;256k | cpp | 5000 | 763.724 | 810.942 | 42.525 | 3.615 | 806.265 | 815.876 | 800.583 | 875.019 | 911.969 |
| tls | bulk&#45;256k | grpc | 5000 | 491.917 | 779.477 | 230.783 | 218.136 | 537.361 | 1007.024 | 851.987 | 1165.533 | 1422.964 |
| tls | bulk&#45;256k | legacy | 5000 | 427.926 | 459.331 | 15.419 | 2.599 | 456.526 | 462.762 | 455.284 | 490.797 | 509.856 |
| tls | bulk&#45;256k | portable | 5000 | 437.274 | 468.794 | 29.052 | 6.681 | 463.566 | 480.485 | 462.201 | 501.609 | 643.009 |
| tls | bulk&#45;256k | websocket | 5000 | 931.036 | 989.928 | 50.810 | 44.354 | 964.675 | 1068.877 | 965.665 | 1089.405 | 1149.442 |
| tls | bulk&#45;1m | cpp | 5000 | 2919.219 | 3071.025 | 170.730 | 26.125 | 3047.103 | 3109.732 | 3035.847 | 3222.480 | 4117.805 |
| tls | bulk&#45;1m | grpc | 5000 | 2147.914 | 3997.202 | 517.470 | 538.638 | 3055.036 | 4380.500 | 4138.614 | 4613.931 | 4938.260 |
| tls | bulk&#45;1m | legacy | 5000 | 1487.508 | 1571.169 | 122.040 | 75.249 | 1523.742 | 1703.918 | 1525.959 | 1917.585 | 1976.119 |
| tls | bulk&#45;1m | portable | 5000 | 1484.700 | 1593.262 | 159.114 | 69.071 | 1520.628 | 1683.853 | 1523.433 | 1948.095 | 2057.670 |
| tls | bulk&#45;1m | websocket | 5000 | 3603.130 | 3859.892 | 242.460 | 118.471 | 3767.313 | 4024.592 | 3778.285 | 4465.338 | 4726.867 |
| tls | bulk&#45;4m | cpp | 5000 | 12800.284 | 14519.768 | 1370.124 | 660.677 | 13923.309 | 15543.405 | 14134.711 | 18230.115 | 19849.226 |
| tls | bulk&#45;4m | grpc | 5000 | 15770.723 | 20802.562 | 4078.220 | 4250.116 | 17607.368 | 27393.960 | 18692.672 | 28497.406 | 29188.044 |
| tls | bulk&#45;4m | legacy | 5000 | 7044.443 | 8440.370 | 1126.631 | 998.687 | 7693.431 | 10148.990 | 7906.857 | 10689.917 | 11004.916 |
| tls | bulk&#45;4m | portable | 5000 | 7070.331 | 8653.031 | 948.069 | 547.118 | 8022.225 | 9490.339 | 8332.513 | 10487.119 | 10781.462 |
| tls | bulk&#45;4m | websocket | 5000 | 23202.224 | 26717.241 | 2287.510 | 1307.955 | 25230.483 | 28423.084 | 25906.612 | 31749.872 | 33623.923 |
| tls | deep&#45;8 | cpp | 5000 | 47.111 | 51.016 | 4.812 | 1.359 | 50.164 | 53.425 | 49.245 | 59.578 | 70.747 |
| tls | deep&#45;8 | grpc | 5000 | 63.061 | 68.987 | 6.230 | 1.795 | 67.405 | 71.746 | 66.531 | 81.864 | 94.591 |
| tls | deep&#45;8 | legacy | 5000 | 31.781 | 34.978 | 3.913 | 0.466 | 34.493 | 35.670 | 33.554 | 42.499 | 52.040 |
| tls | deep&#45;8 | portable | 5000 | 32.657 | 36.108 | 4.272 | 0.776 | 35.460 | 37.427 | 34.680 | 44.542 | 54.139 |
| tls | deep&#45;8 | websocket | 5000 | 27.394 | 36.401 | 5.792 | 0.593 | 35.559 | 37.084 | 35.598 | 45.973 | 55.134 |
| tls | deep&#45;32 | cpp | 5000 | 53.219 | 79.460 | 18.780 | 19.662 | 56.967 | 96.157 | 89.733 | 102.977 | 115.694 |
| tls | deep&#45;32 | grpc | 5000 | 79.684 | 108.356 | 29.027 | 31.507 | 83.540 | 143.381 | 89.482 | 152.845 | 165.768 |
| tls | deep&#45;32 | legacy | 5000 | 37.709 | 49.126 | 12.096 | 11.386 | 40.652 | 64.551 | 41.724 | 67.207 | 76.350 |
| tls | deep&#45;32 | portable | 5000 | 39.059 | 50.166 | 13.049 | 12.090 | 41.213 | 68.148 | 41.559 | 70.300 | 80.234 |
| tls | deep&#45;32 | websocket | 5000 | 45.870 | 66.979 | 17.321 | 17.615 | 52.522 | 87.601 | 58.675 | 97.230 | 107.433 |
| tls | concurrent&#45;32 | cpp | 5000 | 441.219 | 469.071 | 22.199 | 6.946 | 457.639 | 475.522 | 462.751 | 514.906 | 542.454 |
| tls | concurrent&#45;32 | grpc | 5000 | 544.733 | 631.353 | 65.274 | 36.892 | 587.763 | 666.985 | 652.282 | 717.798 | 755.103 |
| tls | concurrent&#45;32 | legacy | 5000 | 106.557 | 115.159 | 8.086 | 2.395 | 113.277 | 118.922 | 112.660 | 130.371 | 146.360 |
| tls | concurrent&#45;32 | portable | 5000 | 131.363 | 143.411 | 10.646 | 3.528 | 140.733 | 149.543 | 139.553 | 164.804 | 183.442 |
| tls | concurrent&#45;32 | websocket | 5000 | 90.571 | 160.435 | 36.188 | 28.650 | 136.975 | 196.042 | 148.806 | 226.498 | 252.804 |
| tls | deep&#45;32&#45;concurrent&#45;32 | cpp | 5000 | 585.310 | 636.784 | 72.544 | 25.697 | 611.430 | 672.585 | 611.553 | 836.382 | 947.349 |
| tls | deep&#45;32&#45;concurrent&#45;32 | grpc | 5000 | 1350.452 | 1453.971 | 88.019 | 15.942 | 1437.435 | 1472.854 | 1432.138 | 1621.428 | 1805.645 |
| tls | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 357.200 | 410.840 | 54.174 | 48.009 | 379.444 | 493.487 | 382.907 | 521.537 | 551.370 |
| tls | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 392.689 | 449.265 | 56.747 | 33.435 | 416.311 | 499.605 | 429.156 | 527.528 | 574.084 |
| tls | deep&#45;32&#45;concurrent&#45;32 | websocket | 5000 | 644.806 | 699.475 | 40.433 | 9.005 | 692.622 | 714.571 | 689.157 | 765.930 | 814.052 |
| tls | services&#45;4&#45;deep&#45;16 | cpp | 5000 | 332.499 | 365.793 | 17.218 | 2.879 | 362.389 | 368.742 | 362.782 | 394.420 | 422.671 |
| tls | services&#45;4&#45;deep&#45;16 | grpc | 5000 | 471.121 | 516.947 | 22.288 | 3.080 | 514.458 | 522.211 | 513.223 | 561.559 | 591.488 |
| tls | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 250.563 | 269.913 | 18.610 | 3.394 | 266.714 | 275.574 | 265.447 | 295.562 | 316.866 |
| tls | services&#45;4&#45;deep&#45;16 | portable | 5000 | 276.070 | 305.077 | 26.525 | 6.523 | 299.012 | 313.328 | 299.937 | 338.452 | 383.385 |
| tls | services&#45;4&#45;deep&#45;16 | websocket | 5000 | 198.761 | 264.858 | 34.770 | 6.557 | 259.644 | 273.531 | 262.649 | 323.340 | 352.867 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | 5000 | 2292.078 | 2584.024 | 83.651 | 26.521 | 2553.126 | 2613.996 | 2575.684 | 2736.233 | 2849.720 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | 5000 | 4091.950 | 4302.252 | 122.209 | 58.938 | 4227.597 | 4387.462 | 4279.678 | 4504.045 | 4695.316 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1420.098 | 1499.652 | 50.415 | 10.970 | 1482.341 | 1510.070 | 1488.721 | 1592.189 | 1663.231 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1550.794 | 1614.266 | 73.704 | 17.903 | 1597.427 | 1641.873 | 1595.305 | 1717.880 | 1809.409 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | 5000 | 1288.198 | 1482.340 | 83.701 | 22.053 | 1455.280 | 1504.489 | 1470.274 | 1631.479 | 1705.588 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 2056.220 | 2474.986 | 119.487 | 36.233 | 2412.797 | 2506.672 | 2473.375 | 2644.598 | 2765.843 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 3432.555 | 4125.185 | 217.232 | 76.866 | 4045.601 | 4243.685 | 4097.591 | 4494.558 | 4720.368 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1151.435 | 1525.967 | 69.336 | 17.074 | 1505.423 | 1552.553 | 1518.258 | 1637.415 | 1719.956 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1322.886 | 1655.310 | 89.090 | 28.086 | 1615.884 | 1683.061 | 1635.324 | 1811.042 | 1885.017 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1094.199 | 1404.469 | 111.503 | 17.435 | 1384.794 | 1432.479 | 1397.197 | 1597.087 | 1699.166 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 3951.003 | 5079.042 | 233.404 | 108.038 | 4953.322 | 5172.304 | 5023.915 | 5528.873 | 5788.958 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 6518.348 | 8219.199 | 487.158 | 163.152 | 8073.660 | 8422.841 | 8249.397 | 8974.683 | 9367.894 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2794.534 | 3088.585 | 134.964 | 51.053 | 3035.243 | 3173.301 | 3059.532 | 3307.186 | 3499.397 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2614.669 | 3326.905 | 204.925 | 95.449 | 3204.354 | 3455.488 | 3325.387 | 3673.848 | 3824.221 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 2199.934 | 2678.334 | 203.383 | 20.354 | 2655.200 | 2702.646 | 2646.624 | 3039.407 | 3270.342 |
| quic | loopback&#45;0 | legacy | 5000 | 39.904 | 42.990 | 5.788 | 1.950 | 41.627 | 46.379 | 41.454 | 51.034 | 65.412 |
| quic | loopback&#45;0 | portable | 5000 | 44.856 | 47.419 | 3.605 | 0.189 | 47.144 | 47.612 | 46.423 | 54.193 | 60.746 |
| quic | loopback&#45;64 | legacy | 5000 | 40.068 | 42.683 | 5.510 | 0.347 | 42.328 | 43.098 | 41.654 | 48.667 | 56.396 |
| quic | loopback&#45;64 | portable | 5000 | 45.086 | 48.220 | 4.424 | 1.063 | 47.373 | 50.075 | 46.796 | 55.560 | 65.315 |
| quic | loopback&#45;1024 | legacy | 5000 | 42.335 | 44.811 | 2.978 | 0.232 | 44.560 | 45.154 | 43.956 | 50.629 | 57.268 |
| quic | loopback&#45;1024 | portable | 5000 | 47.573 | 50.916 | 4.482 | 0.725 | 50.264 | 51.881 | 49.544 | 58.801 | 68.635 |
| quic | loopback&#45;65536 | legacy | 5000 | 182.215 | 269.440 | 51.990 | 49.650 | 190.153 | 305.509 | 294.654 | 319.887 | 337.670 |
| quic | loopback&#45;65536 | portable | 5000 | 187.818 | 248.211 | 58.126 | 60.250 | 195.875 | 317.484 | 210.757 | 327.920 | 349.665 |
| quic | unary&#45;empty | legacy | 5000 | 40.300 | 52.516 | 12.413 | 12.733 | 42.837 | 66.514 | 44.097 | 69.455 | 78.623 |
| quic | unary&#45;empty | portable | 5000 | 45.417 | 61.485 | 15.093 | 14.400 | 48.318 | 76.921 | 53.271 | 82.025 | 89.451 |
| quic | unary&#45;small | legacy | 5000 | 40.350 | 43.107 | 4.252 | 0.714 | 42.269 | 43.942 | 41.870 | 49.997 | 60.475 |
| quic | unary&#45;small | portable | 5000 | 44.800 | 48.242 | 4.916 | 0.821 | 47.259 | 49.285 | 46.767 | 56.320 | 68.500 |
| quic | unary&#45;1k | legacy | 5000 | 42.014 | 44.940 | 3.599 | 0.920 | 43.982 | 46.353 | 43.901 | 51.757 | 60.496 |
| quic | unary&#45;1k | portable | 5000 | 47.494 | 50.289 | 4.174 | 0.440 | 49.647 | 50.792 | 49.186 | 57.286 | 66.680 |
| quic | fanout&#45;32 | legacy | 5000 | 184.560 | 194.751 | 18.111 | 4.840 | 191.825 | 203.257 | 190.083 | 206.829 | 315.671 |
| quic | fanout&#45;32 | portable | 5000 | 218.593 | 256.145 | 57.605 | 26.405 | 229.633 | 292.613 | 229.498 | 385.438 | 398.508 |
| quic | dependent&#45;read | legacy | 5000 | 89.121 | 95.519 | 6.814 | 0.943 | 94.089 | 96.304 | 93.104 | 107.931 | 123.965 |
| quic | dependent&#45;read | portable | 5000 | 120.379 | 128.076 | 8.704 | 3.913 | 125.109 | 134.785 | 124.489 | 145.315 | 161.901 |
| quic | pipelined&#45;read | legacy | 5000 | 53.899 | 56.843 | 3.953 | 0.750 | 56.121 | 58.023 | 55.560 | 64.337 | 74.413 |
| quic | pipelined&#45;read | portable | 5000 | 83.798 | 89.611 | 6.490 | 3.015 | 87.959 | 94.975 | 86.513 | 103.178 | 115.062 |
| quic | write&#45;read | legacy | 5000 | 82.273 | 88.297 | 6.018 | 0.551 | 87.671 | 88.861 | 86.133 | 99.789 | 113.590 |
| quic | write&#45;read | portable | 5000 | 93.439 | 100.141 | 8.890 | 0.737 | 99.484 | 101.385 | 97.502 | 114.147 | 128.805 |
| quic | callback | legacy | 5000 | 88.266 | 97.466 | 8.940 | 2.894 | 94.571 | 101.633 | 94.534 | 115.362 | 130.600 |
| quic | callback | portable | 5000 | 115.337 | 124.041 | 8.293 | 0.891 | 123.250 | 125.385 | 120.934 | 141.251 | 155.823 |
| quic | bulk&#45;64k | legacy | 5000 | 181.603 | 226.714 | 51.595 | 50.558 | 190.702 | 301.475 | 194.823 | 308.321 | 321.861 |
| quic | bulk&#45;64k | portable | 5000 | 185.612 | 218.632 | 44.570 | 48.622 | 195.213 | 305.582 | 197.020 | 310.346 | 323.055 |
| quic | bulk&#45;256k | legacy | 5000 | 459.021 | 528.939 | 400.610 | 88.525 | 479.414 | 686.092 | 480.600 | 713.713 | 745.308 |
| quic | bulk&#45;256k | portable | 5000 | 477.502 | 573.430 | 115.392 | 97.003 | 500.651 | 738.045 | 504.114 | 780.338 | 813.835 |
| quic | bulk&#45;1m | legacy | 5000 | 1707.826 | 2233.733 | 782.886 | 268.921 | 1840.090 | 2503.260 | 2436.217 | 2649.274 | 2712.901 |
| quic | bulk&#45;1m | portable | 5000 | 1703.697 | 2131.940 | 527.171 | 247.894 | 1833.920 | 2452.665 | 1918.923 | 2629.474 | 2735.431 |
| quic | bulk&#45;4m | legacy | 5000 | 8271.611 | 10117.748 | 1512.265 | 997.388 | 9018.401 | 11693.724 | 9732.243 | 13634.069 | 14092.089 |
| quic | bulk&#45;4m | portable | 5000 | 8212.086 | 10082.829 | 1433.107 | 887.331 | 9227.490 | 11290.864 | 9768.342 | 12245.410 | 14320.701 |
| quic | deep&#45;8 | legacy | 5000 | 43.727 | 53.342 | 6.871 | 0.642 | 52.536 | 54.087 | 51.556 | 66.832 | 78.119 |
| quic | deep&#45;8 | portable | 5000 | 48.947 | 57.722 | 6.931 | 0.626 | 57.386 | 58.829 | 56.036 | 70.830 | 85.634 |
| quic | deep&#45;32 | legacy | 5000 | 51.993 | 62.091 | 7.766 | 1.104 | 60.973 | 63.920 | 60.113 | 76.845 | 91.143 |
| quic | deep&#45;32 | portable | 5000 | 56.597 | 67.308 | 7.704 | 0.855 | 66.118 | 68.525 | 65.539 | 81.985 | 97.075 |
| quic | concurrent&#45;32 | legacy | 5000 | 126.410 | 149.120 | 13.435 | 5.280 | 144.330 | 158.106 | 146.896 | 172.781 | 192.901 |
| quic | concurrent&#45;32 | portable | 5000 | 154.552 | 176.223 | 13.070 | 1.072 | 174.982 | 177.671 | 173.737 | 201.090 | 215.915 |
| quic | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 410.286 | 460.561 | 32.961 | 6.461 | 453.666 | 467.145 | 454.436 | 510.526 | 553.307 |
| quic | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 445.556 | 494.988 | 34.459 | 13.744 | 481.733 | 517.923 | 487.899 | 554.765 | 585.816 |
| quic | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 384.486 | 454.097 | 36.213 | 14.366 | 438.931 | 471.283 | 452.268 | 503.516 | 529.888 |
| quic | services&#45;4&#45;deep&#45;16 | portable | 5000 | 378.016 | 437.716 | 28.797 | 9.803 | 425.287 | 451.169 | 435.632 | 483.849 | 508.855 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1669.047 | 1859.416 | 109.780 | 82.380 | 1773.596 | 1963.103 | 1842.406 | 2036.399 | 2076.028 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1697.154 | 1907.538 | 100.539 | 54.604 | 1847.620 | 1965.856 | 1902.735 | 2061.770 | 2132.354 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1620.324 | 1965.082 | 98.720 | 33.290 | 1921.523 | 2002.963 | 1981.960 | 2083.154 | 2157.664 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1516.114 | 1919.699 | 148.295 | 32.753 | 1886.184 | 1965.677 | 1950.109 | 2129.080 | 2229.795 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 3228.506 | 3905.892 | 185.573 | 33.665 | 3852.310 | 3936.289 | 3951.483 | 4176.112 | 4300.963 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2922.860 | 3577.140 | 357.622 | 166.359 | 3315.915 | 3725.121 | 3624.968 | 4084.706 | 4285.600 |

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


