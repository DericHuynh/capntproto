# Quality checks and benchmark reports

Pull requests compile default features on **Linux, macOS and Windows** and run
smoke checks plus selected minimal storage, TLS and QUIC feature profiles.
There is no feature powerset matrix. Full correctness
checks use one entry point on the supported Linux verification host:

```sh
cargo test --workspace
```

That command discovers unit/integration tests, doctests, bounded models, Miri,
mutation controls, fuzz/sanitizer controls, standalone maintained crates, quiche,
C++ interoperability and downstream checks. Native tools and pinned verification
prerequisites are still required; see [Testing](Testing.md) and the
[full setup action](../../.github/actions/quality-setup/action.yml). Tests launch
specialized subprocesses where needed. Benchmarks do not run as tests.
Documented ignored diagnostic/unbounded/release-publication controls remain
outside the default bounded test run; [ignored-tests.json](../../quality/ignored-tests.json)
records their rationale.

| Workflow | Trigger | Scope and README artifact |
| --- | --- | --- |
| [Quality](../../.github/workflows/quality.yml) | PR; push to `main`; manual | Library/binary builds and RPC/pipelining, transport and selected feature checks on all three OSes. Linux/macOS also test storage reopen; Linux enforces allocation budgets and QUIC v2 interoperability. `platform-report` |
| [Workflow checks](../../.github/workflows/workflow-checks.yml) | Affected PR or push to `main`; manual | Workflow, auditable-build, trigger-test and README-reporting changes: actionlint with ShellCheck/Pyflakes, trigger regression tests, reporting tests and Zizmor |
| [Documentation links](../../.github/workflows/links.yml) | Affected PR or push to `main`; Tuesday 05:43 UTC; manual | Documentation/workflow/wiki-tool changes: wiki validation/export and local links. External checks are advisory and run only weekly/manually. `github-wiki`, `documentation-links` |
| [Extended quality](../../.github/workflows/extended-quality.yml) | Wednesday 04:37 UTC; manual | Bounded Miri endian/seed sweep, native cargo-careful checks, and LLVM IR size reports in independent jobs |
| [Full quality](../../.github/workflows/full-quality.yml) | Monday 03:19 UTC; manual | One LLVM-instrumented workspace test invocation, C++ LLVM reference suites, advisory checks, coverage regression report. `full-quality-report` |
| [Dedicated benchmarks](../../.github/workflows/benchmarks.yml) | Manual | Compile release artifacts on Actions, measure on a temporary dedicated CPU Linux droplet, validate downloaded samples. `dedicated-benchmark-report` |
| [Benchmark droplet cleanup](../../.github/workflows/benchmark-cleanup.yml) | Every hour at :17 UTC; manual | Destroy this repository's abandoned benchmark resources older than two hours |
| [Publish README reports](../../.github/workflows/readme.yml) | Full quality or Dedicated benchmarks completion; manual run ID | Validate trusted default-branch producer artifacts and publish reports; it does not rerun their tests |

## Trigger and concurrency policy

Branch work runs through `pull_request` only; automatic `push` checks are limited
to `main`. A Dependabot or feature-branch update therefore gets one run of each
applicable PR workflow. A branch without a PR needs a manual dispatch to run CI.
Tag pushes do not start these checks. The `main` push after a merge intentionally
checks the integrated revision. Workflows with path filters require both the
branch and path conditions to match; see GitHub's
[trigger filters](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax).

New commits to the same PR cancel its stale checks. Workflow, event and PR/ref
identity keep unrelated checks separate, including scheduled external-link
checks versus ordinary documentation pushes. Full and extended verification
retain their own cancellation groups. Dedicated benchmarks, cleanup and README
publication each serialize their own runs without cancelling an active run;
cleanup remains independent of the benchmark job so it can recover abandoned
resources. Manual dispatch is an explicit additional run.

The two GitHub-managed **Dependabot Updates** jobs come from the separate Cargo
and GitHub Actions ecosystems in [dependabot.yml](../../.github/dependabot.yml).
They update dependencies; they are not duplicate Quality runs. GitHub-managed
**Dependency Graph** analysis also has a distinct purpose: it does not run the
platform tests. The README
publisher pushes with `GITHUB_TOKEN`, which GitHub excludes from ordinary
[recursive workflow triggering](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow).

Trigger changes apply when the branch contains the updated workflow files.
Branches created before this policy must incorporate it to stop their old
branch-push triggers. Already queued runs are not removed by a workflow edit.

Workflow checks enforce this policy with regression tests. Locally, with
PyYAML installed (`python3-yaml` on the CI runner), run:

```sh
python3 -m unittest discover -s scripts/tests -p 'test_workflow_triggers.py' -v
```

Require **Required platform report** in branch protection. Full quality and
benchmark workflows are separate from PR latency. Windows storage compiles;
its directory-sync durability is not claimed by the RPC smoke check. Platform
qualification requires successful hosted checks; local Linux validation is not
macOS/Windows validation.

## Auditable Cargo builds

Every CI job that invokes Cargo first runs the shared
[auditable setup action](../../.github/actions/auditable-setup/action.yml). It pins
**cargo-auditable 0.7.6** and **rust-audit-info 0.5.4**, then puts a Cargo wrapper
on PATH. Builds, tests, examples, coverage, benchmark compilation, nested Cargo
commands and subsequent `cargo install` commands use `cargo auditable`. The
wrapper preserves `+toolchain` selection and explicit `cargo auditable` calls.
The initial installation of cargo-auditable itself is the bootstrap exception.

Use the same setup locally from the repository root in bash (Git Bash on Windows):

```sh
bash scripts/setup-auditable.sh
export PATH="$PWD/target/auditable-tools/wrapper:$PWD/target/auditable-tools/bin:$PATH"
cargo test --workspace
```

Keep that PATH active for build, benchmark, verification and release commands.
No system Cargo installation is replaced. The ordinary workspace test command
is unchanged. All tools and the wrapper live under ignored `target/`.
Each platform smoke job checks wrapper argument forwarding, including the
macOS runner's Bash 3.2, before invoking Cargo. Schema fixtures come from the
pinned C++ submodule; Linux jobs install both the compiler and schema headers.

Platform jobs also package and compile the extracted `capnp` crate with Rust
1.97.0, its declared minimum. Package contents include sources, tests, schemas,
license and documentation, while upstream archive metadata remains outside it.
Platform jobs extract dependency metadata from both application binaries and
fail if it is missing; their JSON inventories include binary SHA-256 hashes.
Benchmark bundling requires metadata in **all seven Rust executables**, including
the instruction harness and Gungraun runner, before producing a valid manifest.
Import extracts it again and checks it against the manifest. These inventories
travel with the existing source/compiler/lockfile provenance.

Cargo omits an explicit crate type for benchmark executables. A small
[compiler adapter](../../scripts/auditable-bench-rustc.sh) makes the default `bin`
type explicit so cargo-auditable embeds metadata while retaining individual
`cargo bench --no-run` builds. It rejects a conflicting `RUSTC_WRAPPER`. Existing
benchmark artifacts built without this adapter must be cleaned once before
reuse; the metadata gate rejects them. Benchmark dependencies stay in ordinary
`[dependencies]` because cargo-auditable excludes development-only dependencies.

The embedded inventory describes Cargo dependencies of supported linked binaries
and C-compatible dynamic libraries. It does not describe system libraries or the
separately compiled C++ reference, whose pinned provenance is recorded separately.
Rust libraries, ordinary test harnesses, doctests, Miri interpretation and IR-only
checks do not all produce eligible binaries. Their Cargo invocations still go
through the wrapper; Miri explicitly ignores compiler wrapping. Dependency
inventories complement the existing advisory scan; embedding metadata is not a
security audit.

## Focused checks and tool versions

The standalone maintained `capnp` crate is outside the workspace, so workspace
linting with `--no-deps` does not check it. Platform jobs run its all-targets
Clippy check with warnings denied. The `capnp_runtime_lints` tooling test runs
the same check through `cargo test --workspace`; the Linux
`nightly_rpc_try_contracts` driver also checks the optional feature on the pinned
nightly with Clippy installed.

The platform jobs cover default-feature compile/smoke checks and selected minimal
feature profiles; no feature powerset matrix is introduced. Allocation contracts in [allocations.rs](../../tests/allocations.rs)
use allocation-counter **0.8.1**, run with `cargo test --workspace`, and also run on
Linux PRs. They require zero allocations for repeated borrowed generated field
reads and synchronous decoding into caller storage. Async scratch decoding
permits segment metadata allocation but must avoid allocating the 8 KiB payload
when caller storage fits; a fallback control must allocate that payload.
A positive control must observe a heap allocation. The allocator is linked into this test binary only; measurements are
thread-local and make no claim about other tasks or threads.

Workflow checks use actionlint **1.7.12** and Zizmor **1.30.1**. Actions are pinned
to commit IDs. Zizmor audits the repository's `.github` directory, including
composite actions and Dependabot configuration. Vendored upstream workflows
are not executed by this repository and are outside that input scope.
Zizmor uses `advanced-security: false`, so findings fail the job
without requiring code-scanning merge rules. Targeted suppressions keep
checked-out composite action syntax compatible with the pinned actionlint and
allow the intentional PATH additions for auditable Cargo. They do not exempt
the actions' other contents from analysis.

Lychee **0.24.2** checks first-party documentation. Its
[configuration](../../quality/lychee.toml) identifies historical evidence archived
outside the repository and generated/downloaded paths absent from a clean clone.
Ordinary source links and frozen measurements remain checked. External sites are
checked separately so transient failures do not block code PRs. The workflows
with path filters should not be unconditional required checks in branch protection.

Extended quality has three independent, bounded jobs:

- **Miri:** `REPROTO_MIRI_EXTENDED=1 cargo test --locked --test memory_safety`
  retains the existing 92 interpreted ownership executions and adds six wire
  tests with seeds 2–5 on x86-64 and big-endian `s390x-unknown-linux-gnu`: 48 more
  executions. Compiler, source hashes, exact test inventories, targets, seeds,
  and logs are retained. This is interpretation, not native s390x qualification.
  It uses the pinned nightly **2026-08-29** and a 45-minute job limit.
- **cargo-careful 0.4.10:** pinned nightly **2026-08-29** with `rust-src`, a
  separately built standard library, and a 35-minute limit. Tests cover native
  storage fault recovery, descriptor passing, framing and allocation budgets.
  The TLC descriptor replay remains in the ordinary full suite; this job selects
  native checks. It does not instrument C++ internals or replace sanitizers/Miri.
- **cargo-llvm-lines 0.4.48:** reports optimized IR line and instantiation counts
  for the runtime and `native_store` consumer with Rust **1.97.0**. Generated generic
  code is instantiated by the consumer. Reports retain compiler identity and the
  commit; these counts are not build timings and have no regression threshold yet.

Refresh cargo-careful and its dated nightly together; its compatibility window is
shorter than the main compiler pin. Tool failures fail their respective jobs.

## Complete LLVM reporting and regression gates

```sh
cargo run --locked -p reproto-quality -- coverage target/quality/coverage
cargo run --locked -p reproto-quality -- security target/quality/security
cargo run --locked -p reproto-quality -- report target/quality coverage,security
```

The collector uses pinned nightly Rust/LLVM 22 and matching Clang 22. It runs the
workspace tests once with default features and collects ordinary nested crate
profiles. Miri, sanitizer and mutation subprocesses retain their own toolchains.
Native C++ profiles use `KJ_CLEAN_SHUTDOWN=1`, and every suite must flush counters.
Separate Cargo target directories prevent incompatible quiche artifacts from
being shared across workspaces. Each collection uses fresh profile counters.

Every bundled source has a row in `coverage/FILES.md`. LLVM-mapped files include
zero-hit code; unmapped sources are explicit N/A and never counted as covered.
Source groups separate production Rust, maintained Cap'n Proto code, the optional
`capnp-compat` adapters/codecs, transport, C++ reference and verification tooling.
The compatibility crate's source prefix must have measured files. This is
complete reporting, not 100% execution. Linux measurements do not establish
Windows/macOS coverage. HTML,
LCOV and raw LLVM JSON accompany the README.

Per-file and aggregate **line, region, function and branch ratios** must not
regress against `quality/coverage-baseline.json`. Lost instrumentation, changed
compiler/options, stale source hashes and inconsistent raw counters fail checks.
There is currently no reviewed baseline for this simplified scope: after the
first successful collection, inspect its files/counters and commit its
`coverage/summary.json` as `quality/coverage-baseline.json`. The initial collection
can emit measurements; the final regression report deliberately fails until a
baseline exists. Changes to a baseline require review.

## Dedicated Linux loopback benchmarks

Add **`DIGITALOCEAN_ACCESS_TOKEN`** as a secret in the GitHub environment
**`Benchmarking`** (Settings → Environments → Benchmarking). Both the dedicated
benchmark job and scheduled cleanup job use this environment. It needs access
to read plans, create/read/delete Droplets and SSH keys, and create/read tags.
No local token or persistent SSH key is required. Run **Dedicated benchmarks**
from a trusted revision. The workflow generates temporary client and host keys,
pins the host key, and exposes the token only to the infrastructure steps.

Region and size default to `auto`, with `ubuntu-24-04-x64`. A read-only capacity
check runs before compilation and again before provisioning. Auto selection uses
an available four-vCPU CPU-Optimized `c-4` or `c-4-intel` plan, preferring `c-4`
and `nyc3` when available. Explicit region/size inputs are honored; unavailable
choices fail with available alternatives rather than silently changing the host.
Only dedicated `c-N` / `c-N-intel` sizes are accepted, with a **$0.50/hour rate ceiling**.
Capacity can change between discovery and creation; a failed create still follows
the receipt/tag cleanup path. A failed measurement does not run sample-validation
steps or publish stale measurements. Ubuntu 24.04 builds
run on Ubuntu 24.04 to preserve shared-library compatibility. The C++ reference
uses Clang and its pinned upstream source. Nothing is compiled on the droplet.

The build job invokes each target independently:

```sh
cargo bench --locked --manifest-path benchmarks/rpc/Cargo.toml --bench native --no-run
cargo bench --locked --manifest-path benchmarks/rpc/Cargo.toml --bench capnp_cpp --no-run
cargo bench --locked --manifest-path benchmarks/rpc/Cargo.toml --bench grpc --no-run
cargo bench --locked --manifest-path benchmarks/rpc/Cargo.toml --bench websocket --no-run
cargo bench --locked --manifest-path benchmarks/instructions/Cargo.toml --bench hot_paths --no-run
```

`benchmark-build` also compiles the independent pinned C++ executable and the
comparison driver, copies Cargo-reported executable paths into a bundle, and
records compiler/source/lockfile/binary identities. The remote driver verifies
binary hashes before measuring. An individual `cargo bench --bench NAME` runs
that protocol's workload; the C++ wrapper additionally requires
`REPROTO_CPP_BENCH` pointing to the compiled `capnp-reference` binary. Full
comparison timing runs on the droplet with protocol order rotated between five
repetitions.

The bundle also contains Gungraun **0.20.0**, its matching precompiled runner, and
the instruction benchmark. The droplet installs Valgrind, but no Rust toolchain
or build dependencies. After the RPC timing runs, it profiles encode/decode at
64/1024/65536 bytes. Explicit workspace/output paths prevent Gungraun from invoking
Cargo remotely. The six summaries, runner/binary/lockfile hashes and Valgrind
version are retained; missing, duplicate, stale-baseline or invalid counters fail
import. Local verification uses the executable's `--test` mode without profiling.
Instruction counts measure the maintained serializer, including validation and
ownership cleanup, and do not compare the four RPC transports. No historical
instruction-count regression baseline is claimed.

Measurements use IPv4 loopback, separate single-threaded server/client
processes, one outstanding call, 0/64/1024/65536-byte payloads, 100 warmups and
1,000 timed requests per repetition. Every response's sequence and payload are
validated. Setup and warmup are excluded. Reports include p50/p95/p99, sequential
request rate, comparison ratios, raw samples, CPU/OS details and droplet plan.
Capn't Proto uses authenticated encrypted Native/UDP; C++ Cap'n Proto uses plaintext
TCP, gRPC plaintext HTTP/2, and WebSockets plaintext binary echo. These security
and semantic differences are explicit; this is not peak concurrent throughput.

The runner destroys the droplet in `finally`, including failure/cancellation,
and an Actions `always()` step verifies cleanup. A separate hourly janitor
recovers hosts/keys abandoned by runner loss, scoped by repository tag, unique
name and age. Cleanup failures fail the job. Scheduled Actions may be delayed:
this is not a guaranteed billing cutoff. DigitalOcean bills CPU Droplets by the
second at their hourly rate (minimum charges apply); **destroying** the droplet
ends billing, whereas powering it off does not. See [DigitalOcean pricing](https://docs.digitalocean.com/products/droplets/details/pricing/).

No live droplet or hosted workflow was executed during the local refactor.
Do not treat old `target/quality` output as evidence for changed sources; reports
reject mismatched source fingerprints. Each workflow reports its own scope and
explicitly omits unmeasured coverage/performance rather than inventing results.

## Comparison bar charts

The final **Comparison bar charts** section of each generated report README
embeds standalone SVG figures from validated report data. CI uploads the README,
`charts/*.svg` and `charts/data.json` together; download and extract the report
artifact to preserve its relative image links. The Actions job summary links to
that artifact. Rendering uses pinned Plotters with its SVG backend; no browser,
Python plotting environment, external chart service or extra benchmark run is
needed.

The dedicated benchmark report includes six RPC figures, each with a panel for every
payload size:

- Median (p50), p95 and p99 round-trip latency, in microseconds.
- Sequential requests per second.
- Median-latency and request-rate differences from Capn't Proto, calculated as
  `(comparison / Capn't Proto - 1) × 100`.

A seventh figure shows Gungraun instruction counts by encode/decode operation
and payload size. Its table also records data reads and writes. Charts are drawn
only after the raw Gungraun schema, complete case inventory and stored hashes
validate; missing measurements do not become zeroes.

Latency improves downward; request rate improves upward. Each payload panel has
its own linear axis including zero, so compare protocols within a panel and
read the axis when comparing payloads. Native's authenticated encryption and the
other baselines' plaintext transports remain explicit in the figures.

The full coverage report includes four figures comparing current versus reviewed
baseline line, region, function and branch coverage by source group. Axes span
0–100%; N/A has no bar and never substitutes for zero or full coverage. Figures
are produced only after source identities, raw samples/counters and baseline
consistency pass validation. A failed rerun removes previous generated figures
rather than reusing stale charts. Exact plotted values and source identity are
retained in `charts/data.json`.
