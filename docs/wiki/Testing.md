# Testing

Run commands from the repository root. Run the full suite with:

```sh
cargo nextest run --locked --workspace
cargo test --locked --workspace --doc
```

This includes runtime tests, generated API compiler contracts, bounded model
checks and Rust replay, C++ comparisons and specialized verification subprocesses.
It is much heavier than a compile/smoke check. Use the relevant focused target
during development and record exactly what ran.

## Test runner

Install the pinned runner with `cargo install cargo-nextest --version 0.9.146 --locked`,
or use nextest's [prebuilt binaries](https://nexte.st/docs/installation/pre-built-binaries/).
CI installs the same version through the shared auditable setup action.
`cargo nt` is the short form of `cargo nextest run --locked --workspace`.
Nextest runs each test in a separate process, so the repository no longer sets
`RUST_TEST_THREADS=1`. The [runner configuration](../../.config/nextest.toml)
limits nested compiler and verification campaigns to two simultaneous tests;
ordinary tests run concurrently. Retries are disabled and failures do not stop
remaining tests. Use `-j 1` when diagnosing resource contention.

Nextest does not run doctests: keep the explicit `cargo test --doc` command.
Miri, cargo-careful, LLVM coverage, mutation testing, Valgrind and nested Rust
fixtures use nextest too. Fuzz engines, TLC and the C++ reference retain their own
runners; nextest executes their Rust orchestration tests. Child-process crash
injection still directly executes its crash fixture to preserve signal semantics.

CI uses JUnit for aggregate counts and failed-test links, with a distinct report
per invocation. Tests skipped by a filter are excluded from pass counts. The
experimental nextest JSON stream is used only for individual nested-test
identities, never to sum suite counters. See [README reports](README-Reports.md).

## Prerequisites

Use the pinned toolchains in [rust-toolchain.toml](../../rust-toolchain.toml) and
[quality setup](../../.github/actions/quality-setup/action.yml). The Linux full suite
needs Rust 1.97.0, the pinned nightly tools, C/C++/CMake/Ninja, Cap'n Proto headers
and compiler, libclang, Java 17, the TLC release jar, Valgrind and archive tools.
The setup action specifies exact versions and checksums. Its runner disk-cleanup
step is for disposable CI hosts; do not execute that step on a workstation.

```sh
git submodule update --init --depth 1 -- vendor/capnproto
bash scripts/setup-auditable.sh
export PATH="$PWD/target/auditable-tools/wrapper:$PWD/target/auditable-tools/bin:$PATH"
```

Keep this PATH for subsequent Cargo commands. Set `JAVA` to the Java executable
and `TLA2TOOLS_JAR` to the pinned jar, or install it at `target/tools/tla2tools.jar`.
[Quality and benchmarks](Quality-and-Benchmarks.md) describes the remaining tool installation
and CI lanes. Platform smoke jobs do not qualify storage durability on every OS.

## Focused selections

| Change | Useful command |
| --- | --- |
| Structured replies and call ergonomics | `cargo nextest run --locked --test rpc_reply --test field_api_rpc` |
| Compile-time reply rejection contracts | `cargo nextest run --locked --test api_contracts rpc_reply::structured_rpc_reply_compile_contracts -- --exact` |
| Driver ownership and native vats | `cargo nextest run --locked --test rpc_ownership --test native_vat` |
| Admission, pipelining and forwarding | `cargo nextest run --locked --test rpc_admission --test result_pipeline --test tail_transfer --test answer_adoption` |
| TCP, TLS/mTLS and QUIC v1 | `cargo nextest run --locked --test tcp_rpc --test secure_rpc` |
| Native transport backends | `cargo nextest run --locked --lib transport::backend_tests` |
| Upstream QUIC adapter and registry pin | `cargo nextest run --locked --lib transport:: -- --skip tlc` and `cargo nextest run --locked --test tooling pinned_native_profile -- --exact` |
| Native routes and migration | `cargo nextest run --locked --test native_multiparty --test native_pipeline_migration --test native_deployment` |
| Storage worker and component ORM | `cargo nextest run --locked --test storage_worker --test component_orm` |
| Storage recovery in child processes | `cargo nextest run --locked --test tooling storage_crashes_in_isolated_process -- --exact` |
| Rust compiler and reference comparisons | `cargo nextest run --locked -p capntproto-compiler` and `cargo nextest run --locked --test schema_compiler` |
| Optional codecs and adapters | `cargo nextest run --locked -p capntproto-compat` |
| Generated field API contracts | `cargo nextest run --locked --test tooling generated_api_compile_contracts -- --exact` |
| Source archive and downstream use | `cargo nextest run --locked --test release source_bundle_roundtrip -- --exact` and `cargo nextest run --locked --test tooling external_consumer_default_features -- --exact` |

The structured reply compiler gate includes positive controls and exact expected
Rust diagnostics for invalid ownership transitions. Runtime tests separately
exercise late errors, published child calls, failed parameter construction and
three-party adoption. A compiler test is not evidence of network reliability.

For smaller feature builds:

```sh
cargo check --locked --no-default-features --lib
cargo nextest run --locked --no-default-features --features tls --test secure_rpc
cargo nextest run --locked --no-default-features --features quic --test secure_rpc
cargo nextest run --locked --no-default-features --features storage --test storage_worker --test component_orm
```

Root integration tests are not all feature-gated. Use these selected targets;
`--no-default-features --all-targets` is not a supported substitute.

## Models, simulation and evidence

[models.json](../../test-support/verification/models.json) owns the active bounded
model catalog, corpus hashes, negative controls and scope. Discover focused
model checks with `cargo nextest list --locked --test protocol_models`.
Tests may run TLC and replay real implementation operations; read each model's
bounds and fairness assumptions. One prefix per graph edge does not cover all
histories or executor schedules.

Native packet simulations retain real TLS handshake entropy. Logs and selected
schedules support diagnostics; retired handshake simulators are not TLS evidence.
The [conformance guide](Model-Conformance.md) separates composed-model limits from
implemented RPC features. Historical, per-feature verification notes are retained
in [Testing history](../archive/Testing-History.md), not maintained as setup instructions.

[ignored-tests.json](../../quality/ignored-tests.json) is the current allowlist.
Some ignored negative controls and crash tests are invoked by ordinary parent
gates. Full composed exploration and clean release qualification are explicit:

```sh
cargo nextest run --locked --test protocol_models tlc_protocol_reference -- --ignored --exact
cargo nextest run --locked --test release isolated_release_qualification -- --ignored --exact
```

Do not run every ignored test indiscriminately: intentional negative controls
have different success criteria. Reports belong under `target/`; archived
research data records its original source and environment. Old pass counts do
not qualify changed source. See [release acceptance](Release-Acceptance.md).

## Documentation checks

```sh
python3 scripts/wiki.py check
python3 -m unittest discover -s scripts/tests -p 'test_wiki.py'
python3 scripts/wiki.py build --output target/wiki
```

The check validates local links, anchors, navigation and the documentation
inventory. The export rewrites wiki/source links without changing maintained
pages. CI uploads the export for review. See [Wiki maintenance](Wiki-Maintenance.md).

## AFL++ and IJON

The dedicated [fuzz workflow](../../.github/workflows/verification-fuzz.yml) runs
both engines. Existing libFuzzer targets retain ASan and their full seed matrices.
Pinned `afl` / `cargo-afl` **0.18.2** bundles AFL++ **4.40c**. Four targets share
those same framing, pointer, schema and RPC lifecycle oracles. The two Native
crypto/clock targets remain in libFuzzer because they do not provide deterministic
persistent inputs. Neither engine enables Quiche's crypto bypass.

```sh
cargo install cargo-afl --version 0.18.2 --locked
AFL_NO_CFG_FUZZING=1 cargo afl build --locked --manifest-path fuzz/Cargo.toml \
  --no-default-features --features afl-targets --bins --target-dir target/afl-build
cargo run --locked --manifest-path fuzz/Cargo.toml --no-default-features \
  --example afl_seeds -- target/afl-corpus
python3 scripts/afl_fuzz.py --binaries target/afl-build/debug \
  --corpus target/afl-corpus --output target/afl-results --seconds 120
```

Use a fresh output directory for each campaign. The CI job runs `cargo afl
system-config` only on its disposable hosted Linux runner; this root-level system
tuning is an explicit local administrator decision. See the [Rust Fuzz Book](https://rust-fuzz.github.io/book/afl.html)
and [AFL++ IJON documentation](https://github.com/AFLplusplus/AFLplusplus/blob/stable/docs/IJON.md).

Stable Rust uses cargo-afl's sanitizer-coverage instrumentation and comparison
tracing; this job does not build the optional nightly LLVM CMPLOG plugins.
`-c -` records that choice explicitly. AFL targets have debug assertions and
overflow checks; ASan qualification comes from the separate libFuzzer campaigns.

The RPC IJON observer reports bounded combinations of live capabilities, running
calls, outstanding promises and pending questions. Two max-value channels reward
actual completions and oracle effects. It never rewards input length or raw IDs.
All per-input RPC state is constructed and dropped inside the existing oracle;
immutable fixture caches are warmed before the forkserver. Persistent processes
restart after 1,000 inputs. Shared-oracle tests compare repeated feedback and
results; instability percentages are retained, not hidden.

The AFL++ runtime's IJON flag must be supplied explicitly for the stable sancov
build. CI requires its **Using IJON feature** handshake; merely linking annotation
calls is insufficient. Findings, seed corpus, queue, raw `fuzzer_stats`, logs,
IJON max-input corpus and graph data are retained in `fuzz-report`. Any saved
crash or hang fails CI even if AFL exits successfully. Replay a retained input
with the existing shared-oracle entry point:

```sh
CAPNTPROTO_NATIVE_FUZZ_TARGET=rpc_lifecycle \
CAPNTPROTO_NATIVE_FUZZ_REPLAY=target/afl-results/rpc_lifecycle/default/queue/INPUT \
cargo nextest run --locked --manifest-path fuzz/Cargo.toml --no-default-features \
  --lib tests::replay_saved_fuzz_input -- --exact
```
