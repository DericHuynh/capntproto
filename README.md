# Capntproto verification reports

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-28T18:20:49Z · run 40744243970 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40744243970) · commit `5bdac180fc9b` · **failure**

![Cargo test results](docs/reports/cargo-history.svg)

| Total | Passed | Failed | Errors | Skipped |
| ---: | ---: | ---: | ---: | ---: |
| 2588 | 2567 | [1](docs/reports/failed-tests.md) | 0 | 20 |

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-28T18:20:49Z · run 40744243970 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40744243970) · commit `5bdac180fc9b` · **failure**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

![TLA&#43; checks and expected counterexamples](docs/reports/tla-outcomes.svg)

Unique module&#47;configuration&#47;expected&#45;exit checks&#46; Expected invariant violations are successful controls&#46;

![TLA&#43; explored states by model](docs/reports/tla-states.svg)

Counts sum independent bounded configurations&#59; they are not globally distinct states or a proof beyond those bounds&#46;

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-28T18:20:49Z · run 40744243970 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40744243970) · commit `5bdac180fc9b` · **failure**

![Fuzzing executions by engine](docs/reports/fuzz-executions.svg)

Bounded campaigns including seed calibration&#59; execution counts are not comparable performance benchmarks&#46;

![Fuzzer feedback by engine](docs/reports/fuzz-coverage.svg)

Engine&#45;local counters&#44; not LLVM source coverage&#46; Do not compare counts across engines or builds&#46;

![Saved fuzzing findings](docs/reports/fuzz-findings.svg)

Saved crashes&#47;hangs are findings requiring triage&#44; not confirmed unique bugs&#46; Missing results remain unknown&#46;

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

Latest coverage run: [2026-10-28T18:20:49Z · run 40744244384 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40744244384) · commit `5bdac180fc9b` · **failure**

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-28T18:20:49Z · run 40744243970 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40744243970) · commit `5bdac180fc9b` · **failure**

Measurement evidence: **passed**. Independently validated encrypted benchmark values&#46; Workflow status and diagnostic completion are separate&#59; unavailable values remain blank&#46; Operations include whole concurrent batches and every service hop&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Current TCP fixtures use TLS 1.3, including upstream Rust Cap’n Proto, C++, gRPC over HTTP/2 and WebSockets over WSS; native encrypted transports and standard QUIC are identified separately. Historical data retains its recorded fixture contract. The dedicated campaign also reports recursive data, concurrent calls and independent service chains for gRPC and binary Protobuf WebSockets in its encrypted comparison artifact. Bars compare the recorded workload, rather than universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.


## Exhaustive benchmark tables

Measured run: [2026-10-28T18:20:49Z · run 40744243970 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40744243970) · commit `5bdac180fc9b` · **failure**

All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.

### All encrypted latency comparisons

| Transport | Case | Implementation | Samples | Minimum µs | Mean µs | Sample SD µs | Run mean SD µs | Run mean min µs | Run mean max µs | p50 µs | p95 µs | p99 µs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tls | loopback&#45;0 | cpp | 5000 | 44.143 | 50.683 | 6.175 | 1.065 | 49.707 | 52.481 | 49.168 | 63.726 | 73.748 |
| tls | loopback&#45;0 | grpc | 5000 | 52.197 | 71.100 | 18.223 | 16.111 | 58.950 | 91.885 | 60.712 | 100.519 | 116.531 |
| tls | loopback&#45;0 | legacy | 5000 | 29.066 | 34.481 | 7.652 | 2.860 | 32.259 | 39.475 | 32.439 | 49.178 | 59.203 |
| tls | loopback&#45;0 | portable | 5000 | 30.180 | 38.244 | 9.892 | 9.101 | 33.507 | 54.504 | 33.842 | 56.080 | 69.056 |
| tls | loopback&#45;0 | websocket | 5000 | 26.427 | 37.160 | 9.664 | 7.735 | 31.079 | 46.277 | 31.991 | 51.569 | 62.419 |
| tls | loopback&#45;64 | cpp | 5000 | 45.467 | 77.195 | 12.851 | 8.008 | 62.885 | 81.073 | 77.929 | 95.865 | 108.865 |
| tls | loopback&#45;64 | grpc | 5000 | 84.596 | 93.547 | 9.530 | 0.908 | 92.208 | 94.659 | 91.044 | 110.431 | 124.218 |
| tls | loopback&#45;64 | legacy | 5000 | 31.451 | 50.315 | 7.426 | 1.796 | 47.159 | 51.485 | 49.408 | 63.981 | 73.822 |
| tls | loopback&#45;64 | portable | 5000 | 30.322 | 51.852 | 9.237 | 5.236 | 42.495 | 54.445 | 52.139 | 66.312 | 76.975 |
| tls | loopback&#45;64 | websocket | 5000 | 34.467 | 41.857 | 6.465 | 0.589 | 41.138 | 42.715 | 40.434 | 52.487 | 62.477 |
| tls | loopback&#45;1024 | cpp | 5000 | 46.737 | 59.941 | 14.228 | 13.415 | 52.952 | 83.899 | 53.451 | 87.175 | 100.088 |
| tls | loopback&#45;1024 | grpc | 5000 | 55.791 | 68.217 | 14.225 | 9.265 | 63.671 | 84.775 | 63.365 | 100.136 | 113.651 |
| tls | loopback&#45;1024 | legacy | 5000 | 31.325 | 40.000 | 8.645 | 7.827 | 35.505 | 53.948 | 36.097 | 56.020 | 67.613 |
| tls | loopback&#45;1024 | portable | 5000 | 32.090 | 41.148 | 10.612 | 9.689 | 36.267 | 58.452 | 36.573 | 60.134 | 72.705 |
| tls | loopback&#45;1024 | websocket | 5000 | 23.403 | 29.215 | 5.050 | 0.407 | 28.699 | 29.684 | 28.204 | 38.674 | 47.609 |
| tls | loopback&#45;65536 | cpp | 5000 | 198.018 | 299.664 | 64.532 | 68.403 | 223.824 | 351.177 | 326.029 | 379.653 | 396.013 |
| tls | loopback&#45;65536 | grpc | 5000 | 201.155 | 286.641 | 56.295 | 49.407 | 231.172 | 335.893 | 301.774 | 365.645 | 391.220 |
| tls | loopback&#45;65536 | legacy | 5000 | 143.161 | 182.981 | 40.128 | 36.159 | 159.510 | 243.363 | 162.608 | 257.670 | 280.083 |
| tls | loopback&#45;65536 | portable | 5000 | 149.630 | 218.837 | 46.357 | 48.242 | 166.009 | 256.034 | 236.269 | 278.880 | 300.033 |
| tls | loopback&#45;65536 | websocket | 5000 | 161.528 | 237.900 | 50.077 | 52.998 | 178.928 | 277.683 | 257.635 | 299.772 | 317.648 |
| tls | unary&#45;empty | cpp | 5000 | 44.086 | 49.630 | 5.865 | 0.501 | 48.826 | 50.144 | 48.025 | 62.576 | 73.260 |
| tls | unary&#45;empty | grpc | 5000 | 52.384 | 66.163 | 15.581 | 15.270 | 58.887 | 93.470 | 59.352 | 96.346 | 112.458 |
| tls | unary&#45;empty | legacy | 5000 | 29.116 | 32.600 | 4.546 | 0.191 | 32.313 | 32.796 | 31.457 | 42.304 | 50.632 |
| tls | unary&#45;empty | portable | 5000 | 29.891 | 34.082 | 4.892 | 0.432 | 33.347 | 34.400 | 32.920 | 43.848 | 52.948 |
| tls | unary&#45;empty | websocket | 5000 | 27.549 | 31.836 | 4.193 | 0.313 | 31.446 | 32.171 | 30.999 | 39.086 | 49.294 |
| tls | unary&#45;small | cpp | 5000 | 44.894 | 62.724 | 16.018 | 16.105 | 50.656 | 80.455 | 52.865 | 88.042 | 101.420 |
| tls | unary&#45;small | grpc | 5000 | 53.382 | 74.359 | 19.422 | 19.750 | 59.819 | 97.082 | 62.239 | 105.356 | 121.263 |
| tls | unary&#45;small | legacy | 5000 | 29.360 | 35.891 | 8.037 | 6.512 | 32.485 | 47.528 | 32.640 | 52.492 | 61.508 |
| tls | unary&#45;small | portable | 5000 | 30.142 | 41.861 | 11.117 | 11.008 | 33.638 | 54.169 | 34.658 | 58.206 | 70.839 |
| tls | unary&#45;small | websocket | 5000 | 24.148 | 33.438 | 7.884 | 5.221 | 29.719 | 42.150 | 30.260 | 48.268 | 58.289 |
| tls | unary&#45;1k | cpp | 5000 | 46.760 | 59.033 | 14.705 | 14.205 | 52.161 | 84.436 | 52.814 | 87.270 | 103.118 |
| tls | unary&#45;1k | grpc | 5000 | 56.453 | 66.461 | 12.109 | 3.988 | 64.196 | 73.539 | 63.715 | 93.958 | 111.441 |
| tls | unary&#45;1k | legacy | 5000 | 31.455 | 39.479 | 9.061 | 8.531 | 35.100 | 54.717 | 35.439 | 55.926 | 67.913 |
| tls | unary&#45;1k | portable | 5000 | 31.820 | 36.315 | 4.993 | 0.660 | 35.515 | 37.006 | 35.114 | 47.008 | 57.279 |
| tls | unary&#45;1k | websocket | 5000 | 23.698 | 30.976 | 6.813 | 3.124 | 28.641 | 36.443 | 28.809 | 46.235 | 54.907 |
| tls | fanout&#45;32 | cpp | 5000 | 396.435 | 473.669 | 77.602 | 38.937 | 447.112 | 541.087 | 453.769 | 667.716 | 785.252 |
| tls | fanout&#45;32 | grpc | 5000 | 669.411 | 778.436 | 95.588 | 69.415 | 742.838 | 902.434 | 757.546 | 1031.188 | 1095.823 |
| tls | fanout&#45;32 | legacy | 5000 | 154.414 | 180.036 | 30.783 | 15.881 | 171.721 | 208.358 | 172.503 | 262.172 | 298.840 |
| tls | fanout&#45;32 | portable | 5000 | 182.515 | 202.865 | 16.093 | 1.329 | 201.341 | 204.586 | 199.926 | 235.567 | 250.050 |
| tls | fanout&#45;32 | websocket | 5000 | 185.864 | 236.946 | 44.627 | 37.116 | 217.239 | 303.246 | 221.829 | 341.082 | 372.245 |
| tls | dependent&#45;read | cpp | 5000 | 104.518 | 129.494 | 27.731 | 24.055 | 117.738 | 172.483 | 119.518 | 195.243 | 212.094 |
| tls | dependent&#45;read | grpc | 5000 | 110.114 | 124.848 | 10.834 | 1.408 | 123.234 | 127.039 | 123.428 | 146.499 | 160.620 |
| tls | dependent&#45;read | legacy | 5000 | 64.190 | 80.981 | 19.007 | 19.119 | 71.948 | 115.178 | 73.201 | 118.573 | 135.627 |
| tls | dependent&#45;read | portable | 5000 | 79.484 | 93.017 | 15.507 | 5.872 | 89.849 | 103.496 | 89.448 | 124.468 | 155.882 |
| tls | dependent&#45;read | websocket | 5000 | 45.367 | 56.348 | 7.267 | 0.930 | 55.395 | 57.778 | 54.985 | 70.031 | 81.669 |
| tls | pipelined&#45;read | cpp | 5000 | 76.187 | 109.464 | 30.413 | 32.161 | 85.756 | 145.625 | 91.444 | 157.559 | 170.130 |
| tls | pipelined&#45;read | legacy | 5000 | 39.626 | 50.064 | 11.827 | 9.748 | 44.699 | 67.464 | 45.337 | 74.089 | 85.419 |
| tls | pipelined&#45;read | portable | 5000 | 60.704 | 91.062 | 25.457 | 24.138 | 68.320 | 116.742 | 82.036 | 128.762 | 141.102 |
| tls | write&#45;read | cpp | 5000 | 90.543 | 138.013 | 31.415 | 32.524 | 102.280 | 163.053 | 149.938 | 181.151 | 191.158 |
| tls | write&#45;read | grpc | 5000 | 110.608 | 142.224 | 33.274 | 31.022 | 125.505 | 197.234 | 127.408 | 205.495 | 225.343 |
| tls | write&#45;read | legacy | 5000 | 60.069 | 80.977 | 19.799 | 18.284 | 67.123 | 104.390 | 70.124 | 113.922 | 129.254 |
| tls | write&#45;read | portable | 5000 | 61.464 | 97.461 | 21.850 | 19.560 | 68.766 | 111.426 | 102.742 | 126.938 | 137.898 |
| tls | write&#45;read | websocket | 5000 | 45.724 | 68.293 | 16.647 | 15.380 | 55.800 | 86.975 | 62.532 | 98.550 | 111.299 |
| tls | callback | cpp | 5000 | 96.550 | 135.559 | 34.515 | 35.224 | 108.126 | 174.921 | 116.956 | 190.857 | 205.288 |
| tls | callback | grpc | 5000 | 55.622 | 87.788 | 15.110 | 14.016 | 62.808 | 95.903 | 87.922 | 109.371 | 121.937 |
| tls | callback | legacy | 5000 | 64.717 | 103.877 | 18.708 | 17.836 | 71.991 | 112.579 | 107.642 | 129.549 | 143.296 |
| tls | callback | portable | 5000 | 78.011 | 108.530 | 30.051 | 28.187 | 87.875 | 145.602 | 90.691 | 157.405 | 174.438 |
| tls | callback | websocket | 5000 | 46.419 | 71.560 | 14.617 | 13.453 | 56.593 | 81.604 | 73.892 | 94.267 | 106.737 |
| tls | bulk&#45;64k | cpp | 5000 | 195.558 | 220.551 | 17.859 | 1.685 | 218.316 | 222.815 | 218.099 | 255.145 | 276.688 |
| tls | bulk&#45;64k | grpc | 5000 | 198.833 | 266.759 | 52.629 | 46.634 | 233.163 | 339.428 | 240.206 | 362.151 | 388.442 |
| tls | bulk&#45;64k | legacy | 5000 | 143.960 | 181.154 | 40.131 | 29.237 | 159.081 | 215.313 | 162.263 | 257.997 | 282.928 |
| tls | bulk&#45;64k | portable | 5000 | 148.662 | 168.494 | 20.707 | 6.161 | 163.881 | 179.253 | 164.088 | 205.209 | 256.314 |
| tls | bulk&#45;64k | websocket | 5000 | 165.276 | 201.756 | 40.314 | 41.313 | 182.156 | 275.646 | 184.715 | 284.099 | 310.947 |
| tls | bulk&#45;256k | cpp | 5000 | 766.776 | 852.075 | 42.453 | 6.409 | 841.557 | 858.443 | 852.676 | 918.395 | 950.556 |
| tls | bulk&#45;256k | grpc | 5000 | 491.908 | 883.339 | 193.204 | 190.486 | 549.647 | 1018.868 | 915.408 | 1174.860 | 1245.208 |
| tls | bulk&#45;256k | legacy | 5000 | 432.486 | 480.568 | 23.121 | 0.915 | 479.841 | 481.961 | 480.898 | 518.142 | 537.438 |
| tls | bulk&#45;256k | portable | 5000 | 434.633 | 488.083 | 25.004 | 1.796 | 485.371 | 490.340 | 487.756 | 528.143 | 552.167 |
| tls | bulk&#45;256k | websocket | 5000 | 935.202 | 1023.365 | 40.653 | 8.574 | 1009.896 | 1030.403 | 1024.034 | 1085.962 | 1133.125 |
| tls | bulk&#45;1m | cpp | 5000 | 3004.046 | 3282.489 | 247.588 | 111.607 | 3197.549 | 3478.026 | 3232.747 | 3502.744 | 4446.286 |
| tls | bulk&#45;1m | grpc | 5000 | 3441.419 | 4549.194 | 377.748 | 168.799 | 4337.886 | 4752.614 | 4534.830 | 5135.075 | 5469.293 |
| tls | bulk&#45;1m | legacy | 5000 | 1495.549 | 1847.572 | 207.440 | 182.513 | 1623.630 | 2006.211 | 1947.162 | 2096.014 | 2146.915 |
| tls | bulk&#45;1m | portable | 5000 | 1494.288 | 1920.003 | 202.895 | 190.747 | 1596.555 | 2049.454 | 2002.148 | 2132.557 | 2187.437 |
| tls | bulk&#45;1m | websocket | 5000 | 3781.511 | 4238.856 | 307.895 | 286.950 | 4051.426 | 4746.406 | 4125.653 | 4865.911 | 5093.017 |
| tls | bulk&#45;4m | cpp | 5000 | 13548.737 | 15587.072 | 1953.219 | 1245.321 | 14608.171 | 17677.527 | 14771.001 | 19858.968 | 20403.663 |
| tls | bulk&#45;4m | grpc | 5000 | 17180.040 | 22078.016 | 2756.429 | 2433.434 | 19087.599 | 24177.831 | 22192.526 | 26477.971 | 27186.564 |
| tls | bulk&#45;4m | legacy | 5000 | 7387.968 | 8219.999 | 603.925 | 202.676 | 8047.875 | 8475.092 | 8057.293 | 9901.051 | 10305.098 |
| tls | bulk&#45;4m | portable | 5000 | 7378.913 | 8470.084 | 824.192 | 645.497 | 8040.980 | 9614.214 | 8096.296 | 10021.177 | 10285.877 |
| tls | bulk&#45;4m | websocket | 5000 | 24593.784 | 27506.138 | 2447.815 | 193.780 | 27312.748 | 27816.134 | 26355.438 | 32293.428 | 32754.205 |
| tls | deep&#45;8 | cpp | 5000 | 46.796 | 66.133 | 17.590 | 17.850 | 51.970 | 86.162 | 55.555 | 93.683 | 107.791 |
| tls | deep&#45;8 | grpc | 5000 | 63.347 | 79.713 | 19.422 | 19.419 | 70.078 | 114.429 | 71.449 | 118.109 | 136.020 |
| tls | deep&#45;8 | legacy | 5000 | 31.584 | 43.301 | 10.739 | 10.094 | 35.631 | 55.306 | 37.191 | 59.736 | 73.777 |
| tls | deep&#45;8 | portable | 5000 | 32.187 | 46.751 | 12.442 | 11.121 | 36.728 | 59.811 | 40.832 | 66.454 | 79.271 |
| tls | deep&#45;8 | websocket | 5000 | 27.715 | 44.602 | 11.485 | 9.598 | 36.607 | 55.479 | 42.945 | 66.700 | 76.409 |
| tls | deep&#45;32 | cpp | 5000 | 53.130 | 59.601 | 6.578 | 1.166 | 58.660 | 61.610 | 57.850 | 73.328 | 85.004 |
| tls | deep&#45;32 | grpc | 5000 | 80.119 | 91.577 | 9.873 | 2.259 | 89.491 | 94.882 | 90.035 | 112.269 | 125.996 |
| tls | deep&#45;32 | legacy | 5000 | 38.162 | 43.717 | 5.559 | 0.552 | 43.279 | 44.621 | 42.384 | 55.120 | 65.861 |
| tls | deep&#45;32 | portable | 5000 | 38.069 | 43.543 | 5.651 | 0.939 | 42.400 | 44.606 | 42.140 | 54.847 | 65.327 |
| tls | deep&#45;32 | websocket | 5000 | 46.135 | 56.787 | 7.370 | 0.852 | 55.888 | 58.020 | 55.329 | 70.691 | 82.766 |
| tls | concurrent&#45;32 | cpp | 5000 | 442.002 | 513.690 | 40.519 | 18.154 | 491.437 | 531.701 | 511.581 | 583.736 | 607.481 |
| tls | concurrent&#45;32 | grpc | 5000 | 540.712 | 667.275 | 58.905 | 42.074 | 597.601 | 697.205 | 667.195 | 750.835 | 773.893 |
| tls | concurrent&#45;32 | legacy | 5000 | 105.636 | 126.589 | 25.157 | 11.408 | 117.128 | 143.345 | 119.471 | 190.573 | 216.425 |
| tls | concurrent&#45;32 | portable | 5000 | 129.008 | 145.369 | 13.697 | 2.065 | 142.720 | 147.244 | 142.314 | 170.634 | 188.980 |
| tls | concurrent&#45;32 | websocket | 5000 | 100.059 | 174.693 | 40.431 | 21.014 | 150.163 | 198.721 | 167.240 | 245.412 | 272.212 |
| tls | deep&#45;32&#45;concurrent&#45;32 | cpp | 5000 | 585.194 | 701.578 | 100.808 | 84.239 | 648.787 | 848.414 | 672.677 | 924.761 | 999.901 |
| tls | deep&#45;32&#45;concurrent&#45;32 | grpc | 5000 | 1357.958 | 1630.278 | 150.226 | 111.655 | 1516.024 | 1776.184 | 1589.691 | 1870.815 | 1932.130 |
| tls | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 389.417 | 455.839 | 43.694 | 26.423 | 431.120 | 487.987 | 451.727 | 535.123 | 566.400 |
| tls | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 391.780 | 449.071 | 37.061 | 21.807 | 434.994 | 487.657 | 446.746 | 523.376 | 555.803 |
| tls | deep&#45;32&#45;concurrent&#45;32 | websocket | 5000 | 649.895 | 788.116 | 122.679 | 76.438 | 737.545 | 913.856 | 750.765 | 1064.176 | 1114.259 |
| tls | services&#45;4&#45;deep&#45;16 | cpp | 5000 | 338.576 | 391.175 | 26.432 | 2.593 | 388.354 | 394.864 | 390.554 | 430.364 | 459.982 |
| tls | services&#45;4&#45;deep&#45;16 | grpc | 5000 | 470.793 | 550.227 | 33.708 | 9.866 | 537.584 | 561.182 | 551.046 | 604.075 | 634.289 |
| tls | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 243.402 | 282.136 | 25.264 | 4.555 | 275.609 | 287.885 | 281.194 | 316.735 | 343.039 |
| tls | services&#45;4&#45;deep&#45;16 | portable | 5000 | 268.508 | 316.110 | 27.482 | 7.997 | 307.105 | 326.221 | 315.700 | 354.495 | 385.980 |
| tls | services&#45;4&#45;deep&#45;16 | websocket | 5000 | 201.806 | 278.769 | 39.217 | 1.576 | 277.028 | 280.570 | 275.054 | 344.605 | 368.816 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | 5000 | 2436.071 | 2707.625 | 102.275 | 13.324 | 2690.315 | 2724.859 | 2700.003 | 2879.850 | 2980.584 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | 5000 | 4276.095 | 4641.970 | 161.089 | 17.616 | 4625.348 | 4662.505 | 4627.240 | 4922.064 | 5079.679 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1439.681 | 1586.970 | 60.779 | 3.922 | 1580.454 | 1590.762 | 1587.374 | 1680.980 | 1726.805 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1516.403 | 1667.907 | 66.754 | 8.231 | 1656.483 | 1677.328 | 1671.255 | 1764.730 | 1825.691 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | 5000 | 1287.299 | 1568.450 | 95.089 | 22.587 | 1533.183 | 1591.307 | 1563.605 | 1736.418 | 1808.875 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 2092.041 | 2559.674 | 135.734 | 30.941 | 2507.466 | 2585.208 | 2571.698 | 2767.966 | 2855.767 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 3612.514 | 4356.580 | 211.042 | 45.579 | 4294.986 | 4419.174 | 4342.613 | 4719.355 | 4873.143 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1180.012 | 1626.485 | 73.714 | 7.229 | 1618.330 | 1636.320 | 1626.391 | 1737.586 | 1792.160 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1327.346 | 1695.224 | 119.813 | 28.549 | 1656.200 | 1728.426 | 1689.410 | 1865.430 | 1923.810 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1177.468 | 1558.019 | 143.743 | 55.857 | 1487.871 | 1621.078 | 1549.358 | 1800.416 | 1886.667 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 4081.867 | 5424.738 | 253.495 | 60.667 | 5352.302 | 5492.761 | 5405.567 | 5859.856 | 6053.936 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 6970.572 | 8750.531 | 510.892 | 112.311 | 8565.971 | 8843.400 | 8791.478 | 9509.429 | 9809.718 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2701.870 | 3325.224 | 109.818 | 25.910 | 3290.783 | 3346.644 | 3317.831 | 3511.694 | 3622.695 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2754.938 | 3450.051 | 192.836 | 34.165 | 3426.995 | 3510.424 | 3464.163 | 3752.896 | 3863.020 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 2228.507 | 2833.253 | 217.954 | 19.790 | 2803.713 | 2859.020 | 2805.305 | 3239.079 | 3433.208 |
| quic | loopback&#45;0 | legacy | 5000 | 39.280 | 47.171 | 10.412 | 6.974 | 43.603 | 59.635 | 43.371 | 70.958 | 82.078 |
| quic | loopback&#45;0 | portable | 5000 | 42.809 | 49.148 | 6.513 | 0.989 | 47.843 | 50.368 | 47.841 | 61.389 | 73.172 |
| quic | loopback&#45;64 | legacy | 5000 | 39.530 | 55.735 | 15.283 | 13.593 | 43.846 | 71.088 | 48.325 | 77.431 | 91.335 |
| quic | loopback&#45;64 | portable | 5000 | 43.145 | 67.151 | 17.725 | 17.512 | 47.767 | 80.888 | 73.437 | 91.400 | 105.036 |
| quic | loopback&#45;1024 | legacy | 5000 | 41.620 | 52.650 | 13.211 | 12.307 | 46.695 | 74.651 | 47.421 | 77.190 | 89.878 |
| quic | loopback&#45;1024 | portable | 5000 | 45.164 | 57.106 | 13.737 | 12.060 | 49.922 | 78.587 | 51.689 | 85.448 | 97.912 |
| quic | loopback&#45;65536 | legacy | 5000 | 181.919 | 201.052 | 14.133 | 1.397 | 198.961 | 202.213 | 199.044 | 227.230 | 242.632 |
| quic | loopback&#45;65536 | portable | 5000 | 185.342 | 222.383 | 43.188 | 37.004 | 205.303 | 288.574 | 207.454 | 327.294 | 344.866 |
| quic | unary&#45;empty | legacy | 5000 | 39.208 | 44.881 | 6.576 | 2.074 | 43.127 | 48.427 | 43.449 | 58.102 | 72.685 |
| quic | unary&#45;empty | portable | 5000 | 42.755 | 59.373 | 15.840 | 15.300 | 47.569 | 76.161 | 49.834 | 84.063 | 97.722 |
| quic | unary&#45;small | legacy | 5000 | 39.183 | 58.958 | 14.095 | 13.653 | 43.975 | 70.572 | 64.379 | 78.919 | 89.558 |
| quic | unary&#45;small | portable | 5000 | 42.781 | 57.359 | 15.355 | 12.620 | 48.250 | 76.143 | 49.545 | 81.626 | 96.785 |
| quic | unary&#45;1k | legacy | 5000 | 41.417 | 52.193 | 12.113 | 11.709 | 46.081 | 73.116 | 47.296 | 75.934 | 87.193 |
| quic | unary&#45;1k | portable | 5000 | 45.226 | 55.558 | 13.003 | 9.207 | 50.439 | 71.989 | 51.274 | 84.671 | 96.866 |
| quic | fanout&#45;32 | legacy | 5000 | 185.379 | 218.055 | 40.927 | 17.169 | 203.741 | 243.467 | 207.654 | 333.570 | 360.286 |
| quic | fanout&#45;32 | portable | 5000 | 213.249 | 275.483 | 70.404 | 69.338 | 236.014 | 397.396 | 242.581 | 417.954 | 436.677 |
| quic | dependent&#45;read | legacy | 5000 | 86.712 | 98.391 | 10.773 | 1.708 | 97.074 | 101.359 | 96.556 | 116.642 | 133.964 |
| quic | dependent&#45;read | portable | 5000 | 114.247 | 127.774 | 11.956 | 1.325 | 126.029 | 129.068 | 125.606 | 150.988 | 165.270 |
| quic | pipelined&#45;read | legacy | 5000 | 52.007 | 58.350 | 7.758 | 0.573 | 57.467 | 58.988 | 56.453 | 70.916 | 84.675 |
| quic | pipelined&#45;read | portable | 5000 | 80.582 | 92.674 | 15.892 | 4.938 | 90.072 | 101.484 | 88.692 | 121.543 | 160.564 |
| quic | write&#45;read | legacy | 5000 | 80.737 | 91.546 | 9.916 | 0.695 | 90.787 | 92.681 | 89.650 | 110.033 | 123.301 |
| quic | write&#45;read | portable | 5000 | 88.102 | 100.280 | 9.454 | 1.700 | 97.909 | 102.509 | 98.642 | 118.413 | 133.490 |
| quic | callback | legacy | 5000 | 85.281 | 110.562 | 27.002 | 23.791 | 95.476 | 150.331 | 98.040 | 159.345 | 177.734 |
| quic | callback | portable | 5000 | 108.815 | 140.896 | 34.505 | 31.696 | 120.638 | 194.776 | 124.343 | 205.574 | 221.051 |
| quic | bulk&#45;64k | legacy | 5000 | 182.961 | 203.069 | 14.925 | 1.277 | 201.514 | 204.619 | 201.878 | 229.715 | 247.754 |
| quic | bulk&#45;64k | portable | 5000 | 185.013 | 227.579 | 49.037 | 50.712 | 203.626 | 318.270 | 207.323 | 334.234 | 351.613 |
| quic | bulk&#45;256k | legacy | 5000 | 460.067 | 536.576 | 563.842 | 66.452 | 499.707 | 653.274 | 505.529 | 745.250 | 784.824 |
| quic | bulk&#45;256k | portable | 5000 | 475.141 | 660.087 | 788.719 | 106.608 | 519.501 | 819.875 | 550.521 | 828.542 | 856.318 |
| quic | bulk&#45;1m | legacy | 5000 | 1704.694 | 2103.169 | 766.504 | 203.696 | 1874.518 | 2338.960 | 1883.949 | 2644.220 | 2712.514 |
| quic | bulk&#45;1m | portable | 5000 | 1696.237 | 1977.865 | 626.076 | 118.685 | 1886.894 | 2120.059 | 1853.255 | 2553.348 | 2626.086 |
| quic | bulk&#45;4m | legacy | 5000 | 8604.027 | 10158.679 | 1412.336 | 1076.527 | 9264.996 | 11977.473 | 9472.946 | 12830.927 | 13199.150 |
| quic | bulk&#45;4m | portable | 5000 | 8688.427 | 10621.904 | 1488.095 | 991.817 | 9689.476 | 12126.137 | 9831.596 | 13157.966 | 13519.229 |
| quic | deep&#45;8 | legacy | 5000 | 42.395 | 60.591 | 15.758 | 12.867 | 47.594 | 75.102 | 55.348 | 85.144 | 99.439 |
| quic | deep&#45;8 | portable | 5000 | 45.643 | 63.571 | 18.071 | 16.003 | 51.259 | 83.777 | 53.290 | 92.615 | 109.390 |
| quic | deep&#45;32 | legacy | 5000 | 49.221 | 62.830 | 15.179 | 15.003 | 55.505 | 89.660 | 56.349 | 93.396 | 105.520 |
| quic | deep&#45;32 | portable | 5000 | 53.251 | 68.671 | 17.838 | 18.064 | 60.040 | 100.972 | 60.834 | 103.824 | 116.955 |
| quic | concurrent&#45;32 | legacy | 5000 | 121.010 | 139.217 | 21.077 | 5.893 | 135.835 | 149.674 | 134.193 | 172.654 | 237.327 |
| quic | concurrent&#45;32 | portable | 5000 | 147.676 | 195.916 | 55.212 | 53.057 | 163.445 | 286.905 | 168.527 | 304.661 | 321.769 |
| quic | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 426.475 | 522.373 | 38.566 | 27.507 | 473.183 | 535.614 | 520.833 | 579.868 | 602.213 |
| quic | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 494.485 | 543.713 | 30.129 | 2.446 | 540.094 | 546.215 | 540.419 | 592.287 | 612.944 |
| quic | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 374.828 | 425.340 | 31.358 | 2.089 | 422.584 | 427.550 | 425.233 | 467.884 | 492.324 |
| quic | services&#45;4&#45;deep&#45;16 | portable | 5000 | 359.822 | 405.058 | 25.585 | 3.226 | 400.325 | 408.283 | 403.159 | 446.592 | 475.908 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1671.240 | 1895.519 | 88.356 | 36.613 | 1844.452 | 1943.885 | 1892.165 | 2048.282 | 2085.354 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1678.269 | 1900.690 | 91.623 | 40.678 | 1863.183 | 1947.688 | 1893.445 | 2059.172 | 2109.848 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1659.522 | 2025.799 | 99.182 | 13.643 | 2013.784 | 2048.224 | 2027.254 | 2183.756 | 2255.046 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1509.016 | 1996.840 | 150.948 | 20.284 | 1966.714 | 2021.076 | 2017.990 | 2203.124 | 2256.633 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 3411.165 | 4029.255 | 189.841 | 30.829 | 4002.375 | 4081.299 | 4009.794 | 4373.958 | 4522.363 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2934.199 | 3896.057 | 305.502 | 65.736 | 3821.797 | 3974.555 | 3934.038 | 4372.147 | 4536.863 |

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


