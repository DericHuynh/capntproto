# Capntproto verification reports

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-19T08:55:25Z · run 39226234714 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/39226234714) · commit `2c9548c998fb` · **failure**

![Cargo test results](docs/reports/cargo-history.svg)

| Total | Passed | Failed | Errors | Skipped |
| ---: | ---: | ---: | ---: | ---: |
| 2192 | 2172 | [0](docs/reports/failed-tests.md) | 0 | 20 |

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-19T09:10:48Z · run 39228098017 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/39228098017) · commit `2c9548c998fb` · **success**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

![TLA&#43; checks and expected counterexamples](docs/reports/tla-outcomes.svg)

Unique module&#47;configuration&#47;expected&#45;exit checks&#46; Expected invariant violations are successful controls&#46;

![TLA&#43; explored states by model](docs/reports/tla-states.svg)

Counts sum independent bounded configurations&#59; they are not globally distinct states or a proof beyond those bounds&#46;

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-19T09:16:23Z · run 39228803906 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/39228803906) · commit `2c9548c998fb` · **success**

![Fuzzing executions by engine](docs/reports/fuzz-executions.svg)

Bounded campaigns including seed calibration&#59; execution counts are not comparable performance benchmarks&#46;

![Fuzzer feedback by engine](docs/reports/fuzz-coverage.svg)

Engine&#45;local counters&#44; not LLVM source coverage&#46; Do not compare counts across engines or builds&#46;

![Saved fuzzing findings](docs/reports/fuzz-findings.svg)

Saved crashes&#47;hangs are findings requiring triage&#44; not confirmed unique bugs&#46; Missing results remain unknown&#46;

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-15T15:50:28Z · run 38763852450 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/38763852450) · commit `2c9548c998fb` · **success**

Measurement evidence: **passed**. Measured TCP driver&#58; legacy&#46; Measurement collection completed&#59; performance acceptance is separate&#46; Check the workflow conclusion and benchmark artifact&#59; legacy or unrecorded results do not qualify the portable driver&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Capntproto TCP, upstream Rust Cap’n Proto (`capnp-rust`), and C++ Cap’n Proto use matched plaintext TCP; native QUIC remains a separate encrypted comparison. gRPC and WebSocket baselines use plaintext TCP. Bars compare this workload, not universal protocol performance.

![Median round&#45;trip latency &#40;p50&#41;](docs/reports/latency-p50.svg)

Measured TCP driver&#58; legacy&#46; TCP p50 gate&#58; passed &#40;1&#46;15x at every payload&#41;&#46; Relative bars use C&#43;&#43; Cap&#39;n Proto as the reference&#59; each panel includes zero&#46; Native QUIC is encrypted&#59; other baselines are plaintext&#46;

![Tail round&#45;trip latency &#40;p95&#41;](docs/reports/latency-p95.svg)

Measured TCP driver&#58; legacy&#46; TCP p50 gate&#58; passed &#40;1&#46;15x at every payload&#41;&#46; Relative bars use C&#43;&#43; Cap&#39;n Proto as the reference&#59; each panel includes zero&#46; Native QUIC is encrypted&#59; other baselines are plaintext&#46;

![Tail round&#45;trip latency &#40;p99&#41;](docs/reports/latency-p99.svg)

Measured TCP driver&#58; legacy&#46; TCP p50 gate&#58; passed &#40;1&#46;15x at every payload&#41;&#46; Relative bars use C&#43;&#43; Cap&#39;n Proto as the reference&#59; each panel includes zero&#46; Native QUIC is encrypted&#59; other baselines are plaintext&#46;

![Sequential request rate](docs/reports/request-rate.svg)

Measured TCP driver&#58; legacy&#46; TCP p50 gate&#58; passed &#40;1&#46;15x at every payload&#41;&#46; Relative bars use C&#43;&#43; Cap&#39;n Proto as the reference&#59; each panel includes zero&#46; Native QUIC is encrypted&#59; other baselines are plaintext&#46;

![Median latency difference from C&#43;&#43; Cap&#39;n Proto](docs/reports/latency-difference.svg)

Measured TCP driver&#58; legacy&#46; TCP p50 gate&#58; passed &#40;1&#46;15x at every payload&#41;&#46; Relative bars use C&#43;&#43; Cap&#39;n Proto as the reference&#59; each panel includes zero&#46; Native QUIC is encrypted&#59; other baselines are plaintext&#46;

![Request rate difference from C&#43;&#43; Cap&#39;n Proto](docs/reports/request-rate-difference.svg)

Measured TCP driver&#58; legacy&#46; TCP p50 gate&#58; passed &#40;1&#46;15x at every payload&#41;&#46; Relative bars use C&#43;&#43; Cap&#39;n Proto as the reference&#59; each panel includes zero&#46; Native QUIC is encrypted&#59; other baselines are plaintext&#46;

![Serialization CPU work &#40;Callgrind&#41;](docs/reports/instruction-counts.svg)

Maintained Rust serialization only&#59; includes validation and ownership cleanup&#46; Payload sizes perform different amounts of work&#46; These are not RPC latency or cross&#45;protocol comparisons&#59; no historical regression baseline is established&#46;

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.

