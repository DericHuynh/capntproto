<!-- Generated README: edit docs/README.template.md, then run python3 scripts/update_readme.py render. -->
# Capn't Proto

A Rust implementation of Cap'n Proto schemas, serialization and capability RPC,
with a native schema compiler, compatibility adapters, encrypted transport and
correctness tooling. Start with the **[project wiki](docs/wiki/Home.md)**.

**Experimental developer preview.** The project includes coordinated Rust forks
and independent comparisons against pinned C++ Cap'n Proto. It is not affiliated
with the upstream Cap'n Proto project. Full API parity and production
qualification remain work in progress; see the [release criteria](docs/wiki/Release-Acceptance.md).

## What is here

- **Native schema compiler:** imports, generics, constants, annotations, runtime
  reflection and concurrent parser caches. [Compiler guide](docs/wiki/Schema-Compiler.md).
- **Serialization and capability RPC:** maintained `capnp`, `capnpc` and
  `capnp-rpc` crates, with generated Rust APIs and C++ interoperability tests.
  [Runtime scope](docs/wiki/Runtime-Status.md) · [C++ parity](docs/wiki/Cpp-Parity.md).
- **Adapters and transport:** TCP, TLS 1.3 and standard QUIC v1/v2 with optional mutual
  TLS; JSON/text codecs, ByteStream, HTTP, WebSockets and JSON-RPC; encrypted
  multiparty sessions through quiche and durable storage. [TCP, TLS and QUIC guide](docs/wiki/TCP-TLS-and-QUIC.md).
  [Compatibility adapters](docs/wiki/Compatibility-Adapters.md) · [Architecture](docs/wiki/Architecture.md).
- **Executable correctness checks:** Rust tests, C++ comparisons, bounded TLA+
  models, fuzzing, memory checks and LLVM coverage regression gates.
  [Testing](docs/wiki/Testing.md) · [Qualification limits](docs/wiki/Model-Conformance.md).
- **RPC application APIs:** owned connections and native vats, typed bootstraps
  after startup, peer-specific bootstrap factories, automatic three-party
  capability handoff and joins. [RPC guide](docs/wiki/RPC-Applications.md).
- **Storage and ORM:** whole-entry objects plus opt-in component revisions that
  share unchanged data, mapped typed reads and atomic publication.
  [Component storage](docs/wiki/Component-Storage.md).

## Build and test

Use the pinned **Rust 1.97.0** toolchain. The full suite also needs CMake, a C++
compiler, libclang, Cap'n Proto headers/tools and Java 17; the [setup guide](docs/wiki/Getting-Started.md)
and [CI prerequisites](.github/actions/quality-setup/action.yml) describe them.

```sh
# Git checkouts only; source bundles already include the C++ reference.
git submodule update --init --depth 1 -- vendor/capnproto
bash scripts/setup-auditable.sh
export PATH="$PWD/target/auditable-tools/wrapper:$PWD/target/auditable-tools/bin:$PATH"
cargo build --locked -p reproto --lib --bins
cargo test --workspace
```

The C++ reference is pinned as a Git submodule. Ordinary Rust dependencies are
resolved by Cargo; the modified Rust forks remain source dependencies in this
checkout. [Dependency and checkout policy](docs/wiki/Repository-Layout.md#vendored-and-external-code).

The existing Cargo package/import names (`reproto`, `capnp`, `capnpc`, `capnp-rpc`)
remain stable. Capn't Proto is the project name. Use Bash / Git Bash for the setup
commands. Linux, macOS and Windows have compile-and-smoke CI; full verification
and dedicated benchmarks run on Linux.

Benchmarks are separate from tests: CI builds individual release benchmarks with
`cargo bench --no-run`, transfers the artifacts to a temporary dedicated
DigitalOcean droplet, measures Linux loopback RPC and deletes the droplet.
Credentials stay in GitHub Actions secrets. [Benchmark setup](docs/wiki/Quality-and-Benchmarks.md).

## Documentation and participation

[Project wiki](docs/wiki/Home.md) · [Repository boundaries](docs/wiki/Repository-Layout.md)
· [Correctness roadmap](docs/wiki/Correctness.md) · [Reporting and README generation](docs/wiki/README-Reports.md)

Read [CONTRIBUTING.md](CONTRIBUTING.md) before submitting changes. Use the
[issue templates](.github/ISSUE_TEMPLATE) for bugs and questions, and follow the
[code of conduct](CODE_OF_CONDUCT.md). Report vulnerabilities privately through
[SECURITY.md](SECURITY.md); the maintainer contact is
[huynhderic@gmail.com](mailto:huynhderic@gmail.com).

Original project code is [MIT licensed](LICENSE). Vendored components retain
their own licenses and [third-party notices](THIRD_PARTY_NOTICES.md).
[GitHub upload/setup instructions](docs/wiki/GitHub-Setup.md).

{{REPORTS}}
