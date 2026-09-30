# Documentation

Start with the [project README](../README.md),
[repository layout](REPOSITORY_LAYOUT.md), and
[contribution guide](../CONTRIBUTING.md).

| Area | Starting points |
| --- | --- |
| Build and operate | [Preview setup](PREVIEW.md), [testing](TESTING.md), [quality CI and benchmarks](QUALITY.md) |
| Runtime ownership and APIs | [Architecture](ARCHITECTURE.md), [implementation](IMPLEMENTATION.md), [runtime port](RUNTIME_PORT.md) |
| Protocol models | [Protocol mapping](PROTOCOL.md), [conformance limits](CONFORMANCE.md), [handoff](HANDOFF.md), [feature contracts](FEATURES.md), [realtime contract](REALTIME.md) |
| Generator and API design | [Rust schema compiler](../crates/capnp-compiler/README.md), [Generator guide](RUST_GENERATOR.md), [API design](RUST_API_DESIGN.md) |
| Planning and qualification | [Correctness roadmap](CORRECTNESS_ROADMAP.md), [feature roadmap](ROADMAP.md), [release acceptance](RELEASE_ACCEPTANCE.md) |
| Forks and research | [Fork policy](FORK_POLICY.md), [EAE comparison](EAE_BENCHMARKS.md), [third-party notices](../THIRD_PARTY_NOTICES.md) |
| Repository administration | [GitHub setup](GITHUB_SETUP.md), [security policy](../SECURITY.md), [community support](../SUPPORT.md) |

Topic-specific runtime guides live alongside this index. Active TLA+ source and
configurations are in [verification/](../verification); frozen trace fixtures
are in [test-support/verification/](../test-support/verification). Historical
run output linked by older design documents is described in
[research/reports/README.md](../research/reports/README.md).

[Generated README and public CI charts](REPORTING.md) describes the template, aggregate test history, benchmark bars and publishing workflow.
