# README generation and public CI reports

The project is **Capn't Proto**; the existing Cargo package and import names stay
compatible. The root [README](../../README.md) is generated from
[README.template.md](../README.template.md). Edit the template, not the generated file.
Template links are relative to the repository root because that is where the
rendered README lives.

## What updates automatically

[Full quality](../../.github/workflows/full-quality.yml) runs the workspace tests
once, collects LLVM coverage and checks its reviewed baseline. It exports a small
`readme-data` artifact even when tests fail. [Dedicated benchmarks](../../.github/workflows/benchmarks.yml)
exports the same artifact after validating dedicated-host samples. Existing full
reports retain raw samples, environment information and logs.

[Publish README reports](../../.github/workflows/readme.yml) runs after either producer
finishes. It updates only `README.md` and renderer-owned files in `docs/reports/`,
using one atomic, non-forced commit to the default branch. A concurrent source
commit causes a bounded retry against the new template/history. No source files
are rewritten, and no PR code or executable artifact is run by the publisher.

The publisher accepts only completed runs of the two named workflow files, from
this repository's default branch, triggered by push, schedule or manual dispatch.
It checks the measured commit's ancestry and the artifact's repository, run ID,
attempt and commit. It downloads only the bounded JSON publication artifact;
archive members cannot select output paths. Reruns replace their earlier attempt;
late-finishing older runs do not replace newer benchmark results. Failed or
cancelled runs with missing data are recorded as unavailable, never as success.

The history keeps the latest 365 full-workspace attempts (one entry per run ID)
and the latest benchmark attempt. Historical entries keep test counts, commit,
source fingerprint and CI links; only current coverage/benchmark bars retain
plotted values. The generated report assets are excluded from executable source
fingerprints, so a report-only commit does not invalidate its own evidence.
They remain included in source archives.

## Aggregate test history

The coreutils-style figure plots **Total, Pass, Fail, Error and Skip** by UTC run
date, with a latest-count/percentage box. A single run is a point, not invented
historical progress. Every line starts at measured data; missing reports never
become zeroes. Failed/incomplete workspace commands receive dotted markers.

The coverage lane uses its existing pinned nightly toolchain for JSON libtest
output and `--no-fail-fast`, while keeping one `cargo test --workspace` invocation.
Ordinary users still run `cargo test --workspace` with the pinned stable toolchain.
The public counters are computed from individual events and checked against suite
summaries and the SHA-256-bound command log:

- **Passed / failed:** terminal Rust libtest and doctest results.
- **Skipped:** ignored tests. Filtered subsets are rejected as full-workspace data.
- **Errors:** announced tests that never returned a terminal result when a harness
  aborts. This includes cases that were announced but not reached before the abort.
- **Total:** passed + failed + skipped + errors across observed suites.

Nested C++, TLC, Miri, fuzz and subprocess cases are represented by their parent
Rust test, rather than counted twice. Build failures before harness execution have
unknown totals. A partially executed workspace is explicitly incomplete; these
counts are not a claim that every possible test was discovered or run. Test
success and whole-workflow success are shown separately: coverage/security gates
can fail even when all tests pass.

The README links its failed count and **Show all failed tests and diagnostics**
to [the current failure report](../reports/failed-tests.md). Each completed failure
includes its harness/test name and escaped diagnostic output, with the original
run, commit and attempt linked above it. The job summary also displays these
failures; `FAILED-TESTS.md` is included in the full-quality artifact. Excerpts are
limited to 4,096 characters per test and 262,144 characters total; every failed name is retained, and the
full raw command log remains in the artifact. Diagnostic details are retained for
the latest full run only. Older count-only records link to their original logs.
Build errors and tests without terminal results remain separate from named failures.

## Labelled benchmark bar charts

The README's final section includes horizontal bars for p50/p95/p99 round-trip
latency, sequential request rate, and percentage differences from Capn't Proto.
Every bar has an implementation label and numeric value; every panel identifies
its payload size. Axes include zero, units and the direction of improvement.
Validated instruction-count and LLVM baseline charts appear when available.

The implementation labels distinguish Capn't Proto / Native, C++ Cap'n Proto,
gRPC (tonic) and WebSockets. These are separate processes on the same dedicated
Linux droplet, with one outstanding request, five repetitions and rotated order.
Native is encrypted over UDP; the other compared transports are plaintext TCP.
The README states these differences beside the figures. It does not combine
results from different measured commits into a single claim. Failed/new missing
benchmark data removes the prior bars from the current view; old artifacts stay
linked through GitHub's run history.

No benchmark or coverage values are fabricated for the initial upload. The
checked-in empty state is replaced after the first corresponding CI run.

## Local commands

Use Python 3.12+ and an isolated environment for chart dependencies:

```sh
python3 -m venv target/readme-venv
target/readme-venv/bin/pip install -r quality/reporting-requirements.txt
target/readme-venv/bin/python scripts/update_readme.py render
```

On Windows, use `target/readme-venv/Scripts/python.exe` and `pip.exe`.
To preview without updating tracked output:

```sh
python3 scripts/update_readme.py render --output target/readme-preview
```

To create public data from an existing source-bound CI report:

```sh
python3 scripts/update_readme.py collect --input target/quality --kind full \
  --output target/readme-data/publication.json
```

Use `--kind benchmark` for dedicated benchmark evidence. The collector does not
run tests or benchmarks. It rejects changed logs, stale chart fingerprints,
non-finite values and unexpected lanes/paths. Matplotlib is only needed to render;
collection and validation use Python's standard library.

Contract tests join `cargo test --workspace` through the quality crate. The chart
rendering case runs when Matplotlib is installed; the reporting CI installs the
pinned dependencies and requires this case. Test-only synthetic plots stay under
`target/` and are never copied into public history.

## GitHub setup

Upload the template, generated README, initial `docs/reports/` files, scripts and
workflows together. The configured repository is `DericHuynh/capntproto`; the display
name stays **Capn't Proto**. No owner/repository URL is hard-coded into the
publisher. Repository identity comes from GitHub's event/API.

The publishing job requests `contents: write` and `actions: read` on its own
short-lived `GITHUB_TOKEN`; there is no additional personal token to configure.
The token is only exposed to the publishing step and is not forwarded on artifact
storage redirects. GitHub's [workflow-run security guidance](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_run)
explains why the default-branch and artifact-origin checks are required.

Default-branch rules must permit this reporting bot's direct report commits.
If repository policy blocks them, publication fails visibly; the publisher does
not bypass rules or force-push. Run `render` and commit the generated files through
your ordinary review process in that case. A manually dispatched publisher accepts
a completed producer run ID for retrying after a settings failure. Commits made
with `GITHUB_TOKEN` do not recursively trigger normal push workflows; see
[GitHub's event guidance](https://docs.github.com/en/actions/how-tos/writing-workflows/choosing-when-your-workflow-runs/triggering-a-workflow#triggering-a-workflow-from-a-workflow).

Keep `DIGITALOCEAN_ACCESS_TOKEN` in the **`Benchmarking`** GitHub environment
for the benchmark producer and cleanup job, as described in
[Quality and Benchmarks](Quality-and-Benchmarks.md). Publication does not receive
that secret or create cloud resources. Full quality remains scheduled weekly;
dedicated benchmarks remain manually dispatched to control droplet spend.
