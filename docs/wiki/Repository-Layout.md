# Repository layout and boundaries

Capn't Proto uses a Cargo root package with a small workspace. Production Rust follows
the [Cargo package layout](https://doc.rust-lang.org/cargo/guide/project-layout.html):
`src/` holds the library and binaries, `tests/` holds integration tests, and
`examples/` holds runnable examples. Repository-level community files stay at
the root so GitHub discovers them.

```text
.
├── Cargo.toml, Cargo.lock, build.rs, rust-toolchain.toml
├── README.md, LICENSE, CONTRIBUTING.md, SECURITY.md, …
├── src/                         Capn't Proto runtime and model-exploration binary
├── crates/capnp-compiler/        Rust schema-language frontend library and CLI
├── crates/capnp-compat/          Optional standard codecs and protocol adapters
├── schemas/                     Service schemas and shared schema test inputs
├── tests/                       Runtime, interoperability, and acceptance tests
├── examples/                    Examples and an independent downstream consumer
├── test-support/                Test helpers, fixture bindings, frozen traces
├── quality/                     CI coverage, reports, charts, benchmark orchestration
├── verification/                Executable TLA+ models and native verification inputs
│   ├── configs/                 Bounded protocol-model workload configurations
│   ├── audit/                   Independent conformance model and configurations
│   └── miri/                    Isolated Miri workspace
├── benchmarks/                  Independent RPC, instruction and storage benchmarks
├── fuzz/                        Isolated fuzz package and source seed generation
├── scripts/                     Maintenance, config generation, cloud lifecycle
├── docs/
│   ├── wiki/                    Current guides and GitHub Wiki navigation
│   ├── archive/                 Superseded ledgers and design proposals
│   └── reports/                 Generated public charts and measured history
├── vendor/                      Coordinated forks and pinned dependencies
│   ├── capnproto/               Pinned C++ submodule for verification and benchmarks
│   ├── quiche/                  Quiche workspace with the Native fork
│   └── provenance/              Upstream revisions, schema snapshots, fork patch
├── research/                    Frozen research inputs; no production dependency
│   ├── baseline/                Historical model source, not active verification
│   └── reports/                 Frozen measurements and archive explanation
├── .github/                     Workflows and community templates
└── .cargo/                      Workspace command environment
```

## Cargo workspace ownership

| Package | Location | Role |
| --- | --- | --- |
| `reproto` | Root `Cargo.toml`, `src/` | Production runtime and public API |
| `capnp-rpc` | `vendor/capnp-rpc/` | Maintained RPC engine; its tests run in the root workspace |
| `reproto-test-support` | `test-support/` | Verification tools and fixtures; a dev dependency of the runtime |
| `reproto-quality` | `quality/` | Developer/CI tooling, not a runtime dependency |
| `capnp-compiler` | `crates/capnp-compiler/` | First-party textual schema frontend and CLI; no C++ runtime dependency |
| `capnp-compat` | `crates/capnp-compat/` | Opt-in JSON/text codecs and standard ByteStream, HTTP, WebSocket and JSON-RPC adapters |

The root manifest explicitly lists workspace members and excluded workspaces.
`capnp-rpc` previously joined through Cargo's automatic path-dependency membership;
it is now named explicitly. Workspace members share the root Cargo.lock.

The other vendored crates, the quiche workspace, fuzzing, Miri, downstream example,
and benchmark packages keep their own manifests and lockfiles. Their separation
accommodates different toolchains, fuzz instrumentation, external tools, or
release benchmarks. Relevant maintained-crate checks are launched by the root
test suite; exclusion from Cargo membership is not exclusion from verification.

The full default verification command remains:

```sh
cargo test --workspace
```

Benchmarks use individual `cargo bench --manifest-path benchmarks/rpc/Cargo.toml
--bench NAME --no-run` builds. See [Quality and Benchmarks](Quality-and-Benchmarks.md) for Linux droplet runs
and the lighter Linux/macOS/Windows compile-and-smoke workflow.

## Runtime, tests, and schemas

Production dependency edges run from `reproto` to the coordinated vendored crates
and optional transport/storage dependencies. Production code must not depend on
`test-support`, `quality`, benchmarks, fuzzing, or the EAE research implementation.
`test-support` can depend on the core/RPC crates; runtime tests depend on it through
`[dev-dependencies]`. CI tools may reuse test-support verification code.

Runtime module ownership is described in [Architecture](Architecture.md).
The first-party compiler lives under `crates/`, separate from vendored upstream
forks. It depends on the core wire runtime, not the RPC/Native/storage runtime or
verification helpers. `vendor/capnpc` owns Rust generation from compiled requests;
`crates/capnp-compiler` owns textual parsing, semantic checks and schema emission.
`crates/capnp-compat` depends on the maintained wire/RPC crates and the Rust
compiler’s standalone text lexer. It owns copies of the pinned compatibility
schemas, generates bindings in `OUT_DIR`, and depends on verification helpers
only for tests. HTTP/WebSocket/TLS implementations remain application supplied.
The root runtime does not depend on this optional package.

Private unit tests may sit beside the implementation under `#[cfg(test)]`.
External behavior, generated API contracts, native interoperability, and release
checks belong in `tests/`. Shared test utilities belong in `test-support/`.

`schemas/` is shared source input, not generated Rust. Root `build.rs` compiles
only the enabled runtime service schemas. `test-support/build.rs` compiles the
fixture schemas into the separate test-support package. Cargo writes generated
bindings to `OUT_DIR`; fixture bindings are not re-exported by the runtime.

## Models, evidence, and generators

Active model source lives in `verification/`. The `Capnp*.tla` protocol models
are colocated with focused runtime models. The test runners explicitly set
TLC's module search path to this directory so imports resolve from the repo root.
`verification/configs/` contains protocol workloads; configuration generators
live in `scripts/` and can be run from any working directory:

```sh
python3 scripts/generate_network_configs.py
python3 scripts/generate_feature_configs.py
python3 scripts/generate_realtime_configs.py
```

`verification/audit/` holds the independent conformance checks. The Rust test
runner stages that model with its dependencies. `test-support/verification/`
contains the checked-in model catalog, graph-bound replay corpora, and compiler
fixtures. Those are required test inputs, not generated output to delete.
`research/baseline/` preserves historical snapshots; active checks use `verification/`.

## Vendored and external code

Ordinary Rust dependencies come from crates.io through Cargo, with resolution
recorded in the committed lockfiles. TLS dependencies, including BoringSSL,
are fetched through Cargo. These dependency sources and all Cargo `target/` directories
stay out of Git; see [Cargo dependency sources](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html).

`vendor/` contains the coordinated forks and the C++ reference submodule. The
`capnp`, `capnp-rpc`, `capnp-futures`, `capnpc`, and quiche crates contain local
changes and use Cargo path dependencies. Replacing them with upstream releases
would discard required APIs and behavior. Retain their source here until the
coordinated forks have separately published versions or Git revisions.
`vendor/quiche/` retains its upstream workspace layout to keep the archived fork
patch and upstream updates straightforward. It is ordinary vendored source,
not a Git submodule. Follow [Fork Policy](Fork-Policy.md) and update provenance
under `vendor/provenance/` when changing forks.

`vendor/capnproto/` is the pinned C++ dependency used by verification and benchmarks.
Git records its upstream URL in `.gitmodules` and its exact commit as a submodule
entry. Initialize it after cloning (or switching to a revision with a different pin):

```sh
git submodule update --init --depth 1 -- vendor/capnproto
```

CI uses that same command. Update the submodule commit and
`vendor/provenance/revision.json` / `capnproto-sources.json` together when changing
the reference. Ordinary Rust builds do not need it; full reference checks and
source-release qualification do. Source bundles include the initialized C++
files, so unpacked releases do not need Git. GitHub's automatic source archives
do not include submodule contents; use a Git checkout or the project's source bundle.
Its builds go under `target/`; `cargo clean` does not remove the dependency's
source. Downloaded TLC jars belong in `target/tools/` or an external path
selected with `TLA2TOOLS_JAR`.

## Research and generated output

`research/EAE-Reconstruction.zip` is a frozen, hash-checked research input. The
storage experiment extracts it under `target/eae-benchmark/`; only the separate
storage benchmark package depends on that extracted code. Its current run data
and reports go under `target/storage-benchmark/`. The JSON/JSONL files in
`research/reports/storage-benchmark/` and `research/reports/eae-integration/`
preserve historical measurements; new runs must not overwrite them by default.

Cargo builds, LLVM profiles, TLC logs/graphs, generated reports, and benchmark
bundles belong under ignored `target/` directories. Release qualification evidence
goes in `target/release-qualification/`. Downloaded tool files and release archives
in `dist/` are also ignored. Source distributions
are assembled by `test-support/src/verification/distribution.rs`; changing paths
must preserve that inventory and the model catalog. The layout regression test
checks that required models, generators, dependencies, and research inputs are
bundled. Historical baseline snapshots stay out of release archives, apart from
`research/baseline/CapnpRpc.tla`, which verification uses.

Keep GitHub community files and Cargo entry points at the root. Put maintained
guides in `docs/wiki/`, historical records in `docs/archive/`, scripts in `scripts/`, model source in `verification/`, and
research inputs in `research/`.

The root README is generated from `docs/README.template.md`. The small public
history and SVG figures in `docs/reports/` are intentionally checked in so images
render on GitHub and in source clones. Raw logs, samples and private/local data
stay under `target/`. Collection, charting and publication code lives in
`quality/reporting/`, invoked by `scripts/update_readme.py`; contract tests join
the ordinary workspace suite. See [README Reports](README-Reports.md).

The [Wiki maintenance guide](Wiki-Maintenance.md) owns page navigation, link
validation and export. Root/community/package READMEs remain entry points; the
wiki is the current guide set rather than another copy of the old docs tree.
