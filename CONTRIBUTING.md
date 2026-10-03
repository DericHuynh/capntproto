# Contributing to Capn't Proto

Contributions can improve the runtime, protocol models, regression tests,
documentation, and quality tooling. Capn't Proto is an experimental source preview;
the [correctness roadmap](docs/wiki/Correctness.md) and
[release acceptance criteria](docs/wiki/Release-Acceptance.md) describe current work.
All participation follows the [code of conduct](CODE_OF_CONDUCT.md).
The [repository layout](docs/wiki/Repository-Layout.md) explains where code, model
inputs, fixtures, maintenance scripts, and research belong.

## Before starting

Search existing issues and pull requests for related work. Use the bug-report
template for a reproducible failure, the feature-request template for a proposed
behavior, or the documentation template for unclear or incorrect guidance. Small
fixes can go straight to a pull request. Discuss changes to wire behavior,
capability authority, storage formats, or public APIs before a large implementation.

Report suspected vulnerabilities privately through [SECURITY.md](SECURITY.md).
Use [SUPPORT.md](SUPPORT.md) for usage questions.

## Set up a checkout

Fork the repository, clone your fork, and create a branch from the default branch.
Keep the sources under `vendor/` together with this checkout.
Retain Cargo.lock files for the root and standalone workspaces.

Initialize the pinned C++ reference used by full verification:

```sh
git submodule update --init --depth 1 -- vendor/capnproto
```

Install Rust 1.97.0 through rustup and the Cap'n Proto schema compiler/development
headers. The [platform workflow](.github/workflows/quality.yml) contains the
Linux, macOS, and Windows setup used by CI. `CAPNP_INCLUDE_DIR` selects the schema
include directory when it is outside the compiler's normal search path.

For a quick compile and smoke check from the repository root:

```sh
bash scripts/setup-auditable.sh
export PATH="$PWD/target/auditable-tools/wrapper:$PWD/target/auditable-tools/bin:$PATH"
cargo build --locked -p reproto --lib --bins
cargo run --locked --manifest-path examples/downstream/Cargo.toml
```

Use bash (Git Bash on Windows) and retain this PATH for subsequent Cargo commands.
The [auditable build policy](docs/wiki/Quality-and-Benchmarks.md#auditable-cargo-builds) applies to
tests, examples, benchmarks and installed CI tools as well as application builds.

The smoke example exercises generated capability RPC and pipelining. It also
checks storage reopen on Unix. It does not qualify Windows storage durability.

## Test a change

After staging changes, run `python3 scripts/check_repository.py`. It rejects
tracked ignored files (including nested Cargo `target/` output) and blobs over
50 MiB. CI runs the same check. Keep build artifacts local; `.gitignore` does
not remove files that Git already tracks.

All default tests have one entry point on the supported Linux verification host:

```sh
cargo test --workspace
```

This includes unit and integration tests, doctests, model checks, maintained
standalone crates, and native verification controls. It can take substantial time
and disk space. Install the pinned prerequisites in [Testing](docs/wiki/Testing.md)
and the [quality setup action](.github/actions/quality-setup/action.yml) first:
these include Java/TLC, the pinned C++ reference, Clang/LLVM, Miri, Valgrind,
mutation testing, and fuzzing tools. The setup action's disk-cleanup step is for
disposable GitHub-hosted runners only; do not run it on your workstation.

During development, run the relevant test target or named regression from the
test guide. Before review, record the exact commands and outcomes, including
checks you could not run. Documentation-only changes need working links and
accurate instructions; they do not require rebuilding the test suite.

PR CI compiles and runs smoke checks on Linux, macOS, and Windows. Linux also
runs allocation-budget tests. Workflow changes run actionlint/Zizmor, and
documentation changes run local link checks. Full LLVM
coverage and regression gates run in the separate full-quality workflow. See
[Quality and Benchmarks](docs/wiki/Quality-and-Benchmarks.md) for the boundaries of each check. Do not replace
measured results with estimates or lower a coverage baseline to conceal a
regression.

Benchmarks run separately from tests. Compile an individual release benchmark,
for example:

```sh
cargo bench --locked --manifest-path benchmarks/rpc/Cargo.toml --bench native --no-run
```

The dedicated benchmark workflow transfers compiled artifacts to a temporary
DigitalOcean dedicated CPU Linux droplet and measures loopback RPC there. Use
that workflow for publishable comparisons, with its GitHub Actions secret and
cleanup controls. Record payload sizes, environment, and transport/security
differences. Contributors do not need cloud credentials to submit a patch.

## Implementation and documentation conventions

- Keep patches focused and explain the observable behavior they change.
- Add a regression test for a behavior fix. Model changes should state their
  bounds, assumptions, expected outcome, and applicable negative controls.
- Follow nearby Rust style and applicable `AGENTS.md` files. The quiche fork
  requires nightly rustfmt; use its pinned nightly toolchain when formatting it.
- Update public API documentation and affected guides when contracts change.
  State unsupported behavior and verification limits accurately.
- Follow [Fork Policy](docs/wiki/Fork-Policy.md) for coordinated vendored changes,
  provenance updates, and refreshing the quiche patch with external Git metadata.
- Keep generated build output, coverage profiles, downloaded tools, live secrets,
  and local benchmark artifacts out of commits. Preserve reviewed fixtures,
  lockfiles, source provenance, and upstream notices.

## Open a pull request

Use the supplied template to describe the problem, resulting behavior, and
validation. Link related issues and identify API, wire, storage, or platform
effects. Include minimal reproductions and relevant report artifacts; remove
private keys, introduction tickets, SturdyRefs, tokens, and personal data.

A draft pull request is useful for work still in progress. Keep follow-up changes
focused on review feedback, and update the description when the final scope
changes. Maintainers review correctness, reproducibility, documentation, and
compatibility with the stated preview contracts before merging.

Contribute only material you have the right to submit. Original Capn't Proto code uses
the [MIT License](LICENSE); preserve the licenses of vendored components listed
in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## README and report changes

Edit `docs/README.template.md` and regenerate with `python3 scripts/update_readme.py render`. Do not hand-edit public measurement history. See [the reporting guide](docs/wiki/README-Reports.md) for renderer dependencies, CI origin checks and validation.

## Wiki documentation

Edit the relevant page under `docs/wiki/` and update its Home/sidebar links when
adding or removing topics. Run `python3 scripts/wiki.py check` and build an export
with `python3 scripts/wiki.py build --output target/wiki`. See
[Wiki maintenance](docs/wiki/Wiki-Maintenance.md) for publishing and the distinction
between current guides, historical ledgers and frozen research evidence.
