# Capntproto verification reports

[Project and supported APIs](https://github.com/DericHuynh/capntproto) · [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md)

This branch contains generated evidence only. See each result's measured commit
and workflow link before comparing it with a source checkout.

## Tests, coverage and benchmarks

Generated automatically from CI evidence. Each lane keeps its own measured commit and date; measurements from different commits are not combined into a single qualification claim.

### Cargo tests

Latest Cargo run: [2026-10-26T03:31:04Z · run 40264465280 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40264465280) · commit `518a8fcd89d4` · **cancelled**

![Cargo test results](docs/reports/cargo-history.svg)

[Show all failed tests and diagnostics](docs/reports/failed-tests.md). Cargo tests and doctests exclude the dedicated TLA+, fuzz, Miri and mutation campaigns. Filtered tests are not counted as passes or skips. [Reporting contract](https://github.com/DericHuynh/capntproto/blob/main/docs/wiki/README-Reports.md).

### TLA+ models and Rust trace replays

[2026-10-26T03:31:04Z · run 40264465280 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40264465280) · commit `518a8fcd89d4` · **cancelled**

![TLA+ Rust replay results](docs/reports/tla-history.svg)

[Failed model replay tests](docs/reports/failed-models.md). Expected mutation counterexamples are successful checks, not unexpected failures.

### Fuzzing: libFuzzer and AFL++

[2026-10-26T03:35:27Z · run 40264820978 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40264820978) · commit `c6b8c819dd9a` · **success**

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

Latest benchmark run: [2026-10-26T03:31:04Z · run 40264465280 / attempt 1](https://github.com/DericHuynh/capntproto/actions/runs/40264465280) · commit `518a8fcd89d4` · **cancelled**

Measurement evidence: **unavailable**. This workflow attempt produced no public measurement artifact&#46;

Separate client/server processes, one outstanding request, several payload sizes and five repetitions. Capntproto TCP, upstream Rust Cap’n Proto (`capnp-rust`), and C++ Cap’n Proto use matched plaintext TCP; native QUIC remains a separate encrypted comparison. gRPC and WebSocket baselines use plaintext TCP. Bars compare this workload, not universal protocol performance.

![Benchmark measurements pending](docs/reports/benchmarks-pending.svg)

[Machine-readable history and exact plotted values](docs/reports/history.json). Full logs, raw samples and LLVM exports are retained in the linked workflow artifacts.

