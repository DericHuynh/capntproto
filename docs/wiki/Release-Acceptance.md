# Release acceptance

The target is an **unpublished Linux x86-64 developer preview**, distributed as
coordinated source. The implementation is experimental and does not claim
complete C++ replacement or production security/durability qualification.
A passing historical run does not qualify changed sources.

## Preview gates

| Gate | Implemented mechanism | Acceptance for the candidate revision |
| --- | --- | --- |
| R1 — Downstream dependency graph | Coordinated path dependencies, generated API and external consumer tests | Run the external consumer matrix and inspect the resolved graph |
| R2 — Source distribution | Deterministic archive, file hashes, notices and initialized C++ reference | Pass the source-bundle round trip and clean extracted-source checks |
| R3 — Storage framing | Independent record-header validation; explicit V4/V5 openers | Corruption rejected without destructive repair; valid torn tails recovered |
| R4 — Recovery barriers | Validate and sync the selected file and directory before serving | Inject barrier failures and check second-crash recovery |
| R5 — I/O and crash paths | Short-write/error seams and isolated child-process tests, including V5 components | Atomic data/publication/receipt outcomes, writer quarantine and held-snapshot checks |
| R6 — Clean qualification | Pinned toolchains, ordinary suite, standalone crates, downstream checks and Clippy | Run `isolated_release_qualification` on the candidate; retain matching source hashes |
| R7 — Operating/reporting contract | Setup, ownership, limits, ambiguous outcomes, offline backup and private security email documented | Review deployment assumptions and verify hosted publication/settings before release |

The private reporting contact is configured in [SECURITY.md](../../SECURITY.md).
Enabling GitHub's private reporting form is a separate maintainer setting.
There is no claim here that the latest tree has passed a fresh full release run
or that an archive has been publicly published. Build it only after qualification:

```sh
cargo nextest run --locked --test release source_bundle_roundtrip -- --exact
cargo nextest run --locked --test release isolated_release_qualification -- --ignored --exact
```

See [Testing](Testing.md) for prerequisites and [Getting started](Getting-Started.md) for
the operating contract. Existing source-bound reports identify their own
snapshots; test counts are not a substitute for matching inputs.

## Production and broader qualification

| Gate | Still required |
| --- | --- |
| P1 — Native admission/security review | Review TLS peer pinning, certificate/context/admission bindings, key lifecycle, version policy, replay and resource abuse |
| P2 — Hostile inputs and lifecycle stress | Longer bounded-memory soaks, fuzzing and fault composition across cancellation, reconnect, handoff, revocation and stalled consumers |
| P3 — Advertised interoperability | Name and test peer versions/features; distinguish installed C++ from source-pinned comparisons and native Rust-only profiles |
| P4 — Deployment/storage operations | Integrated offered-load/tail-latency measurements, cold recovery, real writeback failures, supported filesystems and verified backup restoration |

TCP/TLS and standard quiche QUIC v1 are implemented, including TLS/mTLS tests
and an independent QUIC fixture with its documented correction. General Join
and third-party answers remain unsupported by the pinned C++ dispatcher.
[Transport verification](TCP-TLS-and-QUIC.md#verification) states what was tested.

The bounded storage worker and component crash tests advance P4 but do not close
it. Application I/O seams and process exit do not simulate every kernel writeback
or device power-loss failure. See [Storage resilience](Storage-Resilience.md).

## Optional scope

A preview need not include an actor runtime, query engine, EAE backend,
replication, multiwriter storage, cross-file transactions, full ICE/TURN,
transparent identity migration, generic capability-graph persistence, at-rest
encryption, hostile-writer rollback protection or hard realtime guarantees.
Those are distinct contracts, not excuses to defer bugs in supported behavior.
MacOS descriptor support is implemented but native qualification is pending;
other Unix descriptor platforms remain outside the supported implementation.

[Roadmap](Roadmap.md) tracks feature work. [EAE comparison](EAE-Comparison.md)
records why Store remains the default. [Model conformance](Model-Conformance.md) separates
bounded model evidence from whole-runtime proof.
