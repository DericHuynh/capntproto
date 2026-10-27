# Capntproto verification reports

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-27T00:04:04Z · run 40423956285 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40423956285) · commit `c521184eb741` · **failure**

![Cargo test results](docs/reports/cargo-history.svg)

| Total | Passed | Failed | Errors | Skipped |
| ---: | ---: | ---: | ---: | ---: |
| 2530 | 2510 | [0](docs/reports/failed-tests.md) | 0 | 20 |

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-27T00:04:04Z · run 40423956285 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40423956285) · commit `c521184eb741` · **failure**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

![TLA&#43; checks and expected counterexamples](docs/reports/tla-outcomes.svg)

Unique module&#47;configuration&#47;expected&#45;exit checks&#46; Expected invariant violations are successful controls&#46;

![TLA&#43; explored states by model](docs/reports/tla-states.svg)

Counts sum independent bounded configurations&#59; they are not globally distinct states or a proof beyond those bounds&#46;

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-27T00:04:04Z · run 40423956285 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40423956285) · commit `c521184eb741` · **failure**

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

Latest benchmark run: [2026-10-27T00:04:04Z · run 40423956309 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40423956309) · commit `c521184eb741` · **success**

Measurement evidence: **passed**. Independently validated encrypted benchmark values&#46; Workflow status and diagnostic completion are separate&#59; unavailable values remain blank&#46; Operations include whole concurrent batches and every service hop&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Current TCP fixtures use TLS 1.3, including upstream Rust Cap’n Proto, C++, gRPC over HTTP/2 and WebSockets over WSS; native encrypted transports and standard QUIC are identified separately. Historical data retains its recorded fixture contract. The dedicated campaign also reports recursive data, concurrent calls and independent service chains for gRPC and binary Protobuf WebSockets in its encrypted comparison artifact. Bars compare the recorded workload, rather than universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.


## Exhaustive benchmark tables

Measured run: [2026-10-27T00:04:04Z · run 40423956309 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40423956309) · commit `c521184eb741` · **success**

All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.

### All encrypted latency comparisons

| Transport | Case | Implementation | Samples | Minimum µs | Mean µs | Sample SD µs | Run mean SD µs | Run mean min µs | Run mean max µs | p50 µs | p95 µs | p99 µs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tls | loopback&#45;0 | cpp | 5000 | 43.238 | 48.760 | 5.903 | 0.860 | 47.482 | 49.839 | 46.956 | 60.341 | 74.119 |
| tls | loopback&#45;0 | grpc | 5000 | 50.573 | 56.688 | 6.234 | 0.478 | 56.131 | 57.179 | 54.809 | 69.055 | 82.298 |
| tls | loopback&#45;0 | legacy | 5000 | 25.486 | 33.084 | 7.270 | 0.396 | 32.451 | 33.497 | 31.783 | 40.038 | 51.779 |
| tls | loopback&#45;0 | portable | 5000 | 29.325 | 34.706 | 7.226 | 3.563 | 32.464 | 40.978 | 32.147 | 48.133 | 57.228 |
| tls | loopback&#45;0 | websocket | 5000 | 22.208 | 28.575 | 4.117 | 0.731 | 27.611 | 29.556 | 27.739 | 33.735 | 43.441 |
| tls | loopback&#45;64 | cpp | 5000 | 41.769 | 57.711 | 13.555 | 11.933 | 48.422 | 72.110 | 49.937 | 76.676 | 90.150 |
| tls | loopback&#45;64 | grpc | 5000 | 51.222 | 67.302 | 15.550 | 14.579 | 55.910 | 86.201 | 58.189 | 90.147 | 103.279 |
| tls | loopback&#45;64 | legacy | 5000 | 28.449 | 41.605 | 11.285 | 7.292 | 31.624 | 47.287 | 44.703 | 53.528 | 68.761 |
| tls | loopback&#45;64 | portable | 5000 | 29.536 | 45.542 | 9.070 | 6.551 | 34.091 | 50.352 | 46.265 | 57.242 | 70.840 |
| tls | loopback&#45;64 | websocket | 5000 | 23.936 | 31.983 | 6.364 | 5.629 | 27.595 | 38.692 | 28.546 | 40.570 | 48.456 |
| tls | loopback&#45;1024 | cpp | 5000 | 46.236 | 59.467 | 16.475 | 10.331 | 50.119 | 76.599 | 50.618 | 82.626 | 98.870 |
| tls | loopback&#45;1024 | grpc | 5000 | 52.901 | 72.879 | 17.413 | 12.691 | 62.859 | 94.009 | 63.008 | 101.209 | 116.082 |
| tls | loopback&#45;1024 | legacy | 5000 | 31.334 | 38.673 | 7.890 | 6.080 | 33.875 | 46.869 | 34.477 | 50.404 | 62.613 |
| tls | loopback&#45;1024 | portable | 5000 | 31.758 | 36.795 | 15.687 | 3.077 | 34.806 | 42.155 | 33.877 | 50.238 | 61.474 |
| tls | loopback&#45;1024 | websocket | 5000 | 23.200 | 33.241 | 7.759 | 4.126 | 28.542 | 37.042 | 32.776 | 41.421 | 53.949 |
| tls | loopback&#45;65536 | cpp | 5000 | 168.998 | 204.959 | 38.989 | 28.266 | 185.716 | 253.477 | 187.976 | 288.972 | 304.962 |
| tls | loopback&#45;65536 | grpc | 5000 | 168.166 | 206.083 | 42.201 | 37.138 | 186.547 | 272.420 | 187.430 | 303.333 | 328.494 |
| tls | loopback&#45;65536 | legacy | 5000 | 103.908 | 123.463 | 21.516 | 21.757 | 111.940 | 162.337 | 112.335 | 167.533 | 181.336 |
| tls | loopback&#45;65536 | portable | 5000 | 106.748 | 127.429 | 26.411 | 17.564 | 115.455 | 157.303 | 114.975 | 184.318 | 204.187 |
| tls | loopback&#45;65536 | websocket | 5000 | 118.338 | 128.150 | 8.646 | 1.175 | 127.193 | 129.997 | 125.187 | 144.859 | 159.079 |
| tls | unary&#45;empty | cpp | 5000 | 44.154 | 48.813 | 5.838 | 0.711 | 47.957 | 49.738 | 46.949 | 60.334 | 70.037 |
| tls | unary&#45;empty | grpc | 5000 | 50.894 | 56.748 | 6.061 | 1.501 | 55.156 | 58.728 | 54.933 | 69.109 | 79.875 |
| tls | unary&#45;empty | legacy | 5000 | 27.654 | 32.399 | 4.491 | 0.443 | 31.851 | 32.928 | 31.314 | 39.207 | 49.403 |
| tls | unary&#45;empty | portable | 5000 | 28.211 | 32.935 | 4.470 | 0.517 | 32.405 | 33.628 | 31.846 | 39.501 | 52.588 |
| tls | unary&#45;empty | websocket | 5000 | 21.421 | 28.552 | 3.879 | 0.355 | 28.188 | 29.117 | 27.738 | 33.741 | 43.861 |
| tls | unary&#45;small | cpp | 5000 | 43.908 | 48.135 | 5.798 | 0.582 | 47.318 | 48.786 | 46.332 | 59.392 | 69.566 |
| tls | unary&#45;small | grpc | 5000 | 51.584 | 57.142 | 7.724 | 0.859 | 55.846 | 57.979 | 55.011 | 69.481 | 86.410 |
| tls | unary&#45;small | legacy | 5000 | 26.449 | 33.143 | 4.741 | 0.490 | 32.737 | 33.958 | 31.822 | 40.962 | 53.118 |
| tls | unary&#45;small | portable | 5000 | 26.206 | 32.869 | 4.772 | 0.294 | 32.514 | 33.333 | 31.663 | 40.175 | 52.177 |
| tls | unary&#45;small | websocket | 5000 | 22.050 | 28.021 | 3.613 | 0.521 | 27.577 | 28.687 | 27.221 | 34.117 | 44.105 |
| tls | unary&#45;1k | cpp | 5000 | 46.426 | 61.012 | 14.105 | 13.642 | 50.687 | 76.497 | 53.130 | 80.070 | 96.069 |
| tls | unary&#45;1k | grpc | 5000 | 55.981 | 73.868 | 19.983 | 9.449 | 63.570 | 82.392 | 64.780 | 100.650 | 126.711 |
| tls | unary&#45;1k | legacy | 5000 | 31.170 | 36.070 | 10.968 | 4.193 | 33.601 | 43.509 | 33.415 | 47.954 | 65.047 |
| tls | unary&#45;1k | portable | 5000 | 31.653 | 35.257 | 4.989 | 0.476 | 34.586 | 35.806 | 33.920 | 43.067 | 55.269 |
| tls | unary&#45;1k | websocket | 5000 | 22.932 | 28.992 | 3.998 | 1.148 | 27.722 | 30.860 | 28.248 | 35.593 | 45.177 |
| tls | fanout&#45;32 | cpp | 5000 | 369.449 | 414.553 | 53.834 | 18.580 | 395.223 | 442.493 | 398.744 | 499.756 | 609.764 |
| tls | fanout&#45;32 | grpc | 5000 | 626.784 | 693.899 | 64.757 | 10.857 | 679.054 | 707.723 | 679.721 | 763.332 | 1035.463 |
| tls | fanout&#45;32 | legacy | 5000 | 133.045 | 157.884 | 29.841 | 19.877 | 143.063 | 189.115 | 145.022 | 224.275 | 240.493 |
| tls | fanout&#45;32 | portable | 5000 | 160.010 | 186.547 | 34.236 | 23.682 | 174.600 | 228.865 | 173.744 | 274.648 | 295.670 |
| tls | fanout&#45;32 | websocket | 5000 | 176.724 | 217.101 | 32.996 | 13.089 | 205.547 | 234.201 | 206.373 | 292.338 | 325.095 |
| tls | dependent&#45;read | cpp | 5000 | 102.862 | 125.800 | 29.440 | 18.734 | 111.966 | 150.347 | 110.219 | 180.851 | 198.001 |
| tls | dependent&#45;read | grpc | 5000 | 108.428 | 124.879 | 19.592 | 10.367 | 119.097 | 143.366 | 117.970 | 174.538 | 189.090 |
| tls | dependent&#45;read | legacy | 5000 | 65.047 | 81.179 | 16.856 | 14.164 | 69.838 | 102.150 | 71.273 | 108.206 | 122.071 |
| tls | dependent&#45;read | portable | 5000 | 77.073 | 101.132 | 25.733 | 22.113 | 84.502 | 129.563 | 85.930 | 141.254 | 162.009 |
| tls | dependent&#45;read | websocket | 5000 | 45.510 | 56.520 | 7.049 | 1.421 | 54.447 | 58.142 | 55.089 | 68.538 | 84.032 |
| tls | pipelined&#45;read | cpp | 5000 | 73.765 | 106.499 | 27.062 | 18.367 | 79.546 | 119.964 | 122.349 | 141.280 | 152.824 |
| tls | pipelined&#45;read | legacy | 5000 | 40.140 | 57.660 | 8.897 | 8.120 | 43.341 | 63.597 | 59.108 | 70.112 | 81.026 |
| tls | pipelined&#45;read | portable | 5000 | 57.400 | 86.030 | 22.855 | 21.657 | 62.650 | 105.348 | 97.333 | 117.849 | 134.457 |
| tls | write&#45;read | cpp | 5000 | 90.050 | 106.160 | 19.502 | 14.560 | 97.746 | 131.675 | 96.634 | 145.908 | 161.092 |
| tls | write&#45;read | grpc | 5000 | 108.212 | 124.574 | 16.317 | 7.884 | 119.666 | 138.537 | 118.802 | 172.493 | 188.543 |
| tls | write&#45;read | legacy | 5000 | 56.903 | 70.939 | 11.223 | 9.469 | 65.328 | 87.794 | 65.731 | 87.760 | 104.095 |
| tls | write&#45;read | portable | 5000 | 61.392 | 69.053 | 9.633 | 3.403 | 66.907 | 75.096 | 65.617 | 95.231 | 103.724 |
| tls | write&#45;read | websocket | 5000 | 47.445 | 59.064 | 8.260 | 6.576 | 55.915 | 70.825 | 56.174 | 75.913 | 83.145 |
| tls | callback | cpp | 5000 | 95.075 | 103.175 | 10.588 | 0.923 | 102.365 | 104.646 | 100.271 | 119.621 | 132.961 |
| tls | callback | grpc | 5000 | 55.791 | 61.871 | 5.404 | 1.168 | 60.754 | 63.697 | 60.265 | 73.274 | 83.663 |
| tls | callback | legacy | 5000 | 64.700 | 71.170 | 6.728 | 2.272 | 68.503 | 74.329 | 69.093 | 84.144 | 95.069 |
| tls | callback | portable | 5000 | 73.422 | 82.656 | 10.222 | 1.110 | 81.213 | 83.716 | 80.015 | 97.539 | 114.109 |
| tls | callback | websocket | 5000 | 47.217 | 54.255 | 4.740 | 0.599 | 53.738 | 55.007 | 53.352 | 63.021 | 71.896 |
| tls | bulk&#45;64k | cpp | 5000 | 168.509 | 188.964 | 13.681 | 0.507 | 188.249 | 189.568 | 185.824 | 213.294 | 239.038 |
| tls | bulk&#45;64k | grpc | 5000 | 171.374 | 217.019 | 52.920 | 26.344 | 188.525 | 252.949 | 191.895 | 322.992 | 352.720 |
| tls | bulk&#45;64k | legacy | 5000 | 103.704 | 113.615 | 8.231 | 1.356 | 111.876 | 115.263 | 110.970 | 130.114 | 144.457 |
| tls | bulk&#45;64k | portable | 5000 | 106.712 | 117.761 | 10.961 | 1.776 | 115.336 | 119.799 | 114.238 | 136.590 | 154.925 |
| tls | bulk&#45;64k | websocket | 5000 | 119.860 | 131.284 | 13.077 | 2.673 | 128.505 | 135.318 | 127.279 | 149.766 | 192.689 |
| tls | bulk&#45;256k | cpp | 5000 | 671.671 | 750.152 | 72.401 | 27.480 | 731.865 | 796.692 | 728.138 | 966.142 | 1035.567 |
| tls | bulk&#45;256k | grpc | 5000 | 425.262 | 1056.638 | 220.483 | 154.810 | 793.347 | 1176.439 | 1089.200 | 1346.802 | 1547.641 |
| tls | bulk&#45;256k | legacy | 5000 | 295.143 | 345.143 | 55.439 | 38.193 | 314.195 | 397.742 | 319.428 | 450.537 | 469.193 |
| tls | bulk&#45;256k | portable | 5000 | 305.916 | 329.506 | 24.094 | 3.066 | 327.387 | 334.925 | 323.723 | 364.705 | 456.754 |
| tls | bulk&#45;256k | websocket | 5000 | 731.692 | 827.978 | 47.342 | 17.003 | 800.269 | 846.366 | 829.690 | 895.729 | 956.725 |
| tls | bulk&#45;1m | cpp | 5000 | 2651.926 | 2952.425 | 288.646 | 120.330 | 2880.051 | 3163.920 | 2838.349 | 3783.059 | 3885.463 |
| tls | bulk&#45;1m | grpc | 5000 | 1599.646 | 2227.898 | 630.958 | 593.163 | 1732.327 | 2975.281 | 1794.693 | 3374.762 | 3609.135 |
| tls | bulk&#45;1m | legacy | 5000 | 1054.644 | 1243.326 | 154.454 | 98.092 | 1135.751 | 1394.181 | 1154.850 | 1470.608 | 1545.088 |
| tls | bulk&#45;1m | portable | 5000 | 1059.120 | 1170.359 | 120.523 | 32.740 | 1136.696 | 1221.057 | 1134.346 | 1432.056 | 1548.175 |
| tls | bulk&#45;1m | websocket | 5000 | 3309.171 | 3881.319 | 198.906 | 50.033 | 3830.698 | 3950.761 | 3884.527 | 4194.803 | 4406.521 |
| tls | bulk&#45;4m | cpp | 5000 | 12661.179 | 14095.196 | 1330.909 | 220.807 | 13783.599 | 14394.907 | 13689.896 | 17999.819 | 18494.508 |
| tls | bulk&#45;4m | grpc | 5000 | 16853.007 | 22158.378 | 2701.286 | 2830.549 | 18935.288 | 24416.981 | 23163.446 | 25527.505 | 26382.006 |
| tls | bulk&#45;4m | legacy | 5000 | 5229.603 | 6082.583 | 569.098 | 274.463 | 5894.117 | 6561.578 | 5881.052 | 7124.424 | 7407.567 |
| tls | bulk&#45;4m | portable | 5000 | 5264.417 | 6054.953 | 526.364 | 69.795 | 5977.601 | 6157.049 | 5856.998 | 6926.879 | 7192.950 |
| tls | bulk&#45;4m | websocket | 5000 | 22817.558 | 25454.598 | 1635.423 | 448.560 | 25034.920 | 26101.572 | 24742.599 | 28291.171 | 28835.231 |
| tls | deep&#45;8 | cpp | 5000 | 47.243 | 67.603 | 15.339 | 10.595 | 50.406 | 76.353 | 73.758 | 88.227 | 101.688 |
| tls | deep&#45;8 | grpc | 5000 | 60.130 | 84.065 | 19.441 | 13.669 | 70.418 | 102.834 | 74.240 | 111.476 | 128.146 |
| tls | deep&#45;8 | legacy | 5000 | 27.625 | 38.925 | 12.816 | 4.641 | 35.312 | 46.918 | 34.755 | 50.465 | 60.963 |
| tls | deep&#45;8 | portable | 5000 | 32.433 | 39.865 | 10.019 | 8.509 | 35.388 | 55.039 | 35.022 | 57.040 | 73.098 |
| tls | deep&#45;8 | websocket | 5000 | 27.705 | 36.877 | 7.533 | 6.522 | 33.154 | 48.470 | 34.251 | 51.870 | 59.038 |
| tls | deep&#45;32 | cpp | 5000 | 52.317 | 56.664 | 5.737 | 0.767 | 55.829 | 57.624 | 54.758 | 68.394 | 78.203 |
| tls | deep&#45;32 | grpc | 5000 | 78.233 | 87.468 | 9.178 | 1.351 | 86.318 | 89.635 | 85.310 | 101.439 | 114.371 |
| tls | deep&#45;32 | legacy | 5000 | 34.006 | 42.362 | 11.055 | 5.182 | 39.690 | 51.618 | 39.196 | 55.912 | 71.360 |
| tls | deep&#45;32 | portable | 5000 | 35.820 | 46.025 | 9.887 | 9.323 | 40.768 | 62.594 | 41.063 | 62.660 | 73.360 |
| tls | deep&#45;32 | websocket | 5000 | 45.373 | 52.797 | 5.529 | 0.964 | 51.346 | 53.692 | 51.733 | 63.193 | 71.537 |
| tls | concurrent&#45;32 | cpp | 5000 | 409.249 | 469.942 | 78.952 | 15.421 | 451.898 | 489.493 | 449.263 | 540.324 | 890.403 |
| tls | concurrent&#45;32 | grpc | 5000 | 492.185 | 581.819 | 77.575 | 28.031 | 550.082 | 624.833 | 563.921 | 668.491 | 939.687 |
| tls | concurrent&#45;32 | legacy | 5000 | 104.154 | 121.255 | 21.182 | 12.331 | 113.580 | 143.100 | 112.973 | 172.295 | 189.246 |
| tls | concurrent&#45;32 | portable | 5000 | 127.844 | 179.847 | 42.401 | 38.534 | 137.597 | 214.423 | 205.151 | 231.462 | 244.520 |
| tls | concurrent&#45;32 | websocket | 5000 | 92.351 | 146.073 | 27.521 | 23.161 | 133.675 | 187.434 | 138.481 | 199.223 | 220.752 |
| tls | deep&#45;32&#45;concurrent&#45;32 | cpp | 5000 | 541.259 | 660.542 | 111.108 | 73.216 | 599.970 | 760.198 | 614.764 | 875.936 | 953.309 |
| tls | deep&#45;32&#45;concurrent&#45;32 | grpc | 5000 | 1292.671 | 1566.761 | 131.479 | 42.339 | 1506.549 | 1606.725 | 1588.235 | 1777.205 | 1936.618 |
| tls | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 307.834 | 336.016 | 37.301 | 1.995 | 333.710 | 338.967 | 329.030 | 374.381 | 441.058 |
| tls | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 347.457 | 392.720 | 46.077 | 16.990 | 378.184 | 420.799 | 379.621 | 462.112 | 525.705 |
| tls | deep&#45;32&#45;concurrent&#45;32 | websocket | 5000 | 598.273 | 693.606 | 76.036 | 38.177 | 666.272 | 755.166 | 668.410 | 885.289 | 929.090 |
| tls | services&#45;4&#45;deep&#45;16 | cpp | 5000 | 321.218 | 362.315 | 25.627 | 3.935 | 357.139 | 366.903 | 355.420 | 404.712 | 454.130 |
| tls | services&#45;4&#45;deep&#45;16 | grpc | 5000 | 410.473 | 459.613 | 31.825 | 4.216 | 455.832 | 466.002 | 455.020 | 506.070 | 563.855 |
| tls | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 227.384 | 258.711 | 22.950 | 3.370 | 255.192 | 264.267 | 251.479 | 298.630 | 335.189 |
| tls | services&#45;4&#45;deep&#45;16 | portable | 5000 | 242.298 | 273.157 | 24.235 | 4.006 | 267.088 | 276.869 | 265.771 | 314.299 | 369.811 |
| tls | services&#45;4&#45;deep&#45;16 | websocket | 5000 | 186.040 | 232.047 | 26.832 | 2.474 | 228.717 | 235.663 | 229.137 | 276.416 | 309.499 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | 5000 | 2229.370 | 2931.583 | 364.540 | 19.744 | 2900.060 | 2949.858 | 2899.006 | 3591.683 | 3980.056 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | 5000 | 4055.220 | 4986.875 | 392.683 | 59.037 | 4914.025 | 5070.375 | 4969.738 | 5699.954 | 6050.797 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1272.198 | 1454.323 | 227.103 | 95.703 | 1373.384 | 1611.560 | 1364.335 | 1928.762 | 2417.071 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1455.742 | 1783.865 | 292.912 | 74.909 | 1671.206 | 1875.338 | 1670.230 | 2351.250 | 2752.737 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | 5000 | 1119.072 | 1252.994 | 67.695 | 3.420 | 1248.976 | 1257.657 | 1244.331 | 1345.157 | 1530.426 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 1956.686 | 2398.600 | 202.699 | 50.165 | 2339.121 | 2441.573 | 2362.967 | 2854.607 | 2955.972 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 3375.664 | 4270.152 | 413.802 | 37.104 | 4222.267 | 4321.595 | 4204.929 | 5028.715 | 5595.427 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1055.519 | 1455.462 | 173.992 | 53.339 | 1398.361 | 1540.078 | 1400.561 | 1839.161 | 2112.945 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1235.489 | 1506.838 | 77.971 | 9.327 | 1493.928 | 1516.530 | 1501.214 | 1624.717 | 1765.992 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1014.583 | 1220.802 | 103.147 | 16.782 | 1193.300 | 1237.100 | 1206.028 | 1403.424 | 1523.560 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 3824.589 | 4985.963 | 333.003 | 67.354 | 4921.053 | 5081.533 | 4928.017 | 5571.373 | 6064.328 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 6300.830 | 8778.536 | 861.464 | 95.775 | 8673.440 | 8914.484 | 8742.359 | 10265.906 | 11402.235 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2366.462 | 3105.565 | 373.991 | 145.415 | 2923.409 | 3266.270 | 2957.616 | 3892.626 | 4574.740 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2221.461 | 3198.153 | 191.748 | 48.827 | 3163.705 | 3277.765 | 3208.461 | 3494.747 | 3762.308 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1951.347 | 2385.440 | 189.330 | 9.593 | 2372.445 | 2394.994 | 2353.779 | 2745.474 | 2970.445 |
| quic | loopback&#45;0 | legacy | 5000 | 37.537 | 46.034 | 14.909 | 8.618 | 40.548 | 60.652 | 40.478 | 60.489 | 73.388 |
| quic | loopback&#45;0 | portable | 5000 | 40.104 | 52.344 | 13.517 | 9.505 | 44.196 | 65.463 | 44.910 | 69.250 | 84.207 |
| quic | loopback&#45;64 | legacy | 5000 | 37.572 | 55.818 | 12.792 | 9.387 | 39.681 | 62.360 | 59.184 | 68.704 | 79.510 |
| quic | loopback&#45;64 | portable | 5000 | 40.736 | 56.527 | 13.280 | 11.858 | 43.531 | 66.213 | 62.931 | 73.154 | 86.200 |
| quic | loopback&#45;1024 | legacy | 5000 | 37.251 | 48.040 | 10.694 | 7.926 | 42.935 | 61.839 | 42.249 | 64.679 | 78.529 |
| quic | loopback&#45;1024 | portable | 5000 | 42.069 | 56.187 | 13.502 | 13.031 | 45.754 | 71.891 | 48.771 | 75.106 | 94.230 |
| quic | loopback&#45;65536 | legacy | 5000 | 140.008 | 151.858 | 11.000 | 1.551 | 149.930 | 153.503 | 148.321 | 174.551 | 192.222 |
| quic | loopback&#45;65536 | portable | 5000 | 145.463 | 175.549 | 38.143 | 37.967 | 156.407 | 243.398 | 155.736 | 248.508 | 270.474 |
| quic | unary&#45;empty | legacy | 5000 | 34.806 | 44.485 | 9.503 | 8.731 | 39.569 | 60.063 | 39.805 | 59.937 | 72.816 |
| quic | unary&#45;empty | portable | 5000 | 40.256 | 52.350 | 11.757 | 11.759 | 43.134 | 65.823 | 44.660 | 67.704 | 83.471 |
| quic | unary&#45;small | legacy | 5000 | 37.588 | 41.333 | 6.228 | 1.462 | 40.089 | 42.998 | 39.869 | 49.671 | 64.509 |
| quic | unary&#45;small | portable | 5000 | 36.075 | 43.475 | 4.140 | 0.530 | 42.539 | 43.783 | 42.245 | 51.651 | 61.567 |
| quic | unary&#45;1k | legacy | 5000 | 36.573 | 46.686 | 9.763 | 7.803 | 42.412 | 60.593 | 42.418 | 61.637 | 73.053 |
| quic | unary&#45;1k | portable | 5000 | 42.079 | 55.722 | 13.292 | 13.462 | 45.119 | 71.094 | 47.013 | 72.886 | 86.262 |
| quic | fanout&#45;32 | legacy | 5000 | 159.921 | 190.968 | 41.895 | 37.813 | 170.540 | 258.432 | 171.508 | 281.183 | 305.906 |
| quic | fanout&#45;32 | portable | 5000 | 187.946 | 219.854 | 46.881 | 24.783 | 201.635 | 256.943 | 200.243 | 327.221 | 350.636 |
| quic | dependent&#45;read | legacy | 5000 | 82.835 | 104.435 | 22.898 | 18.830 | 90.606 | 134.881 | 91.846 | 144.026 | 163.314 |
| quic | dependent&#45;read | portable | 5000 | 103.886 | 134.015 | 32.512 | 19.240 | 113.629 | 152.041 | 114.809 | 188.201 | 202.288 |
| quic | pipelined&#45;read | legacy | 5000 | 48.705 | 59.747 | 14.153 | 11.348 | 53.209 | 79.575 | 51.997 | 82.063 | 93.903 |
| quic | pipelined&#45;read | portable | 5000 | 74.051 | 90.440 | 20.584 | 21.249 | 80.144 | 128.398 | 79.328 | 129.442 | 144.805 |
| quic | write&#45;read | legacy | 5000 | 76.390 | 95.649 | 21.401 | 10.825 | 82.979 | 109.928 | 84.286 | 130.155 | 142.441 |
| quic | write&#45;read | portable | 5000 | 80.576 | 109.024 | 24.602 | 25.795 | 89.829 | 139.333 | 93.928 | 146.152 | 156.285 |
| quic | callback | legacy | 5000 | 82.675 | 111.792 | 24.595 | 17.273 | 91.148 | 130.070 | 105.687 | 144.840 | 164.910 |
| quic | callback | portable | 5000 | 99.963 | 115.226 | 20.292 | 9.707 | 110.350 | 132.563 | 108.333 | 162.091 | 183.983 |
| quic | bulk&#45;64k | legacy | 5000 | 140.793 | 169.964 | 36.892 | 27.580 | 152.190 | 216.037 | 151.243 | 245.513 | 265.646 |
| quic | bulk&#45;64k | portable | 5000 | 145.934 | 188.302 | 43.444 | 43.021 | 157.454 | 249.673 | 162.061 | 257.481 | 270.596 |
| quic | bulk&#45;256k | legacy | 5000 | 356.054 | 499.596 | 1574.200 | 55.325 | 444.528 | 561.287 | 389.505 | 569.830 | 613.873 |
| quic | bulk&#45;256k | portable | 5000 | 375.170 | 489.039 | 1245.351 | 49.782 | 404.830 | 524.552 | 403.277 | 593.466 | 631.888 |
| quic | bulk&#45;1m | legacy | 5000 | 1351.878 | 1599.241 | 1570.489 | 76.669 | 1500.539 | 1699.523 | 1450.191 | 2025.025 | 2201.732 |
| quic | bulk&#45;1m | portable | 5000 | 1361.454 | 1581.870 | 1154.224 | 104.957 | 1463.841 | 1725.845 | 1449.981 | 2023.695 | 2161.153 |
| quic | bulk&#45;4m | legacy | 5000 | 6535.813 | 7900.968 | 768.530 | 100.107 | 7742.054 | 7990.940 | 7799.176 | 9325.290 | 9787.127 |
| quic | bulk&#45;4m | portable | 5000 | 6758.995 | 8024.739 | 941.633 | 234.938 | 7740.966 | 8306.000 | 7850.677 | 9699.813 | 10140.910 |
| quic | deep&#45;8 | legacy | 5000 | 39.803 | 45.035 | 7.059 | 3.057 | 43.046 | 50.462 | 42.676 | 63.493 | 69.545 |
| quic | deep&#45;8 | portable | 5000 | 43.667 | 54.668 | 20.419 | 11.023 | 48.022 | 73.700 | 47.490 | 73.546 | 87.367 |
| quic | deep&#45;32 | legacy | 5000 | 46.542 | 56.793 | 13.697 | 5.784 | 51.789 | 63.717 | 51.468 | 77.979 | 93.298 |
| quic | deep&#45;32 | portable | 5000 | 50.073 | 71.098 | 18.516 | 13.430 | 56.235 | 81.672 | 78.364 | 91.871 | 104.176 |
| quic | concurrent&#45;32 | legacy | 5000 | 117.130 | 127.874 | 10.768 | 1.155 | 126.046 | 129.145 | 124.787 | 148.231 | 176.174 |
| quic | concurrent&#45;32 | portable | 5000 | 142.069 | 160.164 | 26.429 | 14.371 | 152.295 | 185.787 | 150.907 | 237.319 | 256.024 |
| quic | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 345.189 | 420.537 | 78.882 | 23.759 | 393.101 | 454.936 | 401.177 | 500.493 | 837.029 |
| quic | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 393.933 | 468.760 | 96.923 | 22.182 | 431.744 | 487.631 | 439.694 | 581.552 | 941.379 |
| quic | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 359.082 | 399.711 | 26.065 | 5.247 | 390.807 | 403.601 | 396.523 | 439.772 | 479.640 |
| quic | services&#45;4&#45;deep&#45;16 | portable | 5000 | 324.544 | 367.660 | 26.958 | 7.981 | 354.730 | 375.028 | 362.542 | 408.442 | 460.780 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1493.648 | 1627.324 | 101.363 | 17.017 | 1612.025 | 1654.715 | 1603.617 | 1831.427 | 2009.032 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1617.683 | 1766.799 | 106.397 | 17.181 | 1743.927 | 1786.555 | 1741.656 | 1994.220 | 2202.264 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1427.085 | 1703.126 | 137.250 | 45.326 | 1661.062 | 1771.465 | 1687.184 | 1944.626 | 2192.686 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1429.395 | 1784.631 | 127.587 | 31.366 | 1751.771 | 1834.885 | 1797.124 | 1968.555 | 2094.868 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2943.004 | 3629.011 | 292.763 | 52.516 | 3567.912 | 3708.747 | 3596.051 | 4162.323 | 4703.312 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2764.123 | 3674.795 | 353.739 | 34.497 | 3633.698 | 3705.038 | 3749.341 | 4112.978 | 4249.223 |

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


