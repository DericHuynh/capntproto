# Capntproto verification reports

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-27T02:53:17Z · run 40441022963 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40441022963) · commit `eecb9aef1eaf` · **failure**

![Cargo test results](docs/reports/cargo-history.svg)

| Total | Passed | Failed | Errors | Skipped |
| ---: | ---: | ---: | ---: | ---: |
| 2532 | 2512 | [0](docs/reports/failed-tests.md) | 0 | 20 |

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-27T02:53:17Z · run 40441022963 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40441022963) · commit `eecb9aef1eaf` · **failure**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

![TLA&#43; checks and expected counterexamples](docs/reports/tla-outcomes.svg)

Unique module&#47;configuration&#47;expected&#45;exit checks&#46; Expected invariant violations are successful controls&#46;

![TLA&#43; explored states by model](docs/reports/tla-states.svg)

Counts sum independent bounded configurations&#59; they are not globally distinct states or a proof beyond those bounds&#46;

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-27T02:53:17Z · run 40441022963 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40441022963) · commit `eecb9aef1eaf` · **failure**

![Fuzzing executions by engine](docs/reports/fuzz-executions.svg)

Bounded campaigns including seed calibration&#59; execution counts are not comparable performance benchmarks&#46;

![Fuzzer feedback by engine](docs/reports/fuzz-coverage.svg)

Engine&#45;local counters&#44; not LLVM source coverage&#46; Do not compare counts across engines or builds&#46;

![Saved fuzzing findings](docs/reports/fuzz-findings.svg)

Saved crashes&#47;hangs are findings requiring triage&#44; not confirmed unique bugs&#46; Missing results remain unknown&#46;

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

Latest coverage run: [2026-10-27T02:53:17Z · run 40441022868 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40441022868) · commit `eecb9aef1eaf` · **failure**

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-27T02:53:17Z · run 40441022963 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40441022963) · commit `eecb9aef1eaf` · **failure**

Measurement evidence: **passed**. Independently validated encrypted benchmark values&#46; Workflow status and diagnostic completion are separate&#59; unavailable values remain blank&#46; Operations include whole concurrent batches and every service hop&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Current TCP fixtures use TLS 1.3, including upstream Rust Cap’n Proto, C++, gRPC over HTTP/2 and WebSockets over WSS; native encrypted transports and standard QUIC are identified separately. Historical data retains its recorded fixture contract. The dedicated campaign also reports recursive data, concurrent calls and independent service chains for gRPC and binary Protobuf WebSockets in its encrypted comparison artifact. Bars compare the recorded workload, rather than universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.


## Exhaustive benchmark tables

Measured run: [2026-10-27T02:53:17Z · run 40441022963 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40441022963) · commit `eecb9aef1eaf` · **failure**

All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.

### All encrypted latency comparisons

| Transport | Case | Implementation | Samples | Minimum µs | Mean µs | Sample SD µs | Run mean SD µs | Run mean min µs | Run mean max µs | p50 µs | p95 µs | p99 µs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tls | loopback&#45;0 | cpp | 5000 | 44.591 | 64.812 | 17.331 | 16.462 | 46.616 | 77.263 | 73.272 | 85.442 | 96.072 |
| tls | loopback&#45;0 | grpc | 5000 | 51.803 | 74.978 | 17.728 | 18.689 | 54.277 | 91.178 | 84.930 | 97.245 | 107.491 |
| tls | loopback&#45;0 | legacy | 5000 | 29.095 | 39.229 | 23.866 | 10.900 | 31.104 | 51.356 | 31.325 | 53.570 | 65.235 |
| tls | loopback&#45;0 | portable | 5000 | 30.017 | 41.091 | 12.841 | 11.687 | 31.581 | 54.993 | 32.937 | 57.705 | 68.046 |
| tls | loopback&#45;0 | websocket | 5000 | 26.480 | 38.777 | 7.562 | 7.192 | 30.729 | 44.772 | 41.448 | 49.214 | 58.219 |
| tls | loopback&#45;64 | cpp | 5000 | 44.562 | 49.228 | 5.699 | 1.695 | 47.190 | 51.311 | 47.799 | 59.733 | 70.041 |
| tls | loopback&#45;64 | grpc | 5000 | 52.574 | 56.628 | 20.263 | 0.981 | 55.529 | 58.166 | 54.485 | 66.083 | 77.333 |
| tls | loopback&#45;64 | legacy | 5000 | 29.193 | 31.222 | 3.254 | 0.465 | 30.540 | 31.786 | 30.254 | 37.274 | 46.149 |
| tls | loopback&#45;64 | portable | 5000 | 30.097 | 33.021 | 4.746 | 0.282 | 32.613 | 33.318 | 31.634 | 40.415 | 51.025 |
| tls | loopback&#45;64 | websocket | 5000 | 22.449 | 29.316 | 3.611 | 0.507 | 28.521 | 29.820 | 28.714 | 35.342 | 45.623 |
| tls | loopback&#45;1024 | cpp | 5000 | 46.111 | 49.975 | 4.537 | 1.295 | 48.667 | 51.648 | 48.327 | 58.667 | 68.085 |
| tls | loopback&#45;1024 | grpc | 5000 | 56.096 | 60.837 | 6.571 | 2.159 | 59.504 | 64.637 | 58.790 | 72.660 | 86.774 |
| tls | loopback&#45;1024 | legacy | 5000 | 31.329 | 33.567 | 4.581 | 0.340 | 33.164 | 34.104 | 32.484 | 39.498 | 48.162 |
| tls | loopback&#45;1024 | portable | 5000 | 32.657 | 35.181 | 4.219 | 0.252 | 34.811 | 35.441 | 33.991 | 41.754 | 51.462 |
| tls | loopback&#45;1024 | websocket | 5000 | 23.030 | 27.516 | 3.572 | 0.908 | 26.936 | 29.093 | 26.740 | 33.256 | 42.017 |
| tls | loopback&#45;65536 | cpp | 5000 | 200.890 | 215.791 | 17.770 | 3.816 | 211.461 | 221.490 | 211.900 | 239.197 | 256.390 |
| tls | loopback&#45;65536 | grpc | 5000 | 202.695 | 228.057 | 13.998 | 1.462 | 226.733 | 230.131 | 223.593 | 256.572 | 274.096 |
| tls | loopback&#45;65536 | legacy | 5000 | 146.000 | 171.545 | 33.258 | 34.487 | 154.430 | 233.035 | 155.493 | 243.023 | 259.834 |
| tls | loopback&#45;65536 | portable | 5000 | 152.750 | 162.870 | 9.768 | 2.273 | 159.479 | 165.881 | 159.459 | 181.034 | 197.074 |
| tls | loopback&#45;65536 | websocket | 5000 | 165.413 | 175.752 | 9.126 | 1.330 | 173.988 | 177.333 | 172.538 | 191.723 | 208.177 |
| tls | unary&#45;empty | cpp | 5000 | 44.847 | 60.528 | 15.384 | 15.735 | 47.133 | 78.614 | 51.462 | 83.596 | 94.733 |
| tls | unary&#45;empty | grpc | 5000 | 52.524 | 67.080 | 16.819 | 15.331 | 56.024 | 89.549 | 56.031 | 93.085 | 102.333 |
| tls | unary&#45;empty | legacy | 5000 | 28.819 | 41.666 | 9.953 | 9.700 | 30.992 | 49.241 | 46.715 | 53.379 | 62.556 |
| tls | unary&#45;empty | portable | 5000 | 30.666 | 44.766 | 9.976 | 10.316 | 33.280 | 52.616 | 50.302 | 57.930 | 66.931 |
| tls | unary&#45;empty | websocket | 5000 | 24.506 | 39.508 | 7.480 | 7.582 | 31.120 | 45.524 | 42.694 | 48.512 | 56.512 |
| tls | unary&#45;small | cpp | 5000 | 44.497 | 47.915 | 4.541 | 1.130 | 46.750 | 49.499 | 46.226 | 56.415 | 67.080 |
| tls | unary&#45;small | grpc | 5000 | 53.897 | 64.977 | 15.305 | 15.947 | 56.983 | 93.469 | 56.545 | 94.876 | 105.272 |
| tls | unary&#45;small | legacy | 5000 | 28.827 | 32.629 | 6.564 | 3.774 | 30.580 | 39.355 | 30.104 | 47.550 | 53.007 |
| tls | unary&#45;small | portable | 5000 | 30.443 | 37.392 | 9.435 | 9.571 | 32.188 | 54.448 | 32.227 | 56.317 | 67.534 |
| tls | unary&#45;small | websocket | 5000 | 25.052 | 31.589 | 6.289 | 5.445 | 28.904 | 41.321 | 29.394 | 44.376 | 49.851 |
| tls | unary&#45;1k | cpp | 5000 | 46.423 | 50.287 | 4.519 | 0.706 | 49.334 | 51.162 | 48.867 | 58.152 | 67.870 |
| tls | unary&#45;1k | grpc | 5000 | 57.101 | 61.636 | 6.009 | 0.637 | 60.842 | 62.378 | 60.243 | 72.234 | 83.190 |
| tls | unary&#45;1k | legacy | 5000 | 30.784 | 34.213 | 5.404 | 0.652 | 33.349 | 34.922 | 32.824 | 40.985 | 52.257 |
| tls | unary&#45;1k | portable | 5000 | 32.660 | 36.196 | 4.405 | 1.362 | 34.941 | 38.079 | 34.427 | 44.837 | 55.521 |
| tls | unary&#45;1k | websocket | 5000 | 23.728 | 28.108 | 3.711 | 0.243 | 27.855 | 28.484 | 27.342 | 34.103 | 43.899 |
| tls | fanout&#45;32 | cpp | 5000 | 392.488 | 444.257 | 41.605 | 6.550 | 438.224 | 454.592 | 428.334 | 527.753 | 568.387 |
| tls | fanout&#45;32 | grpc | 5000 | 666.120 | 716.823 | 49.273 | 11.539 | 704.957 | 735.332 | 700.336 | 784.860 | 992.310 |
| tls | fanout&#45;32 | legacy | 5000 | 158.108 | 170.861 | 20.828 | 6.514 | 167.128 | 182.417 | 164.648 | 195.453 | 273.933 |
| tls | fanout&#45;32 | portable | 5000 | 190.776 | 228.991 | 52.657 | 55.409 | 199.588 | 327.991 | 204.674 | 339.300 | 363.760 |
| tls | fanout&#45;32 | websocket | 5000 | 188.155 | 231.265 | 45.392 | 47.145 | 207.605 | 315.487 | 212.890 | 328.671 | 352.527 |
| tls | dependent&#45;read | cpp | 5000 | 105.041 | 112.796 | 7.917 | 1.050 | 111.152 | 113.986 | 109.505 | 128.129 | 142.243 |
| tls | dependent&#45;read | grpc | 5000 | 110.899 | 118.489 | 7.390 | 0.616 | 117.980 | 119.361 | 115.485 | 133.534 | 146.257 |
| tls | dependent&#45;read | legacy | 5000 | 64.524 | 70.665 | 7.711 | 2.608 | 68.671 | 75.248 | 67.992 | 83.663 | 95.576 |
| tls | dependent&#45;read | portable | 5000 | 82.882 | 90.330 | 11.299 | 1.504 | 88.418 | 92.163 | 87.372 | 105.841 | 117.778 |
| tls | dependent&#45;read | websocket | 5000 | 43.951 | 55.320 | 6.288 | 1.459 | 53.264 | 56.534 | 54.292 | 66.449 | 76.906 |
| tls | pipelined&#45;read | cpp | 5000 | 75.949 | 92.300 | 21.477 | 23.060 | 80.638 | 133.526 | 81.235 | 133.789 | 147.060 |
| tls | pipelined&#45;read | legacy | 5000 | 40.316 | 58.141 | 14.456 | 14.213 | 42.375 | 70.887 | 66.618 | 77.368 | 89.204 |
| tls | pipelined&#45;read | portable | 5000 | 67.010 | 92.225 | 24.879 | 26.431 | 72.171 | 122.221 | 76.806 | 129.816 | 144.799 |
| tls | write&#45;read | cpp | 5000 | 90.676 | 109.683 | 24.094 | 25.468 | 95.386 | 155.127 | 97.454 | 159.016 | 170.972 |
| tls | write&#45;read | grpc | 5000 | 111.017 | 118.761 | 8.013 | 1.337 | 116.928 | 120.244 | 115.709 | 133.766 | 150.342 |
| tls | write&#45;read | legacy | 5000 | 60.263 | 71.970 | 16.775 | 16.332 | 63.446 | 101.135 | 63.803 | 102.219 | 114.469 |
| tls | write&#45;read | portable | 5000 | 62.559 | 69.358 | 10.731 | 2.898 | 65.735 | 73.080 | 66.756 | 83.229 | 98.398 |
| tls | write&#45;read | websocket | 5000 | 45.737 | 53.769 | 5.397 | 0.881 | 53.154 | 55.294 | 52.630 | 62.939 | 73.169 |
| tls | callback | cpp | 5000 | 96.947 | 104.102 | 8.789 | 1.675 | 101.862 | 106.384 | 100.812 | 118.894 | 138.658 |
| tls | callback | grpc | 5000 | 54.169 | 58.561 | 4.758 | 0.739 | 57.578 | 59.612 | 56.782 | 68.122 | 78.341 |
| tls | callback | legacy | 5000 | 63.880 | 69.023 | 9.591 | 3.512 | 66.511 | 75.126 | 65.835 | 82.661 | 118.515 |
| tls | callback | portable | 5000 | 79.648 | 96.668 | 25.532 | 24.977 | 83.772 | 141.248 | 83.268 | 144.286 | 158.900 |
| tls | callback | websocket | 5000 | 46.479 | 56.196 | 5.670 | 0.881 | 55.302 | 57.655 | 55.596 | 64.067 | 74.966 |
| tls | bulk&#45;64k | cpp | 5000 | 197.706 | 212.210 | 13.469 | 1.832 | 210.450 | 214.850 | 207.884 | 235.424 | 251.243 |
| tls | bulk&#45;64k | grpc | 5000 | 204.553 | 231.438 | 17.692 | 4.806 | 226.988 | 238.578 | 225.699 | 267.093 | 292.434 |
| tls | bulk&#45;64k | legacy | 5000 | 146.236 | 155.204 | 10.144 | 2.336 | 153.307 | 159.169 | 151.533 | 172.444 | 189.903 |
| tls | bulk&#45;64k | portable | 5000 | 152.169 | 162.308 | 10.178 | 1.854 | 159.841 | 163.911 | 157.776 | 183.034 | 200.995 |
| tls | bulk&#45;64k | websocket | 5000 | 165.098 | 176.433 | 10.193 | 2.765 | 171.893 | 179.354 | 172.500 | 194.365 | 215.970 |
| tls | bulk&#45;256k | cpp | 5000 | 774.002 | 825.677 | 43.848 | 7.371 | 814.439 | 834.378 | 815.487 | 893.039 | 928.376 |
| tls | bulk&#45;256k | grpc | 5000 | 503.878 | 850.351 | 171.560 | 172.177 | 542.965 | 943.082 | 887.786 | 1105.923 | 1185.890 |
| tls | bulk&#45;256k | legacy | 5000 | 447.898 | 478.601 | 18.066 | 2.062 | 476.554 | 481.224 | 472.768 | 514.724 | 534.838 |
| tls | bulk&#45;256k | portable | 5000 | 452.384 | 486.408 | 31.722 | 7.401 | 481.102 | 499.375 | 478.687 | 527.405 | 597.439 |
| tls | bulk&#45;256k | websocket | 5000 | 958.441 | 1008.995 | 31.426 | 7.014 | 1002.301 | 1020.274 | 996.608 | 1071.213 | 1127.857 |
| tls | bulk&#45;1m | cpp | 5000 | 2980.317 | 3152.921 | 98.783 | 30.488 | 3099.684 | 3176.014 | 3136.100 | 3312.476 | 3440.183 |
| tls | bulk&#45;1m | grpc | 5000 | 2047.380 | 3793.579 | 706.717 | 724.371 | 3087.059 | 4549.174 | 3649.429 | 4865.976 | 5104.945 |
| tls | bulk&#45;1m | legacy | 5000 | 1536.501 | 1623.957 | 116.233 | 66.144 | 1592.396 | 1742.216 | 1585.601 | 1986.549 | 2088.578 |
| tls | bulk&#45;1m | portable | 5000 | 1540.604 | 1651.089 | 152.759 | 95.200 | 1589.423 | 1816.607 | 1588.569 | 2020.448 | 2106.049 |
| tls | bulk&#45;1m | websocket | 5000 | 3783.254 | 4119.892 | 296.968 | 138.869 | 4020.434 | 4364.755 | 4023.356 | 4779.103 | 5192.690 |
| tls | bulk&#45;4m | cpp | 5000 | 13156.285 | 14330.412 | 1093.661 | 203.753 | 14112.677 | 14605.072 | 14066.005 | 17148.369 | 19046.570 |
| tls | bulk&#45;4m | grpc | 5000 | 16474.095 | 17966.383 | 942.904 | 180.981 | 17751.476 | 18247.862 | 17771.807 | 19826.389 | 21678.055 |
| tls | bulk&#45;4m | legacy | 5000 | 7201.514 | 7861.474 | 533.545 | 152.119 | 7693.545 | 8052.380 | 7723.840 | 9273.711 | 9869.320 |
| tls | bulk&#45;4m | portable | 5000 | 7294.675 | 7857.336 | 354.501 | 58.104 | 7815.313 | 7959.068 | 7794.515 | 8381.405 | 9354.089 |
| tls | bulk&#45;4m | websocket | 5000 | 24201.471 | 27662.777 | 2590.146 | 765.861 | 26733.936 | 28568.144 | 26401.675 | 32334.552 | 33144.180 |
| tls | deep&#45;8 | cpp | 5000 | 47.055 | 49.999 | 4.370 | 0.560 | 49.231 | 50.599 | 48.645 | 57.347 | 70.897 |
| tls | deep&#45;8 | grpc | 5000 | 63.474 | 68.849 | 6.695 | 1.969 | 67.169 | 71.936 | 66.219 | 81.104 | 98.064 |
| tls | deep&#45;8 | legacy | 5000 | 31.839 | 34.415 | 4.093 | 1.006 | 33.812 | 36.188 | 33.114 | 40.343 | 52.364 |
| tls | deep&#45;8 | portable | 5000 | 32.964 | 36.403 | 4.338 | 1.224 | 35.115 | 37.730 | 34.695 | 43.562 | 55.951 |
| tls | deep&#45;8 | websocket | 5000 | 27.543 | 36.400 | 5.881 | 0.987 | 35.383 | 38.033 | 35.505 | 46.491 | 55.935 |
| tls | deep&#45;32 | cpp | 5000 | 53.531 | 57.625 | 5.039 | 1.026 | 56.075 | 58.424 | 55.470 | 67.203 | 78.881 |
| tls | deep&#45;32 | grpc | 5000 | 80.696 | 86.682 | 6.877 | 1.599 | 85.397 | 88.459 | 83.777 | 100.734 | 115.298 |
| tls | deep&#45;32 | legacy | 5000 | 38.719 | 41.822 | 4.154 | 0.686 | 40.957 | 42.570 | 40.101 | 49.838 | 60.621 |
| tls | deep&#45;32 | portable | 5000 | 40.026 | 42.939 | 3.959 | 0.449 | 42.498 | 43.589 | 41.453 | 50.505 | 60.136 |
| tls | deep&#45;32 | websocket | 5000 | 46.087 | 53.908 | 5.692 | 1.003 | 52.410 | 55.197 | 52.749 | 64.196 | 74.017 |
| tls | concurrent&#45;32 | cpp | 5000 | 441.678 | 483.256 | 32.314 | 24.559 | 461.841 | 524.510 | 468.420 | 541.902 | 570.724 |
| tls | concurrent&#45;32 | grpc | 5000 | 545.197 | 615.248 | 57.257 | 58.520 | 569.069 | 679.339 | 593.236 | 708.378 | 745.500 |
| tls | concurrent&#45;32 | legacy | 5000 | 108.473 | 140.388 | 36.246 | 33.376 | 114.561 | 181.049 | 118.218 | 199.501 | 216.082 |
| tls | concurrent&#45;32 | portable | 5000 | 136.722 | 148.243 | 10.782 | 3.284 | 142.761 | 150.652 | 145.192 | 169.425 | 184.255 |
| tls | concurrent&#45;32 | websocket | 5000 | 94.386 | 180.765 | 40.263 | 38.129 | 136.342 | 210.439 | 186.558 | 242.854 | 265.924 |
| tls | deep&#45;32&#45;concurrent&#45;32 | cpp | 5000 | 590.469 | 702.363 | 122.956 | 76.146 | 614.675 | 783.851 | 632.584 | 916.119 | 991.733 |
| tls | deep&#45;32&#45;concurrent&#45;32 | grpc | 5000 | 1358.071 | 1460.860 | 60.181 | 18.848 | 1439.786 | 1478.833 | 1448.024 | 1576.689 | 1638.194 |
| tls | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 395.043 | 438.319 | 41.296 | 36.313 | 411.775 | 501.441 | 418.431 | 526.075 | 553.892 |
| tls | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 421.476 | 462.678 | 36.533 | 28.289 | 445.687 | 512.670 | 447.064 | 530.311 | 574.574 |
| tls | deep&#45;32&#45;concurrent&#45;32 | websocket | 5000 | 640.374 | 737.015 | 96.502 | 44.554 | 703.218 | 808.719 | 700.485 | 993.635 | 1077.689 |
| tls | services&#45;4&#45;deep&#45;16 | cpp | 5000 | 333.363 | 371.353 | 19.280 | 2.612 | 367.886 | 374.708 | 368.427 | 406.287 | 426.049 |
| tls | services&#45;4&#45;deep&#45;16 | grpc | 5000 | 477.795 | 526.999 | 27.993 | 6.576 | 519.631 | 536.586 | 520.817 | 580.157 | 603.828 |
| tls | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 245.634 | 269.914 | 16.003 | 5.193 | 262.257 | 276.288 | 265.926 | 299.363 | 317.940 |
| tls | services&#45;4&#45;deep&#45;16 | portable | 5000 | 276.822 | 306.300 | 19.154 | 4.748 | 297.996 | 309.916 | 302.489 | 343.091 | 363.991 |
| tls | services&#45;4&#45;deep&#45;16 | websocket | 5000 | 195.996 | 266.193 | 40.335 | 6.775 | 258.433 | 275.399 | 262.893 | 328.024 | 359.345 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | 5000 | 2475.575 | 2614.401 | 110.096 | 32.593 | 2586.711 | 2666.400 | 2593.091 | 2801.677 | 2951.228 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | 5000 | 4131.959 | 4377.961 | 144.921 | 58.098 | 4288.338 | 4434.289 | 4362.087 | 4660.141 | 4831.614 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1481.566 | 1547.039 | 59.630 | 8.880 | 1535.482 | 1555.429 | 1528.110 | 1664.193 | 1737.524 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1670.429 | 1748.128 | 50.326 | 5.445 | 1739.710 | 1754.446 | 1731.668 | 1854.031 | 1909.773 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | 5000 | 1287.772 | 1485.199 | 76.419 | 5.524 | 1479.609 | 1491.399 | 1475.933 | 1613.587 | 1699.857 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 2089.978 | 2491.821 | 114.985 | 9.655 | 2477.995 | 2503.749 | 2488.369 | 2680.285 | 2777.706 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 3482.834 | 4155.654 | 208.729 | 78.983 | 4073.077 | 4274.814 | 4128.096 | 4505.338 | 4739.821 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1211.571 | 1576.258 | 80.183 | 22.406 | 1543.669 | 1599.982 | 1563.403 | 1702.064 | 1774.868 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1445.999 | 1742.289 | 89.250 | 46.726 | 1685.280 | 1789.960 | 1731.039 | 1905.335 | 1995.196 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1157.062 | 1482.668 | 143.340 | 79.400 | 1403.582 | 1568.245 | 1470.904 | 1722.360 | 1818.064 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 3929.189 | 5106.995 | 203.315 | 50.708 | 5067.925 | 5194.235 | 5062.562 | 5478.389 | 5708.727 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 6615.518 | 8226.848 | 463.473 | 82.190 | 8182.353 | 8373.375 | 8206.680 | 8996.387 | 9414.286 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2974.089 | 3235.720 | 112.150 | 18.095 | 3213.160 | 3260.668 | 3214.558 | 3424.087 | 3588.040 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2787.791 | 3514.639 | 182.536 | 86.405 | 3410.879 | 3607.889 | 3506.925 | 3834.132 | 3992.131 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 2252.230 | 2740.574 | 216.383 | 23.536 | 2711.488 | 2767.059 | 2704.083 | 3165.341 | 3421.333 |
| quic | loopback&#45;0 | legacy | 5000 | 38.709 | 41.688 | 3.678 | 0.830 | 41.046 | 43.092 | 40.392 | 48.030 | 58.188 |
| quic | loopback&#45;0 | portable | 5000 | 43.624 | 47.132 | 4.227 | 0.696 | 45.964 | 47.720 | 45.501 | 54.982 | 66.626 |
| quic | loopback&#45;64 | legacy | 5000 | 39.456 | 62.908 | 11.471 | 11.517 | 42.375 | 69.283 | 65.416 | 77.038 | 87.133 |
| quic | loopback&#45;64 | portable | 5000 | 44.025 | 60.355 | 15.983 | 15.946 | 47.123 | 77.707 | 49.569 | 83.665 | 94.438 |
| quic | loopback&#45;1024 | legacy | 5000 | 41.553 | 46.610 | 7.797 | 3.738 | 43.848 | 53.091 | 43.609 | 69.602 | 73.604 |
| quic | loopback&#45;1024 | portable | 5000 | 46.726 | 57.951 | 14.181 | 13.238 | 49.752 | 80.789 | 49.669 | 82.825 | 93.243 |
| quic | loopback&#45;65536 | legacy | 5000 | 188.969 | 201.393 | 11.641 | 2.356 | 199.581 | 205.436 | 197.659 | 223.402 | 240.006 |
| quic | loopback&#45;65536 | portable | 5000 | 197.128 | 252.544 | 57.038 | 61.224 | 206.611 | 320.574 | 214.572 | 334.425 | 353.353 |
| quic | unary&#45;empty | legacy | 5000 | 39.084 | 42.052 | 4.015 | 0.547 | 41.256 | 42.798 | 40.698 | 49.271 | 60.924 |
| quic | unary&#45;empty | portable | 5000 | 43.155 | 46.321 | 3.947 | 0.435 | 45.724 | 46.928 | 44.950 | 53.759 | 64.559 |
| quic | unary&#45;small | legacy | 5000 | 39.288 | 42.454 | 4.053 | 0.568 | 41.877 | 43.374 | 40.962 | 50.098 | 61.278 |
| quic | unary&#45;small | portable | 5000 | 43.431 | 46.924 | 4.153 | 0.396 | 46.441 | 47.546 | 45.481 | 54.905 | 65.837 |
| quic | unary&#45;1k | legacy | 5000 | 41.636 | 45.248 | 4.519 | 1.001 | 44.448 | 46.870 | 43.615 | 53.828 | 64.731 |
| quic | unary&#45;1k | portable | 5000 | 46.724 | 50.660 | 4.840 | 1.526 | 49.346 | 53.262 | 48.886 | 59.148 | 72.047 |
| quic | fanout&#45;32 | legacy | 5000 | 189.064 | 199.958 | 11.127 | 1.648 | 198.549 | 202.516 | 195.844 | 222.630 | 237.217 |
| quic | fanout&#45;32 | portable | 5000 | 224.900 | 240.953 | 13.170 | 1.911 | 238.940 | 243.945 | 238.285 | 266.898 | 279.851 |
| quic | dependent&#45;read | legacy | 5000 | 87.665 | 112.641 | 27.307 | 25.879 | 92.680 | 151.533 | 95.961 | 158.427 | 167.844 |
| quic | dependent&#45;read | portable | 5000 | 117.705 | 128.105 | 12.069 | 3.214 | 124.829 | 133.370 | 123.845 | 147.518 | 176.036 |
| quic | pipelined&#45;read | legacy | 5000 | 52.850 | 57.744 | 5.746 | 2.472 | 55.552 | 61.585 | 55.383 | 68.750 | 82.340 |
| quic | pipelined&#45;read | portable | 5000 | 87.633 | 96.318 | 16.060 | 4.906 | 92.482 | 103.981 | 92.031 | 114.764 | 141.569 |
| quic | write&#45;read | legacy | 5000 | 81.341 | 99.668 | 22.069 | 23.100 | 88.397 | 140.975 | 89.144 | 145.051 | 160.992 |
| quic | write&#45;read | portable | 5000 | 90.224 | 118.152 | 30.659 | 29.888 | 96.557 | 159.174 | 98.647 | 167.444 | 180.148 |
| quic | callback | legacy | 5000 | 85.305 | 92.108 | 6.588 | 1.649 | 90.412 | 94.310 | 89.514 | 104.419 | 117.610 |
| quic | callback | portable | 5000 | 112.664 | 122.250 | 8.675 | 1.784 | 119.905 | 124.468 | 119.667 | 140.548 | 153.976 |
| quic | bulk&#45;64k | legacy | 5000 | 189.418 | 206.403 | 18.599 | 3.062 | 203.277 | 210.983 | 201.592 | 233.184 | 301.882 |
| quic | bulk&#45;64k | portable | 5000 | 192.021 | 223.930 | 45.481 | 43.670 | 201.685 | 301.962 | 204.310 | 328.942 | 352.625 |
| quic | bulk&#45;256k | legacy | 5000 | 483.051 | 510.277 | 21.255 | 2.816 | 507.255 | 513.601 | 504.078 | 549.260 | 571.753 |
| quic | bulk&#45;256k | portable | 5000 | 501.095 | 531.552 | 24.599 | 4.250 | 526.533 | 536.907 | 525.384 | 571.354 | 601.089 |
| quic | bulk&#45;1m | legacy | 5000 | 1775.305 | 1895.323 | 167.164 | 83.820 | 1846.888 | 2043.830 | 1844.665 | 2478.092 | 2595.741 |
| quic | bulk&#45;1m | portable | 5000 | 1757.427 | 1935.755 | 448.441 | 110.319 | 1858.122 | 2116.417 | 1848.661 | 2507.457 | 2617.351 |
| quic | bulk&#45;4m | legacy | 5000 | 8451.253 | 9299.421 | 710.946 | 153.899 | 9158.351 | 9535.720 | 9132.558 | 11407.275 | 12314.839 |
| quic | bulk&#45;4m | portable | 5000 | 8531.144 | 9272.586 | 454.662 | 46.253 | 9218.024 | 9338.056 | 9197.227 | 9911.277 | 11592.917 |
| quic | deep&#45;8 | legacy | 5000 | 42.401 | 50.259 | 10.959 | 7.723 | 45.146 | 63.955 | 45.847 | 72.568 | 83.690 |
| quic | deep&#45;8 | portable | 5000 | 46.696 | 55.127 | 13.597 | 11.317 | 49.726 | 75.368 | 49.019 | 88.570 | 100.753 |
| quic | deep&#45;32 | legacy | 5000 | 49.531 | 53.425 | 4.536 | 1.515 | 52.593 | 56.107 | 51.678 | 61.553 | 73.258 |
| quic | deep&#45;32 | portable | 5000 | 54.810 | 60.315 | 6.408 | 2.018 | 57.825 | 63.033 | 57.925 | 73.087 | 87.718 |
| quic | concurrent&#45;32 | legacy | 5000 | 123.098 | 129.714 | 6.880 | 1.183 | 128.554 | 131.492 | 126.929 | 142.792 | 156.792 |
| quic | concurrent&#45;32 | portable | 5000 | 153.472 | 163.258 | 9.010 | 2.924 | 159.590 | 167.632 | 160.232 | 181.146 | 196.501 |
| quic | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 434.991 | 492.096 | 56.196 | 41.257 | 452.830 | 551.100 | 477.595 | 581.645 | 664.333 |
| quic | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 474.201 | 515.287 | 34.875 | 27.729 | 497.244 | 562.170 | 497.253 | 576.193 | 612.361 |
| quic | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 392.533 | 426.740 | 30.775 | 5.645 | 421.606 | 435.186 | 420.262 | 470.378 | 490.463 |
| quic | services&#45;4&#45;deep&#45;16 | portable | 5000 | 388.087 | 416.660 | 21.819 | 6.729 | 408.785 | 426.750 | 410.311 | 462.017 | 483.542 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1723.695 | 1906.736 | 92.104 | 32.360 | 1854.871 | 1940.538 | 1906.247 | 2032.886 | 2070.239 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1832.332 | 2000.075 | 57.560 | 6.335 | 1990.788 | 2006.815 | 2000.070 | 2089.829 | 2159.962 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1662.957 | 1992.951 | 115.002 | 52.248 | 1900.501 | 2024.147 | 2000.991 | 2160.422 | 2233.827 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1595.495 | 2000.672 | 153.055 | 57.286 | 1958.817 | 2099.292 | 2015.952 | 2224.873 | 2286.084 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 3313.770 | 3972.264 | 192.832 | 19.456 | 3949.658 | 3990.917 | 3987.133 | 4266.430 | 4409.307 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3105.808 | 4027.416 | 367.747 | 118.838 | 3901.345 | 4214.210 | 4095.114 | 4538.594 | 4732.607 |

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


