# CI reports and graphs

Edit the root [README](../../README.md) directly for project prose. Generated evidence
lives on the separate [`reports` branch](https://github.com/DericHuynh/capntproto/tree/reports).
Its dashboard uses [reports.template.md](../reports.template.md); relative graph links
resolve within that branch. The source README links to that dashboard and its failures.

## What updates automatically

[Verification / Cargo tests](../../.github/workflows/verification-tests.yml) runs
ordinary workspace tests/doctests once with LLVM coverage. [TLA+ models](../../.github/workflows/verification-models.yml)
owns the bounded model checks and Rust replays. [Fuzzing](../../.github/workflows/verification-fuzz.yml)
owns libFuzzer/ASan and AFL++/IJON campaigns. [Dedicated benchmarks](../../.github/workflows/performance.yml)
remains the performance producer. CI calls all four after platform validation;
benchmarks follow the verification lanes. Each exports an identity-bound `readme-data-<lane>-<attempt>`
artifact and its own README, graphs and diagnostics, including on test failure.

[Reports / Publish](../../.github/workflows/reports.yml) runs after each producer
finishes, or once after the encompassing CI run completes. A CI publication
merges all four lane artifacts atomically, with separate histories and graphs.
It updates only the dashboard `README.md` and renderer-owned files in
`docs/reports/` on the **reports branch**. The first publication creates an orphan
branch containing reports only, seeded with the frozen history in
`quality/reporting/history-seed.json`. Later publications are atomic, non-forced
updates with bounded retries after concurrent report writes. The default branch
is read only: its code and template are trusted, its ref is never mutated. No PR
code or executable artifact is run by the publisher.

The publisher accepts only completed runs of CI or the four named workflow files (plus their former filenames for manually publishing
runs started before the workflow migration), from
this repository's default branch, triggered by push, schedule or manual dispatch.
It checks the measured commit's ancestry and the artifact's repository, run ID,
attempt and commit. It downloads only the bounded JSON publication artifact;
archive members cannot select output paths.
CI lanes use unique immutable artifact names. Historical standalone `readme-data`
artifacts remain readable, but CI never accepts that ambiguous shared name.
Reruns replace their earlier attempt;
late-finishing older runs do not replace newer benchmark results. Failed or
cancelled runs with missing data are recorded as unavailable, never as success.

The history keeps the latest 365 attempts separately for Cargo, TLA+ and fuzzing (one entry per run ID per lane), the legacy combined history, and the latest benchmark attempt. Shared run IDs must agree on commit, date and URL across lanes. Historical entries keep test counts, commit,
source fingerprint and CI links; only current coverage/benchmark bars retain
plotted values. The generated report assets are excluded from executable source
fingerprints, so a report-only commit does not invalidate its own evidence.
The frozen migration seed remains in source archives; live report history is separate.

## Cargo and TLA+ test histories

The coreutils-style figure plots **Total, Pass, Fail, Error and Skip** by UTC run
date, with a latest-count/percentage box. A single run is a point, not invented
historical progress. Every line starts at measured data; missing reports never
become zeroes. Failed/incomplete workspace commands receive dotted markers.

The coverage lane uses the pinned nightly toolchain for instrumentation and
rustdoc JSON. Nextest unit/integration results use hash-bound JUnit reports;
doctests run separately and their counts are merged explicitly. Missing phases
remain incomplete. Each CI invocation has its own JUnit artifact, so feature
checks cannot overwrite one another's failures. Nested nextest JSON suite
summaries are never interpreted as outer test totals.

- **Passed / failed:** terminal nextest and doctest results.
- **Skipped:** ignored tests. Tests filtered into another partition are excluded from its totals. Legacy full-workspace data still rejects filtered subsets.
- **Errors:** announced tests that never returned a terminal result when a harness
  aborts. This includes cases that were announced but not reached before the abort.
- **Total:** passed + failed + skipped + errors across observed suites.

Nested C++ and subprocess cases are represented by their parent Rust test. TLA+ replay cases and actual TLC checks have separate figures; fuzz executions never count as Cargo test passes. Build failures before harness execution have
unknown totals. A partially executed workspace is explicitly incomplete; these
counts are not a claim that every possible test was discovered or run. Test
success and whole-workflow success are shown separately: coverage/security gates
can fail even when all tests pass.

The dashboard links its failed count and **Show all failed tests and diagnostics**
to [the current failure report](https://github.com/DericHuynh/capntproto/blob/reports/docs/reports/failed-tests.md). Each completed failure
includes its harness/test name and escaped diagnostic output, with the original
run, commit and attempt linked above it. The job summary also displays these
failures; `FAILED-TESTS.md` is included in the Cargo test artifact. Excerpts are
limited to 4,096 characters per test and 262,144 characters total; every failed name is retained, and the
full raw command log remains in the artifact. Diagnostic details are retained for
the latest run in each partition only. Older count-only records link to their original logs.
Build errors and tests without terminal results remain separate from named failures.

## TLA+ and fuzzing graphs

TLA+ graphs show successful invariant/liveness checks, expected counterexamples,
unexpected failures, and generated/distinct states parsed from each final TLC
summary. Logs are hashed; interrupted checks remain failed or unavailable.
Counts sum independent bounded configurations, not unbounded proof coverage.

Fuzzing graphs have separate panels for libFuzzer/ASan and AFL++: executions,
engine-local feedback, and saved crash/hang inputs. An AFL process exiting zero
with saved findings still fails the job. Missing engine results stay unknown.
AFL++ records stability and corpus sizes in its artifact; RPC targets must
acknowledge IJON at the forkserver handshake. These metrics do not substitute for
LLVM source coverage or compare engine performance.

Older combined results stay in `history.json` and `test-history.svg`. New Cargo
and TLA+ histories start at their first measurements; historical totals are not
relabeled as if the partitions had always existed.

## Labelled benchmark bar charts

The dashboard's final section includes horizontal bars for p50/p95/p99 round-trip
latency, sequential request rate, and percentage differences from Capntproto.
Every bar has an implementation label and numeric value; every panel identifies
its payload size. Axes include zero, units and the direction of improvement.
Validated instruction-count and LLVM baseline charts appear when available.

The implementation labels distinguish Capntproto / Native, C++ Cap'n Proto,
gRPC (tonic) and WebSockets. These are separate processes on the same dedicated
Linux droplet, with one outstanding request, five repetitions and rotated order.
Native is encrypted over UDP; the other compared transports are plaintext TCP.
The dashboard states these differences beside the figures. It does not combine
results from different measured commits into a single claim. Failed/new missing
benchmark data removes the prior bars from the current view; old artifacts stay
linked through GitHub's run history.

No benchmark or coverage values are fabricated. The historical seed retains its
original commit identities; each new run must supply fresh verified evidence.

## Local commands

The `capntproto-dev` crate renders standalone SVGs without an external plotting runtime:

```sh
cargo run --locked -p capntproto-dev -- reports render --output target/report-preview
```

Rendering requires a separate output directory and never rewrites the source README.
Pass `--history PATH` to use downloaded reports-branch history instead of the
frozen migration seed. The same command works on Windows, macOS and Linux.

To create public data from an existing source-bound CI report:

```sh
cargo run --locked -p capntproto-dev -- reports collect --input target/quality --kind cargo \
  --output target/readme-data/publication.json
```

Use `--kind models`, `--kind fuzz`, or `--kind benchmark` for the other producers.
`--report-output DIRECTORY` writes standalone SVG graphs and a report README.
`--kind full` reads historical combined evidence. The collector does not run tests
or benchmarks. It rejects changed logs, stale chart fingerprints, non-finite values
and unexpected lanes or paths.

Contract tests run with `cargo nextest run --locked -p capntproto-dev`, including
chart rendering, failed-test inventories, artifact validation and publication races.
