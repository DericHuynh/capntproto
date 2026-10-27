# Capntproto verification reports

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-27T04:42:31Z · run 40451214065 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40451214065) · commit `130481adf487` · **success**

![Cargo test results](docs/reports/cargo-history.svg)

| Total | Passed | Failed | Errors | Skipped |
| ---: | ---: | ---: | ---: | ---: |
| 2539 | 2519 | [0](docs/reports/failed-tests.md) | 0 | 20 |

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-27T04:42:31Z · run 40451214065 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40451214065) · commit `130481adf487` · **success**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

![TLA&#43; checks and expected counterexamples](docs/reports/tla-outcomes.svg)

Unique module&#47;configuration&#47;expected&#45;exit checks&#46; Expected invariant violations are successful controls&#46;

![TLA&#43; explored states by model](docs/reports/tla-states.svg)

Counts sum independent bounded configurations&#59; they are not globally distinct states or a proof beyond those bounds&#46;

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-27T04:42:31Z · run 40451214065 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40451214065) · commit `130481adf487` · **success**

![Fuzzing executions by engine](docs/reports/fuzz-executions.svg)

Bounded campaigns including seed calibration&#59; execution counts are not comparable performance benchmarks&#46;

![Fuzzer feedback by engine](docs/reports/fuzz-coverage.svg)

Engine&#45;local counters&#44; not LLVM source coverage&#46; Do not compare counts across engines or builds&#46;

![Saved fuzzing findings](docs/reports/fuzz-findings.svg)

Saved crashes&#47;hangs are findings requiring triage&#44; not confirmed unique bugs&#46; Missing results remain unknown&#46;

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

Latest coverage run: [2026-10-27T06:36:41Z · run 40462132361 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40462132361) · commit `49f529f3ac47` · **failure**

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-27T04:42:31Z · run 40451214065 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40451214065) · commit `130481adf487` · **success**

Measurement evidence: **passed**. Independently validated encrypted benchmark values&#46; Workflow status and diagnostic completion are separate&#59; unavailable values remain blank&#46; Operations include whole concurrent batches and every service hop&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Current TCP fixtures use TLS 1.3, including upstream Rust Cap’n Proto, C++, gRPC over HTTP/2 and WebSockets over WSS; native encrypted transports and standard QUIC are identified separately. Historical data retains its recorded fixture contract. The dedicated campaign also reports recursive data, concurrent calls and independent service chains for gRPC and binary Protobuf WebSockets in its encrypted comparison artifact. Bars compare the recorded workload, rather than universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.


## Exhaustive benchmark tables

Measured run: [2026-10-27T04:42:31Z · run 40451214065 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40451214065) · commit `130481adf487` · **success**

All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.

### All encrypted latency comparisons

| Transport | Case | Implementation | Samples | Minimum µs | Mean µs | Sample SD µs | Run mean SD µs | Run mean min µs | Run mean max µs | p50 µs | p95 µs | p99 µs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tls | loopback&#45;0 | cpp | 5000 | 43.915 | 48.509 | 9.977 | 0.613 | 48.059 | 49.483 | 46.735 | 59.363 | 68.853 |
| tls | loopback&#45;0 | grpc | 5000 | 51.531 | 61.915 | 12.488 | 12.530 | 55.499 | 84.306 | 55.537 | 83.625 | 95.420 |
| tls | loopback&#45;0 | legacy | 5000 | 29.715 | 33.003 | 4.265 | 0.917 | 31.899 | 34.060 | 31.610 | 40.902 | 52.262 |
| tls | loopback&#45;0 | portable | 5000 | 30.355 | 33.214 | 3.931 | 0.468 | 32.694 | 33.877 | 32.136 | 40.318 | 50.924 |
| tls | loopback&#45;0 | websocket | 5000 | 26.599 | 28.707 | 3.493 | 0.240 | 28.302 | 28.937 | 27.924 | 32.779 | 42.876 |
| tls | loopback&#45;64 | cpp | 5000 | 43.829 | 52.408 | 11.311 | 11.702 | 46.637 | 73.294 | 46.554 | 72.430 | 85.396 |
| tls | loopback&#45;64 | grpc | 5000 | 51.587 | 56.383 | 7.803 | 2.744 | 54.838 | 61.260 | 53.975 | 69.579 | 84.924 |
| tls | loopback&#45;64 | legacy | 5000 | 29.769 | 34.092 | 10.485 | 4.863 | 31.640 | 42.785 | 31.292 | 42.879 | 51.621 |
| tls | loopback&#45;64 | portable | 5000 | 30.199 | 35.719 | 7.458 | 7.194 | 32.208 | 48.581 | 31.888 | 48.272 | 56.941 |
| tls | loopback&#45;64 | websocket | 5000 | 24.635 | 29.511 | 5.455 | 5.029 | 26.990 | 38.498 | 27.040 | 40.140 | 45.497 |
| tls | loopback&#45;1024 | cpp | 5000 | 46.506 | 60.032 | 13.748 | 14.359 | 49.098 | 76.653 | 50.628 | 77.701 | 92.581 |
| tls | loopback&#45;1024 | grpc | 5000 | 57.222 | 70.036 | 13.777 | 13.129 | 62.931 | 93.273 | 62.613 | 93.110 | 106.070 |
| tls | loopback&#45;1024 | legacy | 5000 | 32.041 | 41.182 | 10.230 | 6.064 | 34.245 | 45.815 | 44.196 | 47.663 | 60.147 |
| tls | loopback&#45;1024 | portable | 5000 | 32.076 | 43.335 | 8.448 | 8.110 | 34.255 | 51.305 | 46.759 | 52.968 | 64.235 |
| tls | loopback&#45;1024 | websocket | 5000 | 24.694 | 34.260 | 6.133 | 5.473 | 28.339 | 39.051 | 35.085 | 42.975 | 50.743 |
| tls | loopback&#45;65536 | cpp | 5000 | 171.457 | 209.877 | 44.446 | 26.955 | 188.098 | 246.393 | 190.838 | 290.411 | 299.471 |
| tls | loopback&#45;65536 | grpc | 5000 | 174.224 | 192.885 | 15.461 | 1.824 | 191.252 | 195.880 | 188.374 | 226.338 | 252.135 |
| tls | loopback&#45;65536 | legacy | 5000 | 106.114 | 128.247 | 24.276 | 22.039 | 111.583 | 161.512 | 113.603 | 170.579 | 182.017 |
| tls | loopback&#45;65536 | portable | 5000 | 109.077 | 128.978 | 26.771 | 28.761 | 115.221 | 180.384 | 115.062 | 185.046 | 198.931 |
| tls | loopback&#45;65536 | websocket | 5000 | 121.435 | 132.006 | 15.893 | 8.561 | 127.817 | 147.308 | 126.294 | 178.147 | 190.059 |
| tls | unary&#45;empty | cpp | 5000 | 44.006 | 52.165 | 11.789 | 10.078 | 47.190 | 70.143 | 46.799 | 72.346 | 84.158 |
| tls | unary&#45;empty | grpc | 5000 | 52.190 | 56.074 | 5.290 | 0.688 | 54.968 | 56.719 | 54.698 | 65.486 | 74.956 |
| tls | unary&#45;empty | legacy | 5000 | 29.968 | 32.422 | 3.230 | 0.512 | 31.701 | 33.093 | 31.589 | 37.287 | 47.441 |
| tls | unary&#45;empty | portable | 5000 | 30.604 | 32.929 | 3.959 | 0.334 | 32.415 | 33.309 | 32.044 | 38.006 | 49.000 |
| tls | unary&#45;empty | websocket | 5000 | 26.272 | 28.705 | 3.450 | 0.262 | 28.469 | 29.110 | 28.018 | 31.748 | 42.291 |
| tls | unary&#45;small | cpp | 5000 | 44.015 | 47.565 | 4.406 | 0.380 | 46.981 | 48.026 | 46.269 | 55.940 | 66.497 |
| tls | unary&#45;small | grpc | 5000 | 52.627 | 57.606 | 9.986 | 0.885 | 56.259 | 58.727 | 55.943 | 67.993 | 76.977 |
| tls | unary&#45;small | legacy | 5000 | 29.896 | 32.839 | 3.628 | 0.687 | 32.040 | 33.566 | 31.928 | 38.749 | 49.590 |
| tls | unary&#45;small | portable | 5000 | 30.390 | 32.958 | 3.599 | 0.553 | 32.116 | 33.672 | 32.001 | 39.056 | 49.445 |
| tls | unary&#45;small | websocket | 5000 | 25.209 | 27.925 | 3.346 | 0.340 | 27.550 | 28.463 | 27.387 | 31.178 | 41.138 |
| tls | unary&#45;1k | cpp | 5000 | 46.497 | 51.713 | 5.991 | 0.849 | 50.901 | 53.069 | 49.952 | 63.129 | 72.556 |
| tls | unary&#45;1k | grpc | 5000 | 57.536 | 64.425 | 5.699 | 0.835 | 63.395 | 65.286 | 63.058 | 76.115 | 85.496 |
| tls | unary&#45;1k | legacy | 5000 | 31.861 | 34.662 | 3.777 | 0.685 | 34.034 | 35.437 | 33.582 | 40.279 | 49.931 |
| tls | unary&#45;1k | portable | 5000 | 32.264 | 35.481 | 4.774 | 0.968 | 34.492 | 37.101 | 34.098 | 44.263 | 54.687 |
| tls | unary&#45;1k | websocket | 5000 | 24.967 | 28.748 | 3.232 | 0.483 | 28.319 | 29.555 | 28.253 | 33.605 | 42.955 |
| tls | fanout&#45;32 | cpp | 5000 | 367.695 | 410.354 | 43.973 | 16.377 | 392.372 | 427.990 | 397.035 | 492.115 | 539.242 |
| tls | fanout&#45;32 | grpc | 5000 | 640.339 | 678.001 | 19.253 | 6.137 | 671.293 | 684.884 | 675.889 | 704.153 | 726.410 |
| tls | fanout&#45;32 | legacy | 5000 | 137.007 | 147.662 | 7.693 | 2.349 | 144.318 | 150.362 | 145.625 | 162.116 | 173.290 |
| tls | fanout&#45;32 | portable | 5000 | 165.515 | 177.451 | 13.507 | 2.363 | 174.580 | 180.653 | 174.494 | 194.188 | 208.607 |
| tls | fanout&#45;32 | websocket | 5000 | 181.440 | 209.540 | 17.634 | 3.748 | 205.468 | 215.045 | 204.927 | 241.672 | 256.067 |
| tls | dependent&#45;read | cpp | 5000 | 102.836 | 111.485 | 7.365 | 1.294 | 110.113 | 112.899 | 109.089 | 127.562 | 137.032 |
| tls | dependent&#45;read | grpc | 5000 | 111.062 | 120.594 | 6.993 | 2.026 | 119.064 | 123.965 | 118.295 | 134.404 | 142.672 |
| tls | dependent&#45;read | legacy | 5000 | 65.258 | 70.296 | 5.038 | 1.051 | 69.346 | 72.018 | 68.750 | 81.305 | 90.294 |
| tls | dependent&#45;read | portable | 5000 | 78.851 | 84.204 | 5.647 | 0.497 | 83.649 | 84.977 | 82.212 | 96.926 | 106.847 |
| tls | dependent&#45;read | websocket | 5000 | 48.754 | 55.437 | 4.718 | 0.787 | 54.561 | 56.404 | 54.690 | 64.335 | 73.330 |
| tls | pipelined&#45;read | cpp | 5000 | 72.956 | 79.904 | 5.924 | 1.904 | 78.072 | 82.741 | 78.041 | 92.578 | 103.005 |
| tls | pipelined&#45;read | legacy | 5000 | 39.205 | 42.874 | 4.275 | 1.396 | 42.188 | 45.367 | 41.409 | 51.091 | 60.762 |
| tls | pipelined&#45;read | portable | 5000 | 59.136 | 64.224 | 10.126 | 1.182 | 62.947 | 65.824 | 62.164 | 76.255 | 87.296 |
| tls | write&#45;read | cpp | 5000 | 90.378 | 99.220 | 12.859 | 3.855 | 96.138 | 105.427 | 95.841 | 116.045 | 143.848 |
| tls | write&#45;read | grpc | 5000 | 111.218 | 120.368 | 6.780 | 2.014 | 118.402 | 123.741 | 118.188 | 134.329 | 143.887 |
| tls | write&#45;read | legacy | 5000 | 60.585 | 70.334 | 9.886 | 9.438 | 64.327 | 87.111 | 65.672 | 86.603 | 100.199 |
| tls | write&#45;read | portable | 5000 | 61.989 | 73.263 | 14.387 | 14.609 | 65.425 | 99.355 | 65.995 | 98.765 | 112.685 |
| tls | write&#45;read | websocket | 5000 | 48.521 | 60.280 | 10.437 | 9.937 | 55.036 | 78.015 | 56.191 | 82.553 | 89.469 |
| tls | callback | cpp | 5000 | 94.617 | 102.914 | 11.083 | 1.658 | 100.866 | 104.664 | 100.348 | 117.750 | 128.233 |
| tls | callback | grpc | 5000 | 57.217 | 61.472 | 4.669 | 1.044 | 60.623 | 62.691 | 60.167 | 70.918 | 79.815 |
| tls | callback | legacy | 5000 | 65.066 | 71.739 | 6.626 | 1.428 | 69.637 | 73.536 | 70.060 | 83.228 | 92.431 |
| tls | callback | portable | 5000 | 75.655 | 82.836 | 6.412 | 1.876 | 79.937 | 85.062 | 80.866 | 96.010 | 107.479 |
| tls | callback | websocket | 5000 | 48.671 | 53.922 | 4.716 | 0.253 | 53.495 | 54.144 | 53.265 | 61.416 | 69.649 |
| tls | bulk&#45;64k | cpp | 5000 | 168.686 | 183.869 | 9.566 | 1.858 | 182.406 | 187.011 | 182.565 | 199.716 | 209.987 |
| tls | bulk&#45;64k | grpc | 5000 | 174.809 | 205.485 | 40.510 | 29.753 | 190.471 | 258.657 | 189.275 | 307.108 | 325.807 |
| tls | bulk&#45;64k | legacy | 5000 | 106.093 | 112.221 | 6.817 | 0.788 | 110.941 | 112.945 | 110.352 | 123.601 | 131.477 |
| tls | bulk&#45;64k | portable | 5000 | 108.911 | 114.828 | 6.181 | 1.204 | 113.962 | 116.909 | 112.570 | 127.864 | 137.343 |
| tls | bulk&#45;64k | websocket | 5000 | 122.214 | 129.079 | 14.104 | 0.693 | 127.925 | 129.703 | 126.289 | 141.594 | 151.856 |
| tls | bulk&#45;256k | cpp | 5000 | 673.357 | 719.668 | 21.633 | 3.894 | 714.019 | 724.412 | 717.648 | 745.904 | 781.297 |
| tls | bulk&#45;256k | grpc | 5000 | 413.369 | 897.732 | 184.724 | 113.073 | 789.721 | 1033.340 | 970.207 | 1123.364 | 1156.542 |
| tls | bulk&#45;256k | legacy | 5000 | 300.935 | 318.251 | 26.777 | 9.853 | 313.409 | 335.840 | 311.977 | 334.026 | 456.266 |
| tls | bulk&#45;256k | portable | 5000 | 309.905 | 321.578 | 12.036 | 0.736 | 320.788 | 322.651 | 318.434 | 332.751 | 343.113 |
| tls | bulk&#45;256k | websocket | 5000 | 736.041 | 777.787 | 30.875 | 19.972 | 763.634 | 811.382 | 770.167 | 857.040 | 870.591 |
| tls | bulk&#45;1m | cpp | 5000 | 2649.501 | 2816.654 | 256.830 | 91.007 | 2739.633 | 2964.900 | 2745.751 | 3719.190 | 3837.746 |
| tls | bulk&#45;1m | grpc | 5000 | 1687.543 | 3431.570 | 813.162 | 866.442 | 2747.289 | 4518.321 | 3024.476 | 4692.428 | 5046.018 |
| tls | bulk&#45;1m | legacy | 5000 | 1071.174 | 1133.468 | 78.082 | 24.005 | 1111.392 | 1163.346 | 1113.355 | 1397.119 | 1438.628 |
| tls | bulk&#45;1m | portable | 5000 | 1079.979 | 1134.345 | 70.992 | 24.223 | 1117.999 | 1176.148 | 1118.029 | 1217.948 | 1443.886 |
| tls | bulk&#45;1m | websocket | 5000 | 3216.681 | 3453.943 | 211.145 | 160.591 | 3319.731 | 3726.852 | 3366.770 | 3892.345 | 3951.566 |
| tls | bulk&#45;4m | cpp | 5000 | 11923.609 | 12672.984 | 659.407 | 94.066 | 12557.774 | 12802.487 | 12540.398 | 13317.724 | 16633.311 |
| tls | bulk&#45;4m | grpc | 5000 | 18971.322 | 20584.641 | 882.348 | 266.140 | 20270.323 | 20868.049 | 20332.977 | 22535.195 | 23072.111 |
| tls | bulk&#45;4m | legacy | 5000 | 4947.413 | 5256.236 | 283.754 | 76.264 | 5174.497 | 5378.307 | 5179.058 | 6164.178 | 6374.565 |
| tls | bulk&#45;4m | portable | 5000 | 4935.973 | 5267.660 | 306.005 | 68.081 | 5205.423 | 5356.923 | 5181.707 | 6189.999 | 6431.801 |
| tls | bulk&#45;4m | websocket | 5000 | 21126.209 | 22121.649 | 975.344 | 117.935 | 21945.113 | 22227.387 | 21818.608 | 24886.507 | 25315.800 |
| tls | deep&#45;8 | cpp | 5000 | 46.897 | 50.122 | 3.981 | 0.495 | 49.397 | 50.630 | 48.911 | 58.325 | 68.272 |
| tls | deep&#45;8 | grpc | 5000 | 62.346 | 75.042 | 14.375 | 15.138 | 67.884 | 102.115 | 67.502 | 102.027 | 113.276 |
| tls | deep&#45;8 | legacy | 5000 | 32.093 | 34.138 | 2.975 | 0.213 | 33.927 | 34.401 | 33.331 | 38.827 | 49.108 |
| tls | deep&#45;8 | portable | 5000 | 32.896 | 34.943 | 3.134 | 0.239 | 34.710 | 35.294 | 34.052 | 40.520 | 49.988 |
| tls | deep&#45;8 | websocket | 5000 | 28.570 | 33.722 | 3.963 | 0.121 | 33.596 | 33.916 | 33.155 | 39.428 | 48.809 |
| tls | deep&#45;32 | cpp | 5000 | 52.356 | 67.609 | 14.929 | 15.854 | 55.768 | 85.152 | 57.565 | 87.842 | 101.179 |
| tls | deep&#45;32 | grpc | 5000 | 80.536 | 104.007 | 23.156 | 21.865 | 86.773 | 129.110 | 89.162 | 139.042 | 150.408 |
| tls | deep&#45;32 | legacy | 5000 | 37.314 | 47.750 | 10.144 | 7.599 | 39.707 | 57.352 | 41.289 | 60.834 | 72.622 |
| tls | deep&#45;32 | portable | 5000 | 39.021 | 46.272 | 8.481 | 7.375 | 41.594 | 59.199 | 41.803 | 59.605 | 69.068 |
| tls | deep&#45;32 | websocket | 5000 | 46.287 | 61.482 | 11.710 | 11.687 | 52.647 | 76.052 | 56.095 | 80.452 | 89.466 |
| tls | concurrent&#45;32 | cpp | 5000 | 396.529 | 435.609 | 18.557 | 9.304 | 423.143 | 448.277 | 432.492 | 474.587 | 497.045 |
| tls | concurrent&#45;32 | grpc | 5000 | 503.625 | 541.073 | 38.533 | 14.906 | 524.371 | 555.357 | 528.112 | 635.000 | 654.383 |
| tls | concurrent&#45;32 | legacy | 5000 | 107.060 | 135.990 | 28.951 | 24.795 | 113.621 | 172.051 | 117.118 | 180.811 | 189.559 |
| tls | concurrent&#45;32 | portable | 5000 | 132.828 | 142.481 | 13.159 | 3.779 | 139.767 | 149.043 | 138.605 | 156.710 | 214.159 |
| tls | concurrent&#45;32 | websocket | 5000 | 92.612 | 163.726 | 33.653 | 26.488 | 134.218 | 191.178 | 161.735 | 215.427 | 230.828 |
| tls | deep&#45;32&#45;concurrent&#45;32 | cpp | 5000 | 543.450 | 577.588 | 40.244 | 3.330 | 574.436 | 583.098 | 570.252 | 612.257 | 775.546 |
| tls | deep&#45;32&#45;concurrent&#45;32 | grpc | 5000 | 1307.945 | 1383.933 | 56.489 | 19.912 | 1369.755 | 1418.817 | 1372.170 | 1478.149 | 1633.681 |
| tls | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 315.777 | 333.071 | 14.154 | 3.041 | 329.167 | 336.386 | 331.558 | 347.869 | 368.952 |
| tls | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 362.135 | 382.642 | 11.415 | 1.720 | 380.372 | 384.817 | 380.824 | 401.629 | 414.012 |
| tls | deep&#45;32&#45;concurrent&#45;32 | websocket | 5000 | 604.648 | 687.215 | 68.963 | 29.790 | 662.152 | 726.594 | 667.823 | 893.982 | 932.262 |
| tls | services&#45;4&#45;deep&#45;16 | cpp | 5000 | 318.894 | 350.521 | 18.127 | 2.199 | 347.523 | 352.871 | 347.197 | 379.053 | 392.626 |
| tls | services&#45;4&#45;deep&#45;16 | grpc | 5000 | 419.874 | 456.719 | 19.703 | 1.502 | 454.996 | 458.224 | 455.083 | 491.353 | 509.052 |
| tls | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 232.607 | 258.699 | 17.297 | 4.838 | 252.565 | 265.912 | 254.487 | 289.095 | 304.210 |
| tls | services&#45;4&#45;deep&#45;16 | portable | 5000 | 248.897 | 275.684 | 20.403 | 1.972 | 272.912 | 278.217 | 271.197 | 304.079 | 318.032 |
| tls | services&#45;4&#45;deep&#45;16 | websocket | 5000 | 193.851 | 238.390 | 22.207 | 1.187 | 237.188 | 239.778 | 236.582 | 277.160 | 294.007 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | 5000 | 2321.988 | 2510.251 | 117.280 | 83.068 | 2406.962 | 2596.587 | 2499.102 | 2684.065 | 2740.515 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | 5000 | 3992.448 | 4242.705 | 143.633 | 64.586 | 4155.357 | 4316.400 | 4216.009 | 4466.120 | 4600.391 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1288.497 | 1334.877 | 21.374 | 3.751 | 1331.716 | 1341.322 | 1332.352 | 1361.541 | 1406.389 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1477.291 | 1532.028 | 28.615 | 4.461 | 1524.345 | 1535.762 | 1527.684 | 1566.925 | 1641.069 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | 5000 | 1125.865 | 1243.586 | 71.223 | 2.696 | 1241.013 | 1247.656 | 1238.475 | 1314.564 | 1361.587 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 1944.488 | 2272.391 | 107.421 | 41.939 | 2204.882 | 2304.386 | 2246.565 | 2458.248 | 2506.359 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 3374.783 | 4025.346 | 171.112 | 44.923 | 3958.198 | 4063.722 | 4008.805 | 4313.121 | 4391.033 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1228.996 | 1385.048 | 39.349 | 18.210 | 1358.764 | 1406.840 | 1386.811 | 1437.113 | 1474.687 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1313.664 | 1497.282 | 62.594 | 10.495 | 1486.404 | 1513.718 | 1489.096 | 1591.742 | 1630.970 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1010.169 | 1254.527 | 181.126 | 48.340 | 1201.862 | 1310.932 | 1244.276 | 1426.509 | 1522.650 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 4024.861 | 4921.352 | 584.324 | 272.554 | 4730.376 | 5400.138 | 4804.234 | 5656.148 | 8003.194 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 6489.277 | 8125.623 | 386.068 | 91.631 | 7994.928 | 8232.579 | 8151.813 | 8694.562 | 8857.235 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2573.838 | 2859.651 | 66.265 | 16.463 | 2841.828 | 2881.017 | 2859.891 | 2932.062 | 3053.222 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2825.718 | 3116.809 | 155.030 | 54.237 | 3053.466 | 3189.169 | 3141.655 | 3347.446 | 3413.716 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1905.187 | 2383.682 | 182.772 | 20.028 | 2354.539 | 2405.790 | 2355.739 | 2731.207 | 2948.460 |
| quic | loopback&#45;0 | legacy | 5000 | 38.562 | 45.522 | 9.773 | 8.594 | 40.275 | 60.367 | 40.051 | 60.983 | 72.762 |
| quic | loopback&#45;0 | portable | 5000 | 41.432 | 48.983 | 10.684 | 7.088 | 43.818 | 58.364 | 43.240 | 67.009 | 76.991 |
| quic | loopback&#45;64 | legacy | 5000 | 38.702 | 54.552 | 12.447 | 11.979 | 41.430 | 64.071 | 61.252 | 66.993 | 80.688 |
| quic | loopback&#45;64 | portable | 5000 | 41.770 | 60.705 | 50.750 | 11.684 | 44.664 | 73.027 | 65.096 | 73.639 | 86.345 |
| quic | loopback&#45;1024 | legacy | 5000 | 39.832 | 43.007 | 5.796 | 1.443 | 42.122 | 45.555 | 41.603 | 50.860 | 66.488 |
| quic | loopback&#45;1024 | portable | 5000 | 43.254 | 58.440 | 14.053 | 12.938 | 45.461 | 73.476 | 57.199 | 74.278 | 89.926 |
| quic | loopback&#45;65536 | legacy | 5000 | 144.138 | 167.609 | 34.204 | 21.368 | 151.391 | 195.503 | 149.830 | 239.724 | 257.950 |
| quic | loopback&#45;65536 | portable | 5000 | 146.763 | 186.949 | 43.720 | 36.132 | 156.880 | 245.220 | 157.405 | 257.461 | 268.698 |
| quic | unary&#45;empty | legacy | 5000 | 38.658 | 41.296 | 8.853 | 0.356 | 40.895 | 41.757 | 40.178 | 46.898 | 60.295 |
| quic | unary&#45;empty | portable | 5000 | 41.883 | 44.497 | 3.588 | 0.288 | 44.225 | 44.933 | 43.495 | 50.626 | 62.720 |
| quic | unary&#45;small | legacy | 5000 | 38.933 | 42.112 | 4.433 | 0.600 | 41.179 | 42.781 | 41.011 | 47.983 | 62.427 |
| quic | unary&#45;small | portable | 5000 | 41.785 | 44.648 | 4.714 | 0.475 | 44.107 | 45.320 | 43.453 | 50.989 | 64.214 |
| quic | unary&#45;1k | legacy | 5000 | 40.321 | 51.459 | 10.443 | 10.582 | 43.222 | 63.504 | 44.781 | 64.507 | 77.791 |
| quic | unary&#45;1k | portable | 5000 | 43.700 | 54.046 | 12.799 | 10.684 | 47.313 | 72.175 | 46.973 | 71.798 | 86.993 |
| quic | fanout&#45;32 | legacy | 5000 | 164.571 | 172.896 | 9.183 | 0.722 | 171.651 | 173.464 | 169.852 | 188.530 | 202.399 |
| quic | fanout&#45;32 | portable | 5000 | 193.949 | 206.150 | 13.517 | 5.059 | 201.819 | 214.876 | 201.927 | 226.446 | 261.999 |
| quic | dependent&#45;read | legacy | 5000 | 84.437 | 90.220 | 11.722 | 1.981 | 89.237 | 93.759 | 87.897 | 103.226 | 117.284 |
| quic | dependent&#45;read | portable | 5000 | 106.787 | 114.483 | 10.546 | 1.637 | 113.280 | 116.446 | 111.812 | 130.927 | 145.820 |
| quic | pipelined&#45;read | legacy | 5000 | 50.268 | 53.665 | 5.014 | 0.819 | 52.728 | 54.540 | 52.316 | 63.315 | 75.369 |
| quic | pipelined&#45;read | portable | 5000 | 77.371 | 82.115 | 7.185 | 0.664 | 81.522 | 83.197 | 79.968 | 96.209 | 109.465 |
| quic | write&#45;read | legacy | 5000 | 79.137 | 86.080 | 6.787 | 1.106 | 84.840 | 87.502 | 84.229 | 98.880 | 113.149 |
| quic | write&#45;read | portable | 5000 | 86.270 | 93.546 | 7.937 | 1.979 | 91.901 | 96.455 | 91.325 | 108.390 | 122.514 |
| quic | callback | legacy | 5000 | 83.539 | 89.067 | 6.009 | 0.751 | 88.095 | 90.004 | 87.076 | 101.576 | 112.822 |
| quic | callback | portable | 5000 | 102.561 | 110.684 | 17.312 | 0.872 | 109.397 | 111.805 | 107.679 | 125.858 | 142.684 |
| quic | bulk&#45;64k | legacy | 5000 | 143.442 | 152.067 | 9.485 | 1.158 | 150.480 | 153.245 | 148.960 | 170.079 | 181.459 |
| quic | bulk&#45;64k | portable | 5000 | 147.759 | 174.952 | 38.906 | 42.274 | 154.345 | 250.548 | 153.976 | 254.983 | 266.904 |
| quic | bulk&#45;256k | legacy | 5000 | 361.045 | 398.447 | 674.145 | 15.008 | 380.820 | 412.249 | 378.260 | 404.250 | 421.547 |
| quic | bulk&#45;256k | portable | 5000 | 382.186 | 427.829 | 682.063 | 39.881 | 399.102 | 484.845 | 394.110 | 588.401 | 618.173 |
| quic | bulk&#45;1m | legacy | 5000 | 1388.129 | 1443.142 | 401.613 | 11.991 | 1432.048 | 1458.441 | 1432.239 | 1474.623 | 1536.162 |
| quic | bulk&#45;1m | portable | 5000 | 1381.390 | 1499.364 | 582.353 | 63.466 | 1435.964 | 1591.773 | 1436.772 | 1992.859 | 2038.103 |
| quic | bulk&#45;4m | legacy | 5000 | 6361.227 | 6809.847 | 414.560 | 48.865 | 6723.649 | 6839.030 | 6736.751 | 7102.093 | 9153.710 |
| quic | bulk&#45;4m | portable | 5000 | 6394.407 | 6873.077 | 303.128 | 66.256 | 6817.786 | 6986.858 | 6833.043 | 7168.424 | 7953.319 |
| quic | deep&#45;8 | legacy | 5000 | 41.371 | 44.606 | 3.542 | 0.429 | 44.247 | 45.124 | 43.550 | 51.933 | 60.299 |
| quic | deep&#45;8 | portable | 5000 | 44.570 | 48.079 | 3.717 | 0.889 | 47.184 | 49.145 | 46.972 | 56.475 | 63.012 |
| quic | deep&#45;32 | legacy | 5000 | 47.478 | 50.632 | 3.849 | 0.539 | 49.764 | 51.021 | 49.447 | 59.120 | 66.116 |
| quic | deep&#45;32 | portable | 5000 | 51.586 | 55.132 | 3.781 | 0.491 | 54.282 | 55.545 | 53.959 | 64.087 | 71.301 |
| quic | concurrent&#45;32 | legacy | 5000 | 120.370 | 128.693 | 7.156 | 3.414 | 126.151 | 134.621 | 125.903 | 142.708 | 152.972 |
| quic | concurrent&#45;32 | portable | 5000 | 147.388 | 156.587 | 7.541 | 1.444 | 154.891 | 158.100 | 153.991 | 171.099 | 182.979 |
| quic | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 357.237 | 404.222 | 37.634 | 32.813 | 376.905 | 453.470 | 386.442 | 469.173 | 491.348 |
| quic | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 410.969 | 426.980 | 13.644 | 1.270 | 426.062 | 429.172 | 426.443 | 442.958 | 456.650 |
| quic | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 367.423 | 401.204 | 17.525 | 4.892 | 392.731 | 405.344 | 399.762 | 428.798 | 443.621 |
| quic | services&#45;4&#45;deep&#45;16 | portable | 5000 | 339.276 | 369.600 | 14.098 | 2.860 | 364.914 | 372.115 | 367.688 | 392.518 | 404.665 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1527.551 | 1622.176 | 86.865 | 24.843 | 1594.633 | 1654.270 | 1601.618 | 1753.259 | 2008.862 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1655.401 | 1745.910 | 61.137 | 21.858 | 1724.887 | 1782.260 | 1731.996 | 1835.927 | 2004.390 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1449.408 | 1683.611 | 80.978 | 40.561 | 1613.164 | 1715.494 | 1681.798 | 1810.835 | 1883.041 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1423.847 | 1784.677 | 103.303 | 20.572 | 1757.234 | 1805.261 | 1804.631 | 1913.461 | 1978.943 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2875.714 | 3509.044 | 133.868 | 22.113 | 3469.944 | 3521.577 | 3524.169 | 3684.374 | 3767.758 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2786.527 | 3588.980 | 339.145 | 96.644 | 3434.313 | 3669.999 | 3676.598 | 4040.641 | 4097.156 |

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


