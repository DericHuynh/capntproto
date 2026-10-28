# Capntproto verification reports

[![CI](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/ci.yml)
[![Coverage](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml/badge.svg?branch=main)](https://github.com/DericHuynh/capntproto/actions/workflows/quality.yml)

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-27T06:36:41Z · run 40462132362 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40462132362) · commit `49f529f3ac47` · **success**

![Cargo test results](docs/reports/cargo-history.svg)

| Total | Passed | Failed | Errors | Skipped |
| ---: | ---: | ---: | ---: | ---: |
| 2544 | 2524 | [0](docs/reports/failed-tests.md) | 0 | 20 |

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-27T06:36:41Z · run 40462132362 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40462132362) · commit `49f529f3ac47` · **success**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

![TLA&#43; checks and expected counterexamples](docs/reports/tla-outcomes.svg)

Unique module&#47;configuration&#47;expected&#45;exit checks&#46; Expected invariant violations are successful controls&#46;

![TLA&#43; explored states by model](docs/reports/tla-states.svg)

Counts sum independent bounded configurations&#59; they are not globally distinct states or a proof beyond those bounds&#46;

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-27T06:36:41Z · run 40462132362 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40462132362) · commit `49f529f3ac47` · **success**

![Fuzzing executions by engine](docs/reports/fuzz-executions.svg)

Bounded campaigns including seed calibration&#59; execution counts are not comparable performance benchmarks&#46;

![Fuzzer feedback by engine](docs/reports/fuzz-coverage.svg)

Engine&#45;local counters&#44; not LLVM source coverage&#46; Do not compare counts across engines or builds&#46;

![Saved fuzzing findings](docs/reports/fuzz-findings.svg)

Saved crashes&#47;hangs are findings requiring triage&#44; not confirmed unique bugs&#46; Missing results remain unknown&#46;

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

Latest coverage run: [2026-10-28T15:00:10Z · run 40711514398 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40711514398) · commit `d45a9285b0db` · **failure**

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-27T06:36:41Z · run 40462132362 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40462132362) · commit `49f529f3ac47` · **success**

Measurement evidence: **passed**. Independently validated encrypted benchmark values&#46; Workflow status and diagnostic completion are separate&#59; unavailable values remain blank&#46; Operations include whole concurrent batches and every service hop&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Current TCP fixtures use TLS 1.3, including upstream Rust Cap’n Proto, C++, gRPC over HTTP/2 and WebSockets over WSS; native encrypted transports and standard QUIC are identified separately. Historical data retains its recorded fixture contract. The dedicated campaign also reports recursive data, concurrent calls and independent service chains for gRPC and binary Protobuf WebSockets in its encrypted comparison artifact. Bars compare the recorded workload, rather than universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.


## Exhaustive benchmark tables

Measured run: [2026-10-27T06:36:41Z · run 40462132362 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40462132362) · commit `49f529f3ac47` · **success**

All measured cases and implementations are listed. Minimum latency is the primary estimate; means and spread retain all repetitions. Run mean SD is not a confidence interval. Latency is per complete operation or concurrent batch. Profiler counters sum the client and every service hop. Dashes indicate unavailable values.

### All encrypted latency comparisons

| Transport | Case | Implementation | Samples | Minimum µs | Mean µs | Sample SD µs | Run mean SD µs | Run mean min µs | Run mean max µs | p50 µs | p95 µs | p99 µs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tls | loopback&#45;0 | cpp | 5000 | 52.836 | 67.809 | 21.597 | 22.914 | 56.233 | 108.771 | 54.929 | 107.431 | 124.007 |
| tls | loopback&#45;0 | grpc | 5000 | 61.801 | 84.859 | 24.762 | 25.772 | 65.508 | 115.612 | 69.235 | 123.141 | 138.727 |
| tls | loopback&#45;0 | legacy | 5000 | 34.961 | 43.827 | 11.105 | 11.459 | 38.287 | 64.319 | 37.525 | 63.171 | 76.976 |
| tls | loopback&#45;0 | portable | 5000 | 36.843 | 44.834 | 12.153 | 12.486 | 38.868 | 67.162 | 38.067 | 66.132 | 80.202 |
| tls | loopback&#45;0 | websocket | 5000 | 30.085 | 36.566 | 3.682 | 0.528 | 35.813 | 37.149 | 35.785 | 42.130 | 52.399 |
| tls | loopback&#45;64 | cpp | 5000 | 53.176 | 58.822 | 46.482 | 2.153 | 56.803 | 62.281 | 54.853 | 70.981 | 82.002 |
| tls | loopback&#45;64 | grpc | 5000 | 60.758 | 66.850 | 5.665 | 0.575 | 66.223 | 67.399 | 64.020 | 78.886 | 89.389 |
| tls | loopback&#45;64 | legacy | 5000 | 35.308 | 44.429 | 11.458 | 11.183 | 38.257 | 64.302 | 37.573 | 63.874 | 77.106 |
| tls | loopback&#45;64 | portable | 5000 | 36.843 | 39.878 | 4.427 | 0.697 | 39.178 | 40.918 | 38.137 | 48.293 | 58.527 |
| tls | loopback&#45;64 | websocket | 5000 | 28.067 | 34.782 | 4.043 | 0.167 | 34.497 | 34.914 | 33.909 | 42.049 | 50.360 |
| tls | loopback&#45;1024 | cpp | 5000 | 55.274 | 71.062 | 21.811 | 23.255 | 59.539 | 112.631 | 58.151 | 112.444 | 126.396 |
| tls | loopback&#45;1024 | grpc | 5000 | 66.801 | 86.803 | 22.499 | 22.998 | 74.534 | 127.589 | 75.788 | 129.312 | 142.713 |
| tls | loopback&#45;1024 | legacy | 5000 | 34.790 | 41.032 | 3.986 | 0.271 | 40.634 | 41.280 | 39.379 | 48.622 | 58.314 |
| tls | loopback&#45;1024 | portable | 5000 | 39.545 | 42.610 | 4.430 | 0.203 | 42.383 | 42.862 | 40.815 | 50.864 | 60.407 |
| tls | loopback&#45;1024 | websocket | 5000 | 28.630 | 37.224 | 8.415 | 6.359 | 33.792 | 48.573 | 34.079 | 55.400 | 63.052 |
| tls | loopback&#45;65536 | cpp | 5000 | 229.034 | 282.972 | 65.918 | 71.166 | 250.218 | 410.268 | 249.687 | 419.036 | 440.720 |
| tls | loopback&#45;65536 | grpc | 5000 | 229.842 | 260.128 | 18.147 | 2.258 | 256.123 | 261.546 | 255.278 | 296.467 | 317.477 |
| tls | loopback&#45;65536 | legacy | 5000 | 158.242 | 183.200 | 33.717 | 27.898 | 166.665 | 232.438 | 170.182 | 265.072 | 278.059 |
| tls | loopback&#45;65536 | portable | 5000 | 165.966 | 186.511 | 26.980 | 13.348 | 176.554 | 207.942 | 177.904 | 268.905 | 288.307 |
| tls | loopback&#45;65536 | websocket | 5000 | 179.090 | 221.055 | 45.622 | 37.768 | 188.809 | 267.850 | 195.772 | 298.095 | 311.425 |
| tls | unary&#45;empty | cpp | 5000 | 52.830 | 57.488 | 6.566 | 0.583 | 56.771 | 58.218 | 54.668 | 70.157 | 83.543 |
| tls | unary&#45;empty | grpc | 5000 | 61.912 | 67.032 | 6.540 | 0.882 | 65.885 | 68.211 | 64.024 | 79.675 | 94.542 |
| tls | unary&#45;empty | legacy | 5000 | 32.609 | 43.176 | 10.028 | 7.958 | 38.299 | 56.894 | 38.770 | 62.786 | 73.570 |
| tls | unary&#45;empty | portable | 5000 | 37.171 | 40.323 | 4.707 | 0.491 | 39.967 | 41.159 | 38.445 | 49.607 | 60.740 |
| tls | unary&#45;empty | websocket | 5000 | 29.950 | 37.217 | 4.254 | 0.257 | 36.952 | 37.567 | 36.255 | 43.496 | 53.803 |
| tls | unary&#45;small | cpp | 5000 | 53.314 | 68.307 | 21.705 | 23.252 | 57.375 | 109.888 | 55.535 | 109.230 | 124.375 |
| tls | unary&#45;small | grpc | 5000 | 61.127 | 78.046 | 21.302 | 18.441 | 67.647 | 110.348 | 65.928 | 119.521 | 135.200 |
| tls | unary&#45;small | legacy | 5000 | 33.159 | 38.999 | 4.472 | 0.723 | 38.100 | 39.775 | 37.225 | 47.612 | 57.556 |
| tls | unary&#45;small | portable | 5000 | 36.734 | 40.302 | 4.729 | 0.648 | 39.588 | 40.921 | 38.386 | 49.974 | 59.483 |
| tls | unary&#45;small | websocket | 5000 | 29.117 | 38.229 | 7.464 | 6.910 | 34.828 | 50.583 | 35.316 | 52.835 | 60.682 |
| tls | unary&#45;1k | cpp | 5000 | 54.744 | 71.783 | 21.800 | 22.943 | 60.430 | 112.754 | 58.754 | 112.804 | 126.740 |
| tls | unary&#45;1k | grpc | 5000 | 64.427 | 88.468 | 24.672 | 23.036 | 74.304 | 128.705 | 75.455 | 133.006 | 144.505 |
| tls | unary&#45;1k | legacy | 5000 | 36.690 | 52.079 | 14.135 | 14.928 | 40.742 | 69.128 | 43.301 | 72.363 | 84.660 |
| tls | unary&#45;1k | portable | 5000 | 37.522 | 54.556 | 15.566 | 16.441 | 42.232 | 73.205 | 44.505 | 77.888 | 89.695 |
| tls | unary&#45;1k | websocket | 5000 | 26.753 | 42.477 | 11.020 | 10.944 | 34.289 | 54.741 | 36.562 | 61.976 | 70.318 |
| tls | fanout&#45;32 | cpp | 5000 | 428.910 | 572.949 | 197.516 | 74.339 | 488.058 | 667.670 | 468.310 | 1009.347 | 1042.819 |
| tls | fanout&#45;32 | grpc | 5000 | 800.192 | 973.603 | 199.685 | 134.216 | 853.680 | 1123.280 | 856.477 | 1340.960 | 1376.555 |
| tls | fanout&#45;32 | legacy | 5000 | 174.244 | 188.699 | 16.080 | 1.179 | 187.316 | 190.250 | 181.620 | 221.147 | 246.188 |
| tls | fanout&#45;32 | portable | 5000 | 209.757 | 228.652 | 20.110 | 3.098 | 225.905 | 233.862 | 220.406 | 269.100 | 295.967 |
| tls | fanout&#45;32 | websocket | 5000 | 234.414 | 291.965 | 59.084 | 27.687 | 262.938 | 326.190 | 265.889 | 415.770 | 441.280 |
| tls | dependent&#45;read | cpp | 5000 | 122.842 | 154.659 | 47.232 | 42.519 | 132.900 | 230.598 | 131.309 | 265.599 | 277.086 |
| tls | dependent&#45;read | grpc | 5000 | 131.527 | 145.209 | 19.389 | 2.657 | 141.448 | 148.908 | 140.211 | 166.827 | 197.829 |
| tls | dependent&#45;read | legacy | 5000 | 77.543 | 96.095 | 24.011 | 25.754 | 83.364 | 142.144 | 84.178 | 144.610 | 157.104 |
| tls | dependent&#45;read | portable | 5000 | 95.407 | 106.284 | 16.752 | 5.151 | 103.702 | 115.489 | 99.867 | 127.954 | 188.083 |
| tls | dependent&#45;read | websocket | 5000 | 56.397 | 67.942 | 8.769 | 1.598 | 66.583 | 70.635 | 66.330 | 82.103 | 99.845 |
| tls | pipelined&#45;read | cpp | 5000 | 86.770 | 94.994 | 11.880 | 2.220 | 92.958 | 98.262 | 89.663 | 119.663 | 145.774 |
| tls | pipelined&#45;read | legacy | 5000 | 47.364 | 63.417 | 19.361 | 18.425 | 50.088 | 90.243 | 52.190 | 93.177 | 110.870 |
| tls | pipelined&#45;read | portable | 5000 | 73.419 | 79.332 | 7.302 | 0.686 | 78.411 | 80.244 | 76.015 | 94.559 | 110.115 |
| tls | write&#45;read | cpp | 5000 | 108.410 | 172.229 | 52.230 | 50.202 | 117.384 | 219.284 | 212.159 | 234.261 | 248.661 |
| tls | write&#45;read | grpc | 5000 | 129.668 | 161.711 | 41.580 | 27.319 | 140.946 | 197.700 | 140.943 | 251.366 | 268.945 |
| tls | write&#45;read | legacy | 5000 | 71.425 | 89.572 | 21.067 | 22.391 | 79.099 | 129.623 | 79.301 | 130.164 | 144.104 |
| tls | write&#45;read | portable | 5000 | 73.686 | 91.963 | 26.214 | 24.432 | 80.143 | 135.654 | 78.493 | 140.629 | 155.740 |
| tls | write&#45;read | websocket | 5000 | 58.128 | 80.131 | 19.583 | 18.511 | 66.093 | 104.810 | 70.690 | 116.806 | 128.121 |
| tls | callback | cpp | 5000 | 114.655 | 143.465 | 44.633 | 28.283 | 122.111 | 187.244 | 119.978 | 242.779 | 261.806 |
| tls | callback | grpc | 5000 | 69.284 | 74.152 | 5.388 | 0.840 | 73.084 | 75.146 | 71.573 | 85.540 | 94.248 |
| tls | callback | legacy | 5000 | 74.504 | 83.918 | 6.137 | 0.586 | 83.190 | 84.821 | 81.146 | 96.713 | 107.163 |
| tls | callback | portable | 5000 | 92.495 | 130.514 | 37.072 | 39.952 | 98.984 | 174.740 | 108.762 | 186.092 | 198.360 |
| tls | callback | websocket | 5000 | 56.335 | 68.019 | 6.036 | 0.309 | 67.548 | 68.296 | 66.916 | 79.122 | 89.666 |
| tls | bulk&#45;64k | cpp | 5000 | 230.028 | 260.412 | 41.233 | 22.776 | 248.613 | 301.065 | 246.041 | 391.131 | 418.364 |
| tls | bulk&#45;64k | grpc | 5000 | 232.509 | 273.578 | 41.262 | 19.519 | 263.088 | 308.440 | 261.628 | 383.772 | 449.135 |
| tls | bulk&#45;64k | legacy | 5000 | 157.957 | 197.694 | 47.393 | 39.249 | 167.168 | 241.507 | 173.013 | 289.701 | 310.674 |
| tls | bulk&#45;64k | portable | 5000 | 166.491 | 208.375 | 47.147 | 45.328 | 176.192 | 277.291 | 182.155 | 287.043 | 300.327 |
| tls | bulk&#45;64k | websocket | 5000 | 180.377 | 193.765 | 14.324 | 2.688 | 191.902 | 198.414 | 189.420 | 214.521 | 259.221 |
| tls | bulk&#45;256k | cpp | 5000 | 951.011 | 1102.529 | 173.350 | 125.404 | 1017.043 | 1321.064 | 1019.020 | 1471.575 | 1512.268 |
| tls | bulk&#45;256k | grpc | 5000 | 577.791 | 1311.634 | 339.541 | 204.886 | 1091.350 | 1530.750 | 1401.498 | 1837.012 | 1879.732 |
| tls | bulk&#45;256k | legacy | 5000 | 467.495 | 503.330 | 22.035 | 3.330 | 500.954 | 509.158 | 499.840 | 532.147 | 581.684 |
| tls | bulk&#45;256k | portable | 5000 | 476.938 | 518.644 | 38.827 | 12.653 | 510.991 | 541.162 | 510.365 | 563.142 | 706.961 |
| tls | bulk&#45;256k | websocket | 5000 | 1123.663 | 1205.304 | 60.403 | 30.338 | 1156.724 | 1236.284 | 1178.267 | 1301.276 | 1334.298 |
| tls | bulk&#45;1m | cpp | 5000 | 3626.496 | 4113.639 | 476.606 | 140.384 | 3947.043 | 4301.953 | 3951.537 | 5552.843 | 5662.507 |
| tls | bulk&#45;1m | grpc | 5000 | 2525.928 | 4670.356 | 1266.191 | 1319.566 | 3602.201 | 6314.454 | 4062.904 | 6823.189 | 7316.845 |
| tls | bulk&#45;1m | legacy | 5000 | 1602.076 | 1811.920 | 216.545 | 91.153 | 1702.308 | 1926.950 | 1675.887 | 2154.675 | 2201.124 |
| tls | bulk&#45;1m | portable | 5000 | 1609.206 | 1822.844 | 222.277 | 99.109 | 1670.100 | 1942.995 | 1680.781 | 2168.566 | 2216.145 |
| tls | bulk&#45;1m | websocket | 5000 | 4548.509 | 4982.150 | 383.624 | 121.285 | 4873.483 | 5149.834 | 4783.043 | 5629.188 | 5752.137 |
| tls | bulk&#45;4m | cpp | 5000 | 15996.155 | 17130.262 | 1269.941 | 165.889 | 16914.431 | 17307.404 | 16874.892 | 18026.526 | 23950.969 |
| tls | bulk&#45;4m | grpc | 5000 | 10714.710 | 23987.200 | 6943.854 | 7512.625 | 11170.531 | 28787.680 | 26808.741 | 31613.209 | 32280.864 |
| tls | bulk&#45;4m | legacy | 5000 | 7634.584 | 8372.457 | 756.562 | 251.711 | 8220.980 | 8820.116 | 8058.622 | 10045.991 | 10336.494 |
| tls | bulk&#45;4m | portable | 5000 | 7639.588 | 8359.097 | 737.999 | 361.322 | 8049.206 | 8914.367 | 8050.294 | 9963.438 | 10183.246 |
| tls | bulk&#45;4m | websocket | 5000 | 27804.089 | 30359.660 | 2359.559 | 426.972 | 29805.119 | 30813.278 | 29122.150 | 34719.343 | 35195.768 |
| tls | deep&#45;8 | cpp | 5000 | 55.825 | 65.841 | 14.123 | 7.681 | 59.800 | 79.203 | 59.068 | 111.195 | 121.878 |
| tls | deep&#45;8 | grpc | 5000 | 74.759 | 93.324 | 24.450 | 21.593 | 82.887 | 131.928 | 81.748 | 146.461 | 158.964 |
| tls | deep&#45;8 | legacy | 5000 | 38.439 | 51.235 | 14.512 | 13.351 | 41.175 | 71.454 | 42.741 | 72.695 | 86.639 |
| tls | deep&#45;8 | portable | 5000 | 37.512 | 48.812 | 13.056 | 12.955 | 41.984 | 71.937 | 41.833 | 73.731 | 88.030 |
| tls | deep&#45;8 | websocket | 5000 | 33.788 | 53.135 | 13.353 | 12.627 | 43.324 | 67.257 | 49.688 | 77.011 | 86.022 |
| tls | deep&#45;32 | cpp | 5000 | 61.915 | 74.692 | 21.368 | 17.023 | 66.456 | 105.111 | 64.502 | 127.496 | 143.103 |
| tls | deep&#45;32 | grpc | 5000 | 91.476 | 121.590 | 38.485 | 40.687 | 102.272 | 194.306 | 101.108 | 199.045 | 211.506 |
| tls | deep&#45;32 | legacy | 5000 | 44.976 | 50.905 | 7.977 | 4.504 | 47.713 | 58.847 | 46.674 | 67.198 | 79.430 |
| tls | deep&#45;32 | portable | 5000 | 46.499 | 50.336 | 5.152 | 0.659 | 49.313 | 51.103 | 47.918 | 60.466 | 69.346 |
| tls | deep&#45;32 | websocket | 5000 | 51.981 | 68.830 | 18.907 | 15.751 | 59.927 | 96.800 | 62.160 | 115.229 | 127.704 |
| tls | concurrent&#45;32 | cpp | 5000 | 470.408 | 765.314 | 297.725 | 136.091 | 596.342 | 888.996 | 528.031 | 1180.652 | 1203.224 |
| tls | concurrent&#45;32 | grpc | 5000 | 591.456 | 765.037 | 174.781 | 74.617 | 674.257 | 842.696 | 661.138 | 1046.169 | 1072.179 |
| tls | concurrent&#45;32 | legacy | 5000 | 121.095 | 133.656 | 16.323 | 5.326 | 129.323 | 142.625 | 126.994 | 167.659 | 199.114 |
| tls | concurrent&#45;32 | portable | 5000 | 152.428 | 166.915 | 16.507 | 2.545 | 164.285 | 170.651 | 160.413 | 201.118 | 229.003 |
| tls | concurrent&#45;32 | websocket | 5000 | 109.182 | 180.905 | 36.653 | 20.269 | 168.623 | 216.959 | 172.082 | 269.722 | 302.907 |
| tls | deep&#45;32&#45;concurrent&#45;32 | cpp | 5000 | 630.458 | 958.540 | 316.087 | 196.293 | 709.812 | 1175.832 | 756.076 | 1378.901 | 1410.343 |
| tls | deep&#45;32&#45;concurrent&#45;32 | grpc | 5000 | 1488.328 | 1939.385 | 437.266 | 179.322 | 1685.082 | 2103.316 | 1746.262 | 2751.958 | 2822.143 |
| tls | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 394.986 | 583.539 | 185.600 | 48.948 | 547.493 | 661.694 | 492.795 | 863.831 | 887.124 |
| tls | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 457.321 | 656.343 | 171.162 | 93.479 | 532.276 | 779.831 | 646.554 | 864.818 | 885.920 |
| tls | deep&#45;32&#45;concurrent&#45;32 | websocket | 5000 | 675.844 | 949.800 | 233.140 | 101.191 | 842.345 | 1106.889 | 844.858 | 1259.350 | 1300.312 |
| tls | services&#45;4&#45;deep&#45;16 | cpp | 5000 | 458.751 | 497.637 | 25.315 | 4.165 | 492.128 | 502.802 | 495.633 | 529.555 | 551.957 |
| tls | services&#45;4&#45;deep&#45;16 | grpc | 5000 | 582.471 | 633.877 | 25.747 | 7.651 | 622.604 | 642.649 | 631.644 | 673.244 | 707.581 |
| tls | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 334.487 | 363.831 | 18.122 | 1.312 | 361.900 | 365.451 | 361.109 | 394.445 | 416.298 |
| tls | services&#45;4&#45;deep&#45;16 | portable | 5000 | 365.167 | 393.550 | 18.598 | 2.747 | 388.941 | 396.051 | 390.453 | 426.518 | 450.970 |
| tls | services&#45;4&#45;deep&#45;16 | websocket | 5000 | 262.861 | 334.543 | 36.861 | 3.360 | 329.744 | 339.123 | 332.379 | 396.789 | 417.218 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | cpp | 5000 | 3048.692 | 3929.689 | 273.207 | 40.540 | 3875.811 | 3969.984 | 3940.691 | 4349.800 | 4579.721 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | grpc | 5000 | 4818.706 | 6013.890 | 356.450 | 52.937 | 5968.481 | 6076.414 | 6000.475 | 6631.062 | 6900.903 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 1803.776 | 2263.754 | 152.664 | 22.874 | 2223.914 | 2282.559 | 2270.832 | 2503.461 | 2557.188 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 1888.460 | 2356.814 | 144.651 | 29.827 | 2315.867 | 2387.012 | 2368.721 | 2577.929 | 2641.391 |
| tls | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | websocket | 5000 | 1679.385 | 1965.474 | 76.321 | 7.626 | 1956.673 | 1974.105 | 1966.119 | 2091.158 | 2163.188 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 2689.004 | 3629.930 | 428.073 | 56.043 | 3559.203 | 3699.882 | 3654.548 | 4253.717 | 4436.751 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 4461.809 | 5728.832 | 499.332 | 67.489 | 5642.208 | 5793.829 | 5757.827 | 6479.786 | 6720.358 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 1503.965 | 2279.372 | 158.377 | 26.379 | 2239.150 | 2308.999 | 2281.528 | 2526.228 | 2599.670 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 1844.897 | 2365.210 | 130.108 | 27.167 | 2322.270 | 2388.373 | 2382.082 | 2538.596 | 2588.564 |
| tls | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 1469.907 | 1752.399 | 122.960 | 12.763 | 1734.231 | 1768.724 | 1743.300 | 1952.526 | 2058.671 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | cpp | 5000 | 4998.917 | 7111.741 | 878.424 | 26.559 | 7082.345 | 7147.247 | 7254.494 | 8324.380 | 8602.116 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | grpc | 5000 | 8277.619 | 11006.057 | 1187.798 | 74.244 | 10915.890 | 11092.169 | 11116.470 | 12722.084 | 13140.157 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2936.162 | 4626.541 | 250.968 | 62.589 | 4556.566 | 4685.201 | 4620.279 | 5019.696 | 5247.270 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3216.352 | 4718.614 | 219.094 | 20.062 | 4695.770 | 4743.676 | 4749.903 | 5019.467 | 5151.503 |
| tls | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | websocket | 5000 | 2713.924 | 3310.003 | 236.322 | 11.890 | 3293.494 | 3321.464 | 3274.056 | 3752.732 | 3995.888 |
| quic | loopback&#45;0 | legacy | 5000 | 48.516 | 62.466 | 17.453 | 16.667 | 51.471 | 89.422 | 52.223 | 91.789 | 104.041 |
| quic | loopback&#45;0 | portable | 5000 | 53.648 | 74.668 | 21.959 | 23.321 | 56.873 | 100.978 | 60.817 | 108.033 | 119.770 |
| quic | loopback&#45;64 | legacy | 5000 | 48.664 | 66.762 | 19.308 | 20.664 | 51.537 | 89.775 | 53.172 | 95.699 | 106.752 |
| quic | loopback&#45;64 | portable | 5000 | 52.200 | 75.247 | 22.011 | 23.533 | 57.411 | 101.168 | 60.889 | 108.274 | 119.725 |
| quic | loopback&#45;1024 | legacy | 5000 | 50.571 | 55.250 | 8.086 | 2.486 | 53.417 | 59.549 | 52.254 | 68.143 | 91.272 |
| quic | loopback&#45;1024 | portable | 5000 | 55.902 | 70.121 | 19.194 | 19.976 | 59.930 | 105.805 | 60.497 | 105.190 | 120.317 |
| quic | loopback&#45;65536 | legacy | 5000 | 206.492 | 238.179 | 44.352 | 33.120 | 222.094 | 297.378 | 222.324 | 358.017 | 371.765 |
| quic | loopback&#45;65536 | portable | 5000 | 215.405 | 233.591 | 14.002 | 3.768 | 230.820 | 240.055 | 231.036 | 258.953 | 276.903 |
| quic | unary&#45;empty | legacy | 5000 | 45.866 | 52.843 | 8.749 | 2.566 | 50.972 | 57.298 | 50.056 | 65.787 | 90.217 |
| quic | unary&#45;empty | portable | 5000 | 53.814 | 66.436 | 18.656 | 19.539 | 57.180 | 101.369 | 56.477 | 101.410 | 116.790 |
| quic | unary&#45;small | legacy | 5000 | 48.542 | 81.558 | 15.998 | 16.541 | 52.010 | 90.225 | 86.351 | 99.837 | 109.020 |
| quic | unary&#45;small | portable | 5000 | 53.649 | 97.780 | 15.245 | 8.658 | 82.294 | 101.874 | 97.923 | 117.281 | 132.512 |
| quic | unary&#45;1k | legacy | 5000 | 50.093 | 73.811 | 20.921 | 20.184 | 52.588 | 94.048 | 88.729 | 103.891 | 114.628 |
| quic | unary&#45;1k | portable | 5000 | 55.552 | 69.923 | 19.207 | 19.336 | 59.684 | 104.306 | 59.580 | 105.135 | 118.723 |
| quic | fanout&#45;32 | legacy | 5000 | 208.221 | 241.406 | 51.579 | 21.459 | 226.481 | 277.960 | 222.198 | 399.249 | 436.896 |
| quic | fanout&#45;32 | portable | 5000 | 247.937 | 267.006 | 18.206 | 2.346 | 264.373 | 270.153 | 260.790 | 304.996 | 330.599 |
| quic | dependent&#45;read | legacy | 5000 | 104.929 | 113.996 | 9.448 | 1.497 | 112.190 | 115.880 | 110.067 | 130.881 | 142.928 |
| quic | dependent&#45;read | portable | 5000 | 138.242 | 161.442 | 36.916 | 28.174 | 148.166 | 211.830 | 146.004 | 254.741 | 277.421 |
| quic | pipelined&#45;read | legacy | 5000 | 61.996 | 72.616 | 32.093 | 11.315 | 66.325 | 92.539 | 64.327 | 116.192 | 136.643 |
| quic | pipelined&#45;read | portable | 5000 | 98.371 | 123.293 | 37.906 | 35.266 | 106.032 | 186.345 | 106.594 | 189.665 | 208.860 |
| quic | write&#45;read | legacy | 5000 | 98.602 | 105.400 | 7.997 | 0.595 | 104.802 | 106.239 | 102.027 | 120.624 | 135.857 |
| quic | write&#45;read | portable | 5000 | 107.993 | 118.270 | 10.312 | 0.564 | 117.515 | 118.834 | 114.023 | 137.841 | 154.297 |
| quic | callback | legacy | 5000 | 103.075 | 111.531 | 9.074 | 1.416 | 110.569 | 114.031 | 107.934 | 129.466 | 143.750 |
| quic | callback | portable | 5000 | 131.923 | 143.020 | 11.658 | 1.112 | 142.241 | 144.931 | 138.298 | 164.674 | 180.102 |
| quic | bulk&#45;64k | legacy | 5000 | 207.375 | 245.068 | 54.478 | 32.100 | 221.022 | 283.449 | 222.211 | 356.023 | 377.232 |
| quic | bulk&#45;64k | portable | 5000 | 212.300 | 244.058 | 51.169 | 30.051 | 228.510 | 297.647 | 228.306 | 356.842 | 379.665 |
| quic | bulk&#45;256k | legacy | 5000 | 521.063 | 549.656 | 17.567 | 1.370 | 548.026 | 551.727 | 545.467 | 582.648 | 608.519 |
| quic | bulk&#45;256k | portable | 5000 | 536.083 | 569.511 | 20.970 | 1.770 | 568.279 | 572.633 | 565.603 | 608.847 | 631.913 |
| quic | bulk&#45;1m | legacy | 5000 | 1884.845 | 2125.031 | 302.009 | 182.548 | 1979.651 | 2338.141 | 1991.904 | 2777.115 | 2831.378 |
| quic | bulk&#45;1m | portable | 5000 | 1877.899 | 2115.534 | 302.152 | 141.485 | 1973.476 | 2328.706 | 1981.074 | 2776.504 | 2841.764 |
| quic | bulk&#45;4m | legacy | 5000 | 9226.184 | 10333.523 | 1319.626 | 543.651 | 9697.343 | 10813.893 | 9807.203 | 13474.492 | 13865.443 |
| quic | bulk&#45;4m | portable | 5000 | 9431.104 | 11013.757 | 1631.171 | 436.652 | 10569.557 | 11724.382 | 10115.631 | 13608.504 | 13904.067 |
| quic | deep&#45;8 | legacy | 5000 | 51.573 | 72.442 | 20.672 | 21.501 | 55.809 | 96.521 | 59.372 | 103.521 | 112.680 |
| quic | deep&#45;8 | portable | 5000 | 57.204 | 71.527 | 19.829 | 21.149 | 61.094 | 109.343 | 60.829 | 108.793 | 122.537 |
| quic | deep&#45;32 | legacy | 5000 | 58.889 | 74.809 | 22.911 | 24.373 | 62.448 | 118.343 | 61.701 | 119.468 | 133.230 |
| quic | deep&#45;32 | portable | 5000 | 65.069 | 83.046 | 24.790 | 26.282 | 70.527 | 130.027 | 70.103 | 131.774 | 146.027 |
| quic | concurrent&#45;32 | legacy | 5000 | 138.572 | 155.403 | 33.008 | 9.508 | 148.337 | 171.722 | 143.764 | 203.952 | 325.090 |
| quic | concurrent&#45;32 | portable | 5000 | 173.515 | 214.508 | 64.940 | 48.264 | 191.350 | 300.807 | 186.517 | 391.089 | 409.204 |
| quic | deep&#45;32&#45;concurrent&#45;32 | legacy | 5000 | 441.937 | 603.426 | 191.264 | 113.600 | 506.100 | 793.196 | 475.700 | 910.252 | 931.289 |
| quic | deep&#45;32&#45;concurrent&#45;32 | portable | 5000 | 506.778 | 672.998 | 172.634 | 42.953 | 613.870 | 721.760 | 550.272 | 922.679 | 945.299 |
| quic | services&#45;4&#45;deep&#45;16 | legacy | 5000 | 504.862 | 538.000 | 17.661 | 1.188 | 535.933 | 538.825 | 536.389 | 568.679 | 591.738 |
| quic | services&#45;4&#45;deep&#45;16 | portable | 5000 | 464.302 | 505.502 | 18.712 | 1.438 | 503.988 | 507.311 | 503.387 | 539.476 | 562.157 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | legacy | 5000 | 2137.941 | 2643.013 | 136.848 | 10.806 | 2628.359 | 2653.190 | 2652.739 | 2816.378 | 2868.838 |
| quic | services&#45;4&#45;deep&#45;16&#45;concurrent&#45;32 | portable | 5000 | 2204.149 | 2687.451 | 112.185 | 11.887 | 2673.936 | 2700.234 | 2697.758 | 2847.624 | 2902.983 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 2148.499 | 2626.082 | 170.741 | 18.363 | 2607.108 | 2655.975 | 2587.279 | 2963.489 | 3073.318 |
| quic | services&#45;4&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 2051.889 | 2502.383 | 214.430 | 41.380 | 2458.783 | 2556.173 | 2598.731 | 2749.515 | 2880.543 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | legacy | 5000 | 4237.179 | 5223.607 | 376.259 | 83.899 | 5166.665 | 5371.438 | 5134.894 | 5860.136 | 6349.736 |
| quic | services&#45;8&#45;deep&#45;16&#45;clients&#45;8 | portable | 5000 | 3681.379 | 4718.528 | 551.043 | 72.831 | 4642.232 | 4792.557 | 4835.432 | 5533.315 | 5855.966 |

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


