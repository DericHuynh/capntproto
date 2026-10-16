# Quality checks and benchmark reports

Pull requests compile default features on **Linux, macOS and Windows** and run
smoke checks plus selected minimal storage, TLS and QUIC feature profiles.
There is no feature powerset matrix. Full correctness
checks use nextest and a separate doctest command on the supported Linux verification host:

```sh
cargo nextest run --locked --workspace
cargo test --locked --workspace --doc
```

These commands cover unit/integration tests, doctests, bounded models, Miri,
mutation controls, fuzz/sanitizer controls, owned workspace crates, upstream quiche adapters,
C++ interoperability and downstream checks. Native tools and pinned verification
prerequisites are still required; see [Testing](Testing.md) and the
[full setup action](../../.github/actions/quality-setup/action.yml). Tests launch
specialized subprocesses where needed. Benchmarks do not run as tests.
Documented ignored diagnostic/unbounded/release-publication controls remain
outside the default bounded test run; [ignored-tests.json](../../quality/ignored-tests.json)
records their rationale.

| Workflow | Trigger | Scope and report artifact |
| --- | --- | --- |
| [CI](../../.github/workflows/ci.yml) | PR; push to `main`; manual | Library/binary builds and RPC/pipelining, transport and selected feature checks on all three OSes. Linux/macOS also test storage reopen; Linux enforces allocation budgets and QUIC v1 interoperability. `platform-report` |
| [CI / Workflow validation](../../.github/workflows/ci-workflows.yml) | Called once by CI; manual | Workflow, auditable-build, trigger-test and README-reporting changes: actionlint with ShellCheck/Pyflakes, trigger regression tests, reporting tests and Zizmor |
| [CI / Documentation](../../.github/workflows/ci-docs.yml) | Called once by CI; Tuesday 05:43 UTC; manual | Documentation/workflow/wiki-tool changes: wiki validation/export and local links. External checks are advisory and run only weekly/manually. `github-wiki`, `documentation-links` |
| [Verification / Extended](../../.github/workflows/verification-extended.yml) | Called by CI; Wednesday 04:37 UTC; manual | Bounded Miri endian/seed sweep, native cargo-careful checks, LLVM IR size reports, and guard mutation/coverage controls in independent jobs |
| [Verification / Cargo tests](../../.github/workflows/verification-tests.yml) | Called by CI; Monday 03:19 UTC; manual | One LLVM-instrumented Cargo partition, un-instrumented C++ reference suites, advisory checks, coverage regression report. `cargo-test-report` |
| [Verification / Formal checks](../../.github/workflows/verification-models.yml) | Called by CI; Monday 03:29 UTC; manual | Verus source/temporal proofs (`verus-report`) including portable ownership/count contracts and mutation controls, then distributed TLC checks and Rust replays (`models-report`). Separate proof-duration and model-state graphs. |
| [Verification / Fuzzing](../../.github/workflows/verification-fuzz.yml) | Called by CI; Monday 03:39 UTC; manual | Six libFuzzer/ASan targets and four AFL++ targets with RPC IJON guidance. Engine-specific execution, feedback and finding graphs. `fuzz-report` |
| [Performance / Dedicated benchmarks](../../.github/workflows/performance.yml) | Called by CI; manual | Build and smoke-test release artifacts on every PR; trusted runs measure application workflows across two dedicated VPC peers, followed by separate loopback controls. `benchmark-bundle`, `benchmark-build-provenance`, `dedicated-benchmark-report` |
| [Benchmark droplet cleanup](../../.github/workflows/maintenance-benchmarks.yml) | After CI benchmarks, even on failure; every hour at :17 UTC; manual | Destroy this repository's abandoned benchmark resources older than two hours; requires trusted credentials |
| [Reports / Publish](../../.github/workflows/reports.yml) | CI or standalone Cargo, formal-check, fuzzing or benchmark completion; manual run ID | Validate trusted default-branch producer artifacts and publish to `reports`; it does not rerun their tests |

## Trigger and concurrency policy

CI is the only automatic push/PR entry point. It calls workflow validation and
documentation as reusable workflows, then starts platform builds after workflow
validation succeeds. Allocation-free core/engine checks and a bare-metal link
fixture run alongside platform builds; Cargo/coverage also waits for this lane.
Cargo/coverage, models, fuzzing and extended verification
then run in parallel after platform and documentation checks. Release benchmarks
follow all four verification lanes. Branch work runs through `pull_request`; only `main` has an
automatic `push` trigger. Feature branches without a PR can dispatch CI manually.
The unconditional final CI job checks every required dependency, including failed
or skipped jobs, so workflow validation and documentation failures cannot produce
a green result. Tag pushes do not start CI.

New commits to the same PR cancel its stale checks. Main runs are serialized
without cancelling an active paid measurement. Workflow, event and PR/ref
identity keep unrelated checks separate, including scheduled external-link
checks versus ordinary documentation pushes. Full and extended verification
retain their own cancellation groups. Dedicated benchmarks, cleanup and report
publication each serialize their own jobs without cancelling an active job;
cleanup remains independent of the benchmark job so it can recover abandoned
resources. Cancellation of an enclosing PR run can still interrupt a measurement;
its unconditional cleanup and the independent hourly janitor handle that case.
Manual dispatch is an explicit additional run.

Fork and Dependabot PRs compile the same optimized benchmark bundle without
secrets; cloud measurement and cloud cleanup are explicitly skipped. Same-repository
PRs and main use the `Benchmarking` environment and its configured protections.
Missing credentials fail trusted measurement instead of recording a benchmark pass.
No PR publishes the default-branch dashboard. Four uniquely named `readme-data-*`
artifacts share a CI run; the publisher validates each lane, attempt and commit,
then merges all four into one atomic reports-branch update. Standalone scheduled
or manual lane runs remain publishable. Reusable calls do not trigger separate
publication runs.

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

The Rust development tool enforces this policy. Locally, run:

```sh
cargo run --locked -p capntproto-dev -- check-workflows
```

Require **Required CI result** in branch protection (replace the former
**Required platform report** check when adopting this workflow layout). This gate
now includes coverage, all verification lanes and benchmark compilation/measurement
according to the credential policy above. Windows also exercises
component storage and retained snapshots; CI does not establish power-loss
durability. Platform
qualification requires successful hosted checks; local Linux validation is not
macOS/Windows validation.

## Workflow dependencies

```mermaid
flowchart TD
  event[PR / main push / manual CI] --> checks[CI: workflow validation]
  event --> docs[CI: documentation]
  checks --> platforms[CI: Linux / macOS / Windows builds and tests]
  checks --> portable[CI: allocation-free ARM checks and link fixture]
  portable --> cargo
  platforms --> cargo[Cargo tests + LLVM coverage]
  platforms --> verus[Verus contracts + temporal induction]
  verus --> models[TLA+ checks + Rust replays]
  platforms --> fuzz[libFuzzer + AFL++ IJON]
  platforms --> extended[Extended verification]
  docs --> cargo & verus & fuzz & extended
  cargo & models & fuzz & extended --> benchmark[Release benchmark build; trusted cloud measurement]
  benchmark --> cleanup[Trusted cleanup, even after failure]
  checks & docs & platforms & portable & cargo & models & fuzz & extended & benchmark & cleanup --> result[Required CI result]
  result --> publish[CI completion: validate evidence and publish on main]
  standalone[Weekly / manual lane runs] --> publish
  janitor[Independent hourly cleanup]
  codeql[GitHub-managed CodeQL default setup]
```

Solid edges are `needs`, reusable-workflow calls, or report-producer completion
triggers. CodeQL's default setup runs independently on PR/main and its weekly schedule;
there is no second repository CodeQL workflow. Benchmark cleanup also runs inside
its producing job; the hourly workflow recovers abandoned
resources. Report publication never launches another verification or benchmark run.

## Independent verification partitions

The local `cargo nextest run --workspace` entry point remains comprehensive. CI gives
expensive campaigns explicit owners:

- Cargo selects all ordinary tests/doctests, filtering `tlc`, `native_fuzz_smoke`,
  `serialization_and_ownership_miri`, and the two `authority_and_transition_*`
  controls. LLVM coverage combines the nextest run and the separate doctest run.
- [Verus](Formal-Verification.md) check inventoried production proofs
  and required fault controls before the TLC job. Verus also checks unbounded
  temporal induction through the verified verus-tla library. Each lane has
  separate artifacts and graphs; proof duration is not coverage. Proof counts
  are not added to Cargo test counts or the published TLC replay/state history.
- TLA+ selects tests containing `tlc`, including the bounded catalog, negative
  controls, and Rust trace replays. Helpers reject TLC calls from the Cargo
  partition, so an incorrectly named new test fails visibly. Each model job uses
  a fresh session identity; repeated readers reuse only checks from that session.
- Fuzzing owns both mutation engines. Extended verification owns Miri and guard
  mutation/coverage controls. The platform CI matrix retains its fast regression
  subsets; these are distinct platform qualifications, not full campaign reruns.

`quality/src/lanes.rs` is the shared partition policy. Expected mutation violations
are successful controls only when both TLC's exit status and the named invariant
match. TLA+ reports retain separate nextest replay counts and unique
module/configuration/expected-exit checks; state totals across configurations
are not a claim of globally distinct states. Fuzz feedback and source coverage
have different units and are never combined. The new Cargo coverage scope is
`first-party-owned-crates-v3`: older scopes need explicit review; the imported core, RPC, futures and codegen
crates are now included.

Every producer uploads a README, SVGs, measured counters and diagnostics and
publishes its own measured commit through the trusted report publisher. Cancelled
or missing measurements cannot reuse an earlier green report. No new histories
are invented when migrating the combined reports.

## Rust dependency caching

Every Cargo job uses [Swatinem/rust-cache](https://github.com/Swatinem/rust-cache),
pinned to a reviewed commit in the shared setup action. Restore occurs after
Rust toolchain setup and before cargo-auditable installation and project builds.
Keys separate platform, coverage, extended-check and benchmark configurations;
the action also hashes toolchains, manifests, lockfiles and compiler settings.
Only trusted `main` runs save caches; PRs can restore them. Cargo verifies its
fingerprints, and every requested test still runs after a cache hit.

The cache holds registries, dependency build artifacts and installed Cargo tools.
Coverage uses registry/tool caching only: its large source-bound instrumented
targets are rebuilt, and raw counters and reports are never restored as measurements.
The workspace-specific targets for downstream consumers and benchmarks are mapped
explicitly rather than sharing incompatible quiche build artifacts.

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
cargo nextest run --locked --workspace
cargo test --locked --workspace --doc
```

Keep that PATH active for build, benchmark, verification and release commands.
No system Cargo installation is replaced. The ordinary workspace test command
is unchanged. All tools and the wrapper live under ignored `target/`.
Each platform smoke job checks wrapper argument forwarding, including the
macOS runner's Bash 3.2, before invoking Cargo. Schema fixtures come from the
pinned C++ submodule; Linux jobs install both the compiler and schema headers.

Platform jobs also package and compile the extracted `capntproto-core` crate with Rust
1.97.0, its declared minimum. Package contents include sources, tests,
license and documentation, while upstream archive metadata remains outside it.
Platform jobs extract dependency metadata from both application binaries and
fail if it is missing; their JSON inventories include binary SHA-256 hashes.
Benchmark bundling requires metadata in **all eight Rust executables**, including
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

The security lane audits all eleven owned lockfiles, including independent
benchmark, fuzz, downstream-example, Miri and portable workspaces. It collects
every audit failure before failing the lane; `--deny warnings` remains required.
Rustls has a minimum version of 0.23.46, and all four lockfiles that contain it
were updated for the October 14 advisories.

## Focused checks and tool versions

All maintained Rust implementations live in owned workspace crates. Workspace
Clippy with warnings denied and ordinary Cargo tests cover them directly.
The `core_feature_profiles` test checks no-std and allocation-only builds;
`nightly_rpc_try_contracts` checks the optional feature on the pinned nightly.

The platform jobs cover default-feature compile/smoke checks and selected minimal
feature profiles; no feature powerset matrix is introduced. Allocation contracts in [allocations.rs](../../tests/allocations.rs)
use allocation-counter **0.8.1**, run with `cargo nextest run --workspace`, and also run on
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

Verification / Extended has three independent, bounded jobs:

- **Miri:** `CAPNTPROTO_MIRI_EXTENDED=1 cargo nextest run --locked --test memory_safety`
  retains the existing 96 interpreted ownership executions and adds six wire
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
cargo run --locked -p capntproto-quality -- coverage target/quality/coverage
cargo run --locked -p capntproto-quality -- security target/quality/security
cargo run --locked -p capntproto-quality -- report target/quality coverage,security
```

The collector uses pinned nightly Rust/LLVM 22. It runs the Cargo partition once
with default features and collects ordinary nested Rust crate profiles. Miri,
sanitizer and mutation subprocesses retain their own toolchains. The independent
C++ reference suites still execute, without coverage instrumentation or counters
in this report. Separate Cargo target directories prevent incompatible quiche
artifacts from being shared across workspaces. Every collection uses fresh raw
profiles even when Rust dependencies are restored from cache.

Only project-owned `.rs` sources enter `coverage/FILES.md`, totals and regression
checks: the runtime, core, RPC engine and wire driver, async framing, codegen, schema compiler,
compatibility crate, quality tools, test support,
tests, examples, fuzz harnesses, benchmarks and Miri checks. An explicit file
allowlist is passed to **all** LLVM JSON, LCOV and HTML exports. Vendored crates,
C++ reference sources, Cargo registry dependencies generated `OUT_DIR` files and checked-in `*_capnp.rs` bindings
are excluded. Instrumentation of linked Rust dependencies may still be present
in raw execution profiles; it does not enter the published coverage scope.
See [LLVM's source filtering](https://llvm.org/docs/CommandGuide/llvm-cov.html#export-command).
JSON and LCOV capture stdout separately from diagnostic stderr. LLVM warnings
remain in the export logs and cannot corrupt the machine-readable artifacts;
nonzero exit statuses, deadlines and cancellation still fail the collection.

Zero-hit owned code remains in the denominator. Owned files without executable
mappings are explicit N/A, never counted as covered. The scope is versioned as
`first-party-owned-crates-v4`; this adds the previously omitted RPC wire crate. A baseline from an older scope is
rejected and must be reviewed again. Linux coverage does not qualify other OSes.

Per-file and aggregate **line, region, function and branch ratios** have a maximum
0.5 percentage-point allowance against `quality/coverage-baseline.json` for
scheduler-dependent async paths. Larger drops fail. A drop to zero hits or zero
mapped counters fails even inside that allowance. Lost instrumentation, changed
compiler/options, stale source hashes and inconsistent raw counters fail checks.
The initial baseline is a complete `coverage/summary.json` from a successful
collection of this scope, committed as `quality/coverage-baseline.json` after
inspecting its files and counters. Changes to a baseline require review. Missing
baselines still fail the final report; CI never creates one automatically or
substitutes the current run for a missing comparison.

Owned build scripts declare `LLVM_PROFILE_FILE` with Cargo's
[`rerun-if-env-changed`](https://doc.rust-lang.org/cargo/reference/build-scripts.html#rerun-if-env-changed)
directive. Each collection uses a new profile directory, so a warm Cargo target
reruns schema generation and produces fresh counters. It must not reuse old
profile files or silently lose build-script coverage. A regression test checks
the real RPC package on a cold build, a warm build with a new destination, and a
warm build whose destination is unchanged.

The first `first-party-owned-crates-v3` baseline was measured on Linux x86-64 on
2026-10-03 from commit `ed95a35ac`, using the pinned coverage compiler and flags.
Its source fingerprint is
`fec878f884402877922bf93693050d11cf88fcc25d6ae8ecccb6efab3971e2fd`.
The instrumented workspace run passed 1,236 tests. The baseline inventories 558
owned source files: 515 have LLVM mappings; 43 are explicitly unmapped. The four
imported Rust crates each have measured coverage. Raw-counter hashes, source-file
hashes and per-group totals were checked before saving the initial summary as
the baseline. The initial measurement is a regression reference, not a claim that
all code paths are covered.

The October 2026 CI requalification reviews the baseline after the nextest
migration, separation of Cargo/TLA+/fuzz lanes, codegen file split, and RPC
optimizations. The original comparison and logs remain failed evidence; changing
the reference does not certify that run. A subsequent CI run must pass the
comparison against the reviewed snapshot before merge. Coverage policy errors
retain the measured counters in evidence, and the report includes the original
failure diagnostic rather than only a generic failed-evidence message.

The review distinguishes executable gaps from changed denominators. Added tests
exercise stream rejection and overflow without corrupting pending bytes, real
QUIC reset propagation, copying-output EOF, multi-segment batch fallbacks,
scratch segment inventories, high-ID lookup and owner destruction. Reporting
tests run real passing/failing nextest fixtures and reject stale or absent JUnit
reports. Coverage-parser tests preserve zero-hit sources, reject malformed or
duplicate exports, and verify workspace/profile isolation.

Remaining zero-hit mappings stay in the snapshot. Examples include clock
callbacks and queued futures which negative controls require never to execute,
32-bit framing overflow paths unreachable on the measured 64-bit target,
platform-specific errors, and model replays now exercised in the independent
TLA+ lane. Burst-buffer trimming and canceled borrowed-driver paths also remain
partly unmeasured. Assertions added to unsafe test fixtures contribute untaken
failure branches. Moving code out of `codegen.rs` and removing covered statements from
join handling can lower a file ratio without adding uncovered lines. Changed
LLVM mappings in generic/generated call sites also change region counts. These
are reviewed baseline differences, not exclusions or claims of coverage. Every
source-group metric must still be compared with the original baseline during
review, and CI continues enforcing per-file and per-group ratios thereafter.

The reviewed replacement was collected on Linux x86-64 on 2026-10-06 from
`8f3e05e23`, with the same pinned compiler, flags and ownership scope. All 1,349
instrumented Cargo-partition tests and workspace doctests passed. The snapshot
inventories 603 source files, with 558 mapped and 45 explicitly unmapped. All 36
group metrics meet or exceed the original baseline; 32 per-file metric changes
remain after the added tests. The complete measured summary is committed without
altering its counters. Source hashes and every mapped metric were checked against
the raw LLVM export. Its source fingerprint is
`adaac8a40b0581d2903533d0ae2556b38d94c54d54d2153a1616caa535dac6d7`;
the raw LLVM JSON SHA-256 is
`88143aa8049c8d3b2dfe1661bbb0027fbb6c15904b3f55b7ed2524e03b1e0761`.

The TCP/portability follow-up was reviewed on 2026-10-07 with the same scope,
compiler and flags. All 1,381 instrumented Cargo-partition tests and 64 runnable
workspace doctests passed. The complete replacement snapshot inventories 610
owned source files: 563 mapped and 47 explicitly unmapped. Every source hash,
mapped metric and group sum was checked against the source tree and raw export;
no mappings were lost. Its source fingerprint is
`ee551cefde15012b97ac7ce31ddf70b66469e4f14e5f14c92384f44a66c26f2c`, and the raw
LLVM JSON SHA-256 is
`ec5d221e492642bf181fda4ffc0eaf4f45a2f2469c098102d6867508dc39eed4`.

Added tests cover malformed/missing benchmark trials, the exact 1.15× boundary,
retention of slow samples, valid-but-slow reports and their diagnostic charts,
engine source ownership, and concurrent verification captures. All four tool
group ratios improved. Thirty-two of 36 group metrics meet or exceed the prior
reference. The four decreases retain their actual measured counters:

| Group / metric | Previous covered/total | Reviewed covered/total |
| --- | ---: | ---: |
| Capability RPC / functions | 883/986 | 909/1017 |
| Tests and verification / branches | 2240/4050 | 2241/4056 |
| Wire runtime / functions | 2020/2414 | 2028/2424 |
| Wire runtime / branches | 1537/1976 | 1541/1982 |

Eighteen per-file metric decreases were reviewed. The new framing overflow
closures are unreachable on the measured 64-bit host; separate 32-bit Miri
executions exercise rejection without claiming 64-bit coverage. Hosted Join and
Return handlers validate question state before the portable transition, leaving
its defensive error conversions unexecuted there. Removing two covered RPC lines
leaves the same 216 uncovered lines, and changing generic IO adapters removes
one covered region per transport without adding uncovered regions. Collector
registration, capture cancellation/OS errors, and untaken test-assertion failures
remain explicit gaps. None is removed from the denominator.

The snapshot is the complete measured summary, with no counter adjustments.
Original failed comparisons and the raw review remain in
`target/tcp-portability/coverage-isolated/` and `coverage-review.json` under its
parent directory. Updating the reference does not certify that failed run:
a subsequent collection must pass the unchanged per-file and per-group gates.

The v4 requalification uses fresh instrumented builds and profiles: all 2,014
Cargo-lane tests and 85 runnable doctests passed. The reviewed summary contains
956 source files, 828 mapped and 128 explicitly unmapped, with no lost mappings
among previously measured files. The newly included RPC wire crate contributes
99 mapped files and 18,128/22,518 covered lines. Overall capability-RPC line
coverage is 88.93% versus 91.05% in the last v3 snapshot; codegen is 90.73% versus
91.89%, and tooling is 66.67% versus 71.46%. Expanded and newly mapped code retains
its uncovered counters. An unchanged QUIC adapter also records one fewer covered
line (128/134 versus 129/134); the measurement is preserved without adjustment.
Source identity is `fe494ef61ee07c06fbaf86e6cfb5961ed577cbdc48c68f6b3fe41213cb3cfcff`;
raw LLVM JSON SHA-256 is
`481a66442a0c5cd962c25d2ecd545ff16906b4ce73ced2da07ca51fd673e5cd4`.
The report validator independently checks those raw counters and source hashes.
This local requalification does not certify the other CI lanes or a release.

### October 14 portable-driver coverage review

Follow-up validation includes nine focused nextest cases and both new arena
tests under 32-bit Miri. The generated-schema extent fix changes no unsafe
operations; its reviewed unsafe-debt source hash changes while the twelve
existing undocumented-unsafe diagnostics remain unchanged.

[CI run 38621565282](https://github.com/DericHuynh/capntproto/actions/runs/38621565282) passed the instrumented tests and doctests but failed the coverage comparison and security audit. That failed run remains failed evidence. The baseline replacement is the complete, unmodified measured summary from its [Cargo report artifact](https://github.com/DericHuynh/capntproto/actions/runs/38621565282/artifacts/11927571631), after checking all source hashes against commit `e5c87b41c58f1e405e08fc3a83fd510b91bd5d51`, every mapped metric against the raw LLVM export, and the group totals. It inventories 975 files: 837 mapped and 138 explicitly unmapped. No previously mapped source lost instrumentation.

The review includes all 42 files / 72 metric decreases rejected by the previous gate. These are not all false positives: new defensive and scheduling paths still have gaps. The follow-up adds focused regression tests without deleting uncovered mappings, excluding tests, broadening tolerances, or treating formal proofs as executed coverage. The same per-file and per-group 0.5 percentage-point gates must pass on a fresh follow-up run.

| Source group | Lines, before → reviewed | Regions | Functions | Branches |
| --- | ---: | ---: | ---: | ---: |
| async-framing | 2548/2666 → 2599/2725 | 3879/4072 → 3964/4171 | 256/279 → 262/287 | 266/276 → 271/284 |
| capability-rpc | 58237/65490 → 60623/68116 | 94707/106284 → 98394/110344 | 5362/6288 → 5588/6551 | 4971/6610 → 5344/7120 |
| optional-compatibility | 5222/5802 → 5222/5802 | 8194/9431 → 8194/9431 | 449/542 → 449/542 | 545/742 → 545/742 |
| runtime | 30524/36441 → 30710/36423 | 45029/53726 → 45425/53725 | 2830/3137 → 2846/3141 | 3262/4444 → 3284/4446 |
| rust-codegen | 3874/4270 → 3874/4270 | 6514/7509 → 6514/7509 | 140/160 → 140/160 | 549/638 → 549/638 |
| schema-compiler | 9531/9814 → 9531/9814 | 16222/16842 → 16222/16842 | 654/690 → 654/690 | 1101/1322 → 1101/1322 |
| tests-and-verification | 69728/88975 → 69943/89110 | 101223/130610 → 101679/130904 | 4290/5494 → 4306/5500 | 2388/4224 → 2423/4262 |
| tools-fuzz-examples-benchmarks | 11243/16863 → 11243/16919 | 20645/31572 → 20712/31684 | 891/1307 → 893/1310 | 1515/2455 → 1515/2457 |
| wire-runtime | 19813/23031 → 20493/23752 | 30228/36054 → 31235/37162 | 2314/2771 → 2372/2840 | 1761/2251 → 1846/2353 |

Only async-framing branches exceed the aggregate tolerance (266/276 → 271/284). The new framed-write and queue-failure tests target that gap. Other aggregate changes remain within the existing tolerance; the per-file review below retains the actual lower measurements.

| File | Rejected metrics | Review and remaining limitations |
| --- | --- | --- |
| `crates/capntproto-core/src/capability.rs` | branches | New promise regression tests contain untaken assertion/panic arms; these stay in the denominator. |
| `crates/capntproto-core/src/private/arena.rs` | regions, functions, branches | Cached-reader validation adds extent and segment checks. Follow-up tests cover invalid cached ranges and shared traversal budgets; generated-arena overflow is fixed separately. Unsupported arena operations remain gaps. |
| `crates/capntproto-core/src/private/arena/fixed.rs` | branches | Moving bounds policy into the shared proof-checked helper removes covered branches from this file; remaining paths keep their actual counters. |
| `crates/capntproto-core/src/private/layout.rs` | functions, branches | Checked-view rebasing and near-pointer validation add cold error paths and generic instantiations; not all fallback functions are executed. |
| `crates/capntproto-core/src/private/layout/bounded.rs` | regions | Fast validation and its reference oracle add regions. Multi-pointer/list oracle arms, fixed-list resizing and rejection paths remain partly unmeasured. |
| `crates/capntproto-core/src/serialize/no_alloc_buffer_segments.rs` | functions | New ownership/conversion instantiations change function mappings; impossible checked framing arms and overflow paths are not counted as covered. |
| `crates/capntproto-futures/src/serialize.rs` | branches | Unchanged source lost the contiguous write fast-path hit. A new direct framed-reference test explicitly exercises it. |
| `crates/capntproto-futures/src/write_queue/common_tests.rs` | lines, regions, functions | Partial-write fixtures add intentionally unused segment reconstruction, poll_close, and failing assertions. New fault tests cover write errors, zero writes, and flush errors in contiguous and batched output. |
| `crates/capntproto-futures/src/write_queue/engine.rs` | branches | Contiguous-frame selection adds branches; the stopped/terminal polling combination remains partly unmeasured. Failure ownership is tested in both queue implementations. |
| `crates/capntproto-rpc-engine/src/answers.rs` | branches | Extracted validation and new rejection tests change mappings; defensive answer-state arms remain visible. |
| `crates/capntproto-rpc-engine/src/flow_control/incoming.rs` | branches | The file now delegates branch decisions to shared policy: 4/4 becomes 0/0 (N/A), not zero percent. The policy source is inventoried separately. |
| `crates/capntproto-rpc-engine/src/flow_control/outgoing.rs` | branches | The file now delegates branch decisions to shared policy: 2/2 becomes 0/0 (N/A), not zero percent. The policy source is inventoried separately. |
| `crates/capntproto-rpc-engine/src/local_table/tests.rs` | functions | Factory rollback tests add callbacks that must not execute after capacity/identity failure. |
| `crates/capntproto-rpc-engine/src/messages/tests.rs` | functions | Ownership and frame-publication tests add deliberately uncalled/error callbacks. |
| `crates/capntproto-rpc-wire/src/owned.rs` | functions | Additional owned framing helpers and generic paths are mapped but not all are exercised. |
| `crates/capntproto-rpc-wire/src/owned/answer.rs` | functions | Prepared response and forwarding helpers add generic function mappings and cold fallbacks. |
| `crates/capntproto-rpc-wire/src/owned/connection/awaiting.rs` | regions, branches | Plain result extraction and handoff guards add real gaps: unauthorized/pending exchanges, method mismatch and some forwarded-result failures. |
| `crates/capntproto-rpc-wire/src/owned/connection/growing/tests.rs` | functions | Growing-storage regression fixtures add deliberately uncalled hooks and failure callbacks. |
| `crates/capntproto-rpc-wire/src/owned/connection/prepared_return.rs` | functions | Additional prepared-return helpers and assertion closures change function coverage. Private-header impossible variants and some copy/publication failures remain unmeasured. |
| `crates/capntproto-rpc-wire/src/owned/connection/server_output/tests.rs` | branches | New returned-frame assertions add untaken failure branches. |
| `crates/capntproto-rpc-wire/src/owned/question.rs` | branches | New owned-only and receipt-free response paths add cancellation/fallback/error combinations. Wrong-message and missing-capability rejection paths remain partly unmeasured. |
| `crates/capntproto-rpc-wire/src/owned/tests.rs` | functions | New zero-copy frame fixtures include uncalled failure functions. |
| `crates/capntproto-rpc/src/local.rs` | lines | Direct request dispatch adds a generic ClientHook fallback, private envelope rejection, and other still-unmeasured local/cancellation paths. |
| `crates/capntproto-rpc/src/queued.rs` | lines, regions, functions | Resolved-client forwarding and reentrancy tests add hook methods that the selected scenario does not invoke; queue exhaustion and busy diagnostics remain gaps. |
| `crates/capntproto-rpc/src/scoped.rs` | branches | Additional idle/notification paths and tests change branch mappings; cold rejection combinations remain visible. |
| `crates/capntproto-rpc/src/scoped/admission.rs` | lines, regions, branches | Repeated receipt protection was not exercised. A new test repeats receipt/admission and checks final-owner release and capacity exhaustion. |
| `crates/capntproto-rpc/src/scoped/response.rs` | lines, regions, functions, branches | Lazy results, cache ownership, and fused plain completion introduce actual fallback/cancellation gaps. New tests exercise hook materialization, duplicate completion, and late cancellation observation; mismatched plain completion and reentrant-waker cases remain explicit gaps. |
| `crates/capntproto-rpc/src/scoped/shutdown.rs` | branches | Weak response registration and reentrant-waker fixtures add branches. Duplicate-close and some reentrant outcomes remain unmeasured. |
| `crates/capntproto-rpc/src/scoped/wire.rs` | lines, regions, branches | New plain completion and read-only payload handoff paths add fallbacks and impossible private-state assertions; some error paths remain unmeasured. |
| `crates/capntproto-rpc/src/scoped/wire/coordinator.rs` | lines, regions, functions, branches | Idle certificates and fused dispatch expand the state space. Pending-output, retired-work, custom-control, cancellation, and error combinations remain partly unmeasured; this snapshot does not claim exhaustive scheduler coverage. |
| `crates/capntproto-rpc/src/scoped/wire/coordinator/bootstrap.rs` | lines, regions, functions | New control-idle hook and pending-capacity/invalid-network fallbacks are not all exercised. |
| `crates/capntproto-rpc/src/scoped/wire/coordinator/multiparty.rs` | branches | Additional control-idle and handoff combinations leave unmeasured fallback branches. |
| `crates/capntproto-rpc/src/scoped/wire/coordinator/transport.rs` | lines, regions, functions | Unchanged adapter loses default trait-method hits after optimized dispatch; default metadata and unsupported admission retention remain unexecuted. |
| `crates/capntproto-rpc/src/scoped/wire/server/worker.rs` | lines | Fused ready dispatch adds capacity/cancellation fallbacks and cold worker errors; some remain unmeasured. |
| `dev/src/checks.rs` | functions, branches | Tooling/refactoring changes denominators and removes old executed checks. OS/tool errors are not all exercised. |
| `dev/src/cloud/remote.rs` | functions | Unchanged source loses a cleanup/child-drop instantiation as subprocess execution changes; live remote smoke and retrieval paths remain outside the measured Cargo lane. |
| `quality/src/coverage.rs` | lines, regions, functions | Fresh-inventory reuse changes collector paths. The instrumented test binary does not execute the full outer collector; its orchestration/error paths remain measured gaps. |
| `quality/tests/ignored_inventory.rs` | lines, regions | CI now uses its authenticated compiled inventory instead of a nested rebuild. The standalone rebuild fallback is consequently unexecuted in this lane. |
| `src/transport/tcp.rs` | branches | Unchanged async transport loses a scheduler-dependent branch hit; ALPN mismatch, IPv6, malformed-frame and receipt-fence failures remain partly unmeasured. |
| `test-support/src/verification/nextest.rs` | lines, regions, branches | Inventory reuse and replay add validation/environment branches. Additional malformed-metadata tests cover missing fields, build metadata and suites; environment/error fallbacks remain visible. |
| `tests/runtime_simulation.rs` | lines, regions | Compiled-binary replay replaces nested compilation in CI; standalone-build fallback remains unexecuted here. |
| `tests/schedules.rs` | lines, regions | Compiled-binary replay replaces nested compilation in CI; standalone-build fallback remains unexecuted here. |

Source identity: `9cdafd57ba6ef83abd88489da492ff3b790bfe0ad5a3d69fd358010c44ea08c6`. Raw LLVM JSON SHA-256: `187291d216a007e609040ae1b79a684c360ce46947cf3dff78cfecfa16e4a396`. Previous baseline SHA-256: `d924fa9f85b9b5830d88ff7eeebf3a43f546c4a6dca04ada7397bd9c3da251f7`; replacement summary SHA-256: `713cdf57840f7cd96acaa2357d110443e675c08b23dddb5ba40ef4d6cc1f8e99`. Local comparison and source/raw-counter validation are retained in `target/ci-coverage-20261014/hosted-review.json`. A snapshot comparison is not a claim of complete protocol coverage or a release qualification.

The Fuzzing job packages its reports, logs, queues, crashes and hangs in
`fuzz-report.tar.gz` before artifact upload. This preserves AFL's colon-containing
filenames, which GitHub's artifact uploader rejects as individual files. Extract
the archive to inspect `target/quality/fuzz/README.md` and `FAILED-TESTS.md`, graphs,
`target/verification/native-fuzz/` logs, and `fuzz/artifacts/` reproducer inputs.
Packaging runs even when a campaign fails, and retains the failing run's evidence.

### October 15 instruction-optimization coverage review

The pinned coverage compiler needs a concrete `Box<Call>` binding before the
request is coerced to `RequestHook`. Commit `848a41e54` fixes that inference
without changing the in-place allocation. The full instrumented build then
exposed coverage decreases from the preceding RPC optimization batch. The
original measurement and failed comparison remain in
`target/ci-fix-20261015/coverage/`; they are not relabeled as passing evidence.

Six additional regression tests check forwarded success/error results and both
call permits, fallback request envelopes and response ownership, receiver reuse
after a failed shared response, rejection of occupied queue storage without
losing its lease, raw partial reads and the terminal admission boundary, and
malformed or unknown-question buffered replies without consuming a live question.
All six pass under stable and the pinned coverage compiler. The complete
instrumented Cargo partition passes 2,085 tests and the runnable doctests pass.
These tests additionally exercise 32 lines in `local.rs`, 33 in `scoped.rs`,
41 in `scoped/response.rs`, and 10 in the transport adapter.

The replacement is the complete, unmodified measured summary from
`target/ci-fix-20261015/with-regressions/coverage/summary.json`. Independent checks
match all 977 source hashes, all mapped counters to the raw LLVM export, and all
group totals. There are 839 mapped and 138 explicitly unmapped files; no measured
source loses instrumentation. The scope, compiler, instrumentation options and
per-file/per-group 0.5 percentage-point tolerance remain unchanged. The revised
reference still requires an independent fresh run; copying the reference does
not turn the failed measurement into a successful gate.

| Source group | Lines, previous → measured | Regions | Functions | Branches |
| --- | ---: | ---: | ---: | ---: |
| async-framing | 2599/2725 → 2657/2780 | 3964/4171 → 4055/4258 | 262/287 → 273/297 | 271/284 → 271/284 |
| capability-rpc | 60623/68116 → 62613/70196 | 98394/110344 → 101956/114060 | 5588/6551 → 5741/6719 | 5344/7120 → 5603/7470 |
| optional-compatibility | 5222/5802 → 5222/5802 | 8194/9431 → 8194/9431 | 449/542 → 449/542 | 545/742 → 545/742 |
| runtime | 30710/36423 → 30733/36446 | 45425/53725 → 45485/53785 | 2846/3141 → 2847/3142 | 3284/4446 → 3284/4446 |
| rust-codegen | 3874/4270 → 3874/4270 | 6514/7509 → 6514/7509 | 140/160 → 140/160 | 549/638 → 549/638 |
| schema-compiler | 9531/9814 → 9531/9814 | 16222/16842 → 16222/16842 | 654/690 → 654/690 | 1101/1322 → 1101/1322 |
| tests-and-verification | 69943/89110 → 69986/89155 | 101679/130904 → 101743/130970 | 4306/5500 → 4311/5505 | 2423/4262 → 2424/4262 |
| tools-fuzz-examples-benchmarks | 11243/16919 → 11357/17044 | 20712/31684 → 20895/31894 | 893/1310 → 902/1316 | 1515/2457 → 1533/2485 |
| wire-runtime | 20493/23752 → 20753/23996 | 31235/37162 → 31626/37538 | 2372/2840 → 2400/2864 | 1846/2353 → 1859/2363 |

No group exceeds the old tolerance. The review retains all 31 rejected metrics
across 17 files, including genuine remaining gaps:

| File (under `crates/`) | Rejected metrics | Review and remaining limitations |
| --- | --- | --- |
| `capntproto-core/src/capability.rs` | branches | Response ownership and error tests add untaken assertion/panic arms. Reuse after failure and cancellation ownership are tested; invariant arms remain unexecuted. |
| `capntproto-rpc-engine/src/messages.rs` | functions, branches | Owned-send and copy helpers add checked states and mappings. Error formatting, buffer capacity access and some defensive outcomes remain unmeasured. |
| `capntproto-rpc-engine/src/messages/queue.rs` | lines, regions, branches | The FIFO now owns leases as well as tracked sends. Partial writes, resource lifetime, mixed FIFO order, shared buffers and occupied-storage rejection are tested; impossible owned-state alternatives and some terminal errors remain uncovered. |
| `capntproto-rpc-engine/src/messages/queue/tests.rs` | branches | The new storage-rejection test deliberately never takes its wrong-owner panic arm. |
| `capntproto-rpc-engine/src/questions.rs` | branches | Final plain-return reclamation adds observer/Finish/owner predicates. The existing matrix checks observer counts 0–3, wrong owners and backpressure; not every composite predicate outcome is measured. |
| `capntproto-rpc-wire/src/owned/connection.rs` | lines, regions, branches | Idle maintenance and fused ownership paths expand the driver. Construction failures, redirected calls and some cancellation/maintenance combinations remain gaps. |
| `capntproto-rpc-wire/src/owned/connection/growing/tests.rs` | functions | Invalid-frame tests add sink selectors that must never execute on rejection. Their zero-hit mappings are retained. |
| `capntproto-rpc-wire/src/owned/connection/invocation/template.rs` | branches | Prepared publication adds private-envelope invariants and rejection paths. Some pool/copy failures and impossible message variants remain unexecuted. |
| `capntproto-rpc-wire/src/owned/connection/plain_return.rs` | regions, functions | Malformed framing, unknown questions, unclaimed results and invalid capabilities are tested transactionally. Fixed-storage sharing and some pending-pipeline/ordering fallbacks remain uncovered. |
| `capntproto-rpc-wire/src/owned/connection/prepared_return.rs` | lines, regions, branches | Direct queue publication adds owner/copy paths. Private-header impossible variants and some storage failures remain gaps. |
| `capntproto-rpc/src/local.rs` | lines, regions, functions, branches | New tests restore general request, envelope and direct-response fallback coverage. Third-party-tail eligibility, busy/revoked diagnostics and some cancellation paths remain unmeasured. |
| `capntproto-rpc/src/membrane.rs` | regions, functions | The added cancellation-guard delegation is not exercised by this partition; revocation/resolution error alternatives also remain visible. |
| `capntproto-rpc/src/scoped.rs` | branches | Forwarding success and errors now exercise both parameter representations and permit release. Allocation failure and some introduced/closed endpoint combinations remain gaps. |
| `capntproto-rpc/src/scoped/wire/coordinator.rs` | functions, branches | New work lanes and fused dispatch have fairness/backpressure regressions, but custom-control defaults, simultaneous pending/error conditions and retirement combinations are not exhaustive. |
| `capntproto-rpc/src/scoped/wire/coordinator/transport/vat.rs` | branches | Raw partial reads and admission boundaries are now exercised. Stream-driver EOF, oversize, flush-error and metadata rejection combinations remain partly unmeasured. |
| `capntproto-rpc/src/scoped/wire/coordinator/transport/vat/tests.rs` | lines, functions | Raw-frame fixtures intentionally never decode the payload and include untaken failure arms. |
| `capntproto-rpc/src/scoped/wire/server/results/tests.rs` | branches | Additional ownership assertions introduce untaken panic branches. |

Source identity: `53161b315652a9c9796fc7efda51a312c850e7225804cc16dc6846116f220e60`.
Raw LLVM JSON SHA-256: `3e101fa5182a8ef8ea156e4c714a5ae9be3a86b7840ac42125e08d67b527e3ff`.
Previous baseline SHA-256: `713cdf57840f7cd96acaa2357d110443e675c08b23dddb5ba40ef4d6cc1f8e99`.
Replacement summary SHA-256: `956e9006fd555916cf648bb817070b54d459ac74c2011f35a5136714a5c79c4d`.
The independent audit is retained in `target/ci-fix-20261015/coverage-review.json`.
This review does not claim exhaustive scheduler coverage or production qualification.

The first hosted run's exported counters also match the local snapshot except
for two environment-dependent default closures and minor scheduling variation.
CI supplies `GITHUB_RUN_ID` and `CAPNTPROTO_REFERENCE_CXX`, so default workflow-ID
and compiler selection initially lost one function hit each. Both selections
now have direct tests for configured and absent values, with no process-wide
environment mutation. Their coverage no longer depends on those CI variables;
the measured reference is unchanged. The comparison is retained in
`target/ci-fix-20261015/hosted-local-comparison.json`.

## Dedicated remote workflows and loopback controls

The CI runner provisions **two** dedicated droplets: one client and one server,
on the same plan, region and VPC. Workload traffic is direct plaintext TCP over
the server's private IPv4 address. SSH uploads binaries and retrieves artifacts;
it does not tunnel measured traffic. The server binds only its private address
and restricts the six benchmark ports to the client droplet. The provider's
[`vpc_uuid` placement field](https://docs.digitalocean.com/products/droplets/reference/api/droplets/)
pins the second peer to the first peer's VPC. A second-peer capacity failure
cleans up both peers instead of changing region or hardware silently.

The shared [workflow catalog](../../benchmarks/rpc/workflows.json) exercises
legacy Rust RPC, portable Rust RPC, published upstream Rust RPC (`capnp-rust`),
pinned upstream C++, gRPC and WebSockets.
The Cap'n Proto implementations use the same schema. gRPC uses protobuf unary
methods over HTTP/2; WebSockets use binary protobuf application messages:

| Case | Work per measured iteration |
|---|---|
| `unary-empty`, `unary-small`, `unary-1k` | One validated echo at 0, 64 or 1,024 bytes |
| `fanout-32` | Issue 32 independent 1 KiB calls concurrently and validate every response |
| `dependent-read` | Open a state capability, await it, then read 1 KiB |
| `pipelined-read` | Open the same capability and send its read before awaiting the open response |
| `write-read` | Update a retained state capability with an expected version, then validate the new version and 1 KiB data |
| `callback` | Pass a client capability, receive and validate its callback, then await the outer completion |
| `bulk-64k`, `bulk-256k`, `bulk-1m`, `bulk-4m` | One validated request/response at 64 KiB, 256 KiB, 1 MiB or 4 MiB |

There are **70 supported workflow/implementation cells**. Promise-pipelined
reads are explicitly unsupported for gRPC and WebSockets and excluded from
ratios, rather than replaced with sequential reads. Their state references use
application IDs without capability security. A dependent read consumes its
temporary cell; retained state is released outside timing, with bounded leases
for abandoned clients. Callbacks use four duplex messages: request,
notification, acknowledgement, completion. gRPC uses a bidirectional stream
opened before timing; WebSockets use the existing connection. Fan-out sends
32 outstanding messages or unary calls before awaiting completion. No protocol
uses compression; all are plaintext, and WebSocket clients use standard masking.
The 4 MiB payload is echoed in both directions. Workflow gRPC and WebSocket
message limits are 8 MiB to include framing; portable RPC uses an 8 MiB frame
limit and a 32 MiB buffer budget. These limits apply only to workflow benchmarks;
the existing small-call microbenchmark configuration is unchanged. Workflow
format version 3 retains the same workload semantics. The required matrix now
includes upstream Rust at every payload and capability workflow; older partial
matrices fail validation.

The independent [upstream Rust benchmark](../../benchmarks/rpc-upstream/README.md)
pins crates.io `capnp` 0.27.2, `capnp-rpc`/`capnpc` 0.27.0 and its own lockfile.
It uses upstream’s buffered TCP adapter arrangement and the same application
workload and timing source as our two drivers. It links no capntproto runtime
crate. Bundle construction verifies this separation and records the upstream
versions, lockfile hash, executable hash and embedded dependency inventory.
Both loopback charts and remote workflow charts include it, with ratios
against the matching C++ cell. It is a descriptive reference, not subject to
the performance acceptance threshold for our own TCP driver.

Five repetitions rotate implementation order. Every cell has 250 warmups and
1,000 retained workflow samples, with setup excluded and validation, response
cleanup and a ten-second workflow deadline included. One CPU is selected on each
host and reused across implementations. The client alone measures elapsed time;
no clock synchronization or one-way latency inference is needed. Fan-out latency
is the latency of the whole 32-call batch. Workflow/s and RPC/s are reported
separately; these closed-loop workloads do not measure open-loop saturation or
an arrival-rate SLO. This initial remote suite compares plaintext TCP; native
QUIC, TLS/mTLS, failure recovery and three-party routing remain separate tests.

`target/quality/benchmark/remote/README.md` and `p50-ratios.svg` report each
workflow's p50/p95/p99, throughput and p50 ratio to C++. Adjacent JSON, per-trial
client CPU/RSS logs, both host environments and provisioning receipts preserve
the evidence. Client resource logs include process startup and warmup. Server
CPU counters cover the whole campaign, including idle and
setup time. A missing cell, duplicate repetition, mismatched bundle, same-host
run or local smoke record fails validation. Remote results are descriptive
baselines; the existing loopback acceptance gate does not apply to them.

```sh
# Credential-free smoke test: all 70 supported cells, not timing evidence.
cargo run --locked -p capntproto-dev -- digitalocean-bench smoke \
  --bundle target/benchmark-build/bundle --output target/workflow-smoke \
  --state target/workflow-smoke-state

# Requires DIGITALOCEAN_ACCESS_TOKEN and GITHUB_REPOSITORY; use fresh paths.
cargo run --locked -p capntproto-dev -- digitalocean-bench run-remote \
  --bundle target/benchmark-build/bundle --output target/quality/benchmark \
  --state target/remote-benchmark-state
```

Every PR builds and smoke-tests the workflows locally, including 72 cross-language
checks against C++ in both directions across all three Rust implementations. Trusted CI runs execute
the remote campaign, stop its servers, and then run the existing loopback and
instruction controls on the already-provisioned client. The workflow summary
and downloadable artifact keep remote results separate from those controls.
Local smoke success does not establish a remote performance ratio.

Add **`DIGITALOCEAN_ACCESS_TOKEN`** as a secret in the GitHub environment
**`Benchmarking`** (Settings → Environments → Benchmarking). Both the dedicated
benchmark job and scheduled cleanup job use this environment. It needs access
to read plans, create/read/delete Droplets and SSH keys, and create/read tags.
No local token or persistent SSH key is required. Run **Performance / Dedicated benchmarks**
from a trusted revision. The workflow generates temporary client and host keys,
pins the host key, and exposes the token only to the infrastructure steps.

Region and size default to `auto`, with `ubuntu-24-04-x64`. A read-only capacity
check runs before provisioning. Auto selection uses
an available four-vCPU CPU-Optimized `c-4` or `c-4-intel` plan, preferring `c-4`
and `nyc3` when available. Explicit region/size inputs are honored; unavailable
choices fail with available alternatives rather than silently changing the host.
Only dedicated `c-N` / `c-N-intel` sizes are accepted, with a **$0.50/hour rate ceiling
per host ($1.00/hour combined maximum)**.
Catalog availability does not reserve capacity. A recognized HTTP 422 capacity
rejection tries the next eligible pair (at most 24 attempts), changing only inputs
set to `auto`. Each pair is tried once. Quota, billing, invalid configuration and
ambiguous network failures stop without retrying creation. The provider's error
code and message are retained with credentials and cloud-init data redacted;
Each role's `provisioning.json` records every attempt and `droplet.json` identifies the host
actually created. A failed create still follows the receipt/tag cleanup path.
A failed measurement does not run sample-validation
steps or publish stale measurements. Ubuntu 24.04 builds
run on Ubuntu 24.04 to preserve shared-library compatibility. The C++ reference
uses Clang and its pinned upstream source. Nothing is compiled on the droplet.

The build job invokes each target independently:

```sh
cargo bench --locked --manifest-path benchmarks/rpc/Cargo.toml --bench tcp --no-run
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
`CAPNTPROTO_CPP_BENCH` pointing to the compiled `capnp-reference` binary. Full
comparison timing runs on the droplet with protocol order rotated between five
repetitions. The comparison driver uses `taskset` to pin the server and client
to two distinct cores reported by the host topology, respecting its allowed CPU
set and skipping sibling hardware threads. Every protocol uses the same pair.
The selected CPUs, allowed set and topology are retained in `environment.json`;
unavailable affinity or fewer than two allowed cores fails the comparison.
Individual `cargo bench --bench NAME` runs retain the caller's CPU affinity.

The standalone TCP benchmark measures the opt-in portable RpcSystem by default;
`CAPNTPROTO_BENCH_PORTABLE=0` selects the legacy control. This benchmark choice
does not change the library's default RpcSystem. The CI loopback control explicitly
selects legacy RPC; the remote suite measures both drivers. Comparisons record the selection as `tcp_driver` in
`environment.json`. Build the driver and TCP bench in release mode, then run
`CAPNTPROTO_BENCH_PORTABLE=1 driver compare-tcp TCP_EXECUTABLE CPP_EXECUTABLE NEW_OUTPUT_DIRECTORY`.
Use the actual Cargo-reported TCP executable and the compiled C++ reference.
The portable gate requires pooled p50 no greater than **1.5× C++ at each of 0,
64 and 1024 bytes**. Its 65536-byte results and p95/p99 remain visible as
diagnostics. The legacy control retains its 1.15× gate at all four sizes. Preserve source
and binary identities and avoid compiler/test load during timing. Passing the
legacy path does not qualify the replacement driver.

Actions streams the driver's workflow/protocol, payload size and repetition progress while
retaining it in `runner.log`. Setup stages identify cloud-init, runtime-tool
installation and bundle upload separately. The driver has a 20-minute overall
limit for loopback controls; remote workflows have a separate 25-minute limit.
A failed or timed-out run downloads available partial logs and samples
before cleanup and still fails the job. Partial samples are diagnostic evidence,
not published measurements. A low droplet-wide CPU reading alone cannot establish
whether this sequential latency benchmark is making progress; check the live
trial log and completed samples.

The bundle also contains Gungraun **0.20.0**, its matching precompiled runner, and
the instruction benchmark. The droplet installs Valgrind, but no Rust toolchain
or build dependencies. After the RPC timing runs, it profiles encode/decode at
64/1024/65536 bytes. Explicit workspace/output paths prevent Gungraun from invoking
Cargo remotely. The six summaries, runner/binary/lockfile hashes and Valgrind
version are retained; missing, duplicate, stale-baseline or invalid counters fail
import. Local verification uses the executable's `--test` mode without profiling.
Instruction counts measure the maintained serializer, including validation and
ownership cleanup, and do not compare the five RPC implementations. No historical
instruction-count regression baseline is claimed.

The separate loopback controls use IPv4 loopback, single-threaded server/client
processes, one outstanding call, 0/64/1024/65536-byte payloads, 10,000 warmups and
1,000 timed requests per repetition. Every response's sequence and payload are
validated. The longer warmup gives the host time to settle before collecting
short latency trials; the previous 100-request warmup lasted only a few
milliseconds for small messages. The same budget applies to all protocols,
and setup and warmup are excluded. All measured samples, including slow ones,
remain in the report. Reports include p50/p95/p99, sequential
request rate, comparison ratios, raw samples, CPU/OS details, Linux clock source
and droplet plan. Clock-read costs matter to user-space QUIC recovery and pacing;
compare implementations on the same host rather than extrapolating from a
development machine with a different clock source or power governor.
The default Capntproto TCP adapter and C++ Cap'n Proto both use plaintext TCP.
The separate native QUIC measurement uses QUIC v1 with TLS 1.3 and pinned peer
authentication; gRPC uses plaintext HTTP/2, and WebSockets plaintext binary echo. These security
and semantic differences are explicit; this is not peak concurrent throughput.

Portable acceptance requires plaintext TCP pooled p50 latency at or below
**1.5× C++ for every small-call payload (0, 64 and 1024 bytes)**; the legacy
control retains **1.15× at every size**. Both retain 64 KiB, p95 and p99 measurements.
The report preserves raw samples and graphs even when this gate fails. TLS/mTLS
and QUIC keep their security semantics and have separate measurements; their
ratios do not qualify the plaintext target. For a focused local comparison, build
the release `driver` and `tcp` bench, then run
`driver compare-tcp TCP_EXECUTABLE CPP_EXECUTABLE NEW_OUTPUT_DIRECTORY`.
It alternates the two implementations over five repetitions on the same pair
of physical cores and records executable hashes, topology, clock costs and all
samples. Do not run compilers or other test campaigns during latency measurement.

Measurement version 2 uses bulk payload comparison and includes response/promise
cleanup and a ten-second per-call deadline in each timed round trip. The old C++
harness asserted each byte separately and sampled before destroying its response.
That inflated its large-payload baseline: a five-repetition local control changed
the C++ 64-KiB median from 156.025 to 75.219 microseconds after bulk comparison and
cleanup were matched, before adding the matching deadline. This is a benchmark
correction, not a protocol speedup. Historical unversioned comparisons below are
retained as historical evidence and must not qualify the current target. The
driver and report validator reject legacy or mixed measurement versions. The
version-2 dedicated result below does not meet the 1.2× target.

The runner attempts deletion of both droplets and temporary SSH keys after
success, failure or cancellation, including partial provisioning. Both intended
names are recorded before any creation request; a lost response is not retried.
An Actions `always()` step verifies cleanup. A separate hourly janitor
recovers hosts/keys abandoned by runner loss, scoped by repository tag, unique
name and age. Cleanup failures fail the job. Scheduled Actions may be delayed:
this is not a guaranteed billing cutoff. DigitalOcean bills CPU Droplets by the
second at their hourly rate (minimum charges apply); **destroying** the droplet
ends billing, whereas powering it off does not. See [DigitalOcean pricing](https://docs.digitalocean.com/products/droplets/details/pricing/).

Do not treat old `target/quality` output as evidence for changed sources; reports
reject mismatched source fingerprints. Each workflow reports its own scope and
explicitly omits unmeasured coverage/performance rather than inventing results.

### Portable TCP qualification (October 11, 2026)

The portable driver passed the 1.5× small-call gate in two independent local
release/full-LTO comparisons on the Ryzen 7 5800H, using CPUs 0/2 and the matched
workload above. The confirmation run measured:

| Payload | Portable TCP p50 (µs) | C++ TCP p50 (µs) | p50 ratio | p95 ratio | p99 ratio |
| --- | ---: | ---: | ---: | ---: | ---: |
| 0 bytes | 56.781 | 38.133 | 1.489× | 1.462× | 1.448× |
| 64 bytes | 56.711 | 38.273 | 1.482× | 1.465× | 1.432× |
| 1 KiB | 57.480 | 39.460 | 1.457× | 1.444× | 1.406× |
| 64 KiB | 102.667 | 76.267 | 1.346× | 1.364× | 1.397× |

The initial run's small-call ratios were 1.494×, 1.484× and 1.492×. All samples
were retained; no compiler, tests or profiler ran during timing. Local raw trials,
summaries, executable hashes and CPU/clock metadata are in
`target/portable-optimization/batch54-comparison/` and `batch54-confirmation/`.
The reference is pinned C++ revision `0de72d8d8cec6b69edaa29de51d3bd490341f9c2`.
These are sequential plaintext loopback results, separate from the CI dashboard's
measured commit. They do not qualify encrypted transports or concurrent throughput.

Comparison with the legacy driver exposed avoidable builder allocation,
framing traversal and repeated scheduler work. The hosted adapter now reuses
released request/result segments in a bounded per-connection cache, grants ready
inbox calls a useful polling quantum, and keeps empty lanes out of output
reservation. Queue growth stays outside the common capacity check. The explicit
`cached_message_words` and `cached_message_segments` options bound released
application segments independently of live messages and the wire-buffer budget;
zero disables retention. Regression tests retain responses across reuse, check
zeroed segments, and exercise output exhaustion before call admission.

The borrowed-frame TCP receive path additionally retains one completed spill
allocation up to 8 MiB per connection. This transport cache is separate from
application segments and the portable wire-buffer budget. It never retains a
larger spill, exposes only completely received frames, and is freed with the
stream. Completed spills up to 128 KiB can replace a smaller primary receive
buffer, avoiding the second underlying read for a repeated frame of that size.
The primary buffer remains bounded by the larger of its configured size and
128 KiB; the separate retained spill remains bounded by 8 MiB. Owning receive
APIs continue transferring their frame allocation and do not promote buffers.

The allocation-free engine retains caller-owned storage and the same protocol
transitions. The library's default remains legacy while bilateral Join and
remaining tail/pipeline-only integration are completed. Performance qualification
alone does not establish that functional parity.

### Portable client instruction campaign (October 12, 2026)

The **21 million client instruction** target is met with the opt-in build settings
below: **20,981,715**, with repeat runs of **20,959,647** and **20,957,904**.
The ordinary full-LTO build measures **28,365,674** and does not meet that ceiling.
Local Callgrind measurements use the complete release/full-LTO plaintext
TCP client process, an empty payload, 100 warmups and 1,000 retained calls,
including process initialization, bootstrap and cleanup. The workload, timing
checks and measurement boundaries stay fixed across candidates.

| Candidate | Additional build settings | Client instructions |
| --- | --- | ---: |
| Portable campaign baseline | None | 66,401,123 |
| Portable checkpoint `batch108` | None | 30,149,143 |
| Legacy control `batch101` | None | 24,156,786 |
| Portable checkpoint `batch118` | Static C++ runtime | 27,481,967 |
| Portable checkpoint `batch120` | Static C++ runtime, LLVM PGO | 22,908,528 |
| Portable final `batch129` | Static C++ runtime, LLVM PGO, native CPU | **20,981,715** |
| Matched legacy control `batch130-legacy` | Static C++ runtime, independently trained LLVM PGO, native CPU | 16,723,238 |
| Portable final `batch131-ordinary` | None | 28,365,674 |

The final ordinary build reduces instructions by **57.3%** from the campaign
baseline; the tuned build reduces them by **68.4%**. Against the independently
trained legacy control with identical settings, the portable client uses
**1.255×** as many instructions. These are instruction ratios, not latency ratios.

Raw profiles, executable snapshots and measurement output are retained locally
under `target/portable-optimization/`. These instruction diagnostics do not
establish a latency ratio or qualify a release. The earlier latency qualification
above is historical evidence for its stated candidate.

Changes remove repeated envelope construction, retain validated serialized
frames, reuse request owners, defer general result-hook allocation, and coalesce
task notifications. Driver publication and maintenance now share one polling
budget across flushes. Network-control work invalidated during that scan must
still be polled or explicitly rescheduled; third-party transfer regressions
exercise this requirement at different budgets. More aggressive reply processing
and broad inlining trials regressed instruction counts and were reverted.

The final publication fast path lets a freshly sent, lone direct call sleep until
input or an application wake. Regression coverage checks cancellation and new
admission without unrelated IO. Compact receipt storage preserves fixed inline
arrays for allocation-free profiles, and data-only questions release their
request buffer after publication while retaining their exact method identity.
Selected-pointer validation retains traversal, nesting and capability checks;
tests compare its errors and limits against full-envelope decoding.

The shared benchmark harness now chooses the protocol before the timed loop.
This avoids moving a gRPC-sized future for each TCP call and benefits both Rust
drivers. Every call, timeout, clock read and response check remains present.
Historical checkpoints before `batch126` used the earlier harness layout; the
three final/control builds above all use the same updated harness.

The compiler experiments are opt-in, separate from the ordinary release build.
Static C++ runtime linkage reduces process initialization in this full-process
measurement; it does not remove BoringSSL or any transport. PGO trains both TCP
peers on payload sizes 0, 1, 7, 8, 31, 64, 256, 1,024, 4,096 and 65,536 bytes,
each with 100 warmups and 1,000 measured calls. Build scripts and proc macros do
not contribute training data. Bounds, overflow checks, validation, timeouts and
payload verification remain enabled. Training does not qualify all RPC features.

Reproduce with the pinned Rust toolchain, `llvm-tools-preview`, Valgrind and the
normal native build prerequisites:

```sh
cargo run --locked -p capntproto-dev -- rpc-instructions \
  --pgo --native-cpu --static-cpp --maximum 21000000 \
  --output target/quality/rpc-instructions
```

The command discovers the GNU/Linux C++ static runtime rather than hardcoding a
compiler version. It requires a new output directory, retains raw profiles and
executable snapshots, and writes `report.json` with the compiler, flags, training
outputs, workload, executable hash and instruction total. The maximum enforces
the ceiling after saving evidence. `--native-cpu` is a separate opt-in
for binaries restricted to a compatible CPU; neither PGO nor native CPU targeting
changes the project's default build. Omit all three optimization flags to measure
the ordinary full-LTO build (also omit the maximum for that comparison). Add
`--legacy` and use a new output directory to independently train and measure the
legacy control. Counts are host/toolchain-specific; this result does not change
the library's default driver or build settings.

Qualification for the final implementation: **2,174 workspace nextest tests
passed**, with seven existing skips, including native fuzzing, mutation checks,
Miri, C++ interoperability and optimized-runtime tests. The five portable feature
profiles passed **436 tests**; both allocation-free and allocating bare-metal
checks passed, and the allocation-free executable linked. Documentation tests
passed **85** with 13 existing ignores. The unsafe-documentation, repository and
workflow gates passed. Ten benchmark smoke cases checked 11,000 calls across
plaintext TCP, authenticated TCP, QUIC, gRPC and WebSocket. Kani was not available
locally. These checks cover the implementation before this prose-only report
update; the source checksums and tracked patch are retained beside `batch129`'s
binary and raw report.

### Latency check of the tuned candidate

Two separate, unprofiled comparisons of the same `batch129` executable against
the existing pinned C++ reference produced the following pooled median latencies.
Each comparison contains five alternating repetitions, 10,000 warmups and 1,000
retained calls per peer/payload pair, on separate fixed physical cores. No
compiler, test campaign or profiler ran during timing. All samples were retained.

| Payload | Portable p50 (µs), first run | C++ p50 (µs), first run | First ratio | Confirmation ratio |
| --- | ---: | ---: | ---: | ---: |
| 0 bytes | 38.483 | 37.785 | 1.018× | 1.011× |
| 64 bytes | 38.692 | 38.204 | 1.013× | 1.024× |
| 1 KiB | 39.461 | 38.971 | 1.013× | 1.013× |
| 64 KiB | 81.575 | 76.197 | 1.071× | 1.055× |

Both local comparisons are below 1.15× at all four payloads. This uses the tuned
Rust build above; the C++ reference was unchanged and was not retrained with PGO.
It is sequential plaintext loopback evidence, not qualification of ordinary-build
latency, encrypted transports, concurrent throughput or complete driver parity.
Raw samples, tail percentiles, executable hashes, CPU placement and clock-read
diagnostics are retained in `batch129-comparison/` and
`batch129-latency-confirmation/` under the same local campaign directory.

### Legacy driver instruction campaign (October 12, 2026)

The legacy driver now passes the **18 million client instruction** ceiling with
**17,788,526** instructions, down **23.9%** from the matched **23,366,812** baseline.
Both measurements use ordinary release/full LTO, one codegen unit and enabled
overflow checks, with a generic CPU target, dynamic C++ runtime and no PGO.
Both peers explicitly select the legacy driver. The complete Callgrind client
process includes startup, bootstrap, 100 warmups, 1,000 retained empty calls and
cleanup. Timeouts, timer reads, sequence checks and payload validation remain
unchanged. This is an instruction ceiling, not a latency or throughput ratio.

The changes reuse validated Call envelopes and the owned receive poller, drive
ready output during publication, cache the decoded result payload, simplify
response futures and avoid repeated builder/segment access. Bounds, nesting and
traversal budgets remain enforced. Question ownership is installed before calling
transport code, and protocol table borrows end before polling I/O. Regression
tests cover synchronous replies, reentrant writes, partial input, retained
responses, cancellation, flush ownership and independent template copies.

The raw report, binary hash, build log and Callgrind profiles are retained locally
in `target/legacy-optimization/final-ordinary/`. Reproduce using a fresh directory:

```sh
cargo run --locked -p capntproto-dev -- rpc-instructions \
  --legacy --maximum 18000000 \
  --output target/quality/legacy-rpc-instructions
```

An unprofiled comparison of this ordinary-build executable against the same
pinned C++ reference passed the 1.15× latency gate at every payload. Pooled p50
ratios were **1.004×** (empty), **1.000×** (64 bytes), **1.005×** (1 KiB) and
**0.914×** (64 KiB). Five alternating repetitions retained all 40,000 timed
samples after 10,000 warmups per peer/payload/repetition, on separate fixed
physical cores with compilers and tests stopped. Raw trials and environment
details are in `target/legacy-optimization/final-latency/`. Ten additional smoke
cases passed across plaintext TCP, authenticated TCP, QUIC, gRPC and WebSocket
at empty and 64 KiB payloads. These local sequential loopback results do not
qualify concurrent throughput or encrypted-transport latency.

The portable migration was paused with a source snapshot retained under
`target/portable-optimization/shelved-source.*`. Its later ordinary-build checkpoint
measured **26,053,204** instructions, improving on `batch131` above but still
missing the separate 25 million target. The opt-in tuned results above describe
their historical binaries; they do not qualify the current ordinary build or
establish complete portable-driver feature parity. Work has resumed alongside
the remote workflow comparisons above. The hosted bridge now relies on the
transport-owned frame to retain admission credit through physical flush,
removing a duplicate credit list. A regression test also prevents an earlier
empty-output result from marking newly published, unflushed output idle.

### Follow-up after adding remote workflow comparisons

Unobserved legacy replies now transfer exclusive ownership to the transport and
drive ready output immediately. A response still uses shared ownership when a
pipeline needs it. Completion is published before calling transport code, and
returned capabilities stay alive through enqueue. Regression tests cover both
paths and Return-before-Release ordering.

Three alternating before/after Callgrind runs of ordinary full-LTO binaries
(generic CPU, one codegen unit, overflow checks, no PGO) gave these median counts
for the complete processes, including 100 warmups and 1,000 retained empty calls:

| Driver / process | Before | After |
|---|---:|---:|
| Legacy client | 17,775,500 | 17,777,578 |
| Legacy server | 19,536,650 | 18,045,569 |
| Portable client | 26,082,490 | 26,020,661 |
| Portable server | 35,988,720 | 35,910,480 |

The server improvement is **7.6%** for legacy; its client cost is essentially
unchanged. Portable client instructions improve **0.24%**, still above the
separate 25 million target. With compilers, tests and profilers stopped, five
alternating TCP/C++ repetitions per payload used the existing 10,000-warmup,
1,000-sample budget and fixed physical CPU placement:

| Payload | Legacy before / C++ | Legacy after / C++ | Portable before / C++ | Portable after / C++ |
|---|---:|---:|---:|---:|
| Empty | 1.007× | 0.996× | 1.159× | 1.159× |
| 64 B | 1.004× | 0.989× | 1.168× | 1.161× |
| 1 KiB | 1.003× | 0.993× | 1.159× | 1.157× |
| 64 KiB | 0.889× | 0.843× | 1.166× | 1.135× |

These are descriptive local plaintext loopback results, not two-droplet results.
Small latency differences need further remote confirmation. The **0.90× small-call
target remains unmet**. Raw samples, all instruction repetitions, binary hashes
and validation logs are under `target/remote-workflows/`; the report is
`qualification/README.md`. The final runtime passed all 2,209 workspace nextest
tests (7 documented skips), including Miri, mutation, fuzz and release checks,
and all 43 supported workflow cells plus 36 C++ interoperability checks. The
allocation-free profile passed 86 tests and compiled for `thumbv7em-none-eabi`.

### Reply envelope reuse in both drivers

Portable hosted results now build inside a private Return envelope. Data-only
publication copies the framed segments, preserving far pointers and validating
the payload under the caller's reader limits. Capability-bearing results and
messages beyond the reader's segment-count limit use the general codec. Legacy
TCP reuses a Return template and a checked header selection, invalidated by
unrestricted message edits. Empty portable maintenance lanes advance in batches
while charging the same caller budget and preserving the rotating cursor.

Three alternating ordinary full-LTO Callgrind repetitions (generic CPU, one
codegen unit, overflow checks, no PGO) measured the complete processes, including
100 warmups and 1,000 empty calls:

| Driver / process | Before | After | Change |
|---|---:|---:|---:|
| Legacy client | 17,777,557 | 17,763,900 | −0.08% |
| Legacy server | 18,045,548 | 17,272,688 | −4.28% |
| Portable client | 26,020,829 | 26,048,884 | +0.11% |
| Portable server | 35,910,444 | 32,109,549 | −10.58% |

With compilers, profilers and tests stopped, five alternating Rust/C++ trials
per payload used 10,000 warmups and 1,000 retained samples on separate physical
cores. These are pooled p50 ratios against each paired C++ control:

| Payload | Legacy before / C++ | Legacy after / C++ | Portable before / C++ | Portable after / C++ |
|---|---:|---:|---:|---:|
| Empty | 0.989× | 0.977× | 1.156× | 1.127× |
| 64 B | 0.989× | 0.982× | 1.165× | 1.134× |
| 1 KiB | 0.984× | 0.979× | 1.155× | 1.127× |
| 64 KiB | 0.931× | 0.899× | 1.160× | 1.147× |

Portable's small-call medians are approximately **1.15× legacy**, comparing
their independently measured Rust trials. The **1.00× portable target remains
unmet**, and its client cost is essentially unchanged, still above 25 million
instructions. These local sequential plaintext TCP results do not qualify live
cross-host performance. Raw trials, profiles, source patch and binary identities
are retained in `target/remote-workflows/reply-final/`; its `README.md` records
the measurement conditions and all comparison rows.

The final runtime passed all 2,214 workspace nextest tests (7 documented skips),
85 doctests (13 ignored), 230 engine/wire tests without default features, and
bare-metal RPC checks with and without `alloc`. Release workflow validation
passed all 58 supported cells and 48 C++ interoperability checks. Each of the
256 KiB, 1 MiB and 4 MiB cases also completed 250 warmups and 1,000 retained
calls on all five implementations. These local checks establish correctness,
not remote latency qualification.

### Server admission and maintenance costs

Legacy TCP retains the Call payload view already checked during dispatch,
including third-party answer adoption. Parameter access still imbues the owned
capability table, and transports without a cache keep the decoding fallback.
The completion enum boxes only its locally redirected builder, reducing moves
through the ordinary reply channel at the cost of one allocation for local
redirection. Portable maintenance copies its capacity table only when storage
grows, preserving the cursor and checked total; its hosted worker wraps the
cursor with a comparison instead of division.

Three alternating full-LTO Callgrind repetitions use the same complete-process
100-warmup/1,000-empty-call setup as above (batch186 to batch190):

| Driver / process | Before | After | Change |
|---|---:|---:|---:|
| Legacy client | 17,763,885 | 17,772,656 | +0.05% |
| Legacy server | 17,272,679 | 16,726,896 | −3.16% |
| Portable client | 26,048,824 | 26,048,958 | <0.01% |
| Portable server | 32,109,519 | 31,839,274 | −0.84% |

Five alternating Rust/C++ trials per payload used separate physical cores,
10,000 warmups and 1,000 retained samples. Ratios use pooled p50 and each paired
C++ control; our compilers, profilers and tests were stopped for these trials.

| Payload | Legacy before / C++ | Legacy after / C++ | Portable before / C++ | Portable after / C++ |
|---|---:|---:|---:|---:|
| Empty | 0.955× | 0.996× | 1.136× | 1.136× |
| 64 B | 0.982× | 0.986× | 1.138× | 1.139× |
| 1 KiB | 0.984× | 0.982× | 1.124× | 1.098× |
| 64 KiB | 0.881× | 0.903× | 1.114× | 1.128× |

The repeatable instruction reduction does **not** establish a consistent latency
improvement. Portable small-call medians remain approximately **1.15–1.16×
legacy**; the **1.00× target remains unmet**. These local sequential plaintext
TCP measurements do not qualify remote performance. Raw trials, profiles,
source patch and binary identities are in `target/remote-workflows/server-final/`.

The measured runtime passed all 2,216 workspace nextest tests (7 documented
skips), 85 doctests (13 ignored), 231 engine/wire tests without default features,
Clippy, and bare-metal RPC checks with and without `alloc`. Workspace validation
includes Miri, fuzzing and the 98-case mutation campaign. Release validation
passed 58 workflow cells and 48 C++ interoperability checks; all 15 large-payload
cells also passed with 250 warmups and 1,000 retained calls each.

### Upstream Rust RPC comparison

The portable-driver review uses upstream `capnproto-rust` revision
[`81bc1b815d0f450c9114f9cc2e2274182d210df2`](https://github.com/capnproto/capnproto-rust/tree/81bc1b815d0f450c9114f9cc2e2274182d210df2)
as an implementation reference, without changing the imported dependency revision.

| Area | Upstream Rust implementation | Portable driver and optimization consequence |
|---|---|---|
| Service completion | [`rpc.rs`](https://github.com/capnproto/capnproto-rust/blob/81bc1b815d0f450c9114f9cc2e2274182d210df2/capnp-rpc/src/rpc.rs) polls the call promise before awaiting returned result ownership. | Poll execution before subscribing to completion. Ready methods avoid allocating a waiter and waking their own worker. |
| Message ownership | [`twoparty.rs`](https://github.com/capnproto/capnproto-rust/blob/81bc1b815d0f450c9114f9cc2e2274182d210df2/capnp-rpc/src/twoparty.rs) retains incoming segment storage and shares outgoing builders through the write queue. | Retained frames and reusable Return envelopes already follow this ownership principle. Preserve that storage through adapters instead of introducing another serialization pass. |
| Scheduling | [`task_set.rs`](https://github.com/capnproto/capnproto-rust/blob/81bc1b815d0f450c9114f9cc2e2274182d210df2/capnp-rpc/src/task_set.rs) uses `FuturesUnordered` and an unbounded task channel. | The portable hosted worker scans bounded slots and charges each stage against a polling budget. Reducing idle scans remains useful; replacing it wholesale would require preserving fairness, capacity failure and caller-supplied storage. |

The completion change applies to ordinary execution and transferred answers.
Pending work still registers a completion observer. If a method publishes an
early pipeline during its poll and returns `Pending` without waking, the worker
explicitly preserves that notification. An already-published pipeline does not
cause repeated self-wakes. Ready execution with a retained result builder still
waits for the builder to be returned before collection. Regression tests exercise
these cases alongside the existing cancellation, fairness and handoff tests.
Transferred pipelines also stop polling their shared execution future once it
completes: polling that handle again while waiting for result ownership could
panic. Their unresolved observer retains a waiter for the eventual builder
return. A regression consumes outstanding notifications before testing that
the later return wakes and resolves the client.

This optimization is in the hosted adapter. The allocation-free engine, its
bounded storage and the `no_std` feature boundary remain intact. Upstream's
serialization portability does not make its `std`/heap-based RPC scheduler a
drop-in replacement for the portable engine.

Three alternating complete-process Callgrind trials, with ordinary full LTO,
100 warmups and 1,000 empty calls, compare batch190 (`e50088794`) with batch194:

| Driver / process | Before | After | Change |
|---|---:|---:|---:|
| Legacy client | 17,772,563 | 17,770,509 | −0.01% |
| Legacy server | 16,726,818 | 16,724,750 | −0.01% |
| Portable client | 26,048,852 | 26,046,647 | <0.01% |
| Portable server | 31,839,178 | 29,543,349 | −7.21% |

Five alternating Rust/C++ trials per payload, with 10,000 warmups and 1,000
retained samples on separate physical cores, give these pooled p50 ratios:

| Payload | Legacy before / C++ | Legacy after / C++ | Portable before / C++ | Portable after / C++ |
|---|---:|---:|---:|---:|
| Empty | 0.998× | 0.989× | 1.145× | 1.146× |
| 64 B | 0.993× | 0.975× | 1.142× | 1.149× |
| 1 KiB | 0.986× | 0.975× | 1.139× | 1.132× |
| 64 KiB | 0.908× | 0.920× | 1.171× | 1.151× |

Our compilers, tests and profilers were stopped during latency measurement;
this remains a shared workstation. The instruction reduction does not establish
a consistent latency improvement. Portable small-call medians remain about
**1.15–1.16× legacy**, so the **1.00× target remains unmet**. These measurements
compare our drivers, with C++ as a control; they do not benchmark upstream Rust
itself or qualify remote performance.

The final source passes 294 selected RPC unit/integration tests, including
pipeline, cancellation, multiparty, TLS/mTLS and TLC replay checks; 3 RPC
doctests pass (4 ignored). Clippy, formatting and ARM RPC checks with and without
`alloc` pass. Release validation passes 58 workflow cells, 48 C++ interoperability
cases and all 15 large-payload cells at 250 warmups and 1,000 measured calls.
The full workspace and GitHub CI were not rerun for this change. Raw profiles,
latency trials, source patch and binary identities are retained under
`target/remote-workflows/upstream-qualified/`.

### Bounded portable maintenance fast path

The portable server now skips maintenance scans when small tables contain only
live exports, answers awaiting Finish or retained pipeline leases, and vacant
pipeline observers. A settled initial pipeline used to leave allocated observer
storage that forced every subsequent ordinary call through all 18 maintenance
stages. The fast path checks at most eight slots per relevant table. Larger
tables and active observers keep the budgeted scanner. Reclamation checks use
the same predicates as actual cleanup; forgotten export batches cannot qualify
as idle. No cached ownership flags, heap allocations or `std` dependencies are
added to the engine or wire driver.

Against `7cb732065`, three complete-process Callgrind trials with ordinary full
LTO, 100 warmups and 1,000 empty calls produced these median instruction counts:

| Portable process | Before | After | Change |
|---|---:|---:|---:|
| Client | 26,046,762 | 26,064,458 | +0.07% |
| Server | 29,543,081 | 27,888,953 | −5.60% |

Five rotated, alternating repetitions measured all five peers on the same two
physical cores, with 10,000 warmups and 1,000 retained calls per trial. The
upstream peer uses the independent published-crate benchmark described above.
These are pooled p50 ratios; lower is better:

| Payload | Portable before / C++ | Portable after / C++ | Legacy / C++ | Upstream Rust / C++ | Portable after / legacy |
|---|---:|---:|---:|---:|---:|
| Empty | 1.124× | 1.113× | 0.979× | 0.995× | 1.137× |
| 64 B | 1.129× | 1.127× | 0.980× | 0.998× | 1.150× |
| 1 KiB | 1.134× | 1.127× | 0.977× | 1.012× | 1.154× |
| 64 KiB | 1.120× | 1.142× | 0.874× | 1.052× | 1.306× |

Small-call p50 improved by 0.2–1.0% locally. The initial 64 KiB run was 2.0%
slower after the change; an independent repeat was 1.7% faster. Neither a bulk
latency improvement nor a consistent regression is established on this shared
workstation. Compilers, tests and profilers were stopped during latency runs.
The **1.00× legacy target remains unmet**; these runs do not qualify remote
performance. Raw trials, profiles, source patch and executable identities are
retained under `target/portable-optimization/maintenance-20261012/`.

Validation passed 512 RPC engine/wire/runtime tests, 233 engine/wire tests with
allocation disabled, and 50 selected transport/multiparty integration tests.
Regressions cover deferred final lease release, answer ID reuse, forgotten
export batches and cleanup after an observer table becomes vacant. Clippy,
formatting and ARM RPC checks with and without `alloc` passed. The release
workflow smoke run passed all 70 supported cells and 72 cross-language checks,
including the 256 KiB, 1 MiB and 4 MiB payload workflows. The full workspace and
GitHub CI were not rerun for this change.

### Portable dispatch adapter overhead

The hosted portable adapter reuses the validated shared-parameter allocation as
its `ParamsHook`, avoiding a second wrapper for those data-only calls.
Capability-bearing payloads retain their existing ownership path. The
worker's stage transition is inlined into its bounded scan, and the coordinator
registers the parent waker once before consuming application notifications.
The allocation-free engine and wire driver's interfaces are unchanged.

Against `3265164b2`, three complete-process Callgrind trials with ordinary full
LTO, 100 warmups and 1,000 empty plaintext TCP calls produced these medians:

| Portable process | Before | After | Change |
|---|---:|---:|---:|
| Client | 26,064,614 | 25,979,565 | −0.33% |
| Server | 27,889,334 | 27,632,788 | −0.92% |

Five rotated, alternating repetitions on two physical cores, each with 10,000
warmups and 1,000 retained calls, gave these pooled p50 ratios:

| Payload | Portable after / before | Portable after / C++ | Portable after / legacy | Portable after / upstream Rust |
|---|---:|---:|---:|---:|
| Empty | 0.998× | 1.127× | 1.154× | 1.123× |
| 64 B | 1.005× | 1.131× | 1.151× | 1.127× |
| 1 KiB | 1.005× | 1.134× | 1.159× | 1.115× |
| 64 KiB | 1.014× | 1.149× | 1.281× | 1.078× |

Small-call latency is effectively unchanged. The 64 KiB median difference
ranged from 1.5% faster to 1.4% slower across four runs, including longer paired
trials and intermediate-build comparisons with 10,000 retained calls. Tail
latency also changed direction. These shared-workstation measurements establish
neither a bulk latency improvement nor a consistent regression. Compilers, tests
and profilers were stopped during latency runs. The **1.00× legacy target
remains unmet**; no remote performance qualification is claimed.

Validation passed 512 RPC engine/wire/runtime tests, 233 engine/wire tests with
allocation disabled, 50 selected transport/multiparty integration tests, Clippy,
formatting and ARM RPC checks with and without `alloc`. Regression checks verify
the shared parameter allocation and its lifetime, notifications before initial
parent-waker registration, and replacement of the parent waker during idle polls.
The release smoke run passed all 70 supported workflow cells and 72
cross-language checks, including the large payload workflows. The full workspace
and GitHub CI were not rerun. Raw measurements, intermediate binaries, source
patch and executable identities are retained under
`target/portable-optimization/worker-20261012/`.

### Retained portable request readers (October 13, 2026)

Upstream Rust RPC's Params owns the incoming message and capability table directly
(see the upstream implementation linked above). The portable hosted adapter now
retains the admitted parameter struct and its immutable segment owner. It uses
the frame's validated segment layout and cached method metadata, avoiding another
envelope decode. Prepared server calls own their ParamsHook directly, so dispatch
passes that owner to the service without an intermediate payload representation.
Segment identity checks remain in CheckedStructReader::retain. Requests with
capability descriptors still use their exact receipts; fixed caller buffers still
copy into the hosted Params owner. The allocation-free engine and wire profile
have no new allocation or dependency.

Three complete-process Callgrind trials against a0b5b3926 used ordinary
release/full LTO, one codegen unit, overflow checks, 100 warmups and 1,000 empty
plaintext TCP calls:

| Portable process | Before | After | Change |
|---|---:|---:|---:|
| Client | 25,983,035 | 25,997,058 | +0.05% |
| Server | 27,634,357 | 27,145,018 | −1.77% |

Five rotated repetitions on two physical cores, with 10,000 warmups and 10,000
retained calls per peer and payload, produced these pooled p50 ratios:

| Payload | After / before | After / C++ | After / legacy | After / upstream Rust |
|---|---:|---:|---:|---:|
| Empty | 1.000× | 1.117× | 1.144× | 1.127× |
| 64 B | 0.997× | 1.146× | 1.155× | 1.133× |
| 1 KiB | 0.998× | 1.135× | 1.158× | 1.122× |
| 64 KiB | 1.006× | 1.209× | 1.268× | 1.087× |

P50 stayed within 0.7% of the baseline. P99 was mixed: empty calls changed from
123.5 to 147.9 µs, 64 B from 124.0 to 111.5 µs, 1 KiB from 157.7 to 130.4 µs, and
64 KiB from 223.8 to 242.8 µs. Local builds, tests and profilers were stopped during
latency measurement, but this remains a shared workstation. These measurements
establish lower server instruction cost, not a p99 improvement or remote
qualification. The **1.00× legacy target remains unmet**.

Validation passed 516 engine/wire/runtime tests (the changed hosted adapter was
retested with all 232 RPC unit tests), 233 tests with allocation disabled, and
30 transport/interoperability/handoff tests. Regressions check retained parameter
identity and lifetime for single/multiple segments, fixed-buffer fallback and
capability-bearing requests. The cross-runtime test passes all 132 required
scenarios plus the bounded upstream check and two expected upstream-only controls.
Clippy, formatting and the bare-metal wire build passed. Verus verifies 34 source
functions and lemmas, the 139-function temporal library and nine fault controls;
Kani retains the constructor wrapper check. The full workspace/coverage campaign
is delegated to hosted CI for this batch. Raw profiles, both rejected and retained
candidates, long latency trials and executable identities are retained under
`target/portable-optimization/verus-20261013/`.

### Portable receipt representation (October 14, 2026)

Release compilation already uses fat LTO and one codegen unit. Inspection of the
baseline binary found a 62,424-byte `Coordinator::poll_work_budget` with a
3,384-byte stack frame. Its generic adapters are extensively inlined, but the
state machines still move owned values, validate ledger identities and poll
separate stages. Inlining alone cannot remove these operations or shrink a
retained value's representation. Large machine code is a diagnostic observation,
not evidence that further inlining would improve performance.

One concrete cost was hidden in admission: the hosted driver has zero inline
receipt slots, and even a Call with no descriptors reserved one slot for its
target. That allocated three vectors and their boxed container. The target is
already moved into its own affine owner at acceptance; it now uses temporary
stack slots in the same transactional admission path. The target still consumes
the configured capacity budget. Calls with descriptors retain their existing
storage and validation. Capacity policies are also passed only while reserving
receipts, rather than carried in every accepted Call, Return and Resolve owner.
Neither change introduces an allocation or dependency into the no-alloc profile.

Three alternating whole-process Callgrind trials against `651c7ec81`, with
100 warmups and 1,000 retained empty plaintext TCP calls, produced these medians:

| Process | Before | After | Change | Matched legacy |
|---|---:|---:|---:|---:|
| Client | 25,997,720 | 25,963,270 | −0.13% | 17,744,945 |
| Server | 27,145,087 | 26,168,694 | −3.60% | 16,352,505 |

Both portable binaries use ordinary release/full LTO with overflow checks,
without PGO, CPU-specific flags or static C++ linkage. Legacy uses the candidate
executable with only the driver selector changed. Counts include startup,
bootstrap and cleanup; they are not latency ratios. Portable remains **1.46×**
legacy client instructions and **1.60×** legacy server instructions. The
development `rpc-instructions` report now includes `server_instructions` as well
as `client_instructions`. At that point its optional ceiling applied only to the
client; the later qualification below gates both peers.

A separate experiment boxed immediate promise errors to shrink successful and
pending promises. It saved only about 0.4% client and 0.8% server instructions
beyond the receipt changes while adding an allocation to immediate failures.
That experiment was reverted; the existing promise representation is retained.

Five rotated local latency repetitions used two physical cores, 10,000 warmups
and 10,000 retained calls per payload and implementation:

| Payload | After / before p50 | After / legacy p50 | After / upstream Rust p50 | After / C++ p50 |
|---|---:|---:|---:|---:|
| Empty | 0.992× | 1.156× | 1.116× | 1.112× |
| 64 B | 0.995× | 1.154× | 1.114× | 1.114× |
| 1 KiB | 1.013× | 1.151× | 1.123× | 1.149× |
| 64 KiB | 0.990× | 1.205× | 1.109× | 1.094× |

P50 changes are small and mixed. P99 changed from 212 to 277 µs for empty calls,
262 to 500 µs for 64 B, 455 to 996 µs for 1 KiB, and 1,092 to 765 µs for 64 KiB.
There was substantial variation between repetitions on this shared workstation;
builds, tests and profilers were stopped during latency measurement. These data
establish a repeatable instruction reduction, **not a latency or p99 improvement**.
Dedicated-host latency qualification and the 1.00× legacy target remain open.
Raw profiles, candidate binaries, rejected experiments and latency samples are
retained under `target/portable-optimization/abstractions-20261014/`.

Validation passed 517 engine/wire/hosted-runtime tests, 233 tests with allocation
disabled, 61 transport/ownership/interoperability tests and the instruction
parser test. The new allocation-counter regression covers imported and promised
targets, zero-capacity rejection, retained target lifetime and full cleanup.
The interoperability matrix passes all 132 required scenarios and the bounded
upstream check, with the existing two upstream-only failure controls reported
separately. The four distributed trace replays pass with the pinned Java/TLC
tools. Bare-metal RPC checks pass both without allocation and with `alloc`;
Clippy, formatting and validation of 102 documents pass. Full workspace and
dedicated-host qualification remain with hosted CI.

### Legacy call completion overhead

The legacy driver now polls method execution and result completion in one
future, reducing the state moves performed by nested completion adapters. It
still drops a completed method future before awaiting its released `Results`:
custom futures can retain that owner even after returning `Ready`. A regression
test covers immediate and pending completion, success and failure, and calls
with and without promise pipelining. Wire behavior and public interfaces are
unchanged.

Against `2f013b9cb`, three complete-process Callgrind trials with ordinary full
LTO, one codegen unit, overflow checks, 100 warmups and 1,000 empty plaintext TCP
calls produced these median instruction counts, including startup and warmups:

| Legacy process | Before | After | Change |
|---|---:|---:|---:|
| Client | 17,770,878 | 17,770,706 | Effectively unchanged |
| Server | 16,724,954 | 16,389,388 | −2.01% |

Five rotated, alternating repetitions on two physical cores, each with 10,000
warmups and 1,000 retained calls, gave these pooled p50 ratios:

| Payload | Legacy before / C++ | Legacy after / C++ | Portable / C++ | Upstream Rust / C++ |
|---|---:|---:|---:|---:|
| Empty | 0.970× | 0.984× | 1.113× | 0.993× |
| 64 B | 0.975× | 0.972× | 1.110× | 1.002× |
| 1 KiB | 0.979× | 0.977× | 1.126× | 1.014× |
| 64 KiB | 0.913× | 0.852× | 1.163× | 1.002× |

A separate five-repetition confirmation with 10,000 retained calls per trial
measured legacy/C++ ratios of **0.984× for empty calls** and **0.896× for 64 KiB**.
Against the previous legacy binary, those medians were 0.7% slower and 2.7%
faster respectively; the shorter run measured 1.5% slower and 6.6% faster.
Small-call latency is effectively unchanged, and the size of the bulk gain
varies. Compilers, tests and profilers were stopped during latency runs. These
are shared-workstation diagnostics, not dedicated-host or remote qualification;
the **0.90× small-call target remains unmet**.

The workspace nextest run exercised 2,238 tests: 2,234 passed initially, four
failed, and seven were skipped. All four failures passed subsequent targeted
rechecks after repairing a stale Kani mutation anchor and the report fixture,
and using CI's Java 17 and auditable Cargo wrapper. The isolated ignored-test
inventory also passed; its cold-build deadline is now 30 minutes and it shares
the bounded tooling test group. No entirely clean full-workspace rerun is claimed.
The updated export-lease Kani proof passed, and its negative control failed at
the intended assertion. RPC rechecks, Clippy, and the release smoke run passed,
including all 70 supported workflow cells and 72 cross-language checks. Raw
trials, profiles, logs, source patch and executable identities are retained under
`target/legacy-optimization/dispatch-20261012/`.

### Tail-latency optimization

Use the longer comparison when evaluating p99. A 1,000-call trial contributes
only about ten observations to its slowest 1%; the existing short CI comparison
is primarily a p50 gate. The tail command keeps 50,000 measured calls after
10,000 warmups in each of five repetitions, rotating and alternating before,
after, upstream Rust and C++ on the same two physical cores:

```sh
CAPNTPROTO_BENCH_PORTABLE=0 cargo run --locked --release \
  --manifest-path benchmarks/rpc/Cargo.toml --bin capntproto-rpc-bench -- \
  compare-tail /path/to/before-tcp /path/to/after-tcp \
  /path/to/capnp-rust /path/to/capnp-reference target/tail-comparison
```

Build all peers with the same ordinary full-LTO release configuration first;
the comparison itself only launches prebuilt peers. The output directory must
be new. Set `CAPNTPROTO_BENCH_PORTABLE=1` to compare portable revisions instead.
Retained evidence includes executable hashes, CPU placement, raw samples,
pooled p50/p95/p99, p99/p50, each trial's p99 and its range, and request rate.
Before/after ratios report both median and tail latency as well as throughput.
No slow samples are trimmed and no p99 acceptance threshold is inferred from a
single run. An unchanged-binary comparison helps expose host variation before
attributing a tail improvement to code.
These are sequential round trips; they do not establish a p99 service-level
objective under concurrent load or a fixed incoming request rate.

The legacy receive loop now resets its work budget on each poll, so waiting for
network input or flow control does not accumulate an unnecessary periodic
yield. Continuously ready input still yields every 32 messages. The regression
checks both transport input interfaces with continuous traffic and intermittent
arrivals. This retains bounded uninterrupted work, consistent with the
[executor's fairness requirements](https://docs.rs/tokio/latest/tokio/runtime/index.html#detailed-runtime-behavior).

Against `e9ff1cc5e`, three full-process Callgrind trials (ordinary full LTO,
100 warmups and 1,000 empty calls) reduced median client instructions from
17,770,777 to 17,744,050 (−0.15%) and server instructions from 16,389,436 to
16,351,858 (−0.23%). The receive-budget regression fails against the original
driver and passes with the fix. Validation passed 551 selected RPC, transport,
flow-control and benchmark tests, plus all 70 release workflow cells and 72
cross-language smoke checks. The six model-trace checks initially lacked their
TLC path; all passed with the pinned Java 17 and TLC configuration. Clippy,
formatting and wiki validation passed.

The unchanged-binary control measured empty-call p99 of 68.7–83.5 µs across its
two labels while p50 stayed at 38.2–38.3 µs. A subsequent full comparison
overlapped another project's multi-core tests and produced millisecond-scale
tails in multiple peers; that run is retained but does not qualify an
improvement or regression. After that competing test process finished, a fresh
five-repetition comparison retained 250,000 calls per payload and peer:

| Payload | Before p50 | After p50 | Before p99 | After p99 | After p99 / p50 |
|---|---:|---:|---:|---:|---:|
| Empty | 38.20 µs | 38.20 µs | 66.28 µs | 73.61 µs | 1.927× |
| 64 B | 38.13 µs | 38.13 µs | 55.73 µs | 62.72 µs | 1.645× |
| 1 KiB | 38.83 µs | 38.69 µs | 57.27 µs | 62.30 µs | 1.610× |
| 64 KiB | 69.70 µs | 69.63 µs | 157.63 µs | 105.32 µs | 1.513× |

Pooled small-call p99 was 8.8–12.5% worse, while 64 KiB p99 was 33.2% better.
Request-rate changes ranged from −2.8% to +1.5%. Per-trial tails still varied
widely: empty-call p99 ranged from 53.9–100.1 µs before and 54.0–177.5 µs after.
The deterministic scheduling correction and instruction reduction are verified;
**a repeatable end-to-end p99 improvement is not established**, and p99 is not
yet close to p50 across the matrix. Dedicated-host measurements are still needed
to distinguish smaller runtime effects from host variation. Evidence is under
`target/legacy-optimization/tail-20261013/`, including the control, untrimmed
comparison and confirmation trials, profiles and validation logs.

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
- Median-latency and request-rate ratios relative to C++ Cap'n Proto, with
  the measured Capntproto TCP driver identified in the captions.

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

## QUIC performance investigation (October 2026)

The initial latency target was Native at **no more than 3× the C++ Cap'n Proto median**
for each payload in the fixed workload above. This compares authenticated QUIC
with plaintext TCP; encryption, peer authentication, congestion control and
pacing remain enabled. Tail latency is reported separately, and loopback results
do not establish WAN latency or concurrent throughput.

The major avoidable costs were in the packet adapter:

1. An unconditional `sleep_until(SendInfo.at)` registered a millisecond-resolution
   Tokio timer even when a packet was already due. Repeating this per packet
   turned small RPCs into millisecond operations. The adapter now waits only for
   future deadlines. A paused-clock regression test checks both cases. This
   follows [Quiche's packet pacing contract](https://docs.quic.tech/quiche/struct.SendInfo.html)
   while accounting for [Tokio timer granularity](https://docs.rs/tokio/latest/tokio/time/fn.sleep_until.html).
2. Flushing after every received datagram emitted ACKs before RPC tasks could
   generate their responses. Bounded receive bursts and a bounded opportunity
   to produce the RPC reply let ACKs share data packets. Tiny-request packet
   sends fell from about two to one per endpoint and request in the local probe.
3. Native stayed at a 1,200-byte send limit despite having a 1,350-byte packet
   buffer. Both adapters now probe that bounded limit with Quiche's PMTUD.
   They retain the smaller working MTU when probes fail.
4. Every outgoing datagram required a separate send syscall and the mobility
   path repeated a local-address query. The established path now uses its known
   address. Linux sends bounded UDP segmentation aggregates; other systems and
   unsupported kernels use individual datagrams. Aggregates preserve packet
   boundaries, source/destination, Quiche's send quantum and the latest pacing
   deadline. Migration probes retain their separate cancellation behavior.
5. Waiting for a response while more request bytes were already buffered added
   four scheduler turns to each partial stream delivery. The adapter now drains
   that input before offering the bounded reply opportunity. Inline delivery
   must also advance FIN and shutdown receipts without waiting for another UDP
   packet; large queued shutdowns and crossed receipts cover that requirement.

The [dedicated confirmation run on `66ece9a51`](https://github.com/DericHuynh/capntproto/actions/runs/37179996233)
contains 80,000 validated timed requests across four protocols, four payloads
and five repetitions, after 10,000 warmup requests per trial. It uses the same
fixed server/client CPU pair for every protocol. Its median ratios meet the
3× target for every payload, confirming the [preceding longer-warmup run](https://github.com/DericHuynh/capntproto/actions/runs/37179451459)
(2.43–2.69× medians). Some tail ratios remain above 3×:

| Payload | Native p50 (µs) | C++ p50 (µs) | p50 ratio | p95 ratio | p99 ratio |
| --- | ---: | ---: | ---: | ---: | ---: |
| 0 B | 70.994 | 28.645 | 2.48× | 2.82× | 2.61× |
| 64 B | 71.058 | 28.715 | 2.47× | 1.90× | 1.84× |
| 1 KiB | 75.189 | 30.775 | 2.44× | 3.61× | 3.17× |
| 64 KiB | 439.405 | 159.986 | 2.75× | 3.04× | 3.18× |

Mean elapsed time per request also stays below 3× in both longer-warmup runs:
2.27–2.85× in the confirmation run and 2.40–2.82× in the preceding run. These
include loop overhead and determine the reported sequential request rate.

Before the timer and packet-loop fixes, [the 100-warmup workload](https://github.com/DericHuynh/capntproto/actions/runs/37167747551)
measured roughly 4.65 ms for an empty Native RPC and 142.69 ms for 64 KiB.
Those are separate dedicated-host runs, not paired samples. Ratios in the table
compare protocols measured on the same host.

Benchmark methodology also needed attention. The original 100-request warmup
lasted only a few milliseconds for small messages, and CPU placement was not
controlled. Individual repetitions showed two latency bands in both Native and
C++, making pooled medians sensitive to how often each protocol landed in each
band. [One early optimized run](https://github.com/DericHuynh/capntproto/actions/runs/37177323633)
met all median targets, but [a follow-up](https://github.com/DericHuynh/capntproto/actions/runs/37177863847)
measured 3.93× at 1 KiB. Affinity alone was insufficient: an
[affinity-controlled repeat](https://github.com/DericHuynh/capntproto/actions/runs/37178808621)
measured 3.48× for empty requests. Longer warmup made C++ repetition medians
more consistent; Native still has occasional slower repetitions. Their remaining
cause is not isolated, and the results do not promise a 3× bound on every request
or host. No slow samples are removed.

Memory copying through the bounded application bridge and per-packet QUIC
cryptography remain visible in CPU profiles. Increasing the stream buffers from
16 to 64 KiB did not materially improve the local large-message median and was
reverted. A receive-offload prototype offered a smaller additional gain than
removing unnecessary scheduler turns; it was not merged. The shipped changes
use the unmodified Quiche crate and keep bounded buffering and backpressure.

### Follow-up target: 1.5×

The next target is at most **1.5× C++ median latency for every payload** with the
same authentication and validated workload. The [first follow-up run](https://github.com/DericHuynh/capntproto/actions/runs/37223461974)
reached 1.81–1.86× for small-call medians and 1.60× at 64 KiB; it does not yet
establish the 1.5× target.

The candidate caches the active socket address until validated migration,
enables ThinLTO and one codegen unit for checkout release builds, and runs all
Rust benchmark application futures as local tasks. Linux sockets now prohibit
IP fragmentation and allow Quiche to probe up to 16 KiB. Larger stream staging
and bounded segmentation batches become useful with that larger packet size.
Real encrypted relay tests cover initial MTU fallback and a silently shrinking
path, including exact transferred bytes and shutdown receipts. Shared-listener
queues have a separate 128 KiB byte budget.

Local ablations rejected custom queue-turn yielding, a local RPC task queue,
and lazy pipeline resolution because they did not improve latency. Local HPET
timing has substantially higher clock-read costs than the dedicated host's
clock; local ratios are diagnostics, not evidence that the target is met.

The [second follow-up run](https://github.com/DericHuynh/capntproto/actions/runs/37225071824)
uses full LTO, eight bounded local reply turns, and Linux
read-only UDP registration with temporary write interest on backpressure. A
syscall trace identified an EPOLLOUT event after each successful UDP send;
removing continuous write interest approximately halved client epoll calls in
the small-call diagnostic. Its median ratios were 1.82×, 1.82×, 1.73×, and
1.56× for 0, 64, 1,024, and 65,536 bytes respectively, still above the target.
A stack-framing experiment and a smaller request arena showed no clear latency
gain and were not retained.

The [third follow-up run](https://github.com/DericHuynh/capntproto/actions/runs/37226839751)
increases QUIC staging and application bridges to 128 KiB:
a 64 KiB payload plus framing otherwise spills over the previous 64 KiB bound.
Local measurements reduced large-message latency by about 21%. Calls that explicitly disable
promise pipelining also avoid allocating a queued answer pipeline and shared
completion future. Early publication, errors, capability ownership, cancellation,
and normal pipelining retain their existing semantics.

| Payload | Native median (µs) | C++ median (µs) | Median ratio |
| --- | ---: | ---: | ---: |
| 0 bytes | 60.012 | 36.991 | 1.62× |
| 64 bytes | 59.645 | 37.002 | 1.61× |
| 1,024 bytes | 62.991 | 39.726 | 1.59× |
| 64 KiB | 264.326 | 182.701 | 1.45× |

Only the large payload meets 1.5× in this run. All 5,000 samples per cell were
retained; the empty-call p95 was 101.390 µs versus C++'s 43.676 µs. This droplet
used a Xeon Platinum 8168, while the preceding run used an 8280. Ratios compare
protocols within one run; changes between those runs cannot isolate the effect
of a code change from host variation. CI passed on Ubuntu, macOS, and Windows.
A later experiment removing two boxed completion futures showed no consistent
local latency improvement and was reverted.

### Current optimization target

The current optimization target is at most 1.2× the pinned C++ median for each
of the four payload sizes, with unchanged authentication, encryption, validation,
warmups, repetitions, and sample retention. This target has not yet been met.
The [first 1.2× campaign](https://github.com/DericHuynh/capntproto/actions/runs/37252316131)
measured commit `311e7d8d5b6955bc61905a2de068bfa212dafbdf` on dedicated Xeon
Platinum 8168 cores with fast `kvm-clock` reads. All five repetitions and 5,000
measured samples per cell were retained; droplet cleanup succeeded.

| Payload | Native p50 (µs) | C++ p50 (µs) | Native / C++ |
| --- | ---: | ---: | ---: |
| Empty | 63.200 | 38.090 | 1.66× |
| 64 B | 65.710 | 47.530 | 1.38× |
| 1 KiB | 64.390 | 64.610 | 1.00× |
| 64 KiB | 266.770 | 194.840 | 1.37× |

Several repetitions were bimodal for both implementations; the apparent 1 KiB
parity is not a stable cross-run guarantee. Native retains TLS encryption and
mutual authentication, while the pinned C++ reference uses plaintext TCP.

That candidate reuses write-queue buffers (discarding exceptional capacities
above 1,024 entries), borrows batches without temporary message/receipt vectors,
and uses stack framing for up to two messages with up to two segments each.
A non-pipelined call receives one initial poll before a background completion
task is allocated; pending calls are always queued for another poll. Native
reliable traffic avoids reading application clocks for absent datagrams and
migrations. Upstream QUIC recovery and transport pacing continue to use their
normal clocks and deadlines.

The [second 1.2× campaign](https://github.com/DericHuynh/capntproto/actions/runs/37254318347)
measured commit `fb9ee230d37d66d99fa41793bc57a06db574d8c0`, which stores single-segment receive ranges inline, keeps one
input cancellation registration per connection with a bounded cooperative read
loop, and generates QUIC packets directly into their final batch buffer. Packet
boundaries, pacing, path changes and congestion quantum still determine flushes.
Disabled pipelines allocate their diagnostic only on use, and dropped pipeline
recipients do not allocate unused result wrappers. A local allocation diagnostic
counted approximately 41 client allocations per empty RPC versus 56 in the prior
main baseline, including setup amortized over 11,000 calls. Server Callgrind
counts fell from 623 million to 574 million instructions for that workload.
These diagnostics are not latency acceptance evidence. Its dedicated results
retained all samples and again missed the target:

| Payload | Native p50 (µs) | C++ p50 (µs) | Native / C++ |
| --- | ---: | ---: | ---: |
| Empty | 57.996 | 37.736 | 1.54× |
| 64 B | 59.102 | 38.312 | 1.54× |
| 1 KiB | 77.517 | 40.695 | 1.90× |
| 64 KiB | 255.865 | 187.048 | 1.37× |

Native per-repetition medians were 57.6–59.0 µs for empty calls, but 60.9–101.3 µs
for 1 KiB. The slower repetitions remain in the result. Cleanup succeeded.

The third candidate transfers outgoing-call permits to the queued write, retaining
admission through partial writes, flush, cancellation, and failure without a
per-call background completion task. Transports without this facility retain
the completion-promise fallback. Sends whose completion is unobserved avoid a
receipt channel; warmed detached batches allocate nothing. The client allocation
diagnostic fell from about 41 to 34 allocations per empty call. The
[third dedicated run](https://github.com/DericHuynh/capntproto/actions/runs/37255751713)
measured commit `bc4acf904581d64a3f3591119cb60f632c7f7581` and confirmed cleanup:

| Payload | Native p50 (µs) | C++ p50 (µs) | Native / C++ |
| --- | ---: | ---: | ---: |
| Empty | 56.87 | 36.67 | 1.55× |
| 64 B | 57.35 | 37.94 | 1.51× |
| 1 KiB | 60.24 | 39.65 | 1.52× |
| 64 KiB | 248.69 | 179.77 | 1.38× |

These results still miss the target. Correctness tests subsequently caught an
early output-closure fence on write failures in that candidate. The fix separates
write-queue completion from transport closure: RPC errors propagate even if
close blocks, while the closure fence waits for close and the network driver
waits for connection handles to disappear. That corrected source requires its
own qualification; the table measures the earlier success path only.

The next candidate removes an allocated task-completion wrapper and replaces
the RPC task set's thread-safe admission channel with a reusable local queue.
The task set already owns non-Send futures. Queue wakeups and canceled-task
destructors run outside mutable borrows, including reentrant callbacks. It also
corrects two-party message allocation: a zero size hint means a bounded 2 KiB
initial segment instead of growing from a zero-word segment. Unhinted calls
use the same initial size, while explicit request hints reserve payload and RPC
envelope space. Small responses consequently avoid unnecessary segmentation.
Local diagnostics count about 29 client allocations per empty RPC, down from
34 before these changes, over 11,000 calls including setup. The
[fourth dedicated run](https://github.com/DericHuynh/capntproto/actions/runs/37258956487)
measured `0ea8e0fc071d40c95d52c4ad5b8445c15f9e0626`, including the corrected
output fence, and confirmed cleanup:

| Payload | Native p50 (µs) | C++ p50 (µs) | Native / C++ |
| --- | ---: | ---: | ---: |
| Empty | 55.27 | 37.34 | 1.48× |
| 64 B | 55.98 | 37.46 | 1.49× |
| 1 KiB | 57.61 | 40.52 | 1.42× |
| 64 KiB | 254.41 | 182.98 | 1.39× |

The target remains unmet. Tail latency was also variable: native p95 values were
92.65, 94.82, 67.95, and 393.69 µs respectively. Every repetition remains in the
report; these results do not establish a consistent improvement at every size.

Further candidates use a bounded local byte stream between native RPC and its
TCP/QUIC drivers, retaining Tokio's cooperative budget and remote receipt fences.
Ordinary server replies construct local result wrappers only for live pipelines;
redirected calls retain their explicit results, and each connection reuses its
call executor. Local server instruction diagnostics fell from approximately
549 million to 540 million instructions over 11,000 empty calls including setup.
These are CPU-work diagnostics, not evidence that the latency target is met.

The [fifth dedicated run](https://github.com/DericHuynh/capntproto/actions/runs/37261038782)
measured the local byte stream at `b7c0b4ea8`, before the result-wrapper changes,
on a Xeon Platinum 8280. All samples were retained and cleanup succeeded:

| Payload | Native p50 (µs) | C++ p50 (µs) | Native / C++ |
| --- | ---: | ---: | ---: |
| Empty | 47.70 | 30.75 | 1.55× |
| 64 B | 69.61 | 31.48 | 2.21× |
| 1 KiB | 50.37 | 35.62 | 1.41× |
| 64 KiB | 225.25 | 187.41 | 1.202× |

This also misses the target, including narrowly at 64 KiB. Native 64-byte
repetition medians split between 47–48 and 70–71 µs; C++ also showed variation
between repetitions. The slower repetitions remain included. A different host
and these distributions prevent attributing all cross-run changes to the patch.

The [sixth dedicated run](https://github.com/DericHuynh/capntproto/actions/runs/37262860181)
at `7b91e576e` includes the result-wrapper, retained-frame, first-segment, and
idle-check changes. Its Xeon Platinum 8280 used `kvm-clock` (23–25 ns clock-read
diagnostics). All five repetitions and all samples were retained; sample
validation and droplet cleanup succeeded:

| Payload | Native p50 (µs) | C++ p50 (µs) | Native / C++ |
| --- | ---: | ---: | ---: |
| Empty | 42.53 | 27.68 | 1.54× |
| 64 B | 42.44 | 27.99 | 1.52× |
| 1 KiB | 45.19 | 30.09 | 1.50× |
| 64 KiB | 213.91 | 156.76 | 1.36× |

The 1.2× target remains unmet at every size. Both implementations were faster
than in the fifth run, so lower absolute native latency alone cannot establish
the patch's benefit across hosts. One native 1-KiB repetition had a 70.21-µs
median; it remains included. These results precede the separate TCP/TLS
vectored-write change, which does not alter the canonical native QUIC workload.

### Lessons from the pinned C++ implementation

The comparison uses upstream commit
[`0de72d8d8cec6b69edaa29de51d3bd490341f9c2`](https://github.com/capnproto/capnproto/tree/0de72d8d8cec6b69edaa29de51d3bd490341f9c2).
The relevant optimizations are specific ownership and scheduling decisions:

| C++ mechanism | Application to the maintained Rust implementation |
| --- | --- |
| `arena.h` / `arena.c++`: inline `segment0`, lazily allocated metadata for other segments | Buffered framing keeps the first range directly. Builders now also keep the first segment's metadata inline, with a separate vector only for additional segments. Bounds and alignment checks remain. An earlier enum-based inline builder experiment increased measured instructions and was rejected; the current representation keeps segment zero directly addressable. |
| `serialize-async.c++`: separately owned retained frames; direct reads for large incomplete frames | Retained Rust frames now own a boxed word slice instead of allocating another `Arc` control block. Short-lived views still share the receive buffer and remain valid after the stream is dropped. Large-frame direct reads already exist. |
| `rpc-twoparty.c++`: `evalLast()` batches related messages into one vectored write and propagates write failures to reads | Rust already batches queued messages, uses stack framing for small batches, and propagates output failure separately from transport-close completion. Native TCP/TLS now also submits its kind, length, and payload together with a vectored write. KJ's end-of-event-queue scheduling is stronger than a fixed number of Tokio yields; this is a remaining scheduling opportunity. |
| `kj/async-inl.h`: `PromiseDisposer::appendPromise()` stores continuation nodes in an existing promise arena | Prefer fusing Rust async continuations and reusing task storage before type erasure. A C++-style raw arena cannot be copied blindly: Rust futures must retain pinning, cancellation and destructor guarantees. |
| `rpc.c++`: `checkIfBecameIdle()` first checks whether protocol tables are empty | Skip allocating and scheduling an idle-check task while an import or export proves the connection is active. Fallible borrows preserve reentrant capability destruction; the last release still requests a deferred check. |
| `rpc.c++`: capability-free successful returns set `noFinishNeeded` and release answer state | Already supported, with explicit exceptions for joins and callee-allocated answer IDs. Errors and redirected responses retain their required pipeline/Finish semantics. |
| `message.c++`: reusable scratch segments clear only their used portion | Existing Rust scratch allocators already provide this. General RPC arena pooling needs bounded retention and ownership through partial writes and retained pipelines; the benchmark must not receive special scratch-only behavior. |

C++ also caches segment pointers in a non-movable reader arena. Rust readers
can move and accept user-provided segment storage, so caching a pointer across
moves would require an additional stable-storage guarantee. The framing change
caches offsets instead and adds no unsafe code.

Another C++ allocator optimization remembers the last segment with available
space instead of scanning every prior segment. Rust's `allocate_anywhere()`
currently uses first-fit scanning. A cached allocation candidate could help
messages with many segments, but it needs fragmented-message and memory-retention
measurements: skipping usable holes can trade CPU savings for larger messages.
This is not a leading cost in the measured small-RPC profile and remains deferred.

Local diagnostics used 10,000 warmups and 1,000 empty calls, including amortized
setup. Client allocation counts were 299,838 for the pinned C++ executable and
308,784 for the Rust candidate (about 27.3 and 28.1 per call). C++ still allocated
more cumulative bytes because its default message arenas are larger. Removing
the retained-frame `Arc` alone increased Rust server instructions from 539.76
million to 541.03 million; the simpler first-segment lookup brought that down
to 537.35 million. This is why an allocation count alone is insufficient to
accept an optimization. The C++ server used 293.17 million instructions in the
same diagnostic, but its plaintext TCP transport omits QUIC recovery and TLS
cryptography. These are instruction/allocation diagnostics, not latency results
or an attribution of the entire performance gap to one layer.

The C++-inspired idle precheck further reduced the Rust client diagnostic to
264,786 allocations (about 24.1 per call) and server instructions to 524.67
million. Idle model replay, both capability/question release orders, reentrant
disconnect cleanup, joins, redirected calls, and output closure checks retain
their original behavior. Dedicated latency qualification is still required.

Following the vectored-write path exposed three separate TLS writes for each
native TCP frame: its kind byte, length, and payload. The TCP bridge now presents
the five-byte stack header and borrowed payload together. Partial writes resume
at the exact byte boundary, and the caller still publishes acknowledgement only
after flush succeeds. In the same 11,000-call local diagnostic, server instructions
fell from 469.66 million to 393.53 million (16.2%) and client allocations fell from
275,492 to 242,491 (about three fewer per call). This diagnostic uses native
TCP/TLS with the same authentication and workload; it does not change the
canonical QUIC/C++ benchmark or establish the QUIC latency target. Targeted tests
cover partial and pending writes, write-zero and flush errors, authenticated RPC,
large byte transfers, crossed receipts, and shutdown.

The 2026-10-05 builder-metadata diagnostic compares optimized binaries before
and after replacing the metadata vector's first entry with an inline field.
Across 10,000 warmups and 1,000 empty native QUIC calls, client allocations fell
from 264,790 to 253,787 (about one fewer per call, 4.2% overall); cumulative
allocated bytes fell from 48,584,039 to 47,703,806. In separate Callgrind runs,
client instructions changed from 531.24 to 529.85 million and server instructions
from 529.06 to 529.86 million. This is an allocation improvement, not a demonstrated
latency or material instruction-count improvement. Unlike the earlier three-way
enum experiment, segment zero stays separate when later segments are allocated.
The tradeoff is larger inline builder metadata. Allocation contracts now require
one allocation for a single-segment heap builder and zero for fitting scratch
storage; moved multi-segment builders, external segments and allocator cleanup
retain their ownership checks. Dedicated latency qualification is still required.

The next local diagnostic adds bounded per-connection segment reuse and avoids
background protection tasks for non-streaming, non-pipelined methods that finish
on their first poll. Pending methods retain their executor ownership; ordinary
local calls remain deferred. A connection retains at most 128 KiB in 16 released
segments, with exact-size reuse and clearing of the used prefix. Against the
inline-builder baseline above, empty-call server instructions fell from 529.86
to 475.29 million (10.3%). Client allocations fell from 253,787 to 242,787 and
cumulative allocated bytes from 47,703,806 to 25,353,757. These are local
instruction/allocation diagnostics; they do not establish the 1.2× latency target.
The memory suite includes pooled multi-segment reuse with simultaneous live
messages under both Miri aliasing models, and cancellation tests exercise an
initially pending protected method and immediate success/error completion.

Combining near-struct offset and complete-target validation into one segment
lookup reduced the same local server profile further, from 475.29 to 466.29
million instructions. It retains alignment, nesting, traversal and complete
bounds validation without caching addresses into movable readers. The malformed
offset cases passed the extended memory suite's 144 interpreted executions,
including both aliasing models and big-endian wire decoding.

The [next dedicated run](https://github.com/DericHuynh/capntproto/actions/runs/37273929820)
measured `9d87134b2` before that pointer change. Measurement and droplet cleanup
succeeded, but its report job lacked the schema compiler and failed before
sample validation. Raw pooled medians were Native/C++ 44.04/39.54, 44.65/31.23,
44.63/33.85 and 163.08/126.57 microseconds at 0/64/1024/65536 bytes, respectively.
Those are diagnostic samples, not a passed quality report or a demonstrated
1.2× result. All five repetitions remain included. The report job now installs
its schema compiler before provisioning a measurement host.

The subsequent QUIC candidate uses upstream `BufFactory` and `stream_send_zc`
with immutable `Bytes` views. Reads are capped at the existing 128-KiB bridge
budget. An acknowledged slab can be reclaimed; a retained retransmission view
prevents mutation, and later small writes consume the unused tail before another
slab is allocated. At most 4 KiB of unused tail is retired per slab. A regression
holds 1,024 outstanding one-byte views and checks that this does not allocate a
128-KiB slab per write. Partial writes, cancellation and EOF preserve their
existing state transitions. The production quiche crate remains unmodified.

Local empty-call diagnostics for this candidate used 471.93 million server
instructions versus 466.29 million before it, while client allocations fell
from 242,787 to 231,787. A three-repetition local comparison showed lower 64-KiB
latency, but the small-message instruction increase is a tradeoff and local
timings do not qualify the 1.2× target.

The [validated dedicated run on `5ec4a320a`](https://github.com/DericHuynh/capntproto/actions/runs/37277836998)
included the pooled arenas, combined struct validation and owned QUIC buffers.
All 80 trials passed report validation, and resource cleanup succeeded. On its
Xeon Platinum 8168 host, the pooled results still miss the target:

| Payload | Native p50 (µs) | C++ p50 (µs) | Native / C++ |
| --- | ---: | ---: | ---: |
| Empty | 53.069 | 37.366 | 1.42× |
| 64 B | 54.513 | 37.527 | 1.45× |
| 1 KiB | 54.864 | 40.502 | 1.35× |
| 64 KiB | 239.210 | 185.883 | 1.29× |

Every repetition remains included, including native empty-call medians ranging
from 50.97 to 87.36 µs. This host differs from earlier runs, so absolute times
across runs are not a paired comparison. The complete workspace nextest run at
this revision passed 1,558 tests with seven documented skips.

The next candidate transfers immutable bytes from the local RPC bridge into
QUIC without copying them into a second staging buffer. Copied adapters retain
the bounded fallback, while partial writes and unacknowledged retransmissions
keep their own immutable views. One-byte retention tests guard against an
allocation per message. It also shares incoming answer flags, returns arena
allocations without a second segment lookup, and follows C++ in leaving empty
capability tables null instead of allocating a list tag.

For 10,000 warmups plus 1,000 empty calls, local server instruction counts fell
from 471.93 to 460.00 million. Five paired local repetitions of the buffer and
arena changes showed only modest latency differences. Direct polling of the
transport on each write regressed latency and was discarded; polling at flush
and replacing RPC oneshots showed insufficient benefit to retain. These are
diagnostics, not a demonstrated 1.2× result. The new candidate passed 291 focused
tests and the unsafe documentation gate before full qualification.

The [validated run on `2ef360212`](https://github.com/DericHuynh/capntproto/actions/runs/37284232525)
retained all 80 trials and confirmed droplet deletion. It used a dedicated Xeon
Platinum 8168 with `kvm-clock`; source identity was
`d63ba30ce055df40f40f208fbf23e0e5b1c454f51373027d19c389f3e429bfd6`.

| Payload | Native p50 (µs) | C++ p50 (µs) | Native / C++ |
| --- | ---: | ---: | ---: |
| Empty | 56.762 | 39.703 | 1.43× |
| 64 B | 58.082 | 39.437 | 1.47× |
| 1 KiB | 59.001 | 42.851 | 1.38× |
| 64 KiB | 234.549 | 191.508 | 1.225× |

The 1.2× target remains unmet at every payload. The full workspace run at this
revision passed 1,559 tests, including extended memory-safety checks, with seven
documented skips and one failure: an allocation-hint assertion still counted
the omitted empty capability-table tag. That expectation needs updating; this
is not a claim that the complete suite passed at this revision.

The following candidate keeps 16 peer IDs inline with a sparse fallback,
retains one framing-state allocation per connection, skips inactive shutdown
reads, and defers unused application clock reads. At `c11475e00`, the full
workspace suite passed all 1,565 tests with seven documented skips; the stale
allocation-hint expectation was corrected. Local empty-call instructions fell
from 460.00 to 446.53 million and five paired runs showed about 3% lower small-call
latency. A larger MTU showed no convincing gain and was discarded.

The subsequent receive-buffer handoff preserves partial admission through the
copying path and transfers whole buffers only into an empty bridge. Reserving
for quiche's readable fragments first prevents buffer growth from reintroducing
the removed copy. For 1,000 warmups and 1,000 64-KiB calls, local server
instructions fell from 2.127 to 1.995 billion; five paired local repetitions had
pooled medians of 176.07 versus 171.60 µs. Small-call latency was essentially
unchanged. The candidate passed 297 focused transport/RPC/model checks, ten
bridge ownership/property checks and the unsafe-documentation gate. These
diagnostics still require a new dedicated acceptance run.

Keeping one native timer/notification registration and reducing two-party RPC
read-ahead to 8 KiB further reduced local server instructions: 447.98 to 437.64
million for 10,000 warmups plus 1,000 empty calls, and 1.995 to 1.861 billion for
1,000 warmups plus 1,000 64-KiB calls. Smaller staging reduces the copied prefix;
message limits and framing validation stay unchanged. A separate large receive
allocation pool showed no meaningful instruction improvement and was rejected.
The combined runtime at `ae8285127` passed all 1,568 workspace nextest checks,
with seven documented skips, including memory-safety and mutation controls.

Version-2 local controls used the same CPU pair, release flags, five repetitions
and full sample retention. Authenticated TCP/TLS measured 45.75/45.96/47.42/161.06
µs at 0/64/1024/65536 bytes; native QUIC in that comparison measured
64.88/64.95/66.77/170.83 µs, and C++ plaintext TCP
38.06/37.99/39.04/76.27 µs. In a separate plaintext two-party Rust RPC control,
Rust/C++ measured 49.52/37.85, 49.03/37.72, 42.88/38.55 and 84.37/76.20 µs.
These diagnostics distinguish the authenticated transport path from serialization
and RPC machinery. The plaintext control omits native session/transport features
and cannot qualify the native QUIC target; these local results are not dedicated
acceptance measurements.

The [first validated version-2 run on `80a470fd6`](https://github.com/DericHuynh/capntproto/actions/runs/37293782939)
used four dedicated `c-4` vCPUs in `nyc3`, an Intel Xeon Platinum 8358 and
`kvm-clock`, at $0.125/hour. Its source fingerprint is
`58ad234aceb9b11239d6ddfdbb918daa6691c9dd0653852991631522892a1b90`.
All 80 trials passed validation, and cleanup confirmed deletion of droplet
`606256499`. Five repetitions contribute every sample to these pooled medians:

| Payload | Native QUIC p50 (µs) | C++ TCP p50 (µs) | Native / C++ |
| --- | ---: | ---: | ---: |
| Empty | 41.803 | 30.895 | 1.353× |
| 64 B | 42.137 | 31.607 | 1.333× |
| 1 KiB | 44.621 | 32.898 | 1.356× |
| 64 KiB | 144.610 | 68.046 | 2.125× |

The target remains unmet for every payload. Compared with this run's baseline,
native medians would need roughly 10–12% further reduction for small calls and
44% at 64 KiB. This host differs from earlier Xeon 8168 runs, and the C++
measurement contract changed; absolute times across those runs are not an
isolated optimization comparison. Native remains authenticated QUIC, while C++
uses plaintext TCP. The narrower local plaintext Rust control does not substitute
for this acceptance result.

The preceding attempt failed before host creation because DigitalOcean rejected
its newly registered SSH key. The development tool now bounds retries for that
exact rejection, checks key ownership and absence of a matching host, and still
refuses retries for ambiguous creation responses. Ten cloud-tool regression
tests passed. The successful follow-up created its host on the first attempt,
so it verifies the ordinary provider path; delayed-key recovery is covered by
the simulated tests.

Allocation checks cover the warmed single-segment queue, and partial-write
tests cover every byte boundary of small frames plus large multi-segment batches.
RPC regression tests cover self-wakes, independent subsequent wakes, late errors,
pipeline publication, cancellation, and shutdown. An absolute-deadline experiment
was rejected because system time and paused Tokio time are distinct clock domains.
Local paired timings varied with other machine activity and cannot establish
the target; dedicated measurements remain the acceptance evidence.

### Investigating the 64-KiB transport cost

The 2026-10-05 version-2 size sweep covered twelve payloads from 8 to 128 KiB,
with three repetitions and every sample retained. Native QUIC's local median
rose from 149.04 µs at 48,000 bytes to 169.85 at 64,000, 170.34 at 65,536 and
175.86 at 66,000. There was no isolated cliff at exactly 65,536 bytes. Larger
steps appeared at packet boundaries near 16 and 32 KiB. Upstream quiche 0.30.0
caps established packets at 16,383 bytes to retain a two-byte packet-length
varint; requesting a larger loopback MTU does not remove this cap.

The pinned C++ implementation provided two useful controls:

- [`rpc-twoparty.c++`](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c%2B%2B/src/capnp/rpc-twoparty.c%2B%2B#L176)
  batches queued messages into `MessageStream::writeMessages`, retaining their
  owners until completion and propagating write failures. Native TCP/TLS now
  similarly drains up to eight queued frames into one vectored batch and one
  flush. Its local bridge also admits 128 KiB: a 64-KiB body plus its RPC envelope
  no longer spills across a 64-KiB capacity boundary. Frame bytes and the receipt
  flush fence retain their original semantics. The sender holds at most eight
  active frames beside its eight queued frames; each data frame remains limited
  to 16 KiB.
- [`serialize-async.c++`](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c%2B%2B/src/capnp/serialize-async.c%2B%2B#L724)
  reads large incomplete frames directly into their final allocation. Rust
  already has that framing path. Instrumenting native receive delivery showed
  over 99.9% of 64-KiB calls arriving at the RPC bridge as one complete chunk.
  An additional owned-chunk API and a receive-allocation pool did not produce
  a repeatable latency gain and were discarded.

Linux UDP receive aggregation now complements the existing segmented sender.
`recvmsg` exposes the original packet boundaries without another payload copy;
the driver splits the aggregate before quiche processing, and the shared
listener splits it before connection-ID routing and per-route admission.
Unsupported kernels and other platforms retain ordinary datagrams. Truncated
data or control metadata is discarded as packet loss, and a burst always
finishes the bounded aggregate already received. Migration enables aggregation
on the newly committed socket. Authentication, encryption, congestion control,
pacing, packet limits and the crates.io quiche source are unchanged.

A 20,000-call, 64-KiB client syscall diagnostic fell from 100,010 successful UDP
reads to 40,023 (about five to two per call). The new run observed 100,002
datagrams in GRO aggregates and no send errors. Clock reads stayed near 461,000;
these are mechanism counts, not timing samples. The first five-repetition local
release comparison measured:

| Transport | Previous p50 (µs) | Batched p50 (µs) | Reduction |
| --- | ---: | ---: | ---: |
| Native TCP/TLS, 64 KiB | 160.076 | 145.060 | 9.4% |
| Native QUIC, 64 KiB | 167.410 | 161.334 | 3.6% |

Each comparison alternated executable order, used the same CPU pair, 10,000
warmups and 1,000 measured calls per repetition, and pooled all 5,000 samples
per row. Every QUIC 64-KiB repetition improved; QUIC small-message pooled medians
varied by less than 1%. These Ryzen 5800H/HPET results are local diagnostics,
not dedicated acceptance evidence. They reduce the large-message cost but do
not establish the 1.2× native/C++ target.

Regression checks cover scalar/vectored partial writes, pending writes and
flush errors, IPv4/IPv6, canceled receives, truncated aggregates, mixed
connection IDs, unknown routes and oversized packets. The initial focused
transport/listener run passed all 99 tests, including the socket and listener
TLA+ replays.

After adding shared-listener support and aligned ancillary storage, a final
five-repetition local release comparison repeated the same experiment and
included the C++ executable. All 60 trials and their samples were retained.
Native QUIC's 64-KiB p50 fell from 168.249 to 158.821 µs (5.6%), p95 from
182.707 to 173.557 and p99 from 194.301 to 184.522. Every paired large-payload
median improved. Empty/64-byte/1-KiB pooled medians changed by +0.4%/+1.0%/+2.1%,
respectively: the small-message cost of the new receive path is a tradeoff.
C++'s 64-KiB median in this experiment was 76.966 µs, so the final local native
ratio was still 2.064×. These measurements ran after builds and tests exited.

The [dedicated run on `eb2d04927`](https://github.com/DericHuynh/capntproto/actions/runs/37318535864)
validated all 80 trials and confirmed deletion of droplet `606308934`. It used
four dedicated `c-4` vCPUs in `nyc1` at $0.125/hour, a Xeon Platinum 8280 and
`kvm-clock`. Source fingerprint:
`6ebfc3515cb5c4ad6d8b9347f515d5c6e4607ecfddf517e9d682281e30c0a8d0`.

| Payload | Native QUIC p50 (µs) | C++ TCP p50 (µs) | Native / C++ |
| --- | ---: | ---: | ---: |
| Empty | 54.064 | 36.011 | 1.501× |
| 64 B | 52.615 | 35.660 | 1.475× |
| 1 KiB | 55.742 | 40.214 | 1.386× |
| 64 KiB | 207.927 | 85.553 | 2.430× |

The target remains unmet. This host differs from the preceding Xeon 8358, so
those two runs are not a controlled before/after measurement of batching.
All five repetitions remain included, including native 64-KiB medians from
202.744 to 224.804 µs. The paired local comparisons isolate the change; this
dedicated comparison measures the remaining gap against C++ on the new host.

The full workspace nextest run completed 1,574 tests: 1,572 passed, seven
documented tests were skipped, and two checks required correction or rerun.
A compiler CLI test reused a fixture path from another worktree while inheriting
the current directory as its source prefix; it now explicitly uses the fixture
root. The mutation guard correctly invalidated its run after that test edit.
With the tree held unchanged, all nine compiler tests and both qualification
checks passed. Clippy and the unsafe-documentation gate passed; the workspace
run also passed Miri, native fuzz smoke, C++ interop and optimized runtime checks.

### Profiling and owned TCP frame buffers

A 2026-10-05 follow-up profiled the release client and counted copies at their
call sites. About 24% of sampled user cycles were in AES-GCM encryption and
decryption; memory copies were another substantial cost. Profiling itself
increased elapsed time, so these samples identify work rather than establish
latency. The C++ control remains plaintext TCP. No encryption, authentication,
congestion control, pacing or deadline checks were removed.

For 2,000 native QUIC calls at 64 KiB, the copy counter identified approximately
131 MB at each of the application payload setter, local send bridge, quiche
send emission, quiche receive-frame ownership, quiche receive emission and
local RPC reader. It also observed two full-payload clearing paths, the smaller
framing-prefix copy, and about 32.8 MB of UDP batch-boundary copies. The earlier
owned-reader experiment did remove its intended copy and clearing work, but
that did not translate into a repeatable latency improvement. This confirms
that removing a copy alone is insufficient evidence to keep a change.

A new QUIC experiment retained the packet at a batch boundary instead of
moving it, at the cost of another packet of buffer capacity. The counter
confirmed removal of that roughly 16-KiB copy per call. However, seven alternating
release repetitions measured 173.765 µs before and 177.118 µs after at 64 KiB.
Background compilation made those timings provisional; they did not justify
the added complexity, and the experiment was discarded. All 84 trials and
their samples were retained locally, including slow repetitions.

The retained change applies C++'s message-ownership pattern to native TCP/TLS.
The bridge splits bounded owned chunks into views of at most 16 KiB, retaining
them until the vectored batch finishes. It no longer copies outgoing data
through a stack buffer and a fresh vector for each frame. The queue and active
batch still each hold at most eight frames, and the producer retains at most
one additional 128-KiB chunk. Views can retain their backing allocations until
the batch completes. Receive framing reuses initialized storage, limits each
read to the validated frame length, and transfers complete buffers into an
empty local pipe. A full pipe still uses bounded partial admission. Receipt
acknowledgements still require actual delivery and successful output flush.

Two local release comparisons each used five alternating repetitions, CPUs 0
and 2, 10,000 warmups and 1,000 measured calls at every canonical payload size:

| Native TCP/TLS, 64 KiB | Previous p50 (µs) | Owned-frame p50 (µs) | Reduction |
| --- | ---: | ---: | ---: |
| First comparison | 156.445 | 149.181 | 4.6% |
| Repeat comparison | 160.286 | 154.838 | 3.4% |

Pooling all 10,000 samples per payload gives 157.981 → 151.906 µs at 64 KiB
(3.8%); pooled small-message medians change by +0.6% / +0.3% / −0.7% at
0 / 64 / 1024 bytes. Tail latency varied substantially with background load;
these are local diagnostics, not dedicated acceptance results. The copy counter
separately confirms removal of both outgoing staging copies. This TCP improvement
does not change the canonical native QUIC/C++ acceptance ratio. The last dedicated
64-KiB result remains 2.430×, and the 1.2× target is still unmet.

The retained change passed 138 selected nextest checks covering transport
framing, local buffer ownership, blocked-receiver receipt delivery, native vats,
TLS/mTLS, capability pipelining, shutdown and transport TLA+ trace replay.
Workspace Clippy with warnings denied, the unsafe-documentation gate and all
98 project-document checks also passed. This was a focused regression run,
not a new complete-workspace qualification.

### Queue clock and executor tracing

Further release tracing on 2026-10-05 found that the bounded executor drive
already stops after two turns on ordinary server deliveries and three on client
deliveries. Of 10,000 observed 64-KiB receive deliveries, 48 server and ten client
deliveries reached the eight-turn limit without producing output. Reducing that
limit would change scheduling for the exceptional cases, rather than remove
eight turns from every call; no scheduling change was retained. A separate
8-KiB → 2-KiB initial reader-buffer experiment did not improve large-message
latency (163.917 → 164.896 µs over five alternating repetitions) and was discarded.

The retained queue change avoids sampling the diagnostic clock for every message
when only the oldest timestamp in a pending batch is needed. A relaxed atomic
hint allows producers to skip that read; the existing locks remain authoritative.
If the receiver has drained the queue, the producer releases both locks, samples
the clock and retries. User clocks can still reenter the queue. Concurrent
producers may take redundant samples, but cannot leave an accepted batch without
its timestamp. FIFO order, active-batch metrics, admission and flush receipts are
unchanged. Pinned C++ queue-age and batch-boundary comparisons still pass.

A separate native QUIC diagnostic enqueued 32 independent echo RPCs before
awaiting all responses. Every sequence and payload was validated, using the
same authenticated transport and ten-second batch deadline. Each latency sample
measured completion of the entire batch. Its measurement version was `20032`,
so the canonical version-2 acceptance validator rejects these samples.

| Payload per call / experiment | Previous batch p50 (µs) | New batch p50 (µs) | Change |
| --- | ---: | ---: | ---: |
| 64 B / initial comparison | 270.985 | 171.740 | −36.6% |
| 64 B / independent repeat | 268.753 | 166.504 | −38.0% |
| 64 KiB / initial comparison | 3086.078 | 3102.701 | +0.5% |

Each comparison used five alternating release repetitions, 1,000 warmup batches
and 1,000 measured batches per repetition, on CPUs 0 and 2 of the local Ryzen
5800H with HPET. All samples were retained, and builds and tests were stopped
during latency runs. The 64-byte repeat improved in every paired repetition.
A separate client counter over 2,000 batches recorded 90,161 → 28,121 clock reads:
about 31 fewer per batch, confirming the intended mechanism. The slow local HPET
clock magnifies the benefit; this result must not be projected onto the dedicated
host's much faster `kvm-clock` without measuring it.

Sequential RPCs do not amortize queue timestamps: the 20,000-call 64-KiB counter
remained near 461,000 clock reads before and after. Initial five- and
seven-repetition sequential comparisons showed approximately 1.6% and 2.0%
higher 64-KiB medians, respectively. These results are retained along with the
batch gains, rather than treated as progress toward the sequential target.
A rebuilt control was byte-for-byte identical to the original executable.
Nine further alternating repetitions increased measured calls to 10,000 after
10,000 warmups, retaining all 90,000 samples per executable. Pooled medians
were 165.525 → 164.617 µs (−0.5%), while p95 rose 205.544 → 214.065 µs and
p99 rose 265.608 → 290.472 µs. The varying results establish neither a reliable
sequential gain nor uniformly improved tails. This change is retained for the
repeatable small-batch benefit; dedicated qualification is still needed.

The change passed 69 selected nextest checks, including concurrent producer
FIFO, partial writes, cancellation, reentrant clocks, allocations, admission,
TCP/TLS and QUIC native vats, C++ comparisons and TLA+ trace replay. Workspace
Clippy with warnings denied, the unsafe-documentation gate, formatting and all
98 project-document checks also passed. This is a focused regression run. The
last dedicated canonical 64-KiB ratio remains 2.430×; the 1.2× native QUIC/C++
target is still unmet.

### Packet ceilings and inline framing metadata

A follow-up raised the configured Linux packet ceiling from 16 KiB to 65,507
bytes as an experiment. The 20,000-call client counter still observed about
100,000 QUIC datagrams in 40,000 successful UDP reads. Upstream quiche 0.30.0
[caps established data packets at 16,383 bytes](https://docs.rs/quiche/0.30.0/src/quiche/lib.rs.html#6751-6755)
regardless of a larger configured ceiling. Five alternating repetitions changed
64-KiB p50 from 165.664 to 164.546 µs without changing the packet-count mechanism;
the experiment was discarded. The production packet bounds and upstream quiche
implementation remain unchanged.

The retained framing change stores the second segment's end offset inline.
Frames with three or more segments build their validated ranges directly into
an immutable allocation. The metadata structure remains 48 bytes on the tested
x86-64 build. An allocation regression demonstrated the old two-segment path's
one 32-byte metadata allocation and now requires zero metadata allocations for
both shared and retained messages. Retained messages still allocate their own
payload storage; short-lived views retain their existing lifetime checks.

The initial representation reduced local 64-KiB p50 from 167.899 to 166.153 µs
but enlarged the frame metadata. The final immutable representation preserves
its original size. Seven alternating release repetitions on the same Ryzen
5800H/HPET host used CPUs 0 and 2, 10,000 warmups and 1,000 measured calls:

| Payload | Previous p50 (µs) | Inline framing p50 (µs) |
| --- | ---: | ---: |
| Empty | 67.397 | 67.188 |
| 64 B | 67.676 | 68.096 |
| 1 KiB | 69.423 | 69.771 |
| 64 KiB | 166.502 | 165.594 |

All 56 trials and all samples were retained. Small-message medians changed by
less than 0.7%; the 64-KiB reduction was 0.5%, with p95/p99 increasing from
204.496/248.007 to 206.312/264.210 µs. The allocation reduction is established;
these timing differences are too small and variable to establish a reliable
end-to-end speedup.

A longer nine-repetition comparison used 10,000 warmups and 10,000 measured
64-KiB calls per repetition, retaining all 90,000 samples per executable.
Pooled p50 changed from 166.991 to 166.781 µs (−0.1%); p95/p99 changed from
232.781/434.205 to 223.353/378.052 µs. Tail results vary between experiments.
No builds or tests ran during these timing comparisons, but this remains a
shared local host rather than dedicated acceptance evidence.

All 68 focused nextest checks passed, including allocation budgets, maximum
segment tables, fragmented/truncated inputs, traversal limits, cancellation,
retained capabilities, native TCP/TLS and QUIC vats, C++ comparisons and TLA+
replays. A property test compares the buffered parser against the independent
synchronous decoder across randomized segment contents, read chunks, frame
truncations and traversal budgets. Workspace Clippy also passed with warnings
denied, along with formatting, the unsafe-documentation gate and all 98 project
document checks. This was a focused regression run. The last dedicated native
QUIC/C++ 64-KiB ratio remains 2.430×, and the 1.2× target remains unmet.

### C++ call-context comparison and rejected allocation experiment

The pinned C++ implementation keeps cancellation and response ownership in one
[RpcCallContext](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c%2B%2B/src/capnp/rpc.c%2B%2B#L3159)
and allocates its response through `getResults()` when needed. The October 5
comparison also checked its batched outgoing writes, short-lived receive-buffer
views, direct reads for large messages and capability-free `noFinishNeeded`
returns. Those optimizations already have counterparts in this runtime.

An experimental Rust call context combined the separate return guard and
retained-results allocation. It kept protected application owners separate from
pipeline owners, preserved cancellation Return-before-capability-release
ordering, and released capabilities outside internal borrows. Its first form
removed one allocation and about 32 requested heap bytes per server call. A
refinement boxed retained results only when a protected method outlived its
Results owner, increasing the saving to about 152 bytes per ordinary call.
The allocator counter measured 253,753 allocations before and 242,753 after
11,000 calls, including connection setup in both totals. These are allocation
traffic measurements, not peak-memory or latency savings.

The refined candidate passed 94 focused nextest checks, including cancellation,
early result release, third-party answer adoption, admission, allocations,
native vats, TLA+ trace replays and installed C++ interoperability. Workspace
Clippy with warnings denied, formatting and the unsafe-documentation gate also
passed. A new lifetime test covered multiple protected owners, surviving
pipeline owners and reentrant result destruction. Correctness checks did not
establish a performance improvement.

Seven alternating release repetitions on the Ryzen 5800H/HPET host used CPUs 0
and 2, 10,000 warmups and 1,000 measured calls for each payload and executable.
All 112 trials and their samples were retained. The diagnostic harness checked
each executable's protocol identity and measurement version 2, including bulk
payload validation, response cleanup and per-call deadlines. Pooled p50 values:

| Payload | C++ plaintext TCP (µs) | Rust plaintext TCP, candidate (µs) | Previous native QUIC (µs) | Candidate native QUIC (µs) |
| --- | ---: | ---: | ---: | ---: |
| Empty | 38.902 | 49.936 | 67.886 | 68.165 |
| 64 B | 38.971 | 49.658 | 68.235 | 68.236 |
| 1 KiB | 39.600 | 41.416 | 69.981 | 70.330 |
| 64 KiB | 81.994 | 89.258 | 166.014 | 168.388 |

A separate nine-repetition check retained 90,000 measured 64-KiB samples per
native executable after 10,000 warmups per trial. The refined candidate changed
p50 from 165.664 to 167.899 µs, a 1.35% regression, consistent with the shorter
comparison's 1.43% regression. Its p95/p99 were lower, 212.318/348.509 versus
229.429/438.534 µs, but tails varied across experiments. The earlier inline
form was essentially neutral in its longer check: 168.457 versus 168.807 µs.
Neither form established a latency win, so neither ownership change was
retained in the shipped runtime. No builds or tests ran during the final timing
comparisons; this remains a shared local host, not dedicated acceptance evidence.

The matched plaintext transport narrows the Rust/C++ gap to 1.046× at 1 KiB and
1.089× at 64 KiB, while small calls remain about 1.28×. Native QUIC in the same
experiment is 1.75–2.05× C++ plaintext TCP. This supports looking beyond RPC
ownership for the larger native gap: authenticated transport, packet protection,
packet processing, session bridges and executor work are absent or different
in the plaintext control. The comparison does not isolate encryption alone.
Copying a C++ allocation strategy is therefore a hypothesis to measure, not an
assurance of equal end-to-end performance. The 1.2× native target remains unmet;
the last dedicated canonical 64-KiB ratio remains 2.430×.

### Matched TCP/TLS and QUIC architecture comparison

The `native_tcp` benchmark now uses the same native `Network`, echo service,
pinned mutual authentication, payload validation, response cleanup and version-2
timing contract as `native`. It selects `transport::tcp` rather than the QUIC
adapter. Build and run the standalone diagnostic with:

```sh
cargo bench --locked --manifest-path benchmarks/rpc/Cargo.toml --bench native_tcp
```

Its protocol identity is `native-tcp`. It is deliberately outside the canonical
four-protocol bundle and QUIC/C++ acceptance gate; TCP results cannot establish
the QUIC target. As with other individual targets, this command retains the
caller's CPU affinity. The comparison below instead pins server/client to
physical cores 0/2 and alternates executable order.

On October 5, seven release/full-LTO repetitions per payload, each with 10,000
warmups and 1,000 retained samples, compared the published QUIC runtime, the
final idle-bulk-path changes and the authenticated TCP control. Setup and handshakes are excluded. The local
Ryzen 7 5800H uses HPET; these are local diagnostics, not dedicated-host results.

| Payload | TCP/TLS p50 (µs) | QUIC before (µs) | QUIC after (µs) | After / TCP |
| --- | ---: | ---: | ---: | ---: |
| 0 bytes | 47.283 | 67.886 | 66.908 | 1.415× |
| 64 bytes | 47.423 | 68.165 | 66.908 | 1.411× |
| 1 KiB | 49.238 | 69.702 | 69.003 | 1.401× |
| 64 KiB | 146.667 | 163.918 | 163.010 | 1.111× |

The small-call median reduction is about 1.0–1.8%. The 0.6% large-call difference
is small relative to local run variation. Every sample is retained; no best-run
selection or outlier removal is used.
A separate nine-repetition 64-KiB confirmation, with 10,000 warmups and 10,000
measurements per repetition, retained 90,000 samples per executable. Its median
was 164.476 µs before and 164.058 µs after (0.25% lower), supporting a neutral
large-call result rather than a material speedup. p95/p99 were
201.144/297.455 µs before and 190.109/218.674 µs after; tail variation remains
too large to treat those tail differences as an established gain.

Both paths use the local RPC byte bridge. QUIC additionally drives packet
protection, recovery, pacing, stream state and UDP readiness in user space.
A separate syscall counter over 20,000 64-KiB calls counted 461,840 clock reads
for QUIC versus 218,937 for TCP/TLS, including setup. This is a count of work,
not a division of wall time into costs. The earlier CPU profile also identified
packet encryption/decryption and payload copies as substantial user CPU costs.

Three proposed adapter changes were evaluated and rejected: using the live task
waker for every bridge probe, caching the absolute recovery deadline (including
a variant with a single application-deadline timer), and replacing Linux's
temporary receive-readiness future with its single-reader waker slot. The
recovery deadline changed too frequently in sequential RPC to reduce clock
reads. The reader-slot variant improved small-call medians by 0.8–1.2%, but a
separate nine-repetition 64-KiB run with 90,000 samples per executable regressed
164.058 → 166.292 µs (1.36%). None of those three prototypes is retained.

The bulk driver now keeps a small idle check separate from its full handler.
After initialization, if there are no queued grants, active transfers, partial
prefaces or readable streams, it avoids constructing a readable-stream snapshot
or entering the stream-processing/initialization stack frame. Pending grants
still run before the peer sends data, and existing transfers retain deadline,
cancellation, flow-credit and receipt processing. The allocation-only prototype
had regressed the 64-KiB median by 2.6%; separating the idle path removed that
regression in the final comparison above.

The allocation optimization targets active split-plane traffic: Quiche already
returns an owned snapshot from `readable()`, so the bulk driver now iterates that
snapshot without collecting a second `Vec`. Draining pending grants also keeps
the bounded admission queue's capacity. A full engine regression holds an
authorized bulk stream unread, confirms data remains readable in Quiche, then
drives 100 further engine passes. The former code allocated 100 times / 3,200
bytes; the new code allocates zero times. Resuming the reader delivers the exact
payload, demonstrating that the optimization does not consume or lose readiness.
This counter measures allocation traffic under bulk backpressure, not bulk
throughput. The sequential timings above measure the combined changes.

The timer investigation also found a correctness issue: the adapter's combined
timer can wake for migration polling or an advanced Tokio clock before Quiche's
system-clock recovery deadline. Such a wake previously updated the no-ACK
observation and could spuriously revalidate the MTU. A separate, retained guard
checks that Quiche's timer has actually expired before recording recovery or
requesting a reprobe. The regression fails on the former implementation before
even sending a packet; genuine loss and silent path-MTU reduction still exercise
the encrypted recovery tests. This guard adds no work to ordinary packet events.

The final source passed 158 focused nextest checks covering transport engines,
real and simulated packet faults, bulk services and grants, TCP/QUIC native vats,
TLS/mTLS, migration, MTU fallback and TLA+ trace replay. The benchmark harness's
three checks, workspace and benchmark Clippy with warnings denied, formatting,
unsafe documentation checks and validation of all 98 project documents passed.

Ordinary RPC remains ordered on stream 0. QUIC's cross-stream loss isolation
benefits independent streams, as described by [RFC 9000 §13](https://www.rfc-editor.org/rfc/rfc9000.html#section-13).
The [split-plane API](Split-Plane.md) exercises that architecture for explicitly
granted bulk traffic while retaining control credit. A loss-free, single-call
echo does not exercise it. Existing encrypted packet tests cover control
progress across a bulk packet gap, independent stream credits, packet loss,
reordering, duplicates and stalled consumers. Those correctness tests do not
establish a throughput advantage. The 1.2× QUIC/C++ target remains unmet.

### Clock diagnostics

The report records current and available Linux clocksources and five batches of
100,000 `std::time::Instant` reads on each assigned benchmark CPU. The reported
nanoseconds per read include loop overhead and are diagnostics only; they are
never subtracted from latency samples. The `clock-reads` driver command also runs
this probe independently.

Use the OS monotonic clock for benchmarks, QUIC timers, pacing, and deadlines.
On supported systems it uses a fast TSC-backed path without application assembly.
The dedicated host currently selects `kvm-clock`, which applies virtualization
offsets and multipliers to TSC. An invariant TSC rate alone does not establish
cross-CPU synchronization or migration safety; see the
[Linux timekeeping documentation](https://cdn.kernel.org/doc/html/latest/virt/kvm/x86/timekeeping.html).
Do not force a TSC source excluded by the kernel or disable its reliability
checks just to improve a measurement. Local HPET clock reads measured roughly
1.4–1.6 microseconds in October 2026, so local timing can magnify clock-heavy
transport paths relative to the dedicated host.

The third QUIC follow-up run measured 26–31 ns per `Instant` read on both assigned
droplet cores using `kvm-clock` (TSC was also listed as available). Its clock is
already fast; raw TSC cannot account for the remaining several-microsecond gap
to the small-payload target. No clocksource settings were changed.

### Congestion control and submillisecond pacing

The pinned [quiche 0.30.0 congestion-control API](https://docs.rs/quiche/0.30.0/quiche/enum.CongestionControlAlgorithm.html)
provides Reno, CUBIC and `Bbr2Gcongestion`. CUBIC remains the production default.
Algorithm experiments change the selected upstream controller on both peers;
they do not modify quiche, disable encryption, increase the congestion window,
or bypass pacing. The pinned backend exposes no separate BBRv3 controller or
public custom-controller hook. Its `Bbr2Gcongestion` implementation includes
[some BBRv3 parameter updates](https://github.com/cloudflare/quiche/blob/0.30.0/quiche/src/recovery/gcongestion/bbr2.rs),
but these trials retain the upstream BBRv2 label. BBRv3 trials will wait for
official upstream support; the project keeps the unmodified quiche dependency.

The experiments exposed a pacing problem in the adapter:
[Tokio's sleep timer has millisecond granularity](https://docs.rs/tokio/latest/tokio/time/fn.sleep_until.html).
Waiting for a short future `SendInfo.at` could therefore add a millisecond-scale
pause. The BBRv2 comparison was particularly sensitive to this delay. Expired deadlines already
had a fast path; the missing piece was waiting accurately for future deadlines.

Real Linux sockets now lazily create one nonblocking, close-on-exec timerfd per
connection, use the OS monotonic clock, and reuse the descriptor. The fallback
runtime timer is also retained outside the packet driver's future. Already-due
packets allocate no timer resources. Creation, arming or reactor failures select
the portable timer. Simulated sockets use their virtual-clock timer, including
simulations with no OS I/O reactor. A kernel wakeup cannot advance a paused
runtime clock, and stale readiness from a canceled wait cannot release the next
packet early. Other platforms retain the portable timer.

The pinned C++ KJ event port also lazily creates and reuses a monotonic timerfd
in [`updateNextTimerEvent()`](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c%2B%2B/src/kj/async-unix.c%2B%2B#L785).
That is a useful implementation reference, not evidence that the plaintext
C++ benchmark measures QUIC pacing. The adapter preserves each batch's latest
upstream pacing deadline and congestion quantum. It adds no busy-wait loop,
clock-source override, kernel configuration requirement or unsafe Rust.

Seven release/full-LTO repetitions per payload on the local Ryzen 7 5800H
(HPET clock) compared the old adapter, the timer-reuse implementation, upstream
BBRv2 with timer reuse, and authenticated TCP/TLS. Server/client used physical
cores 0/2, executable order alternated, and each trial kept 1,000 samples after
10,000 warmups. The 112 trials retained all 112,000 measurements. These are
sequential loopback diagnostics, not a controlled wide-area congestion test or
the dedicated QUIC/C++ acceptance run.

| Payload | Old CUBIC p50 (µs) | Timer-reuse CUBIC p50 (µs) | Timer-reuse BBRv2 p50 (µs) | TCP/TLS p50 (µs) |
| --- | ---: | ---: | ---: | ---: |
| 0 bytes | 68.305 | 67.676 | 156.444 | 47.283 |
| 64 bytes | 69.073 | 67.816 | 172.648 | 47.213 |
| 1 KiB | 70.469 | 70.051 | 207.568 | 49.378 |
| 64 KiB | 170.552 | 167.969 | 459.486 | 151.137 |

The CUBIC median differences are small (0.6–1.8%). The 64-KiB p95/p99 changed
from 265.187/564.737 µs to 230.407/432.806 µs, but tail variation is substantial.
BBRv2 remains slower than CUBIC here even with the more accurate timer. An earlier
prototype recreated a combined waiting future on every paced send and regressed
the CUBIC 64-KiB median by 2.3%; retaining the fallback timer outside the packet
loop's future removed that regression in this comparison.
Raw trials, executable hashes, CPU topology and clock source are retained in
`target/quic-ci-algorithms/pacing-reuse-comparison/` during local development.

A longer 64-KiB confirmation used nine repetitions with 10,000 retained samples
after 10,000 warmups per executable and repetition (180,000 samples total).
The old/new CUBIC medians were 173.695/172.927 µs, a 0.4% difference; p95 was
324.622/321.479 µs and p99 was 721.950/783.968 µs. This supports treating CUBIC
performance as effectively unchanged, without claiming a tail-latency benefit.
The complete confirmation is in `target/quic-ci-algorithms/pacing-reuse-long/`.

A separate paired BBRv2 confirmation compared the old and retained adapters,
with three repetitions, the same 10,000 warmups and 1,000 retained samples per
payload and executable (24,000 samples total). Both peers used upstream
`Bbr2Gcongestion` in both builds. Median latency changed as follows:

| Payload | Old BBRv2 p50 (µs) | Timer-reuse BBRv2 p50 (µs) |
| --- | ---: | ---: |
| 0 bytes | 2,314.471 | 144.572 |
| 64 bytes | 2,313.911 | 168.247 |
| 1 KiB | 2,326.483 | 205.263 |
| 64 KiB | 6,778.166 | 454.946 |

The 11–16× improvement isolates the adapter's coarse timer as a major obstacle
to these BBRv2 trials. It does not establish an advantage over CUBIC or predict
behavior under network congestion. Complete measurements and executable hashes
are in `target/quic-ci-algorithms/bbr2-reuse-confirmation/`.

The research points to the following evaluation order:

| Candidate | Decision and next evidence needed |
| --- | --- |
| CUBIC | Keep as default; measure packet-loop changes against it. [RFC 9438](https://www.rfc-editor.org/rfc/rfc9438.html) describes its window growth and behavior over high-bandwidth, long-delay paths. |
| Reno | Useful as a simpler loss-based comparison. A loopback latency result alone cannot establish its suitability for large bandwidth-delay products. |
| Upstream BBRv2 | Evaluate with accurate pacing, then controlled RTT, bottleneck rate, loss, competing flows and application-limited traffic. Sequential loopback results do not justify changing the default. [RFC 9743](https://www.rfc-editor.org/rfc/rfc9743.html) describes the broader congestion-control assessment needed. |
| Deficit round robin for bulk producers | Consider only after measuring contention between independent transfers. [DRR](https://repository.library.washu.edu/cse_research/339/) accounts for service in bytes, but it does not itself solve ownership of unconsumed flow credit. Retain the per-transfer caps: lending a stalled transfer every unused slot's credit could block later transfers indefinitely. |
| Kernel timestamp pacing with FQ/ETF | A separate, optional experiment for hosts with suitable queue configuration. [QUIC Steps](https://arxiv.org/html/2505.09222v1) evaluates interactions between pacing, GSO and queue disciplines. Timerfd improves wakeup precision without requiring those host changes; it is not hardware transmit-time scheduling. |

Every controller must retain congestion response, authenticated transport,
bounded buffering and [QUIC's pacing requirements](https://www.rfc-editor.org/rfc/rfc9002.html#section-7.7).
The 1.2× QUIC/C++ target remains unmet.

## TCP default and vectored IO

The legacy acceptance baseline was matched plaintext TCP at no more than 1.15×
the pinned C++ implementation. The opt-in portable migration now uses a 1.5×
small-call gate, described above. `rpc::connect` selects TCP, and native
directory routes select authenticated TCP/TLS. QUIC remains an explicit option;
the historical QUIC comparisons above do not qualify this plaintext baseline.

The main TCP regression was at the Tokio-to-futures IO boundary. The write queue
already batches framing headers and message segments, but `tokio_util::compat`
does not forward vectored writes. Its scalar fallback split the batch into
separate sends and introduced additional readiness wakeups. The shared adapter
now forwards `poll_write_vectored`, including partial progress and `Pending`, to
TCP, TLS and QUIC writers. It preserves flush, shutdown and error propagation.
Fault-injection tests exercise short writes across segment boundaries,
backpressure and shutdown failure without relying on real-socket timing.

A local release/full-LTO comparison on the Ryzen 7 5800H used server/client
CPUs 0/2 on different physical cores, five alternating repetitions, 10,000 warmups and 1,000
retained calls per executable, repetition and payload. All 40,000 timed samples
were retained, with matched payload validation, response cleanup and per-call
deadlines. Compilers and test campaigns were stopped during measurement. This
host uses HPET; `Instant` reads cost about 1.4 µs, making redundant runtime
wakeups particularly expensive.

| Payload | Legacy Capntproto TCP p50 (µs) | C++ TCP p50 (µs) | p50 ratio | p95 ratio | p99 ratio |
| --- | ---: | ---: | ---: | ---: | ---: |
| 0 bytes | 39.740 | 37.784 | 1.052× | 1.017× | 1.014× |
| 64 bytes | 39.671 | 37.645 | 1.054× | 1.085× | 1.052× |
| 1 KiB | 40.438 | 38.692 | 1.045× | 1.003× | 1.018× |
| 64 KiB | 69.981 | 75.220 | 0.930× | 0.934× | 0.945× |

Every pooled median and every individual repetition's median passed the 1.15×
gate. Tails also passed in this run, though the gate uses pooled p50. Raw trials,
the pass/fail summary, executable hashes and CPU/clock measurements are retained
locally in `target/tcp-portability/acceptance-confirmation/`. The earlier
`vectored-comparison/` run also passed every payload. The C++ peer was built
in release mode from pinned revision
`0de72d8d8cec6b69edaa29de51d3bd490341f9c2` with the same benchmark schema.
These results qualify this local sequential loopback workload, not encrypted
transport performance, concurrent throughput, other hosts or the incomplete
allocation-free RPC profile.

### Portable workflow gap (October 15, 2026)

[Gap #11](https://github.com/DericHuynh/capntproto/issues/11) targets pooled p50
≤ **1.05 × min(legacy, C++) for each matching workflow**. The
[roadmap baseline](Roadmap.md#portable-rpc-performance-gap) records seven failing
remote workloads from source `2c9548c998fb`. Remote report JSON now includes an
advisory `portable_target` with each cell's faster reference, ratio and decision;
the comparison uses integer cross-products and includes the exact 5% boundary.
Missing or mismatched trials still fail matrix validation before any report.

The first implementation slice keeps direct reply delivery available beyond
eight outstanding calls. A lazily allocated owner index is bounded by admitted
calls, validates the full question key/generation, and removes entries on reply,
cancellation and ordinary completion. Small calls retain their short scan;
capability and queued paths keep their existing protocol handling. Two-segment
prepared Returns now overwrite the complete reusable frame, including initialized
segment-table padding, without clearing the payload immediately before copying
it. Reader limits, capability validation, far pointers and answer authority are
unchanged.

Local complete-process Callgrind diagnostics used ten warmups and fifty retained
workflows with both peers profiled. These totals include startup and setup; they
are neither per-request costs nor latency qualification:

| Workflow / process | Before instructions | After instructions | Change |
| --- | ---: | ---: | ---: |
| fanout-32 / client | 89,380,001 | 78,211,868 | −12.50% |
| fanout-32 / server | 24,650,717 | 25,530,171 | +3.57% |
| bulk-4m / client | 217,241,931 | 217,433,092 | +0.09% |
| bulk-4m / server | 123,907,830 | 110,336,423 | −10.95% |
| callback / client | 7,624,223 | 7,620,045 | −0.05% |
| callback / server | 5,831,077 | 5,834,358 | +0.06% |

The before profile attributes 11.4% of fan-out client instructions directly to
the call scanner, with additional work in repeated `take_output` polling. Copies
and zeroing account for 97.0% of the 4-MiB server's instructions. The new return
path removes one redundant clearing pass; receive-buffer allocation/zeroing,
incoming frame copies and outgoing Call reconstruction remain follow-up work.
Capability-bearing workflows and output batching also remain open.

The same final driver executable ran the empty TCP echo fixture with 100 warmups
and 1,000 retained calls. Portable/legacy totals were **17,705,930 / 17,134,501
client (1.033350×)** and **16,402,479 / 16,044,316 server (1.022324×)**, below the
separate 1.05× instruction limit on both peers. This uses the driver's `server
tcp` and `measure tcp` entry points with the same echo fixture. It does not
supersede the earlier pinned-binary qualification below.

The environment was a shared five-vCPU Linux VM in Debian 13 containers, Rust
1.97.0 with auditable Cargo, ordinary release/fat LTO, one codegen unit and overflow
checks, Valgrind 3.24.0, and Clang 19 for the pinned C++ reference. GCC 14.2 hit its
known internal compiler error building that reference. Local latency runs retain
all twelve workflows, five rotating repetitions, 250 warmups and 1,000 samples,
with server/client processes pinned to CPUs 1/2. Repetitions exhibit substantial
latency shifts; these local results do not close the dedicated two-host gap.

The final latency run also shows why instruction reductions alone are
insufficient: fan-out pooled p50 rose from **227.515 to 283.768 µs (+24.7%)**,
empty unary rose from **20.811 to 23.004 µs (+10.5%)**, and 4-MiB echo changed
from **5,921.544 to 5,992.361 µs (+1.2%)**. Only two of twelve local workflow
cells meet the faster-reference target in that run. Retain this slice as an
optimization candidate, not a demonstrated latency improvement. In particular,
investigate per-reply executor yielding and remaining scans under fan-out before
qualifying the owner-index change. All samples, including regressions, remain in
the local evidence bundle.

First-slice executable SHA-256:
`f7737934a089a190bf3ad1f58609bd62cb32079fed94bc5a571a110b73a206cc`.
Raw local trials, profiles, executable identities and experiment scripts are
retained under `target/portable-performance-20261015/`. Validation passed 253
RPC unit tests, 119 wire unit tests, four remote-report tests and 72 workflow
interoperability smoke cases across portable/legacy combinations and both
portable/C++ directions, including 4-MiB payloads. Clippy, formatting and wiki
checks passed. Full workspace/Miri/formal campaigns and a new dedicated latency
campaign are still required for broader qualification.

The follow-up candidates batch up to eight ready outgoing Calls before flushing
and up to eight incoming frames before yielding. Admission still rotates through
older cancellation/local/pipeline work; controls and shutdown run between reply
frames. Two-segment prepared Calls preserve their validated far pointers while
copying the frame directly, and borrowed receive spills reuse one allocation
within the 8-MiB transport-cache bound described above. Capability-only pending
calls no longer attempt the ineligible direct-return copy path.

Local diagnostics with the same pinned processes show these incremental gains;
each row compares implementations within its own rotating campaign. The burst
campaign used five repetitions and the cache campaign used three, each with 250
warmups and 1,000 retained workflows per trial. They remain separate from remote
qualification, and all trial samples are retained.

| Candidate / workflow | Previous portable p50 (µs) | Candidate p50 (µs) | Faster reference p50 (µs) | Candidate/reference |
| --- | ---: | ---: | ---: | ---: |
| Batching / fanout-32 | 206.915 | 146.897 | 144.934 | 1.014× |
| Spill reuse / bulk-256k | 250.729 | 142.460 | 168.669 | 0.845× |
| Spill reuse / bulk-1m | 895.421 | 535.510 | 685.371 | 0.781× |
| Spill reuse / bulk-4m | 6,346.351 | 4,684.619 | 4,317.088 | 1.085× |

The 4-MiB and capability-workflow gaps remain open pending the complete follow-up
campaign. Regression tests exercise mixed direct/general reply batches, stopping
between frames, cancellation while newer calls wait, changed reader limits,
dirty two-segment destination storage, partial receive reuse and the cache bound.

A full intermediate run exposed an empty-call instruction regression when every
output frame waited for a later flush: portable/legacy rose to approximately
1.146× client and 1.145× server. That prototype was rejected. The current output
stream sends the first frame eagerly and defers only subsequent frames until
flush. The same executable now measures **17,824,838 / 17,136,792 client
(1.040150×)** and **16,423,553 / 16,043,307 server (1.023701×)**. A larger call-lane
quantum was also removed after unfavorable capability-workflow latency results.
The complete intermediate trials remain diagnostic evidence, including misses.

The new comparison harness was exercised locally with an explicit CLI adapter
around the release driver (not the CI bundle). Its four portable/faster-reference
ratios are **1.010×, 1.012×, 1.015× and 1.134×** for 0, 64, 1,024 and 65,536 bytes.
The 64-KiB target is still open. The current executable SHA-256 is
`5d3bbd5f0a5164e4b097d6c9214eff00fd5eb01407cc9b287086668b09919f35`.
Validation passes 256 RPC, 120 wire, 42 futures, nine remote-run/report and
15 benchmark unit tests, all 24 buffered-input integration tests (including the
pinned C++ and TLC checks), and 72 mixed-driver workflow checks. These checks do
not substitute for latency qualification.

The full follow-up local workflow run retains all 240 trials (five rotating
repetitions, 250 warmups and 1,000 samples). Six of twelve cells meet the target.
Fan-out is 0.966× the faster reference, bulk-256k 0.727× and bulk-1m 0.794×.
Misses remain: unary-small 1.340×, dependent-read 1.281×, pipelined-read 1.246×,
callback 1.393×, bulk-64k 1.314× and bulk-4m 1.127×. The small unary cell shows
the host's large timing shifts: individual portable medians alternate between
about 20 and 30 µs, while the independent loopback campaign above measures 1.012×.
Both campaigns remain visible; neither qualifies the dedicated-host target.
Raw evidence and the combined report are retained in
`target/portable-performance-followup-20261015/`.

Dedicated campaigns also run `compare-portable` on the client host after the
existing controls. It rotates portable, legacy and C++ across all four loopback
payloads, explicitly selects the Rust driver for both peers, verifies executable
hashes, and retains five repetitions of 10,000 warmups plus 1,000 samples. Its
separate `portable-loopback/README.md` and `summary.json` apply the same 1.05×
faster-reference target to every payload, including 64 KiB; invalid/missing or
duplicate trials fail validation. Valid misses remain advisory evidence while
the gap is open. Partial results are salvaged before host cleanup on failure.

### Capability and receive-path experiments (October 15, 2026)

The next candidate keeps admitted capability payloads in their immutable frame
with their independently owned hooks. Descriptor validation and binding still
precede sharing, and original capability slot indices remain intact. Fixed
storage still copies the reachable payload. A private one-slot outgoing table
can likewise skip the intermediate payload copy; foreign and larger tables
retain the ordinary collection/remapping path. Tests cover null slots, hook
identity, response and parameter owners surviving their input frame, and pool
storage remaining unavailable while shared bytes survive.

Settled remote calls now consume their driver's result directly even when they
permit pipelining. Their projections use the original queued task or published
question, and ordered Return propagation still precedes result release. Local,
unresolved and third-party calls retain invocation bindings. Tests exercise
projections before and after publication, remote exceptions, late projections,
queued cancellation and late Returns. All 258 RPC tests and 72 mixed-driver
workflow checks pass, together with Clippy. The borrowed-buffer promotion also
passes 43 futures tests and all 24 buffered-input integration checks.

Two other prototypes were rejected: expanding synchronous server dispatch to
multiple active entries, and polling a queued completion binding immediately
after Return. Their full timing and instruction evidence is retained; neither
improved the complete capability comparison. Payload sharing and direct remote
completion alone also do not establish the 1.05× latency target.

The complete rotating local run still passes only six of twelve workflow
cells. Portable/faster-reference p50 ratios are 1.019× (empty), 1.481× (small),
1.488× (1 KiB), 1.305× (fan-out), 1.685× (dependent), 1.345× (pipelined),
0.895× (write/read), 1.312× (callback), 0.964× (64 KiB), 0.710× (256 KiB),
0.845× (1 MiB), and 1.113× (4 MiB). A separate complete four-payload loopback
campaign passes all four cells at 0.659×, 0.905×, 1.032× and 0.994×. Both
campaigns retain every trial; the host still alternates between roughly 20 and
30 µs small-call regimes. These local results do not qualify the dedicated
workflow target. The candidate SHA-256 is
`c8eab04905e72eb873be9eaef823bc5624b0912048d70fedf0fc05b61b91d18d`.
Matched empty-call instructions remain 17,835,521 / 17,147,351 on the client
(1.040×) and 16,434,449 / 16,057,415 on the server (1.023×). All candidate,
rejected-prototype and raw timing evidence is retained in
`target/portable-performance-capabilities-20261015/`.

Dedicated qualification encountered two independent stale-cache failures:
[the first run](https://github.com/DericHuynh/capntproto/actions/runs/38819400269)
found an existing smoke directory, and
[the retry](https://github.com/DericHuynh/capntproto/actions/runs/38820781958)
passed smoke but found an existing remote report directory. Both stopped before
provisioning measurement hosts. Smoke and measurement evidence now live under
fresh runner-temporary paths, outside the restored Cargo target cache. Neither
failed run supplies remote latency evidence.

### Dedicated checkpoint and scheduling follow-up (October 16, 2026)

[Run 38822957148](https://github.com/DericHuynh/capntproto/actions/runs/38822957148)
at `b174998ae60cc803e02bf1510786882c69050041` completed measurement and cleanup.
Artifact `12015243458` retains the complete report and raw trials. Seven of
twelve remote workflows and three of four loopback payloads pass. The valid
misses remain advisory, so a successful workflow does not close the gap.

| Remaining workload | Portable p50 (µs) | Faster reference p50 (µs) | Ratio |
| --- | ---: | ---: | ---: |
| Remote fanout-32 | 763.145 | C++ 511.930 | 1.491× |
| Remote dependent-read | 251.566 | Legacy 236.981 | 1.062× |
| Remote pipelined-read | 178.644 | Legacy 148.922 | 1.200× |
| Remote callback | 264.003 | Legacy 219.818 | 1.201× |
| Remote bulk-4m | 13,231.881 | C++ 11,355.130 | 1.165× |
| Loopback 64 KiB | 81.799 | Legacy 73.389 | 1.115× |

The next candidate admits data-only pipeline-capable calls immediately when the
target is settled, retaining the ordinary codec, publication ordering and
pipeline ownership. Rejected pipeline sinks cancel the exact published question
or unpublished task. Published direct calls share the bounded small-call scan,
and ready callback workers resume within the existing frame budget. A growing
one-element buffer uses one allocation while retaining a pointer-sized empty
owner; admission retains its existing boxed Call instead of reallocating it.

Validation passes 260 RPC tests, 121 allocation-enabled wire tests, 86 fixed and
89 growing portable fixture tests, 72 mixed-peer workflow checks, and Clippy.
The final local candidate binary SHA-256 is
`39e482a96ffcd5d759dacf3085968ff3b04c7ed0364901945b30cb58a37dfe4d`.
Empty-call portable/legacy instructions are 17,877,620 / 17,145,610 client
(1.043×) and 16,409,626 / 16,055,624 server (1.022×). Its local four-payload
loopback ratios are 1.011×, 1.034×, 1.045× and 1.115×; 64 KiB still misses.
The most recent complete local twelve-workflow run belongs to the earlier
`fastcalls` candidate and passes six cells. It is not evidence for the later
allocation changes. Every intermediate trial, profile, patch and protocol trace
is retained in `target/portable-performance-scheduling-20261015/`.

### Dedicated checkpoint and direct receive follow-up (October 16, 2026)

[Run 38827168630](https://github.com/DericHuynh/capntproto/actions/runs/38827168630)
measured `89b9d4ee1f33bc24ac3d57ee97bc39a890ae60f2` and completed cleanup.
Artifact `12017182524` retains all raw trials and environments. Ten of twelve
remote workflows and two of four loopback payloads pass: **12 of 16**, so the
gap remains open. Different host/network timing makes cross-campaign absolute
latency comparisons inappropriate; each ratio uses that campaign's own peers.

| Remaining workload | Portable p50 (µs) | Faster reference p50 (µs) | Ratio |
| --- | ---: | ---: | ---: |
| Remote fanout-32 | 998.017 | C++ 855.475 | 1.167× |
| Remote callback | 574.565 | Legacy 528.118 | 1.088× |
| Loopback empty | 40.673 | Legacy 36.353 | 1.119× |
| Loopback 64 KiB | 93.711 | Legacy 83.485 | 1.122× |

The next experiments preserve idle call scans after receipt-free data replies,
stream incomplete large frames directly into the bounded message pool, and
defer an internal completion binding until a queued call is accepted locally.
Task generations, cancellation, pipeline projections and local early results
retain their ordinary owners. Prepared data-only Call certificates also support
the requested pipeline hint without mutating their source envelope.

The direct receive prototype reduces local 4 MiB pooled p50 from 5.35 ms to
4.23 ms, below both references in that run. It removes about 15 million
instructions on each peer in the 10-warmup/50-workflow diagnostic. Preserving
idle reply scans cuts the fanout client's instructions from 56,564,584 to
36,636,892, but its first timing campaign regresses fanout. Deferring completion
bindings reduces pipeline client instructions from 144,189,791 to 135,070,318.
These instruction results do not qualify latency. The complete rotating local
`lazybind` campaign passes seven of twelve cells and retains every miss.

A further candidate batches replies only when a complete successor is already
buffered. It retains the eight-frame cooperative limit and flushes when input
runs out, output capacity blocks progress, or that limit is reached. Its large
receive path preserves reusable buffers through 128 KiB and streams larger
incomplete frames. The batching candidate's local fanout
p50 falls from 185.103 µs to 138.785 µs (0.777× the faster reference), while
64 KiB still misses at 1.314×. Tests cover partial
stream reads, truncation, abandoned frames, sticky IO/counter failures, exact
frame boundaries, retryable binding rejection, local pipelines, and bounded
reply bursts. Synthetic benchmark lifecycle tests exercise both controls and
artifact salvage; they are never measurement evidence. Coverage baselines and
the 1.05× acceptance rule remain unchanged.

The adaptive-prefix experiment is excluded: its 64 KiB client/server instruction
counts rise from 21,691,798 / 19,702,169 to 21,751,863 / 19,737,810, and its
five-repetition timing campaign still misses that cell at 1.152×. All trials,
including this rejected experiment, remain under
`target/portable-performance-receive-20261016/`. The retained `batchreply` binary
SHA-256 is `dd2700cda19a8e21e0889b78da6bd22c52658ba1b84cc6c2cd07cc17dd35d4ff`.
Validation passes 261 RPC, 124 allocating wire, 48 futures, 24 buffered-input,
86 fixed and 89 growing portable tests, plus 72 mixed-peer workflow checks.
These checks include C++ framing and the buffered-input TLA+ model; they do not
establish the latency target.

Updates to the explicit `codex/portable-qualification` branch run the same
dedicated workflow as manual dispatch. PR correctness, coverage and formal
verification remain independent required checks; a timing campaign does not
replace them.

### Single-capability publication follow-up (October 16, 2026)

Single-capability Calls can now publish directly through the ordinary descriptor
transaction after exclusive dispatch preflight. Only reachable capability-table
entries are retained. The path preserves resource attachments, frozen routes,
question generations, grant validation, cancellation and rollback; unsupported
calls keep queued dispatch. Capability publication still invalidates ordinary
control and maintenance scans. Private generated request identity is used only
to inspect its own parameter table, without treating public hook pointers or
brands as authority. The invocation completion stage now contains local results
only; remote results already transfer directly from their sent state.

The `immediatecap-final` local campaign passes eight of twelve workflow cells.
Callback p50 changes from 54.651 to 52.167 µs and callback-client instructions
from 83,764,623 to 76,961,074. Dependent read (1.282×), pipeline read (1.205×),
callback (1.280×) and 64 KiB (1.159×) still miss the faster same-campaign peer.
All samples are retained, including noisy controls and regressions. Narrowing
worker wakeups was rejected because it did not reduce meaningful work and
increased pipeline-server instructions; its early-pipeline wake regression
remains as a correctness fixture.

The subsequent `localstage` binary has SHA-256
`92a20f420bd83ad573d4748249925ad8d839a3a3a54347b373f4e126fc859afd`.
Its four local loopback ratios are 1.019×, 1.012×, 1.011× and 1.106×: 64 KiB
still misses. Empty-call instructions are 17,998,014 / 17,158,581 client and
16,602,062 / 16,068,910 server against legacy. These local checks do not close
the dedicated latency gap. Validation passes 263 RPC, 125 allocating wire,
86 fixed and 89 growing portable tests, 72 mixed-peer workflows and Clippy.
A buffer regression now counts its initializer invocations directly instead
of introducing never-called panic closures. Coverage thresholds are unchanged;
the previous PR coverage failure remains outstanding until the new tree passes.
Profiles, executable identities, trials and patches are retained in
`target/portable-performance-capabilities-20261016/`.

### Shared receive-buffer follow-up (October 16, 2026)

The preceding dedicated campaign, [38834035472](https://github.com/DericHuynh/capntproto/actions/runs/38834035472)
at `a1b8d38a590f`, passes nine remote and three loopback cells. The remaining
ratios are fanout 1.191482×, pipelined read 1.102962×, remote 64 KiB 1.070215×,
and loopback 64 KiB 1.076264×. The following campaign stopped during provisioning
without timing results. Its active second host lacked a private address;
both hosts were cleaned up. Provisioning now retries incomplete network metadata
within the existing deadline and cancellation policy, while rejecting populated
invalid addresses. A complete mocked campaign exercises that delayed readiness.

Large complete input frames can now retain immutable receive allocations through
the ordinary message pool. Full allocator capacity is charged until the final
protocol owner drops, including across scopes; buffer count, generations and
per-frame limits remain enforced. Fixed storage and oversized prefetch allocations
retain the copying path. Ordinary buffered readers preserve pointer-stable spill
promotion. Only the explicit sharing path permits detaching a receive allocation
while an earlier frame is retained. No unsafe code is introduced.

The `unified` diagnostic reduces 64-KiB client/server instructions from
321,925,631 / 309,983,912 to 251,352,463 / 238,483,864 by removing one full receive
copy per peer. Its five-repetition workflow campaign passes six of twelve cells.
The misses are empty unary 1.078758×, small unary 1.507409×, dependent read
1.181579×, pipelined read 2.690098×, write/read 1.236035× and callback 1.648067×.
Controls show distinct timing regimes; no samples are discarded. All four local
loopback cells pass at 1.013132×, 1.024642×, 1.012916× and 1.046033×. These are
shared-VM diagnostics and do not qualify the dedicated target. Empty-call
instructions are 18,218,919 / 17,318,003 client and 16,885,721 / 16,229,143 server;
the client also misses the separate instruction target at 1.052022×.

Validation passes 187 engine, 265 RPC, 49 futures, 24 buffered-input, 86 fixed
and 89 growing portable tests, 72 mixed-peer workflows, and Clippy for the changed
runtime packages and development tools. Focused ownership checks cover retained
input, exact range validation, byte accounting, bounded fallback, successor
preservation and parameter-grant cleanup after a resumed service completes.
All prototype profiles, timing samples and patches are retained under
`target/portable-performance-shared-input-20261016/`. The gap stays open.

### Direct original-route publication (October 16, 2026)

Routable remote promises can publish data-only Calls without an intermediate
queued envelope when no older task exists. The exclusive graph ticket freezes
the authenticated import or parent-question route through FIFO acceptance and
marks promise ancestors used only on commit. Waiting promises and reflected
barriers still defer. Promised-answer envelopes retain their exact transform,
parameter graph, reader limits and ordinary question lifecycle.

The `routed` profile reduces pipelined-read client/server instructions from
136,685,037 / 85,918,608 (`unified`) to 121,741,844 / 82,093,143. The paired
four-workflow local campaign passes only fanout at 0.985541×; callback 1.256677×,
dependent read 1.244141× and pipelined read 1.708362× still miss. Empty-call
instructions remain above the separate client limit: 18,226,930 / 17,318,282
client and 16,907,729 / 16,229,427 server against matched legacy. All trials,
including unfavorable control shifts, remain recorded.

An independent syscall trace of 100 fanout batches finds 1,105 portable client
and 2,097 server send/write calls, versus 101 / 102 C++ vectored writes. The
trace validates complete workflows but its perturbed latency is not timing
evidence. This identifies output coalescing as the next fanout investigation.
Validation passes 127 allocating wire and 265 RPC tests, 86 fixed/89 growing
portable fixtures, 72 mixed-peer workflows, runtime Clippy and two focused Miri
tests. The maintenance vacancy optimization examines at most eight slots;
general-scan fixtures now exceed that bound and retain exact budget/cleanup
assertions. Evidence is in `target/portable-performance-routed-20261016/`.

### Shared-input dedicated checkpoint (October 16, 2026)

[Campaign 38839510111](https://github.com/DericHuynh/capntproto/actions/runs/38839510111)
at `cb3f85ff5db0b8382d53279e74a7729c998f4392` completes measurement and cleanup
with **10/12 remote and 4/4 loopback cells passing (14/16)**. Independent
recomputation verifies every target median and inclusive decision against all
350 remote trials and 60 portable/legacy/C++ loopback trials.

| Remaining remote miss | Portable p50 | Faster reference p50 | Ratio |
| --- | ---: | ---: | ---: |
| fanout-32 | 1,000.659 µs | C++ 810.345 µs | 1.234856× |
| pipelined-read | 315.412 µs | C++ 294.146 µs | 1.072297× |

The remote 64-KiB cell now passes at 0.979402× C++; loopback 64 KiB passes at
1.020071× legacy. The other loopback ratios are 1.013652×, 1.016123× and 1.027125×.
Artifact `12021629318` retains every raw sample, resource record and fixture
result; ZIP SHA-256 is
`c89751f4de74641cf0e57fe599fa2293b31a91cd1491cedb729230fab9639beb`.
These results apply to the shared-input commit, before original-route publication
or the output-coalescing experiments. The gap stays open until all 16 cells pass.

### Original-route dedicated checkpoint (October 16, 2026)

[Campaign 38841734555](https://github.com/DericHuynh/capntproto/actions/runs/38841734555)
at `1f07fcf40f188e18b058238718643ab963f4fb69` completes measurement and cleanup
with **9/12 remote and 4/4 loopback cells passing (13/16)**. Independent
recomputation checks all 350 remote and 60 portable/legacy/C++ loopback trials.

| Remaining remote miss | Portable p50 | Faster reference p50 | Ratio |
| --- | ---: | ---: | ---: |
| fanout-32 | 941.657 µs | C++ 679.101 µs | 1.386623× |
| pipelined-read | 307.787 µs | legacy 282.372 µs | 1.090005× |
| callback | 553.003 µs | legacy 515.077 µs | 1.073632× |

Artifact `12022477908` retains every sample and fixture result; ZIP SHA-256 is
`7f6c56da24dc83dcef31a6fc518c6d81cc8d3e4d2c402bf7ec68cd453ade44b9`.
The change does not meet the target. Ratios use each campaign's faster control;
the preceding 14/16 result remains recorded and does not qualify this commit.

### Bounded coalescing candidate (October 16, 2026)

The next candidate batches known outgoing calls only while FIFO and buffer
capacity remain available. A physical flush still owns accepted resources;
pool pressure and an unproductive work quantum force progress. Complete retained
frames use vectored output without rebuilding segment tables. Only streaming
receive bursts grow their buffer, geometrically and at most to 128 KiB; retained
owners and partial successors remain valid. Cancellation maintenance now records
whether any exchange needs cancellation scanning, and queued pipelines allocate
a shared driver only when one exists.

Local fanout tracing records 106 client writes and 808 server writes for 100
batches, versus 102 server writes for C++. This is syscall evidence, not latency
qualification. The full five-repetition local campaign passes **7/12 remote-style
workflows and 1/4 loopback payloads**. Every miss and both controls are retained;
the shared VM's large shifts also affect the controls. The empty-call instruction
comparison is **18,312,409 / 17,363,548 client (1.054647×)** and
**16,967,680 / 16,275,010 server (1.042560×)**. The client instruction gate still
misses. Pipelined-read client instructions fall from 122,042,966 for the batch-hint
prototype to 120,118,665 after removing empty shared pipeline drivers.

Validation includes 268 RPC tests, 51 async framing/queue tests, all 72 mixed-peer
workflow checks, and Clippy for all four runtime crates. Prototype patches,
failed pressure experiments, raw timings, profiles and executable identities are
preserved in `target/portable-performance-coalescing-20261016/`. This candidate
requires a dedicated campaign; neither the local results nor a green workflow
status close the 16-cell gap.

### Prepared pipeline completion candidate (October 16, 2026)

Prepared requests now keep their wire envelope when promise pipelining is
enabled. Operation status and result ownership share one completion, while the
existing split promise retains response and pipeline interests independently.
Early pipeline publication still wins over later parent failure. Final response
sharing is lazy when an early pipeline has already been supplied.

The local pipelined-read profile falls from **120,118,665 to 112,288,824 client
instructions**; server instructions are 82,320,244. All 72 mixed-peer workflows
pass. A focused regression test drops the response before admission and checks
that a projected child keeps its parent alive, including early publication and
parent failure. It and the optional-pipeline-driver test pass under Miri, as does
the shared-buffer owning-read/spill test.

The full five-repetition local sweep passes **6/12 remote-style and 3/4 loopback
cells**. The separate four-case diagnostic and all repetitions are retained in
`target/portable-performance-completion-pipeline-20261016/`; neither campaign is
dedicated qualification. Empty-call instructions are 18,312,695 / 17,361,573 on
the client (1.054783×, still a miss), and 16,965,608 / 16,270,687 on the server
(1.042710×). No latency or instruction limit has been relaxed.

The preceding CI run `38839513626` passed platform, Miri, native, mutation,
fuzz, Verus and TLA+ checks, but failed coverage ratchets and the dependency
audit. Its complete cargo report is artifact `12022794784`, ZIP SHA-256
`c3717dc15e358a3429d2cd0684d78ac3f485608c9d884498058ed068b1d86667`.
The yanked `zip` 2.4.2 dependency is replaced with 9.0.3; archive filename errors
remain rejected. All 62 developer-tool tests and the warning-denying audit pass.
Coverage repair adds retained-buffer immutability and optional transport fallback
checks; the existing coverage baselines remain unchanged.

### Coalescing qualification and callback follow-up (October 16, 2026)

The dedicated campaign for `508c030` (`38847075747`, artifact `12024921004`)
passes **13/16 cells: 10/12 workflows and 3/4 loopback payloads**. Fanout now
passes at 0.980409× C++. The remaining misses are pipelined read at 1.094809×
C++, callback at 1.082528× legacy, and 1 KiB loopback at 1.105815× legacy.
Every pooled p50/p95/p99 and inclusive 1.05 gate was independently recomputed
from all 350 remote and 60 loopback trials. The artifact ZIP SHA-256 is
`5f6050346300188a3950bf59cedf3fd9006d26221fd78c5b91d7069ca660253c`.
Gap #11 remains open. The prepared-pipeline campaign for `166c91b`
(`38848920337`, artifact `12024953869`) subsequently passed 11/16 cells.
Its separate legacy/C++ 1 KiB guard failed at 1.1510× against 1.15×.

The next candidate preserves idle outgoing-call lanes while serving a plain
callback. Application notifications, cancellation, output pressure and the
existing cooperative budgets still invalidate or limit that work. Bootstrap
commands retain one stable coordinator notifier, and hook refresh upgrades each
weak endpoint once. A data-only Return can also complete a capability-bearing
request directly when `releaseParamCaps` is false and the question is not
redirected. Its exported grants remain live until the peer's Release; replies
which release parameter capabilities retain the full descriptor transaction.

Callback client instructions fall from **78,430,660 to 67,062,093** for the same
100 warmups and 1,000 measured operations. Server instructions are 61,012,549;
the matched legacy diagnostic is 36,255,957 client and 44,765,923 server.
The separate empty-call comparison remains a miss on the client:
18,279,234 / 17,363,836 (1.052719×), with 16,900,980 / 16,273,076 on the server
(1.038585×). These instruction results do not establish latency parity.
The four-case rotating local diagnostic and full four-payload loopback campaign
retain all samples and controls; loopback passes only 1/4 on this shared VM.

Validation passes 647 runtime library tests (270 RPC, 132 wire, 191 engine,
54 futures), all 72 mixed-peer workflows, Clippy for the four runtime crates,
and both portable verification profiles. Focused Miri checks pass for parameter
grant retention, redirected replies, retained plain results, late pipeline
observers and authenticated handoff. Coverage additions exercise incomplete
successor bodies, streamed counter overflow, late observers and callback idle
state; no coverage baseline or benchmark acceptance limit is reduced.
Candidate evidence is retained in `target/portable-performance-callback-20261016/`.

The callback follow-up `ac6ed49` completed dedicated run `38852324003`
(artifact `12026394729`) with **12/16 passing cells**. The four misses were
fanout at 1.054226× C++, pipelined read at 1.089118× C++, callback at
1.083644× legacy, and 64-byte loopback at 1.072378× legacy. All 350 remote
and 60 loopback trials were independently checked, including pooled
p50/p95/p99 and the inclusive 1.05 limit. Artifact ZIP SHA-256:
`af7f88a399c40b788e832e732849553076cb7637087b22ced6c167b3a381f9c2`.
A successful workflow status does not imply that all portable parity cells pass.

### C++ read/write scheduling investigation (October 16, 2026)

Timestamped syscall traces decode the actual RPC frames in each TCP write.
C++ schedules its write queue with `kj::evalLast()` after immediately ready
work, retaining message owners through the physical write. The portable
coordinator flushed a parent request before forwarding its already requested
pipeline child, then flushed each reply separately. For two warmups and ten
measured pipelined reads, the initial portable trace contained 54 client and
26 server writes; C++ used 13 and 14, respectively. These totals include
bootstrap and final cleanup, and are diagnostics rather than latency evidence.

The candidate resolves requested pipeline projections when their parent
question is published, using the existing authority checks. It batches ready
work within cooperative budgets and known pool capacity, and admits buffered
successors without waiting for new input. The first revised trace combines the parent
and child in one write and their replies in one write; a separate Release
remains. Fanout replies reach the same one-write-per-32-replies pattern after
the receive buffer grows. No benchmark fixtures or validation are changed.

Temporary scheduler traces explain the remaining Release split: the coordinator
charged the full remaining maintenance quantum even when maintenance returned
after its first productive transition. Retiring an endpoint could therefore
exhaust the outer budget before publishing its ready Release. The next change
reports and reuses unspent maintenance budget while preserving the rotating scan
and charging every visited slot. Waiting questions resume after physical output
without forcing that output ahead of ready cleanup. The earlier isolated change
to waiting-question scheduling did not improve the write pattern and is retained
as a diagnostic result. All temporary logging is removed from production code.

With actual maintenance accounting, the decoded trace now matches C++'s steady
write pattern: each callback Return and Release share a 120-byte write, and each
pipelined operation batches Finish, both Calls and Release, followed by one
server write containing both Returns. Rust's pipeline frames total 1,376 bytes
versus C++'s 1,368; their corresponding combined replies are 1,224 versus 1,216.
Bootstrap and final cleanup still differ. The new budget regression, parent/child
publication test and opaque-response ownership test all pass under Miri. Full
five-repetition untraced comparisons pass 8/12 workflows and 2/4 loopback
payloads on the shared VM; all 72 mixed-peer workflows validate. The pipeline
cell still misses substantially locally, despite the matched write pattern.
All controls, repetitions and misses are retained. Fresh dedicated
qualification is still required.

This checkpoint passes 405 RPC/wire unit checks, all 16 runtime integration
tests (including broker-drop introduction and TLC replay), and warning-denying
Clippy for the four runtime crates. Evidence is retained under
`target/portable-performance-io-accounting-20261016/`.

The preceding PR CI run `38848998858` passed platform, formal, Miri, native,
fuzz and mutation jobs, but its cargo job stopped before coverage collection:
`portable_automatic_introduction_survives_broker_drop` failed. A decoded protocol
trace showed the forwarding vine released after local marker publication but
before the remote Accept completed. Retaining that vine through the Accept
future fixes the reproduced failure. All 16 runtime integration tests pass,
including TLC trace replay with the cached model-checker JAR. Fresh coverage
and full parity qualification remain required; baselines are unchanged.

Dedicated run `38860047520` for `ef9f4c8` subsequently completed with **11/16
passing cells**: 9/12 remote workflows and 2/4 loopback payloads. Independent
validation checks all 350 remote and 60 loopback trials, five repetitions per
cell, pooled p50/p95/p99 and the inclusive 1.05 limit. Fanout improves to
0.931282× C++. The remaining misses are:

| Case | Portable p50 | Faster reference p50 | Ratio |
| --- | ---: | ---: | ---: |
| dependent-read | 548.198 µs | C++ 517.253 µs | 1.059826× |
| pipelined-read | 315.533 µs | C++ 268.362 µs | 1.175774× |
| callback | 538.057 µs | legacy 502.716 µs | 1.070300× |
| loopback 1 KiB | 28.499 µs | legacy 27.138 µs | 1.050151× |
| loopback 64 KiB | 73.505 µs | legacy 63.866 µs | 1.150925× |

The successful workflow status does not close the parity gap. Artifact
`12030357582` has ZIP SHA-256
`54d5eee2a0ba6076f529209f47b5e4cbe490360a922326ca6401c20cf71b2f40`.
The complete report, raw trials and independent validation are retained in
`target/portable-performance-io-dedicated-20261016/`.

The read-side follow-up charges the frame budget when a frame is admitted,
and consumes bounded already-buffered replies before yielding to the application.
The fairness/stop regression covers both plain and pipelined replies across four
budgets. It passes Miri, all 272 RPC unit tests, 16 runtime integration tests and
Clippy. Pipeline client instructions decrease from 102,030,159 to 97,422,810;
server instructions are 74,801,968. Steady writes still match the C++ grouping.
This shared-VM diagnostic does not establish latency acceptance.

A separate indexed pipeline-completion experiment preserved observer propagation
and passed correctness checks, but increased client instructions to 99,476,958.
It was discarded. Its full local workflow sweep passed 5/12; no loopback sweep
was run for that discarded prototype. Both experiments, including initial failed
assertions and corrected validation, are retained with exact source patches and
SHA-256 manifests in `target/portable-performance-readside-20261016/`.

The next candidate keeps that bounded read-side change, reuses an already-shared
pipeline driver directly and removes intermediate boxed futures during queued
forwarding. Already-settled exported endpoints omit unused resolution channels.
Pipeline client/server instructions are 95,534,645 / 74,797,415; callback is
62,181,104 / 60,969,272. Matched legacy pipeline totals are 56,352,420 / 56,823,453
after the shared queued-future change, so the control's improvement is retained.
The portable pipeline client uses 6.4% fewer instructions than `ef9f4c8`, but
still exceeds both controls. Empty-call instruction ratios are 1.059037 client
and 1.043918 server; the client gate remains a miss.

The complete local comparison passes **7/12 workflows and 1/4 loopback payloads**.
All five repetitions, raw samples and misses are retained. All 72 mixed-peer
workflows pass. Validation includes 272 RPC unit tests, 87 focused integration
tests for queued forwarding, 32 integration tests after the endpoint change,
three queued and 25 scoped Miri checks, and Clippy with warnings denied. Fresh
traces retain the C++ steady write grouping. Evidence is retained under
`target/portable-performance-shared-driver-20261016/`; dedicated qualification
is required before claiming progress against the latency gate.

The completed dedicated run for `46c3a69`,
[38864720957](https://github.com/DericHuynh/capntproto/actions/runs/38864720957),
passes **9/12 workflows and 2/4 loopback payloads**. Independent recomputation
checks all 350 remote and 60 loopback trials, including every p50/p95/p99 and
the inclusive 1.05 limit. Misses are pipelined read (1.146512), 64 KiB
(1.083280), 1 MiB (1.055378), empty loopback (1.253681) and 1 KiB loopback
(1.410684). The raw artifact and validation are preserved under
`target/portable-performance-settled-dedicated-20261016/`. No campaign has
passed all sixteen cells.

Further Return-admission batching, including exact parent propagation, restored
an extra Release write per operation and regressed instructions; both prototypes
were discarded. An unused-pipeline admission shortcut also showed no CPU gain:
the dependent-read fixture retains its pipeline handle across the await. Their
patches, traces, profiles, raw controls and validation logs remain under
`target/portable-performance-rejected-read-batches-20261016/` and
`target/portable-performance-unobserved-pipeline-20261016/`.

The next retained candidate delivers an admitted Return directly to its original
owner when no pipeline projection has been requested. It preserves the ordinary
pipeline completion for later projections, exact question generations,
authorization and cancellation. Capability export avoids a redundant refresh.
Queued forwarding allocates its queue only when used, stores the first entry
without a tree node, and omits a reply channel for detached publication. FIFO,
stale cancellation, identity exhaustion and destructor reentry remain checked.
Dependent-read client instructions are 77,040,613 versus 79,187,297 before this
batch; pipelined-read client/server totals are 93,974,372 / 74,794,034. Matched
legacy pipeline totals also improve, to 55,282,988 / 55,648,078. Empty-call
instruction ratios remain a client miss and server pass.

The complete local workflow campaign passes **8/12 workflows**;
all 72 mixed-peer workflows pass. The accompanying loopback report's 3/4
result was incorrectly attributed to this candidate: its copied `tcp` wrapper
still executed `settled-channel20261016`, despite the `driver` symlink naming
the new binary. It is an additional older-binary diagnostic, not qualification
for this candidate. The original samples and hashes are retained; subsequent
local bundles record and check the executable behind the wrapper explicitly.
Validation includes 274 RPC unit tests,
81 integration tests, four queue Miri tests and the direct-completion Miri
regression. Developer-tool tests cover diagnostic failure cleanup and ensure
tracing follows both untraced control campaigns. Evidence is retained under
`target/portable-performance-compact-sender-20261016/`. The dedicated workflow
now collects paired portable/legacy/C++ syscall traces after all untraced
measurements; traced latencies never enter acceptance reports.

The following local candidate adopts existing queued pipeline clients directly
and selects an outgoing observer's exact question before searching authorized
responses. Parent cancellation interest and recycled observer identity remain
covered by regressions and Miri. Pipelined-read client instructions fall to
90,846,884, from 93,974,372; dependent-read and callback instruction totals are
essentially unchanged. The verified fresh local bundle passes **4/4 loopback
payloads**, while the separate workflow sweep passes **5/12**. The empty-call
instruction client still misses (18,490,977 versus legacy 17,362,388); the server
passes (16,987,967 versus 16,273,319). None of these local results establishes
dedicated qualification. All 240 timing trials were independently recomputed;
all 72 mixed-peer workflows pass. Evidence, rejected cleanup-order experiments,
source patches, traces, and the older bundle attribution correction are retained
under `target/portable-performance-observer-question-20261016/`.

Dedicated run [38869439794](https://github.com/DericHuynh/capntproto/actions/runs/38869439794)
at `5692624` completes **9/12 remote and 1/4 loopback cells (10/16)**.
All 410 trials and their p50/p95/p99 were independently checked. Remote misses
are pipelined read (1.133091× C++), callback (1.055468× legacy), and 256 KiB
(1.072719× C++). Loopback misses are empty (1.513130× legacy), 64 bytes
(1.108635× legacy), and 64 KiB (1.076115× legacy). Artifact `12034344823`
has ZIP SHA-256 `b780775c937552417c01d875877835b9f3381622bdee875ac8b622c061752092`.
The post-measurement paired traces confirm one steady pipeline write per peer
and callback Return/Release batching matching C++. Portable still interleaves
the previous operation's cleanup between its next two Calls. A separate local
timestamp journal records socket entry/exit without ptrace or writes during the
operation; its first pipeline diagnostic shows roughly 14.1/9.5 µs client/server
read-to-write intervals versus C++'s 5.4/6.2 µs. These traced intervals are
diagnostics only and are excluded from every acceptance comparison.

The next candidate reuses the original projection path and keeps the first
cached projection inline. Bounded maintenance retires up to eight adjacent
endpoints without rebuilding application lanes between them; every owner is
charged to the existing budget, and returned application owners and output
still stop the scan. Pipeline client/server instructions reach
89,923,653 / 73,793,334; dependent read reaches 76,159,701 / 64,847,122;
callback reaches 60,470,369 / 60,270,134. A receive-buffer reuse prototype showed
no material gain and was discarded. Its weak-owner regression exposed a real
reuse panic: a lone strong `Arc` with outstanding weak references is not mutable
through `Arc::get_mut`; the reader now detaches safely in that case.

The complete new local campaign passes **6/12 workflows and 2/4 loopback
payloads**, with all 72 mixed-peer checks passing. The empty instruction gate
still misses on the client and passes on the server. Validation includes 135
wire, 276 RPC and 58 framing unit tests, 92 integration tests, Clippy, and
focused Miri checks for retirement fairness, projection caching and weak receive
owners. Five rotating timestamp repetitions are retained for both the prior
and new candidate; their large timing variation prevents treating a single
interval comparison as a measured improvement. Raw samples, journals, source,
rejected experiments and verification logs are archived under
`target/portable-performance-retirement-burst-20261016/`. The full target remains
open pending dedicated qualification. PR CI for the previous candidate now
passes the complete Miri job. Its Cargo job completed 2,232 tests with zero
failures and 20 skips, but failed the unchanged coverage regression limits.
Artifact `12036078454` has ZIP SHA-256
`a73fe8f49824298663f6beeb33c2d7adca4c7cc50408ef9c208c7019aa17d5c4`.
The tested optimization commit `2a2cda0` is in dedicated run
[38877354685](https://github.com/DericHuynh/capntproto/actions/runs/38877354685)
and PR CI run
[38878916088](https://github.com/DericHuynh/capntproto/actions/runs/38878916088).
The dedicated run completes **11/12 remote and 3/4 loopback comparisons
(14/16)**. The remaining misses are pipelined read at 326,547 ns versus
C++'s 301,881 ns (1.081708×), and 64 KiB loopback at 76,937 ns versus
legacy's 72,805 ns (1.056754×). All 410 raw trials and the reported
p50/p95/p99 were independently verified. Artifact `12037881984` has ZIP
SHA-256 `bdcc2ae9d36186d6a11fc2316ee08e224ef475eb4538f565acd414a0e5ded5df`.
The complete 5% requirement remains open.

Further paired tracing identifies the steady C++ pipeline write order as
**Finish, Release, Call, Call**. Giving completed capability cleanup an earlier
bounded maintenance turn reproduces that order in all eleven steady local
trace groups; both server Returns remain in one write. This reduces pipeline
client instructions to 88,205,859 from 89,923,653. A complete untraced local
campaign passes **7/12 workflows and 3/4 loopback payloads**; all 240 trials and
their percentiles were independently checked, and all 72 mixed-peer checks
pass. Pipeline latency still misses at 1.758011× matched legacy locally.
The empty-call instruction client also still misses at 18,492,561 versus
17,370,094; the server passes at 16,987,676 versus 16,281,081. Matching the
traced write order is therefore an implementation improvement, not latency
qualification. Serving immutable completed payload pipelines directly removes
two intermediate allocations, reducing pipeline client instructions further
to 87,770,982 and dependent-read client instructions to 75,879,258. The wire
grouping remains unchanged and 277 RPC unit tests pass, including repeated
pipeline cleanup with small budgets, output backpressure and control wakes.

Inline Return-observer settlement was then evaluated and discarded. Its first
version moved Release behind new Calls; a larger bounded cleanup burst restored
C++ ordering but increased pipeline client instructions to 91,155,965. Both
versions, raw profiles and traces are retained under
`target/portable-performance-return-observers-20261016/` and
`target/portable-performance-return-cleanup-20261016/`. Five rotating thread-CPU
and elapsed-time trace repetitions locate the pipeline gap in computation:
approximately 15/10 µs portable versus 6/7 µs C++ client/server processing.
The added thread-clock calls impose diagnostic overhead; none of these samples
enters the acceptance gate.

The retained candidate instead preserves idle application scans when an idle
control policy receives Finish/Release and has no execution to resume. Normal
ledger retirement, independent control wakes and destructor notifications
remain active. Pipeline client/server instructions are 87,774,260 / 72,485,730;
dependent read is 75,888,327 / 63,726,864; callback is
58,743,579 / 60,387,969. All eleven steady pipeline write groups retain the
C++ Finish/Release/Call/Call order. The complete local campaign passes
**7/12 workflows and 3/4 loopback payloads**, with all 240 raw trials checked
and all 72 mixed-peer checks passing. Local timing variation remains large;
the latest completed dedicated result remains **14/16**, and the complete
target remains open. The empty instruction gate still misses on the client
(18,492,801 versus 17,370,217) and passes on the server
(17,003,704 versus 16,281,212).

Final review restricts that idle-retirement certificate further: no outgoing
call may be waiting, because releasing storage can enable an admission even
without an application wake. The final `idle-retirement20261016` executable has
SHA-256 `c23028be7f8b676371f10bd33aa6a98af262c112dfe9e1e776b29efa5fed6d9a`.
Pipeline client/server instructions are 87,774,435 / 72,511,188, dependent read
75,887,829 / 63,752,406, and callback 60,579,761 / 60,388,472. The earlier
callback reduction does not survive this necessary guard. All eleven steady
pipeline write groups still match C++'s Finish/Release/Call/Call order. Final
native validation passes 277 RPC tests, 92 integration tests and Clippy. Miri
passes the cleanup/backpressure, retained-payload and buffered-control tests
on the preceding candidate; that run predates the outgoing-call guard.
The final candidate's complete local campaign passes **7/12 workflows and
2/4 loopback payloads**. All 240 trials and pooled percentiles are independently
checked, and all 72 mixed-peer checks pass. Empty-call instructions are
18,494,123 / 17,005,111 versus legacy's 17,371,608 / 16,282,563
(client MISS, server PASS). Raw evidence and the exact source patch are in
`target/portable-performance-idle-retirement-20261016/`. These local results
retain all misses and do not replace the latest 14/16 dedicated result.

Paired gperftools CPU samples use the original pipeline and 64 KiB operations
with 100,000 iterations for sampling only. These samples, five rotating
thread-CPU journals, profiles, all 240 untraced local trials and source identities
are retained in `target/portable-performance-control-retirement-20261016/`
(1,436 files plus checksums). ELF symbolization identifies system-call execution,
coordinator work and payload copies; the initial unresolved-symbol reports are
also retained. The 64 KiB Callgrind call graph confirms one complete extra
payload copy from a prepared Call into the portable wire buffer. Legacy/C++
retain prepared segments through output. Avoiding that copy requires preserving
bounded storage accounting and ownership through partial writes; it remains
an optimization opportunity, not a claimed improvement.

The `2a2cda0` PR checkpoint subsequently completes all 2,241 Cargo tests
without a failure (20 skips). Miri, distributed models, platforms, fuzzing,
mutation checks and portable checks pass. Its unchanged coverage regression
gate fails; exact counters and missed mappings are archived under
`target/portable-performance-retirement-ci-20261016/`. Cargo artifact
`12040512363` has SHA-256
`538fad97a8194664bbff7d693ee79b91ace1e9f768d1e3b5405bc1d13baa56fd`.

Further paired traces retain the C++ cleanup order while removing notifications
from nonfinal pipeline clones and already-observed replies. This candidate
passes 278 RPC tests, focused Miri and all 72 mixed-peer workflows. All 240
local timing trials are independently checked: **8/12 workflows and 1/4
loopback payloads** pass. Evidence, including an invalid startup-only profile
attempt and its correctly named rerun, remains in
`target/portable-performance-reply-wakes-20261016/` (870 files plus checksums).
These shared-VM timings do not establish dedicated qualification.

A portable pipelined response and its queued projections now share one
completion driver, removing a second shared future around the unit result.
Both response-only and pipeline-only interests still drive completion;
final cancellation and wake delivery pass focused Miri. Pipeline client
instructions fall from 87,667,251 to 86,696,943, with server instructions
72,510,276. All eleven steady client writes remain Finish/Release/Call/Call,
and both server Returns remain in one write. The full local campaign passes
**6/12 workflows and 3/4 loopback payloads**, with 64 KiB loopback at
49,893 ns versus legacy's 47,500 ns (1.050379, MISS). All 240 trials and
percentiles are independently verified; all 72 mixed-peer workflows and
279 RPC tests pass. Empty-call instructions remain a client miss/server pass.
The exact source, frozen executable identity, traces and raw samples are in
`target/portable-performance-shared-pipeline-20261016/` (866 files plus checksums).

Dropping a finished pipeline also skips its obsolete cancellation wake;
final capability endpoints still notify cleanup independently. Conservative
notification remains available when a reentrant hook holds the completion
cell. The final `finished-pipeline20261016` executable has SHA-256
`d8d398f243713d454be8dc98f7de486c290b6e4116da89e22df50c3870d48ac0`.
Pipeline instructions are 86,659,782 / 72,510,270, dependent read
74,518,076 / 63,751,548, callback 60,586,651 / 60,391,292 and 64 KiB
251,428,598 / 238,666,309. The additional reduction is small; C++ pipeline
write ordering and batching remain unchanged. Native validation passes
279 RPC tests, 92 integration tests and Clippy. Focused Miri passes the
synchronous pipeline-drop test and the cleanup/backpressure matrix, including
budgets 1/8/64 and output capacities 1/4. The full local campaign passes
**7/12 workflows and 3/4 loopback payloads**; all 240 trials and percentiles
are independently checked and all 72 mixed-peer workflows pass. Empty-call
instructions remain client MISS/server PASS (18,494,219 / 17,003,605 versus
legacy 17,370,093 / 16,281,103). Raw samples, traces, source and executable
identities are retained in `target/portable-performance-finished-pipeline-20261016/`.
The full dedicated 16-cell target remains open.

The completed dedicated `6b31ee0` campaign (run `38886103592`) passes
**9/16 targets: 9/12 remote workflows and 0/4 loopback payloads**. This earlier
result and the preceding 14/16 campaign remain historical. All 350
remote and 60 loopback trials, five repetitions per cell, p50/p95/p99 and the
inclusive 5% decisions are independently verified. The misses are:

| Case | Portable p50 ns | Faster reference p50 ns | Ratio |
| --- | ---: | ---: | ---: |
| dependent-read | 389,433 | C++ 356,475 | 1.092455 |
| pipelined-read | 240,688 | legacy 201,538 | 1.194256 |
| callback | 372,673 | legacy 345,281 | 1.079332 |
| loopback 0 B | 37,976 | legacy 33,969 | 1.117960 |
| loopback 64 B | 36,160 | legacy 33,516 | 1.078888 |
| loopback 1 KiB | 46,520 | legacy 35,664 | 1.304397 |
| loopback 64 KiB | 99,329 | legacy 81,305 | 1.221684 |

All 21 steady pipeline write groups match C++'s Finish/Release/Call/Call
ordering, both server Returns share one write, and there is one steady epoll
wait per round trip. Legacy makes more socket calls but has less processing
between IO. Traced intervals include instrumentation overhead and are excluded
from qualification. Ordering alone has not closed the gap. Artifact
`12041701268` has ZIP SHA-256
`b4b96caf7d0998342466113d8d0757ae0ab5ca92d0daef169500a748495cda20`;
all raw evidence and recomputation are archived under
`target/portable-performance-idle-retirement-dedicated-20261016/`
(1,205 files plus checksums).

The subsequent dedicated `d9d1f7f` run `38890319120` is a completed
comparison: **9/16 targets, with 8/12 remote workflows and 1/4 loopback**.
All 350 remote and 60 loopback trials, five repetitions per cell, p50/p95/p99
and inclusive 5% decisions are independently verified. The seven misses are:

| Case | Portable p50 ns | Faster reference p50 ns | Ratio |
| --- | ---: | ---: | ---: |
| dependent-read | 244,913 | C++ 222,531 | 1.100579 |
| pipelined-read | 160,056 | C++ 127,925 | 1.251171 |
| callback | 234,512 | legacy 204,029 | 1.149405 |
| bulk-64k | 543,131 | legacy 505,572 | 1.074290 |
| loopback 0 B | 32,846 | legacy 28,554 | 1.150312 |
| loopback 64 B | 32,197 | legacy 30,068 | 1.070806 |
| loopback 64 KiB | 97,631 | legacy 78,305 | 1.246804 |

All 21 steady pipeline writes retain C++ Finish/Release/Call/Call ordering and
both Returns remain batched. The workflow failed a separate report quality
requirement: legacy TCP/C++ at 64 bytes was 1.2053×, above its unchanged 1.15×
limit. Sampling validation and infrastructure cleanup succeeded. The measurement
lane's passed status does not imply overall quality acceptance. Artifact
`12043337184` has ZIP SHA-256
`fd8430ba3d5821d280664eff28f6da70858bec4f52776c225e8056f6fbe16db1`.
Raw samples, recomputation, job log and paired traces are archived under
`target/portable-performance-finished-pipeline-dedicated-20261016/`
(1,206 files plus checksums). The performance gap remains open.

The corresponding PR CI run `38890329685` completed 2,244 Cargo cases with
zero failures/errors and 20 skips. Full Miri, models, platform, portable,
fuzz, mutation, native-careful, LLVM and documentation checks passed; the
unchanged coverage regression gate failed. Its report ZIP has SHA-256
`8912c11abab5b48d2ac5bf662d385b57bdd299fd1c4d962c1e179f2770b9c128`.
The compact report, complete job log and coverage summary are archived under
`target/portable-performance-finished-pipeline-ci-20261016/`.

The subsequent local prototype publishes the wire projection queue when a
prepared pipelined request is sent. Pending child clients retain that queue
until their target is published; they no longer pass through a second queued
pipeline or an extra request wrapper. Public response/child interests retain
the completion driver independently. Failed admission releases its credit even
if the caller keeps the failed pipeline, and ordinary no-pipeline calls retain
the lazy ResultsHook path. Call admission is outlined to limit duplicated code;
three private wire export sites also omit a redundant metadata refresh.

The frozen executable SHA-256 is
`454050eb580912886ae8d160154e094f3e9f7f101741d62b282ccebbb0124fd2`.
It passes 281 RPC tests, 92 integration tests, 72 mixed-peer workflows, Clippy,
formatting, and six focused Miri executions, including the cleanup matrix with
budgets 1/8/64 and output capacities 1/4. Pipeline client/server instructions
are 85,133,110 / 72,284,012; dependent reads 73,223,988 / 63,622,057;
callbacks 60,462,175 / 60,150,267. All 11 steady traced pipeline writes retain
C++ Finish/Release/Call/Call ordering and both Returns share one write.
All 240 local trials are independently checked: **8/12 workflows and 3/4
loopback targets pass**. Alternating five-repetition comparisons with the prior
prototype show only a 0.3% pipelined median difference, with substantial host
variation. These diagnostics do not establish dedicated latency qualification.

Callgrind cache/branch simulation identified a larger remaining footprint:
the export-scan client had 5,747,521 instruction-cache misses versus C++
1,895,628 for 100 warmups and 1,000 pipelined operations. The current prototype
reduces client instructions 1.38% and simulated instruction-cache misses 1.07%
relative to that portable baseline, while conditional branch misses rise 6.32%.
These are simulation counts, not hardware counters. Matched legacy instruction
counts move by +0.074%/+0.492% on the two peers; controls are retained.
Empty-call instructions remain client MISS/server PASS (18,510,707 / 17,003,489
versus legacy 17,367,049 / 16,359,460). Raw samples, controls, traces, source
patches and exact executable identities are retained in
`target/portable-performance-direct-wire-interest-20261016/`.

The next local experiment defers an exported pending endpoint's resolution
channel until a caller actually observes resolution. Unobserved routes still
settle and snapshot their terminal endpoint; observers retain the route, and
bridge shutdown wakes them. Notification state is detached before callbacks.
The `lazy-resolution20261016` executable has SHA-256
`acf5326677b904465e2cc74bd267d79ad69ad8def5ecbab3e1f3c3e2d643611d`.
It passes 284 RPC tests, 92 integration tests, 72 mixed-peer workflows, Clippy,
formatting and 14 focused Miri cases. The new cases cover late observers,
unpolled observers, bridge drop and reentrant resolution callbacks.

Pipeline client/server instructions are 84,233,085 / 72,290,060; dependent
reads 72,994,276 / 63,638,266; callbacks 60,474,375 / 60,016,881. This is a
1.06% pipeline client instruction reduction from the direct-wire candidate,
with all 11 steady C++ Finish/Release/Call/Call groups and batched Returns
preserved. All 240 local trials were independently checked: **7/12 workflows
and 1/4 loopback targets pass**. Large shared-VM timing variation remains;
these results do not qualify latency. Empty-call instructions remain client
MISS/server PASS: 18,406,791 / 17,010,769 versus legacy 17,367,714 / 16,360,118.
Raw trials, controls, paired traces, cache simulation, tests, source patch and
executable identities are retained in
`target/portable-performance-lazy-resolution-20261016/` (936 files plus checksums).
The next dedicated `d888c30` campaign, workflow `38897701997`, completed
with **11/16 targets, with 9/12 remote workflows and 2/4
loopback payloads**. All 350 remote and 60 loopback trials, five repetitions
per cell, p50/p95/p99 and inclusive 5% decisions were independently verified.
The five misses are:

| Case | Portable p50 ns | Faster reference p50 ns | Ratio |
| --- | ---: | ---: | ---: |
| dependent-read | 628,790 | legacy 597,300 | 1.052721 |
| pipelined-read | 347,678 | C++ 318,119 | 1.092918 |
| callback | 615,499 | legacy 564,904 | 1.089564 |
| loopback 0 B | 37,720 | legacy 35,514 | 1.062116 |
| loopback 64 B | 38,417 | legacy 36,225 | 1.060511 |

All 21 steady pipeline writes match C++ Finish/Release/Call/Call, and both
Returns share one write. Build provenance, all four environment manifests and
recorded executable hashes agree. The source fingerprint is
`b2b7689f9bd574230c600f38c48ffde8256f96108de0fdc0850b3432e9094df1`.
The dedicated workflow and its existing report quality requirements passed;
the separate all-sixteen portable goal remains open. Network latency differs
from previous campaigns, so reference selection remains within this campaign.
Artifact `12046522376` has ZIP SHA-256
`03464493b67e934f6014ca26b69fea80027de697addc140af4e1dcd82395e328`.
Raw samples, recomputation, build provenance, job log and paired traces are
retained in `target/portable-performance-direct-wire-interest-dedicated-20261016/`
(1,537 files plus checksums). PR CI `38898845818` subsequently completed
2,246 tests with zero failures or incomplete results and 20 skips. Miri, models,
platform, portable, fuzz, mutation, native-careful and LLVM checks passed. The
unchanged coverage-regression gate failed. Its diagnostics and coverage summary
are retained in `target/portable-performance-direct-wire-interest-ci-20261016/`;
artifact `12048991202` has ZIP SHA-256
`bc44f57888bf506baa2e2a2f676cfd715dcc9f76d3a92ece3d81e01881ed014e`.

Three subsequent local experiments retain that wire ordering. A bounded cache
of four interface/method headers avoids rebuilding alternating request headers;
its saturation test checks the fallback header and payload. Retirement-only
Finish/Release publication preserves idle scans for unrelated coordinator lanes,
while observing reentrant wakes and revisiting calls blocked by capacity. Finally,
contiguous single frames use one stack-allocated IO vector, matching C++'s
`writev` transport path while preserving partial writes and scalar fallbacks.

Each candidate passed all 240 independently recomputed local trials, with
**7/12 workflow and 2/4 loopback performance targets met**. Passing trial
validation does not mean passing every performance target. Shared-VM variation
prevents a latency claim. The cumulative single-vector executable SHA-256 is
`f82da5948ff8afce5333faafe4f53e5add118ac37b60c7079dbb1399941dd4df`.
Its client/server instructions are 84,324,835 / 72,404,492 for pipelined reads,
71,811,902 / 63,698,993 for dependent reads, and 60,551,167 / 59,936,063 for
callbacks. The header and retirement changes reduce dependent client work;
pipeline work changes little. Single-vector writes alone establish matching
syscall selection, not a measured speedup. The empty-call instruction gate
remains client MISS/server PASS (18,436,059 / 17,031,436 versus legacy
17,361,979 / 16,354,390).

The cumulative changes pass 285 RPC, 135 wire, 58 futures and 92 integration
tests, 72 mixed-peer workflows, Clippy and formatting. Focused Miri verifies
the lazy-resolution observers, saturated header cache, cleanup/backpressure
matrix, retirement classification and contiguous partial-write/error paths.
All raw samples, profiles, controls, traces, patches and identities are retained
in `target/portable-performance-method-templates-20261016/`,
`target/portable-performance-retirement-publication-20261016/` and
`target/portable-performance-single-iovec-20261016/`, with per-file checksums.
No fixture, performance threshold or coverage floor changed.

The next `deferred-request20261016` experiment retains an intact no-pipeline
request in the pending wire projection's ordered queue. Forwarding can then
use the direct response completion instead of allocating a local-results
channel and joining it with another completion. Opaque hooks retain their
ordinary path; projected endpoints hold the parent interest through completion.
Mixed hook/direct calls preserve order, and canceled requests disappear before
forwarding. The parent-failure matrix also covers early pipeline publication.

It passes 286 RPC tests, 92 integration tests, 72 mixed-peer workflows, Clippy,
formatting and two focused Miri tests (including the eight-case lifetime matrix).
Pipeline client instructions fall 2.13% to 82,527,571; server instructions are
72,401,717. All 11 steady C++ write groups and batched Returns remain intact.
Empty client instructions increase 0.61%, and that separate gate remains
client MISS/server PASS. All 240 local trials validate, with **5/12 workflow
and 3/4 loopback targets met**; these noisy local timings do not establish a
latency improvement. Executable SHA-256 is
`9815f8e7f41b551d3e7d374f5a10e40de792de8a27b81f830897eb8b2ae31ad6`.
Raw evidence and exact source identity are retained in
`target/portable-performance-deferred-request-20261016/` (937 files plus checksums).
The committed earlier batch `600f649` subsequently completed dedicated workflow
`38904229528` with **9/16 targets: 8/12 workflows and 1/4 loopback payloads**.
All 350 remote and 60 loopback trials, five repetitions, p50/p95/p99, matched
reference selection and inclusive decisions were independently verified.
The seven misses are:

| Case | Portable p50 ns | Faster reference p50 ns | Ratio |
| --- | ---: | ---: | ---: |
| dependent-read | 419,583 | legacy 380,180 | 1.103643 |
| pipelined-read | 266,160 | legacy 215,372 | 1.235815 |
| callback | 416,400 | legacy 387,987 | 1.073232 |
| bulk-256k | 1,222,548 | C++ 1,110,273 | 1.101124 |
| loopback 0 B | 44,256 | legacy 39,555 | 1.118847 |
| loopback 1024 B | 46,208 | legacy 40,579 | 1.138717 |
| loopback 65536 B | 94,531 | legacy 84,771 | 1.115134 |

All 21 steady traced pipeline writes retain C++ Finish/Release/Call/Call
ordering, and both server Returns remain batched. Build provenance, all four
runtime manifests and recorded executable hashes agree. The source fingerprint
is `b99a7605c4089d501be881aa2867217813929a0af646f474d4e59a2c23b39d5d`;
driver SHA-256 is
`95966ba76703d0d8021b30c9ef47c724da3298fa1f4044cb929d745c7cbd89a8`.
Artifact `12049352687` has ZIP SHA-256
`0173be04fad84f9b90cb688a2474114e1d3d4a7bec6a48e33db60a7972dfa56c`.
The workflow and its existing quality checks passed; the separate sixteen-target
goal remains open. Differing network latency between campaigns prevents
attributing pass-count changes alone to the implementation. Raw evidence is in
`target/portable-performance-single-iovec-dedicated-20261016/`
(1,538 files plus checksums).

Outlining the shared plain-reply validation transition reduces pipeline
instructions another 0.30% client and 0.14% server, with 1.45% fewer simulated
client instruction-cache misses but more branch misses. Four existing wire
plain-return tests and all 72 mixed-peer workflows pass. The 240 local trials
validate, with **6/12 workflow and 3/4 loopback targets met**. Raw evidence is in
`target/portable-performance-outlined-plain-reply-20261016/` (932 files plus checksums).

Keeping forwarded request parameters concrete through admission and encoding
then avoids repeated dynamic downcasts without moving their original allocation.
The cumulative `owned-request-params20261016` candidate passes 286 RPC tests,
92 integration tests, 72 mixed-peer workflows, Clippy, formatting and the two
focused Miri cases. Pipeline instructions are 82,186,415 / 72,299,804, a 2.54%
client reduction from the committed single-vector batch. Empty-call instructions
are 18,384,025 / 16,995,718 versus matched legacy 17,362,399 / 16,354,788;
that separate gate still misses on the client and passes on the server.

All 240 local trials validate, with **7/12 workflow and 3/4 loopback targets
met**. Portable pipeline repetition medians range from 43,324 to 93,788 ns;
the pooled ratio is 3.208430 against the faster matched legacy control. These
misses and variability are retained, and no latency improvement is claimed.
The executable SHA-256 is
`836075ea0dd1f37f0e15ea6520b1d878b1bbf0e202d69e5b9e4035580aecd989`.
Evidence is in `target/portable-performance-owned-request-params-20261016/`
(1,340 files plus checksums, including publication and socket-timing evidence).
All 11 steady C++ pipeline write groups and batched
Returns remain intact in each candidate's paired syscall diagnostic.

The cumulative request changes are committed as `5891490`; dedicated workflow
`38909792248` completed successfully; PR CI `38909630783` is still running.
The dedicated campaign meets **11/16 targets**: 8/12 workflows and 3/4 loopback
payloads. All 350 remote and 60 loopback trials, pooled p50/p95/p99 and inclusive
1.05 gates are independently verified. The five misses are:

| Case | Portable p50 ns | Faster reference p50 ns | Reference | Ratio |
| --- | ---: | ---: | --- | ---: |
| dependent-read | 261,023 | 241,526 | legacy | 1.080724× |
| pipelined-read | 167,203 | 143,596 | legacy | 1.164399× |
| callback | 244,064 | 224,844 | legacy | 1.085481× |
| bulk-4m | 13,000,668 | 12,064,641 | C++ | 1.077584× |
| loopback-0 | 41,924 | 37,121 | legacy | 1.129388× |

All four runtime manifests agree with the build provenance and recorded binary
hashes; the checkout log identifies `5891490`. Source fingerprint is
`10bda1b461d11cb3f34b2ff7fa1c7cc87c10c2302fda65cef36f8c32782dec26`,
and driver SHA-256 is
`b996c82b41ef8018269e3b81d04a86b171cb3287e28327462bd7de32c7a8cfac`.
Artifact `12052276550` has ZIP SHA-256
`faedaaeb9fb13361a0dc8bb725bfe3382dddde3015666e6cf9e1d10256c20577`.
Evidence is in `target/portable-performance-owned-request-dedicated-20261016/`
(1,538 files plus checksums). All 21 steady pipeline client writes preserve
C++ ordering and both server Returns remain batched. The workflow passing does
not close the separate 16-target gap. Network conditions differ from earlier
campaigns; each target uses its faster matched control.

A separate lower-overhead
socket diagnostic recorded five rotating repetitions of 1,000 pipeline calls.
All 5,500 steady writes including warmup preserve the same batching, despite
variable read-to-write intervals. Portable client/server batches are 1,376/1,224
bytes versus C++ 1,368/1,216 bytes. These instrumented intervals do not enter
qualification; the remaining application and scheduling work is under analysis.

The `ordinary-reply20261016` experiment preserves certified idle application
lanes after ordinary server completion. Capability-bearing Returns still rescan
controls; maintenance, independent wakes and output pressure remain active.
It passes 287 RPC tests, 92 integrations, 72 mixed-peer cases and four focused
Miri checks. Pipeline server instructions fall 0.54% to 71,912,345 while client
instructions remain 82,186,305. The 240 local trials validate, with **6/12
workflows and 2/4 loopback targets met**. Pipeline remains a 1.862603 ratio miss.
Evidence is in `target/portable-performance-ordinary-reply-20261016/`
(951 files plus checksums); executable SHA-256 is
`1f3f0087a9cfe7cc2c55123c9f512e260f4ec15cd9f1af8c23873daed4d3500e`.

Word-level trace comparison identified an unnecessary staging pointer in the
child Call and capability-bearing Return. The parent Call was already 1,144
bytes. The `empty-stage20261016` candidate copies directly into an empty fixed
arena destination, restoring null and reclaiming all reservations on failure or
unwind. Replacing an existing value retains its separate transactional staging
slot. Portable now matches C++ steady pipeline frame sizes and grouping exactly:
**1,368 bytes client-side and 1,216 bytes server-side**, with all 11 steady
Finish/Release/Call/Call groups and both Returns batched.

The cumulative candidate passes 74 core, 135 wire, 287 RPC and 157 focused
integration tests, 72 mixed-peer cases, Clippy, formatting and wiki validation.
Four additional Miri checks cover exact capacity, partial-copy failure, nested
invalid indices, unwind, far pointers and recursive capability remapping.
Pipeline instructions fall another 0.43% client and 1.10% server to
81,834,863 / 71,123,670. Simulated client instruction-cache misses increase 0.20%
while server misses fall 0.50%. Empty-call instructions remain client MISS/server
PASS: 18,383,330 / 16,997,411 versus legacy 17,362,113 / 16,354,657.

All 240 local trials validate, but only **5/12 workflows and 0/4 loopback targets
pass**; pipeline is a 2.872247 ratio miss. All misses remain in the evidence,
and lower instruction counts do not establish a latency improvement. Executable
SHA-256 is `59840c351d3f5e6ae417d9eaa1f3015dad9b5c46304086d73a04642ebd52f287`;
runtime patch SHA-256 is
`0e027e27f2d1ff4570fbead8d05b1dab2752d4ba88b0f76e405681a5c3d81892`.
Evidence is in `target/portable-performance-empty-stage-20261016/`
(955 files plus checksums). These local experiments remain unqualified; the
latest completed dedicated campaign is the `5891490` 11/16 result above.

The cumulative `prepared-cap-results20261016` experiment reuses the local
prepared Return envelope when its payload contains capabilities. It validates
indices and reader limits, retains the same graph clients, and leaves descriptor
grant staging to the ordinary publication transaction. Oversized retained
segments fall back to the existing compacting codec. Rejection and retry leave
the original result and live answer usable.

It passes 288 RPC tests, 135 wire tests, 92 integrations, 72 mixed-peer cases,
Clippy, formatting, allocation-free core/wire builds and wiki validation. Four
Miri checks cover policy rejection and retries, early pipeline publication,
control rescans and cleanup. Initial test compilation and pool-count assertion
errors are retained alongside the corrected passing results. Pipeline server
instructions decrease **5.07% to 67,518,623** and dependent server instructions
fall **5.80% to 58,796,084** from empty-stage; client instructions are essentially
unchanged at 81,835,179 and 71,760,316. Empty-call instructions still miss on the
client and pass on the server: 18,383,608 / 16,997,540 versus matched legacy
17,362,618 / 16,354,936.

All 240 local trials independently validate, with **6/12 workflows and 3/4
loopback targets met**. Pipeline remains a 2.973518 ratio miss. Five rotating
socket diagnostics retain raw events for five workflows and all three variants.
The pooled pipeline read-to-write medians are 12,779 / 8,563 ns client/server
for portable, 5,258 / 6,189 ns for C++, and 3,235 / 4,116 ns for legacy. Steady
pipeline ordering, batching and 1,368 / 1,216 byte sizes match C++. These
instrumented intervals are diagnostic and do not enter acceptance. Instruction
savings do not establish a latency improvement.

Evidence is in `target/portable-performance-prepared-cap-results-20261016/`
(1,363 files plus checksums). Executable SHA-256 is
`342c8e12f4bd97d0f3707ae7345ac9880b1f65da2d33a88c06d930aeeaecea07`;
runtime patch SHA-256 is
`92e2f395da5ba4a96fde118449bdfe89aeb14b0901042fa67ccc67f02d0fdf91`.
The full 16-cell qualification remains open; fixtures and thresholds are unchanged.

The cumulative checkpoint `a18ef899` completed dedicated run `38916425998`
with **11/16 targets met**: 9/12 workflows and 2/4 loopback payloads. Independent
recomputation verifies all 350 remote and 60 loopback trials, five repetitions,
pooled p50/p95/p99 and inclusive 1.05 gates. Its remaining misses are:

| Target | Portable p50 (ns) | Faster reference p50 (ns) | Ratio |
| --- | ---: | ---: | ---: |
| dependent-read | 275889 | legacy 253655 | 1.087654 |
| pipelined-read | 187107 | C++ 158975 | 1.176959 |
| callback | 257088 | legacy 229643 | 1.119512 |
| loopback 0 bytes | 41044 | legacy 35436 | 1.158257 |
| loopback 65536 bytes | 93405 | legacy 80924 | 1.154231 |

All four environment manifests match the build provenance and recorded binary
hashes; source fingerprint is
`171c16d7547bdc1db2bbdd4889d2c638dd4adb0e8fe03cedcd2f2f0e1b32774b`,
driver SHA-256 is
`171f87321dd7b1156d57fe464a4400f8a6ccbbf39d155424e8092682859c9979`.
All 21 steady pipeline writes have C++ Finish/Release/Call/Call ordering;
23 server writes batch both Returns. Steady 1368/1216 byte sizes also match.
Evidence is in `target/portable-performance-prepared-dedicated-20261016/`
(1538 files plus checksums), artifact `12054851562` with ZIP SHA-256
`f90a3906b3bc505d542cb56aff689be17d6d752aef1d2168d1379883ee720e5f`.
The workflow passed its existing report gates; the separate full portable
performance target did not pass.

The following `segmented-output20261016` prototype added bounded immutable
output parts but connected them only to forwarded Request owners. Normal
generated calls use a different parameter owner, so their bulk copy remained.
All 240 local trials and 72 mixed-peer cases validated; 10/16 local targets
passed. Pipeline client instructions increased 1.23%. This negative result is
retained in `target/portable-performance-segmented-output-20261016/`
(1372 files plus checksums); it is not a claimed speedup.

The corrected `shared-call-params20261016` experiment freezes large, data-only
parameters after the normal request editor relinquishes ownership. Output
copies and patches the small envelope, then retains the original payload
allocation through physical writes and flush. Growing storage charges the
complete retained allocator capacity and one slot until the final owner drops.
Fixed storage, unsupported layouts and capacity pressure keep the copy path.
Partial writes preserve FIFO order and resource ownership.

The 64 KiB client profile falls from **251,608,997 to 181,641,314 instructions
(27.8%)**, with the large copy removed. Small workflow instruction counts are
nearly unchanged from the prototype. Five rotating socket diagnostics report
bulk client read-to-write p50 of 6169 ns (C++ 6320, legacy 4437), versus 7371 ns
for the earlier prepared-cap-results checkpoint. These instrumented intervals
are diagnostic, not acceptance evidence. All 240 local trials independently
validate, but only **6/16 local targets pass**; every miss is retained. The
empty instruction gate remains client MISS/server PASS.

Validation includes 74 core, 194 engine, 136 wire, 290 RPC, 92 integration and
72 mixed-peer cases, five Miri checks, Clippy, formatting, allocation-free builds,
unchanged unsafe-debt limits and wiki checks. The new coordinator test verifies
that the actual generated request retains the original payload allocation,
including a one-slot queue, blocked writes and flush, then reclaims all slots.
Initial measurement launch errors produced no timing samples and are retained.
Evidence is in `target/portable-performance-shared-call-params-20261016/`
(1356 files plus checksums); executable SHA-256 is
`9a5814450c02ff30b93f74d82806870981c349d67ac5d3dc506f212c2e05756c`,
runtime patch SHA-256 is
`9e07cbf42eb1400aa29b15ed2ddc6c5cae8cb396ce49f6bad43e8b204da32135`.
This experiment is not yet dedicated-qualified.

The cumulative `shared-return-parts20261016` experiment also retains large
completed Return payloads through hosted output, using their existing shared
owner. Validation and queue reservation still precede answer retirement; fixed
storage and capacity pressure fall back to copying. A one-step worker budget
keeps its existing staged-copy path instead of combining collection and
publication across the fairness boundary.

Bulk 64 KiB server instructions decrease **29.6%, 238,899,122 to 168,192,911**;
client instructions remain about 181.7 million. Small workflows are essentially
unchanged. Rotating bulk socket read-to-write medians are 6129/3635 ns
client/server (C++ 6309/5118, legacy 4467/2623). Pipeline remains substantially
slower in these diagnostics: 26314/19238.5 ns versus C++ 5628/6459. All 240 local
trials independently validate, with **9/16 targets met**; pipeline ratio is
2.974924. No local timing result replaces dedicated qualification, and no miss
is omitted. Empty instruction qualification misses on both peers: the server
ratio is 17,221,543 / 16,381,676 = 1.051269× (corrected from an earlier PASS).

Validation passes 137 wire tests, 291 RPC tests, 92 integrations, 72 mixed-peer
cases, Clippy, formatting and wiki checks. Two additional Miri checks validate
retained Return bytes, limits, capacity fallback, original application payload
identity, blocked writes/flush and complete slot reclamation. The earlier
unchanged core/engine, allocation-free and unsafe-debt results still apply.
Initial test compilation/expectation errors and a missing-TLC integration
configuration are preserved alongside the corrected passing runs.

Evidence is in `target/portable-performance-shared-return-parts-20261016/`
(1361 files plus checksums), executable SHA-256
`79e80af66b0609ae4c677bb0786ac4ebe6bd133e16aa1dd3faee5fed0670a8c7`,
runtime patch SHA-256
`7f3c67aa50569c1221a4cf02e489db02d3fd9dd2affe056c7523b0fc7fd5b638`.
The binary and source were frozen before measurement; the following queue
representation experiment is excluded. The complete 16-target goal remains open.

The committed ownership checkpoint `d396af2` completed dedicated run
[`38924362921`](https://github.com/DericHuynh/capntproto/actions/runs/38924362921)
with **11/16 targets met** (8/12 remote, 3/4 loopback). All 350 remote and 60
loopback trials independently validate, including pooled p50/p95/p99, five
repetitions and the inclusive 1.05 gate. The remaining misses are:

| Case | Portable p50 ns | Faster matched reference p50 ns | Ratio |
| --- | ---: | ---: | ---: |
| dependent-read | 595912 | C++ 563782 | 1.056990 |
| pipelined-read | 335524 | C++ 303371 | 1.105986 |
| callback | 596899 | legacy 565241 | 1.056008 |
| bulk-64k | 603005 | C++ 573686 | 1.051106 |
| loopback-0 | 34017 | legacy 32251 | 1.054758 |

The workflow passed its report-quality checks; the separate performance goal
remains open. Network latency differs from the preceding campaign, so only this
run's matched references determine these ratios. Traces preserve all 21 steady
C++ Finish/Release/Call/Call groups and all 23 paired server Returns, with the
same steady 1368/1216-byte sizes. Four environment manifests equal the build
manifest, recorded binary digests agree, and the checkout log names `d396af2`.
Source fingerprint is
`73b702e839c90e708fea6813e8775288c7f76ada3a698c90dd664dee83cbd815`;
driver SHA-256 is
`1238f6b7cf1bbe0300863589aaba15862201b4d2ad78e106e252a4fa94933adc`.
Artifact `12059030615` has SHA-256
`ccb68487437d99b9ad621766a6a95760fc4711bb7bba2e83cc5ed96f825680cf`.
Evidence is in `target/portable-performance-retained-dedicated-20261016/`
(1541 files plus checksums). Later queue, observer and selected-reader changes
are excluded from this campaign.

The subsequent `compact-output-window20261016` experiment replaces mutually
exclusive queue-window options with an enum. FIFO progress, borrowed-buffer
release order and shared-payload ownership are unchanged. All 240 local trials
and 72 mixed-peer cases validate, with **8/16 targets met**. Pipeline instructions
decrease only 0.167%/0.101% client/server; simulated instruction-cache misses are
essentially unchanged. Evidence is in
`target/portable-performance-compact-output-window-20261016/` (1352 files plus
checksums), executable SHA-256
`2e031443ab4442f4f0438aa3dfe9eab5420fabdc4846907b2fa555ebda0b7028`.
The 194 engine tests, 291 RPC tests, two engine Miri checks, Clippy and
allocation-free checks pass; unchanged wire/core/integration checks are inherited.

The cumulative `waiting-observers20261016` experiment also avoids scanning
outgoing observers when no authorized Return exists. It checks live exchange
state on every call, keeps the conservative path for tables larger than eight
slots, and does not skip invocation-pipeline binding or abandoned-client cleanup.
The exact-question/slot-reuse test now covers pending questions, unauthorized
Returns and immediate invalidation when a Return becomes authorized.

Pipeline instructions fall another **1.25% on the client**, from 82,735,526 to
81,701,098; server instructions are 67,894,233. Other traced workloads are
essentially unchanged. Pipeline simulated instruction-cache misses fall
1.91%/1.69% client/server. All 240 local trials independently validate, with
**10/16 targets met**, and all 72 mixed-peer cases pass. Pipeline socket
read-to-write medians remain 13179/8723 ns, versus C++ 6380/7401 and legacy
3385/4296. Matching steady-state message batches has not eliminated the CPU
gap. Empty-call instruction qualification remains client MISS/server PASS.

Validation passes 137 wire tests, 291 RPC tests, 92 integrations, the focused
observer Miri test, Clippy, formatting and allocation-free checks. The initial
helper-visibility compiler error is recorded in a note because its original log
was overwritten. Evidence is in
`target/portable-performance-waiting-observers-20261016/` (1359 files plus
checksums), executable SHA-256
`249c7dea54b52d1bccc3b18fa80023f23ab2abd085b10318f22bb0f3321efb61`,
runtime patch SHA-256
`ca3fb2955dd5b600d588490268e9b2c85408f8414a8ba2f8f96a0672ee4228ce`
against `96c4219`. These local results do not replace dedicated qualification.

Two verification fixtures required updates to follow the implementation.
`bounded_generic_fields` now expects exact-fit success without the eliminated
scratch pointer word, while retaining insufficient-capacity rollback checks.
All five portable profiles (436 tests) and focused Miri on x86-64, i686 and
big-endian s390x pass; evidence and the original CI failures are in
`target/portable-performance-empty-stage-fixture-fix-20261016/`.
The next CI run `38926087383` rejected an ambiguous `if complete` mutation
marker after segmented output added a second completion branch. The marker now
selects the original buffer-release branch, preserving the same FIFO-ordering
fault, harness and expected assertion. All 62 development-tool tests pass and
the final marker matches exactly once in both committed and candidate code.
Evidence is in `target/portable-performance-queue-mutation-marker-20261016/`;
Kani execution remains pending full CI. No benchmark fixtures, limits, coverage
floors, mutation controls or unsafe-debt allowances were relaxed.

Outlining the call-lane adapter (`outlined-calls20261016`) was rejected and
reverted: pipeline client instructions increased 0.191%, and instruction-cache
misses increased 0.115%. Four long workload profiles and pipeline cache profiles
are preserved in `target/portable-performance-outlined-calls-rejected-20261016/`
(33 files plus checksums); no full timing campaign was run for that experiment.

The next `selected-payload20261016` experiment retains the already admitted
content pointer with `CheckedPointerReader`, avoiding another Call/Return/Payload
ancestor decode. The existing checked owner verifies every immutable segment's
identity and extent. Capability slots retain their exact admitted hooks, and
fixed storage retains its copy fallback. No new unsafe code is introduced.
The retained-frame test additionally rejects equal bytes from a foreign message.

Pipeline client instructions decrease 0.72% to 81,114,999, dependent client
instructions decrease 0.81% to 72,036,646, and callback server instructions
decrease 0.94% to 59,912,000. Pipeline simulated instruction-cache misses are
5,499,867/4,846,761 client/server. Validation passes 291 RPC tests, 92 integrations,
72 mixed-peer cases, two focused Miri tests, Clippy and formatting. Both initial
test compilation errors are preserved with the corrected passing runs.

All 240 full local timing trials independently validate, but only **10/16 targets
pass** (8 remote, 2 loopback). Pipeline ratio is 3.101199, and rotating socket
medians regress to 28492/17641 ns versus C++ 5519/6279 and legacy 3395/4256.
This triggered 40 alternating previous/new/reference trials, all independently
validated. Their pipeline pooled medians are previous 57575 ns, selected 44606,
legacy 28633 and C++ 43024. Multiple variants show large variation between
repetitions. Both the original regression and the follow-up are retained; these
results establish no broad latency improvement. Dedicated qualification is
required. The separate empty instruction gate remains client MISS/server PASS.

Evidence is in `target/portable-performance-selected-payload-20261016/`
(1523 files plus checksums), executable SHA-256
`690e9773bb8fcb40c89ad6ab9746bca3f51deadc5c0d3da0470096f361523ba0`,
runtime patch SHA-256
`aeb70797bbd2a2949acbe4a49bb7cbd85eebb9be95f2acb38d875b99987efd45`
against `d917300`. The latest completed dedicated result remains `d396af2`
at 11/16; the full target remains open.

The earlier `5891490` PR CI run `38909630783` passed 2251 Cargo tests with 20
skips and no incomplete or failed tests. Its unchanged coverage floors failed;
all other executed jobs passed. The coverage report and full job log are saved
in `target/portable-performance-owned-request-ci-20261016/` alongside the
previous full gap issue body. Coverage and performance requirements remain
unchanged.

The earlier `ef9f4c8` Cargo CI completed 2,230 tests with zero failures and
20 skips, but its coverage regression gate failed. The Miri job reached its
45-minute limit after the 228-test 32-bit RPC matrix passed, leaving later
replay tests incomplete. The job allowance is now 90 minutes with all checks
retained. Coverage requirements and performance limits are unchanged; both
coverage qualification and the full performance target remain open.

The `pending-direct-calls20261016` experiment tried a bounded live-state idle
certificate for groups of direct calls. Native validation passed 137 wire,
292 RPC and 92 integration tests; two focused Miri checks covered cancellation
and late pipeline projections under backpressure. Pipeline client instructions
nevertheless increased 0.39% to 81,430,993, and instruction-cache misses rose
0.45%. It was rejected and all six changed files restored. Profiles, 45 paired
CPU/wall trials, 75 wall-only trials and the source patch are retained in
`target/portable-performance-pending-direct-calls-rejected-20261016/`.
Executable SHA-256 is
`d15c5cdfaa9a419572cb2d9014a7e21df433cce2623ec5bdf4310d676d96598f`;
patch SHA-256 is
`ef3f3ae5b9a7c66202828a717fe299b8128b2bf3c01688c77ad19435462d6b0f`.

New traces compare elapsed and thread CPU time between socket reads and writes
for the committed selected-content baseline. Across five rotated repetitions,
pipeline CPU medians are 13190/9103 ns for portable, 3616/4707 ns for legacy,
and 6079/6940 ns for C++ (client/server). Wall and CPU intervals track closely,
supporting investigation of processing work rather than attributing the entire
gap to descheduling. Repetition values remain bimodal. Clock instrumentation
adds overhead; these 45 trials never enter qualification.

SIGPROF instruction-pointer sampling over 100000 iterations complements those
intervals. Coordinator polling, hook/ledger maintenance, decoding and copying
contribute across the portable path. Samples at syscall-return sites include
charged kernel CPU and do not identify user-space cancellation as a bottleneck.
The first mapping snapshot and nearest-symbol analysis were incomplete; both
are retained alongside a repeat with final library mappings, executable ELF
address translation and DWARF resolution. Use the mapped campaign's
`dwarf-summary.json`. The complete diagnostic archive is
`target/portable-performance-cpu-tracing-20261016/` (313 files plus checksums).

The `buffered-return-pair20261016` experiment then admitted one buffered successor
after a Return before ordinary scheduler work. All 291 RPC tests passed,
including stronger independent-control wake coverage, but pipeline instructions
increased to 88,650,786/71,339,646 (9.29%/5.08%). Decoded traces identify the
consequence: portable wrote Finish separately, followed by Call/Call/Release,
instead of the committed baseline and C++ Finish/Release/Call/Call group.
The extra write and delayed cleanup added scheduler and maintenance work.
The server still grouped Return/Return. This experiment was rejected and both
files restored; no full timing campaign or Miri run was justified after rejection.
The 321-file archive plus checksums is
`target/portable-performance-buffered-return-pair-rejected-20261016/`.
Executable SHA-256 is
`ff5f8973d6fb2ca5aa9f4108b746f27ebcaccdcdcef13d6225f389324a1e17e0`;
patch SHA-256 is
`38a2d47a497148776a64187180c67fbb958c1fe15eed91c78eac1865c8cb750b`.
These findings favor reducing work inside the existing ordering. They do not
close the dedicated performance gap.

The selected-descriptor experiment reuses the descriptor already decoded by
`Payload::capability_at`, avoiding a second cap-table decode while preserving
the same receipt and index checks. Native validation passed 137 wire, 291 RPC
and 92 integration tests, plus focused Miri, hosted Clippy, no-allocation checks
and 72 mixed-peer cases. Pipeline client instructions decreased 0.2924% to
80,877,817; cache misses did not improve. All 240 local timing trials validate,
but only 8/16 targets pass. The empty instruction gate remains client MISS,
server PASS. The 1357-file archive plus checksums is
`target/portable-performance-selected-descriptor-20261016/`, with executable
SHA-256 `469480307b58731117b2846bf08657c575c27e78c01c8f6de62b72c85da69254`
and runtime patch SHA-256
`38e2ccd1cd79e205da83ce13f012e10c5a9e1d802407825cd42b05f1bb9e5a19`.

Dedicated run `38931617998`, commit `57f873f`, subsequently completed at
**13/16**: 10/12 remote and 3/4 loopback targets pass. The remaining ratios
against the faster matched reference are pipeline 1.173179 (132886 ns versus
C++ 113270), callback 1.087431 (202832 versus legacy 186524), and empty loopback
1.053022 (28122 versus legacy 26706). All 350 remote and 60 loopback trials,
percentiles, repetitions and inclusive gates were independently recomputed.
All four embedded manifests, recorded executable hashes, build provenance and
checkout commit agree. Source fingerprint is
`54d2e70d96625fcbb87f11044d6f21821f9840e495f5f22feec32a64c01755e8`;
driver SHA-256 is
`05588ff52299e7ad1786a5cc4bdda43240dd28e308d18fea7e3a2397f7444c00`.
The report artifact `12061517121` has SHA-256
`1937ac3709c79538beb61da95e54b1efffa8161ff37f4ce21e8d2a5224c701f9`.
Complete evidence is in
`target/portable-performance-selected-dedicated-20261016/`
(1541 files plus checksums).

Decoded steady pipeline writes still match C++: 21 client groups of
Finish/Release/Call/Call, 1368 bytes, with both server Returns grouped into
1216 bytes. Instrumented timing does not enter qualification. Network conditions
differ from the previous dedicated campaign, so the score change alone does
not establish a causal speedup. This campaign excludes the subsequent
selected-descriptor experiment. No 16/16 campaign has passed.

Full CI `38928495060` at `d917300` also completed: 2262 Cargo tests passed,
20 skipped, zero failed or incomplete. Full Miri and TLA+ passed. The Cargo job
failed 53 unchanged coverage metric floors across 32 files plus the
async-framing aggregate. Coverage qualification remains open; no floor was
relaxed. The Cargo artifact `12061691233`, SHA-256
`dd462c7972c692f6b9cf66943c930fb41a6d0a3953f1d681d4071427769098be`,
full job log and summaries are retained in
`target/portable-performance-waiting-ci-20261016/`.

The next checkpoint stores one capability's parallel receipts inside the
existing heap box, reducing four allocations to one while keeping empty
zero-inline storage one pointer wide. Growth reserves all arrays before moving
owners, and capacity policy still applies after allocation. A two-slot prototype
passed native tests but failed the large-enum lint; the one-slot refinement
passes without suppression. The alloc-only HeapAllocator path also avoids a
literal-None unwrap, with identical allocation semantics and unchanged reviewed
unsafe-debt counts.

This checkpoint passes 139 wire, 74 core, 291 RPC and 92 focused integration
tests, allocation-only and hosted Clippy, allocation-free checks, formatting,
unsafe review and focused receipt-ownership Miri. All 72 mixed-peer cases pass.
Pipeline instructions are 80,261,486/67,901,389 (client/server), a further 0.7621%
client reduction from selected-descriptor; dependent client work decreases
0.8618%. Client instruction-cache misses do not improve. All 240 local trials
validate, but only 8/16 targets pass (6 remote, 2 loopback). Empty instructions
remain client MISS/server PASS. No latency improvement is established.

Decoded writes preserve C++ Finish/Release/Call/Call and Return/Return grouping.
Across five rotated CPU-timing repetitions, portable pipeline client medians
are 31136, 14001, 12979, 30105 and 12258 ns; server medians are 25718.5, 9439,
9084, 25898 and 8633 ns. Event counts and write groups agree across these runs,
but processing intervals remain bimodal. These diagnostics never enter timing
qualification. Evidence, including the rejected two-slot prototype, is in
`target/portable-performance-small-one-receipts-20261016/`. Executable SHA-256:
`a38599981d8dc8755966f98be72d20573a8202fa7dbc1259b779f4f626d5197c`;
runtime patch SHA-256 against `57f873f`:
`169c32c49336e330fca88ff39e92250b36a7db80e070a57030354ab75fa5dd44`.

Dedicated `--trace-io` runs now also collect separate wall/thread-CPU socket
journals after every uninstrumented measurement and syscall trace. Five rotated
repetitions cover empty, pipeline and callback workflows, with 100 warmups and
1000 iterations each. The Linux probe keeps a fixed in-memory journal; it
captures no payload and writes no logs on the measured path. Two CPU clock
syscalls per intercepted operation add overhead. The analyzer pairs successful
reads and writes only on the same thread and socket, rejects corrupt or
nonmonotonic journals, and verifies the probe source, shared library and both
benchmark executables against recorded hashes. Raw samples and both clocks are
retained independently from acceptance results.

The 62 development-tool tests pass, including injected diagnostic failure with
server shutdown and artifact recovery. Parser tests reject truncation, overflow,
invalid records and cross-thread/socket pairing. Native timeout/TERM lifecycle
checks pass for portable, legacy and C++, and analysis of all 90 local journals
agrees with the independent local analyzer. Use the retained `analyze.py` with
its socket-timing directory and `--manifest` pointing to the exact bundle
manifest. The subsequent remote timing results are recorded below.

Dedicated run `38940986481` at `a2822536` subsequently completed with **11/16
targets met** (9 remote, 2 loopback). All 350 remote and 60 loopback trials,
pooled percentiles and inclusive gates are independently verified. Its misses are:

| Case | Portable p50 ns | Faster reference p50 ns | Reference | Ratio |
| --- | ---: | ---: | --- | ---: |
| dependent-read | 614,627 | 575,725 | C++ | 1.067570× |
| pipelined-read | 335,690 | 318,029 | C++ | 1.055533× |
| callback | 603,193 | 563,858 | legacy | 1.069760× |
| loopback 0 bytes | 34,754 | 32,776 | legacy | 1.060349× |
| loopback 64 bytes | 34,384 | 32,713 | legacy | 1.051081× |

The four manifests, build/runtime evidence, recorded binary hashes and checkout
commit agree. Source fingerprint is
`e0042d96501f400519f7dcf15c927abb8cfd3247b2b9f51b8de03be5016c950b`;
driver SHA-256 is
`95588ad1fd290b1114f13ab26e4c6947feb180f8af194adf672beee601b25d10`.
Artifact `12065879162` has ZIP SHA-256
`074e71ffc6cc31a2fe4c7a1970fd43760c4bbf0a0d5c340a958fa11b18621f15`.
Complete evidence is in `target/portable-performance-small-one-dedicated-20261016/`
(1,824 files plus checksums). The workflow and host cleanup passed; the separate
sixteen-target goal remains open. The empty reference median is 280.153 µs,
versus 89.349 µs in the preceding campaign, so pass-count changes cannot be
attributed to code alone.

All 21 steady pipeline writes still match C++ Finish/Release/Call/Call and
1,368-byte size; both Returns share a 1,216-byte server write. The new journals
independently verify each peer's executable and probe hashes. Pooled pipeline
read-to-write CPU medians are 29,829/31,927.5 ns portable client/server, versus
18,348/31,449 ns C++. Callback medians are 12,868.5/15,611.5 ns portable versus
6,451/8,419 ns legacy. These instrumented intervals identify processing work;
they are not acceptance samples or evidence of a latency improvement.

The next prototypes carry an immutable checked payload and its descriptor table
from receipt initialization into transactional Call/Return admission. They avoid
repeating content traversal while retaining descriptor identity, resource, and
ledger checks. Invalid indices are rejected before import factories. The combined
prototype passes 141 wire tests, Clippy, allocation-free builds and three focused
Miri checks, including mixed receipt rollback. Pipeline client instructions fall
0.62% to 79,764,963; server instructions are 67,936,013. Callback server work falls
0.66% to 59,647,171. Simulated pipeline instruction-cache misses fall 1.37% client
and 0.34% server. Elapsed-time qualification has not yet run for this intermediate
prototype. Evidence is in `target/portable-performance-checked-return-prototype-20261016/`
(275 files plus checksums) and `target/portable-performance-checked-payload-prototype-20261016/`
(51 files plus checksums). The latter executable SHA-256 is
`078a0b5bb422d1928721e433f4bb0f41b225f83e626140b2c8115b002ff4cfb4`.

A subsequent existing-hook reuse experiment passed 294 RPC tests, four-crate
Clippy and focused Miri. It lowered pipeline instructions slightly to
79,600,382/67,961,276 but increased client instruction-cache misses 1.26% relative
to checked-payload. It was rejected and reverted before elapsed-time testing.
Its source, regression, executable identity and raw profiles remain in
`target/portable-performance-existing-hook-rejected-20261016/` (46 files plus
checksums). Executable SHA-256 is
`e008af75e9517e665d9e20c7230666a67646b70d3b0cbb193906af8c4dd0a335`.

The paired-receipt box experiment reduced callback server instructions a further
1.03%, but increased pipeline client instructions 0.21% and simulated
instruction-cache misses 1.22% relative to checked-payload. It was rejected and
reverted after 141 wire tests, five allocation tests and alloc-only Clippy passed.
The larger box affected ordinary single-capability replies as well as calls.
Evidence is in `target/portable-performance-paired-receipts-rejected-20261016/`
(52 files plus checksums); executable SHA-256 is
`61d4d8faf601ef7bb089180595123f2e4c3f47cbba798b74f5442004ada8cce9`.

The next prototype keeps the one-receipt box and temporarily pairs its payload
receipt with the call target on the stack. A scope guard restores persistent
receipts on success, rejection and unwinding. Capacity still includes the target,
and duplicate export references pass through one atomic admission batch. All 144
wire tests, allocation-only Clippy, allocation-free builds and focused Miri checks
pass, including one-capability capacity rejection and retry. Callback server
instructions fall another 1.10% to 58,992,090; pipeline instructions are
79,759,282/67,994,320. Simulated instruction-cache misses rise to
5,530,163/4,918,476, so elapsed-time evaluation remains necessary. Initial evidence
is in `target/portable-performance-stack-target-prototype-20261016/` (49 files
plus checksums); executable SHA-256 is
`5e855833343d02ef730074cad09c17ba4b16a5cc5356c59b9e98956459f3af5b`.

A bounded scheduler check for retained vacant call slots passed 293 RPC tests,
four-crate Clippy and out-of-order/cancellation Miri. It did not reduce work in
the matched fixtures: pipeline instructions rose to 79,821,084/68,004,549.
It was rejected and reverted. Evidence is in
`target/portable-performance-vacant-call-rejected-20261016/` (49 files plus
checksums), executable SHA-256
`7b0f9ccfab3b341c8373ab2dc5459307d704e14ca17a6d3bde31227cf9ad88cb`.

The complete stack-target local campaign validates 180 workflow and 60 loopback
trials, all pooled percentiles and gates, and 72 mixed-peer cases. It meets 11/16
local targets (8 remote, 3 loopback), with empty, dependent, pipeline, callback
and zero-byte loopback still missing. No latency improvement or dedicated
qualification is established. Forty-five rotated CPU/wall trace trials give
pipeline CPU p50 of 13,369/9,594 ns portable client/server versus 5,779/6,660 ns
C++; callback gives 5,473/4,972.5 ns portable versus 2,073/1,832 ns legacy.
Decoded steady pipeline writes remain Finish/Release/Call/Call (1,368 bytes),
with both Returns in one 1,216-byte write. Callback Return/Release batching
continues to match C++.

A diagnostic-only Callgrind probe collects 800 windows from the first successful
socket read to the following write entry after skipping 210 completed windows.
It counts additional reads in each window and rejects foreign sockets/threads,
failed selected writes and unfinished selected windows. The initial probe's
rejection of multiple legacy reads is retained. Portable and C++ pipeline
windows transfer identical byte totals. Portable client simulated instruction
cache misses are 4,402.24/window versus 1,577.89 C++; conditional branch misses
are 1,018.02 versus 213.69. These are simulated counters, not measured hardware
events. Legacy batching differs, so its per-window values are not per-workflow
ratios. The complete campaign, traces, probe sources, frozen identities and
rejected diagnostic are in `target/portable-performance-stack-target-20261016/`
(1,311 files plus checksums). The empty instruction gate remains client MISS
(18,672,387 versus 17,389,291) and server MISS (17,237,616 versus 16,381,648; 1.052252×).

An intermediate shared ownership-refresh loop reduced generated code from
68,729 to 64,453 bytes for the connection driver, 27,589 to 20,574 for active
work, and 12,116 to 8,725 for budgeted work. It passed 293 RPC tests, Clippy and
resolution-observer Miri. The dynamic metadata callback added 0.91% pipeline
client instructions (80,484,145), while simulated client instruction-cache misses
fell 1.53% to 5,445,517. The 45 CPU/wall trials remain bimodal and establish no
latency gain. Its 18 gated cache profiles and exact implementation are archived
in `target/portable-performance-shared-collection-prototype-20261016/`
(346 files plus checksums), executable SHA-256
`351fc2a6dc2a8d23df7389212dae05b44a25127d1190f0caa13b7e033dba477c`.
The dynamic variant was restored before testing a common concrete helper at
the six coordinator refresh sites, preserving their original observation points.

The concrete refresh candidate keeps static metadata dispatch and shares only
the six identical coordinator walks. It reduces connection-driver code to
64,373 bytes and active-work code to 23,748 bytes, with a 3,361-byte common
helper. Pipeline instructions are 79,858,900/68,070,929, versus
79,759,282/67,994,320 for stack-target; simulated instruction-cache misses are
5,514,955/4,867,661, versus 5,530,163/4,918,476. The change preserves all refresh
points, receipt checks and notification ordering; it does not cache absent
metadata or skip later aliases.

All 180 workflow and 60 loopback trials have independently verified pooled
percentiles and gates, and all 72 mixed-peer cases pass. The candidate meets
11/16 local targets (8 remote, 3 loopback); empty, 1 KiB unary, pipeline,
callback and 64-byte loopback miss. Different cells pass between local campaigns
because the shared-VM controls vary substantially; the pass count establishes
neither a latency gain nor dedicated qualification. Forty-five rotated CPU/wall
trials give pipeline read-to-write CPU p50 of 13,230/9,074 ns portable versus
5,868/6,760 ns C++. The 18 gated cache profiles give portable pipeline client
61,095.11 instructions and 4,396.78 simulated instruction-cache misses per
selected window. The empty instruction gate remains client MISS
(18,645,544 versus 17,388,575) and server MISS (17,259,199 versus 16,380,933; 1.053615×).
The frozen executable SHA-256 is
`e8fe90c1752af11d68a540152844555d035d863f7d495358560e4c6bf833fbda`.
Native RPC tests (293), RPC Clippy and the capability-control Miri test pass.
The metadata Miri filter selected zero tests because that test excludes Miri;
it is not counted as a successful Miri test. The unsafe documentation gate and
formatting pass after removing three redundant casts in a regression test.
The release executable's eight runtime source hashes remain unchanged.
All 92 integration tests and the four-crate hosted Clippy check pass. Decoded
steady pipeline writes retain the C++ grouping of Finish/Release/Call/Call
(1,368 bytes) and both Returns (1,216 bytes); callback Return/Release writes
also match. Bootstrap and final cleanup differences remain in the raw traces.
The complete local campaign, probe sources, failed preliminary checks and
frozen source identities are retained in
`target/portable-performance-concrete-refresh-20261016/` (1,254 files plus
checksums). Full dedicated performance and CI coverage qualification remain open.

The completed dedicated campaign at `6f82f8bd` passes 12/16 targets after
independent recomputation of all 410 trials and pooled p50/p95/p99 values.
The remaining ratios to the faster matched reference are dependent-read
1.052058×, pipeline 1.111865×, 64 KiB 1.052453× and empty loopback 1.077783×.
All four manifests, available executable digests, build/runtime source identity
and checkout commit agree; workflow and cleanup succeeded. Steady pipeline
writes still match C++ Finish/Release/Call/Call in 1,368 bytes and paired Returns
in 1,216 bytes. Ninety validated CPU/wall journals give pipeline read-to-write
CPU p50 of 40,812/19,366.5 ns portable client/server versus 29,381/13,726.5 ns
C++; these instrumented intervals remain diagnostic. Complete evidence is in
`target/portable-performance-concrete-refresh-dedicated-20261016/` (1,827 files
plus checksums). The source fingerprint is
`985e165cee56a8157f6504c5684649f03226d030aeaa3a39954b1cc43b60a1aa`;
driver SHA-256 is
`6996a6239d88bedcd18c2e9045d0cb51e0d8091d5ffe90d8332999b1f8795ded`.
This excludes the subsequent allocation changes below. Its unfinished full CI
was canceled by the PR concurrency rule when `6c63432b` was published, so it
does not establish coverage qualification. No 16/16 campaign has passed.

Follow-up tracing found three temporary allocations per pipeline iteration:
forwarding cloned a projected hook and its inner endpoint, then projected-target
resolution boxed a hook before moving it into shared storage. Borrowed forwarding
now admits through the queue's retained target and constructs the shared target
directly. Opaque/introduced/streaming/pipeline-only requests keep their existing
fallback, and the admitted call retains its endpoint and parent interest.
All 293 RPC tests, hosted Clippy and two focused Miri checks pass, including
cancellation after forwarding when public parent/projected owners have dropped.
Pipeline instructions fall from 79,858,900 to 79,164,876 on the client; server
work stays essentially unchanged. Full-process malloc/free counts each fall
by exactly 3,300 across 100 warmups and 1,000 iterations. Client simulated
instruction-cache misses fall to 5,424,665 and conditional branch misses to
1,181,628. The 45 CPU/wall trials show no clear pipeline latency gain
(13,260/9,344 ns portable client/server versus 5,909/6,770 ns C++). This
intermediate's four workflow profiles, six gated pipeline profiles and raw
timing journals are retained in
`target/portable-performance-borrowed-forwarding-prototype-20261016/`
(305 files plus checksums), executable SHA-256
`f039e88d43be8540e03252da3ceb72b5f19039f0942dc5964a74dacaacb750eb`.
It has no separate complete 240-trial or dedicated qualification campaign.

The next candidate retains two exclusive empty completion states per inbox,
covering overlapping parent/child requests without an unbounded cache. Retained
owners still prevent reuse, overflow states are released, and each state retains
its own shutdown subscription. All 294 RPC tests, Clippy and three focused Miri
checks pass. Pipeline client malloc/free counts each fall another 2,198;
instructions fall to 78,677,222, 1.48% below concrete-refresh. Simulated client
instruction-cache misses are 5,404,086. The frozen executable SHA-256 is
`9a7c21e4879ae8c1b780fe7a32c6a1c6e3de19aafd87fce93415a43e871899c8`.
All 180 workflow and 60 loopback trials independently validate; 7/16 local
targets pass (5 remote, 2 loopback), and all 72 mixed-peer cases pass. All
92 integration tests, formatting, documentation and unsafe-documentation checks
pass. No elapsed-time improvement or dedicated qualification is established.
The 45 CPU/wall diagnostic trials give pipeline CPU p50 of 18,843/13,380 ns
portable versus 5,979/6,730 ns C++, with strongly variable repetitions. The
18 gated profiles preserve identical selected pipeline and callback byte totals
for portable and C++. Pipeline client instructions are 60,016.10 per selected
window. Both empty instruction peers miss: 18,644,408/17,259,128 portable versus
17,388,609/16,380,973 legacy (1.072220×/1.053608×).
Complete evidence is retained in
`target/portable-performance-two-completion-cache-20261016/`.

A subsequent exact-integer audit corrected the earlier stack-target and
concrete-refresh empty-call server classifications: their ratios are 1.052252×
and 1.053615×, so both miss the inclusive 1.05× instruction gate. The raw
counters and latency classifications are unchanged. Original evidence remains
immutable; the correction and source hashes are in
`target/portable-performance-instruction-gate-correction-20261016/`.
The preceding small-one receipt server result, 1.049440×, still passes.
An expanded audit verifies 69 same-executable portable/legacy pairs. It also
corrects the older shared-return server classification (1.051269×); the
accounted-batch server ratio is 1.050155× and misses despite rounding to 1.05.
One older portable sample lacks a same-executable legacy control and remains
unclassified. All audited counters, source hashes and exact integer decisions
are preserved in `target/portable-performance-instruction-gate-audit-all-20261016/`.

A shared callback type now gives immediate, scanned and local completions the
same wire poller. The first-refresh idle certificate retains its original
semantics and every later refresh remains observable. The two main completion
implementations (42,587 and 42,609 bytes) become one 43,075-byte implementation,
removing 42,121 bytes. All 294 RPC tests, Clippy and resolution-observer Miri pass.
Pipeline client instructions fall slightly to 78,572,023; simulated instruction
cache misses fall 0.78% to 5,361,833. Server instructions remain essentially
unchanged, but simulated conditional/indirect branch misses rise to
887,155/284,955 from 850,333/261,861. The 45 CPU/wall trials remain variable and
do not establish an elapsed-time gain. Eighteen gated profiles retain matched
portable/C++ pipeline and callback byte totals. This intermediate has no separate
complete latency qualification. Its source, four workflow profiles and timing
evidence are in `target/portable-performance-common-completion-prototype-20261016/`;
executable SHA-256 is
`ff8906b3ee97ca581cf8af16f25cb204aa854811ae6eb909615d72795a81014e`.

The next candidate keeps one promised-answer field inline in the hosted driver.
The configured path limit still governs both inline and growing storage. This
removes two path allocations on each peer per pipeline workflow, verified by
2,200 fewer malloc/free calls per peer across 100 warmups and 1,000 iterations. All 294 RPC tests, Clippy and the inline-budget/owner-growth Miri test
pass. Pipeline instructions fall to 78,189,410/67,577,157; client simulated
instruction-cache misses rise to 5,437,800, while server misses fall to
4,797,229. All 240 local trials and pooled gates validate, with 10/16 cells
passing (6 remote, all 4 loopbacks); small unary, dependent-read, pipeline,
write-read, callback and 64 KiB miss. All 72 mixed-peer cases and 92 integration
tests pass, as do formatting, 102-document validation and unsafe documentation.
Both empty instruction peers miss (1.071764×/1.053348×). No causal latency gain
or dedicated qualification is established.

Forty-five rotated CPU/wall trials and 18 gated cache profiles remain diagnostic;
all three sampled cases have equal portable/C++ selected read/write byte totals.
A separate SIGPROF probe gates instruction-pointer samples to read-to-write
processing, excluding wrapped socket syscalls and startup/final windows. The
pipeline sample counts are 501/325 portable client/server, 144/153 legacy and
172/206 C++. They suggest distributed pointer/layout, maintenance and scheduling
costs; they do not measure hardware cache misses or establish latency effects.
C++ source mapping is incomplete. Unary portable captured only 54/40 samples;
its original minimum-sample check stopped the run before legacy/C++ unary, so
no complete unary sampling comparison is claimed. The earlier over-budget launch
and unmatched final-cleanup window are retained as rejected diagnostics.
Complete evidence is in
`target/portable-performance-inline-pipeline-field-20261016/`; executable SHA-256
is `9038f2c43f6ad593ec3f4de1d3177dc254b258c9c4843a7b910c411236f0e865`.

The next receive-path check declines canonical non-Return frames and Returns
which still require Finish before attempting shared plain-result storage. It
only chooses the ordinary decoder; all admission and ownership validation still
runs there. This avoids a speculative buffer copy and decoding work. Whole-process
allocator counts remain unchanged because the shared buffer is reused. Against
the preceding inline-path candidate, client instructions fall 0.60% for pipeline,
0.67% for dependent-read and 0.68% for callback; server instructions remain nearly
unchanged. Pipeline instructions are 77,716,749/67,577,221. Simulated instruction
cache misses are 5,430,296/4,841,321, a slight client reduction and server increase.
All 145 wire tests, 294 RPC tests, hosted Clippy, the allocation-free wire build
and three focused buffered-return Miri tests pass. The new regression verifies
zero allocations and unchanged question/input ownership on both declined cases.

All 240 local trials independently validate, with 10/16 targets passing
(8 remote, 2 loopback); unary 1 KiB, dependent-read, pipeline, remote 64 KiB,
loopback 1 KiB and loopback 64 KiB miss. All 72 mixed-peer cases pass. Both empty
instruction peers miss (1.072951×/1.053363×). Forty-five rotated CPU/wall trials
remain strongly variable: pipeline CPU p50 is 28,893/24,577 ns portable versus
6,469/7,281 ns C++. Eighteen gated profiles retain matching portable/C++ byte
totals for all three cases. Pipeline client work is 59,203.11 instructions per
selected window. Steady pipeline and callback message batching remains matched
to C++; no causal latency improvement is established. Evidence is retained in
`target/portable-performance-nonplain-prefilter-20261016/`; executable SHA-256 is
`ef748aa38a03b814dc5e0f9125e570ca2c1de2eae51d9011a3384ee62a86ddf1`.

The dependent-read fixtures retain different ownership lifetimes. Rust's parent
response and borrowed child capability survive across the child await. C++'s
continuation releases them after constructing the child promise. Their steady
writes accordingly alternate Call/Finish and Call/Release for portable, versus
Call/Finish/Release and Call for C++; both perform two writes per workflow.
Earlier Release would require changing the retained Rust ownership. The fixtures
and acceptance criteria remain unchanged.

The subsequent `outlined-immediate20261016` experiment replaced forced inlining
of the immediate sender with an out-of-line call. All 139 wire tests passed, but
pipeline instructions rose to 80,292,202/67,901,865 and simulated instruction-cache
misses rose to 5,551,529/4,915,140 from 5,521,074/4,866,707. It was rejected and
reverted before elapsed-time qualification. Its exact patch, frozen executable
identity and four workflow profiles are retained in
`target/portable-performance-outlined-immediate-rejected-20261016/` (38 files
plus checksums). The measured executable SHA-256 is
`421a55c0d237b0d80f09f584416a0584440cbcad5f659539191f50b8d8b36117`.

Coverage follow-up adds malformed and retained-frame ownership cases, zero-progress
output failures, scalar transport backpressure, external arena allocation, and
fragmented/truncated/large-stream reads. The focused checks pass, including
32-bit Miri for retained frames and Miri for external-arena ownership. A narrow
coverage run uses CI's exact LLVM 22 toolchain (`nightly-2026-03-05`); it does not
replace the full Cargo coverage gate. Generic-function coverage must be assessed
using LLVM's instantiation-group summaries: combining branch hits across
different monomorphizations can incorrectly suggest that a required floor has
passed. No coverage floor has been changed, and full coverage remains open.
The follow-up sources, logs, raw profiles, exports and reproduction are retained
in `target/portable-performance-coverage-followup-20261016/` (45 files plus
checksums); generated coverage objects are excluded.

Full CI `38940989997` for `a2822536` subsequently finished with 2,264 tests
passed, 20 skipped and no failed or incomplete test cases. All other executed
jobs passed, including Miri, native unsafe checks, Verus and TLA+. The unchanged
coverage regression gate failed 55 metric floors; the required CI result failed
and its dependent benchmark job was skipped. The local coverage follow-up above
is not part of that commit. The complete Cargo artifact and job log are retained
in `target/portable-performance-small-one-ci-20261016/` (8 files plus checksums).

### Portable and legacy instruction qualification

The October 15 optimization batch meets a **1.05× portable/legacy instruction
limit on both peers** for 100 warmups and 1,000 retained empty plaintext TCP
calls. Each comparison runs both drivers from the same ordinary full-LTO
executable. Complete-process Callgrind Ir includes startup, bootstrap and cleanup.
This qualifies the local small-call workload; it does not establish latency,
encrypted transport or large-payload parity.

| Process | Portable instructions | Matched legacy instructions | Portable / legacy |
| --- | ---: | ---: | ---: |
| Client | 18,317,712 | 17,449,559 | 1.049752× |
| Server | 17,061,152 | 16,264,956 | 1.048952× |

Two more paired runs of that executable also pass both limits:

| Run | Portable client / server | Legacy client / server | Client ratio | Server ratio |
| --- | ---: | ---: | ---: | ---: |
| Repeat 1 | 18,318,194 / 17,062,974 | 17,451,839 / 16,267,066 | 1.049643× | 1.048928× |
| Repeat 2 | 18,319,310 / 17,063,592 | 17,452,607 / 16,267,288 | 1.049660× | 1.048951× |

The worst ratio across all three runs is **1.049753× client, 1.048952× server**
(rounded upward). Portable instructions decreased 8.2% client and 11.3% server
from this batch's starting measurements. Shared promise polling also improves
legacy; the comparison does not rely on making the control slower.

The qualifying build used clean commit
`0d3a409d124563837f828fea69b33550d54fb0cb`, Rust 1.97.0, Valgrind 3.25.1,
one codegen unit, fat LTO and enabled overflow checks on an AMD Ryzen 7 5800H.
Executable SHA-256:
`c11d46ed16a19bb59df15efb7b9c60b7b6834ec2dfdba14bd41cd2a20ca272e8`.
The repeats reuse that exact binary; subsequent commits adjust test assertions,
reviewed source hashes and documentation. No PGO, target-CPU tuning, static C++
runtime or reduced benchmark workload is used.

The production changes reuse exclusive response/result storage, preserve typed
response ownership without another boxed future, and transfer affine message
leases directly into the FIFO. Ready server dispatch avoids redundant task
scheduling; validated final Returns reclaim their records without intermediate
updates that are immediately discarded. Concrete reader validation and focused
inlining remove repeated dispatch and table lookups.

These paths retain protocol ledgers, admission limits, partial-write ownership,
FIFO ordering, cancellation, shutdown and control wakeups. Generic hooks,
pipelines and retained Finish obligations keep their general paths. Fixed-storage
RPC keeps caller-owned memory and has no new allocator dependency. A malformed
relative-pointer offset that could panic on 32-bit targets now returns a bounds
error, with native and Miri regressions.

Reproduce the paired gate with:

```sh
cargo run --locked -p capntproto-dev -- rpc-instructions \
  --output target/quality/portable-instructions \
  --compare-legacy --maximum-ratio 1.05
```

The command fails if **either peer** exceeds the ratio. `--maximum` can impose
an additional absolute ceiling. JSON reports preserve both drivers' totals,
compiler flags, workload, executable hash and source revision; raw Callgrind
profiles are retained. Local evidence is under `target/portable-105-20261014/`,
including `qualified/report.json`, both `qualified-repeat*/report.json` files,
and `qualification-summary.json`.

All 48 application-workflow smoke checks passed across the four legacy/portable
client/server combinations, including fanout, dependent and pipelined reads,
write/read, callbacks and payloads through 4 MiB. Twenty additional echo cases
passed through 256 KiB. The small-call harness caps frames at 512 KiB; larger
workflows use a separate 8 MiB budget. These checks validate results and
completion, not latency on this workstation.

The frozen workspace run passed **2,301 tests, with seven intentionally skipped**.
All 85 workspace doctests passed (13 intentionally ignored). The six portability
profiles passed 522 test executions, including allocation-free ownership checks;
bare-metal core/engine/wire/RPC checks and the portable release executable passed
for `thumbv7em-none-eabi`. The full memory gate passed 180 interpreted Miri
executions. Additional 32-bit Miri checks passed response/admission/shutdown/task
ownership, affine queues, selected readers and promise ownership, including the
oversized-relative-offset and fragmented-frame regressions.

The native mutation campaign caught all 86 compilable faults; eight mutations
were unbuildable, with no surviving mutations or timeouts. Native fuzz smoke,
all 132 required cross-language RPC scenarios and the bounded upstream-Rust
check passed; two documented upstream-only limitations remain separately
reported. Clippy, formatting, unsafe-documentation debt and workflow checks
passed. Updated unsafe source hashes preserve every existing exception count.

Verus passed 155 production obligations, all 139 temporal-library obligations
and all 43 deliberate-fault controls, with source-bound evidence covering 122
inputs. The added lemma justifies eliding the intermediate state for an exclusive
validated Return with no Finish obligation. Three optional bounded Kani queue
proofs also passed for tracked and directly owned sends. Formal contracts do not
prove IO, callback behavior or pointer provenance; native fault/reentrancy tests
and 32-bit Miri cover those boundaries. Hosted CI qualifies the published tree
and remote measurements separately.
