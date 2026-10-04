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
remains the performance producer. Each exports an identity-bound `readme-data`
artifact and its own README, graphs and diagnostics, including on test failure.

[Reports / Publish](../../.github/workflows/reports.yml) runs after each producer
finishes. It updates only the dashboard `README.md` and renderer-owned files in
`docs/reports/` on the **reports branch**. The first publication creates an orphan
branch containing reports only, seeded with the frozen history in
`quality/reporting/history-seed.json`. Later publications are atomic, non-forced
updates with bounded retries after concurrent report writes. The default branch
is read only: its code and template are trusted, its ref is never mutated. No PR
code or executable artifact is run by the publisher.

The publisher accepts only completed runs of the four named workflow files (plus their former filenames for manually publishing
runs started before the workflow migration), from
this repository's default branch, triggered by push, schedule or manual dispatch.
It checks the measured commit's ancestry and the artifact's repository, run ID,
attempt and commit. It downloads only the bounded JSON publication artifact;
archive members cannot select output paths. Reruns replace their earlier attempt;
late-finishing older runs do not replace newer benchmark results. Failed or
cancelled runs with missing data are recorded as unavailable, never as success.

The history keeps the latest 365 attempts separately for Cargo, TLA+ and fuzzing (one entry per run ID), the legacy combined history, and the latest benchmark attempt. Historical entries keep test counts, commit,
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

Use Python 3.12+ and an isolated environment for chart dependencies:

```sh
python3 -m venv target/readme-venv
target/readme-venv/bin/pip install -r quality/reporting-requirements.txt
target/readme-venv/bin/python scripts/update_readme.py render --output target/report-preview
```

On Windows, use `target/readme-venv/Scripts/python.exe` and `pip.exe`.
Rendering requires a separate output directory and never rewrites the source README.
Pass `--history PATH` to use a downloaded reports-branch history instead of the
frozen migration seed:

```sh
python3 scripts/update_readme.py render --output target/readme-preview
```

To create public data from an existing source-bound CI report:

```sh
python3 scripts/update_readme.py collect --input target/quality --kind cargo \
  --output target/readme-data/publication.json
```

Use `--kind models`, `--kind fuzz`, or `--kind benchmark` for the other producers. `--report-output DIRECTORY` writes standalone SVG graphs and a report README. `--kind full` is retained for historical combined evidence. The collector does not
run tests or benchmarks. It rejects changed logs, stale chart fingerprints,
non-finite values and unexpected lanes/paths. Matplotlib is only needed to render;
collection and validation use Python's standard library.

Contract tests join `cargo nextest run --workspace` through the quality crate. The chart
rendering case runs when Matplotlib is installed; the reporting CI installs the
pinned dependencies and requires this case. Test-only synthetic plots stay under
`target/` and are never copied into public history.

## GitHub setup

Land the template, source README, frozen history seed, scripts and workflows together.
The first trusted producer completion creates `reports`; dashboard links become live
after that publication. Existing producer artifacts remain accessible through Actions. The configured repository is `DericHuynh/capntproto`; the display
name stays **Capntproto**. No owner/repository URL is hard-coded into the
publisher. Repository identity comes from GitHub's event/API.

The publishing job requests `contents: write` and `actions: read` on its own
short-lived `GITHUB_TOKEN`; there is no additional personal token to configure.
The token is only exposed to the publishing step and is not forwarded on artifact
storage redirects. GitHub's [workflow-run security guidance](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_run)
explains why the default-branch and artifact-origin checks are required.

Protect the source branch normally; it needs no reporting-bot bypass. Repository
rules must permit the publishing token to create/update `reports`. If rules block
that branch, publication fails visibly without a force-push or fallback to main. A manually dispatched publisher accepts
a completed producer run ID for retrying after a settings failure. Commits made
with `GITHUB_TOKEN` do not recursively trigger normal push workflows; see
[GitHub's event guidance](https://docs.github.com/en/actions/how-tos/writing-workflows/choosing-when-your-workflow-runs/triggering-a-workflow#triggering-a-workflow-from-a-workflow).

Keep `DIGITALOCEAN_ACCESS_TOKEN` in the **`Benchmarking`** GitHub environment
for the benchmark producer and cleanup job, as described in
[Quality and Benchmarks](Quality-and-Benchmarks.md). Publication does not receive
that secret or create cloud resources. Cargo, TLA+ and fuzzing verification remain scheduled weekly;
dedicated benchmarks remain manually dispatched to control droplet spend.
