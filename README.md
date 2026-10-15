# Capntproto verification reports

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-15T14:25:42Z · run 38751293855 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/38751293855) · commit `4344e78c595e` · **cancelled**

![Cargo test results](docs/reports/cargo-history.svg)

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-15T14:25:42Z · run 38751293855 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/38751293855) · commit `4344e78c595e` · **cancelled**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

![TLA&#43; checks and expected counterexamples](docs/reports/tla-outcomes.svg)

Unique module&#47;configuration&#47;expected&#45;exit checks&#46; Expected invariant violations are successful controls&#46;

![TLA&#43; explored states by model](docs/reports/tla-states.svg)

Counts sum independent bounded configurations&#59; they are not globally distinct states or a proof beyond those bounds&#46;

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-15T14:25:42Z · run 38751293855 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/38751293855) · commit `4344e78c595e` · **cancelled**

AFL++ guides the RPC lifecycle oracle with IJON state and progress annotations. Corpus inputs, crashes, hangs, logs and engine statistics are retained in the linked run. Fuzzer counters are not source coverage percentages.

### LLVM coverage

No validated coverage/baseline comparison is available for the latest run. Missing or unmapped counters are never presented as 100% coverage.

### Linux loopback benchmark comparisons

Latest benchmark run: [2026-10-15T14:25:42Z · run 38751293855 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/38751293855) · commit `4344e78c595e` · **cancelled**

Measurement evidence: **unavailable**. This workflow attempt produced no public measurement artifact&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Capntproto TCP, upstream Rust Cap’n Proto (`capnp-rust`), and C++ Cap’n Proto use matched plaintext TCP; native QUIC remains a separate encrypted comparison. gRPC and WebSocket baselines use plaintext TCP. Bars compare this workload, not universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.

