# Building and operating the source preview

The supported preview target is **x86-64 Linux**, Rust **1.97.0** (the declared
minimum and pinned tested version), with a single cooperating storage writer on
a local filesystem implementing file/directory `fsync`, atomic same-directory
rename and advisory locks. Storage benchmarks use Btrfs; temporary-file tests
also run on tmpfs. Network filesystems, Windows/macOS storage, physical power-cut
durability and other Rust versions are not qualified. EAE additionally requires
4 KiB host pages and remains a separate experiment.

## Install and run

This preview is a source bundle, not a crates.io package. Unpack
`reproto-0.1.0-source.tar.gz`, verify its published SHA-256 against a trusted
delivery channel, and keep its `SOURCE_MANIFEST.json`. That manifest identifies
the exact files; a checksum alone does not authenticate the distributor.

Install Rust/rustup, a C++ compiler, pkg-config, and Cap'n Proto's
compiler/development libraries. The validation host uses installed C++ 1.5.0.
The bundled pinned C++ source is a separate reference, not the installed peer.
From the unpacked directory:

```sh
rustup toolchain install 1.97.0 --profile minimal --component rustfmt --component clippy
cargo fetch --locked
cargo test --test tooling external_consumer_feature_matrix -- --exact
cargo run --locked --example noise_store
```

The downstream check copies `examples/downstream` outside the workspace. It
generates the field API, starts two RPC systems, calls a returned capability
through a pipeline, and commits/reopens storage. It executes all eight feature
subsets and an optimized default build. Its manifest shows the dependency
recipe: point `reproto`, `capnp`, `capnp-rpc` and build dependency `capnpc` at
the **same unpacked bundle**. Internal paths also select vendored
`capnp-futures`; no consumer `[patch.crates-io]` table is needed. Consumers own
their Cargo.lock and must retain the coordinated versions. The separate sample
uses in-memory duplex transport; `noise_store` exercises the UDP/Noise service.

All combinations of `noise`, `storage`, and `services`, including none, are
supported by the build matrix. Default enables all three. Upstream fork features
outside that matrix, particularly `capnp/rpc_try`, TLS/FFI combinations with Noise,
and alternate crypto configurations are not part of the preview.

## Application contract

RPC clients, servers and route tasks belong to a Tokio `LocalSet`; keep the
drivers alive while using their capabilities. Configure explicit peer addresses
or delegate a [discovery directory](NOISE_DEPLOYMENT.md); sessions still pin Noise
identities. Introductions delegate capability rights and bind
the new connection; distribute private keys, tickets and SturdyRefs as secrets.
The custom IK/IKpsk2 BLAKE3 profile is experimental and rejects application
0-RTT. It is not TLS QUIC. See [Noise setup](IMPLEMENTATION.md) and
[persistence](PERSISTENCE.md) for concrete constructors and provisioning.

Set application request deadlines and resource limits for message traversal,
pending calls, routes, bulk transfers, subscriptions and storage. Local cancellation
or timeout does not undo a remote method that already ran. A disconnected route
does not make an old live capability a fresh connection; reacquire the configured
bootstrap or restore an authorized SturdyRef. Persisted references remain subject
to owner, expiry and revocation checks. Never treat serialized capability pointers
as authority. There is no exactly-once application execution guarantee.

The [connection recovery policy](NOISE_RECOVERY.md) offers bounded setup retries
and opt-in replacement of failed routes for newly acquired capabilities. Native
third-party answer setup has a configurable ten-second default deadline. Setup
expiry leaves already-executed remote effects intact and does not limit a method
after authenticated adoption.

Storage calls perform synchronous writes and `fsync`, including inside ORM
handlers. They can stall the local executor and other RPC traffic. This preview
does not promise latency isolation or hard realtime deadlines; qualify latency
under your workload before sharing a storage executor with deadline-sensitive work.

After an ambiguous mutation/compaction failure, stop using the quarantined Store
or Realm, release its snapshots, and reopen successfully before serving again.
Recovery validates framing before using lengths, repairs only genuine incomplete
trailing records, then syncs the selected file and directory. Either sync failure
prevents a serving handle. A failed/lost reply may have committed: inspect the
recovered revision/receipt and use application operation identifiers where needed;
do not blindly replay a mutation. Existing snapshots remain readable but do not
grant authority after revocation. Default quotas are 16 MiB per entry and 256 MiB
per store; limits are host policy and must be supplied again when reopening.

Select retention deliberately. `History` keeps available publications and drafts;
`Publishable` preserves eligible drafts; `Latest` can retire intermediate drafts
and advances the history floor. Expired history is an explicit gap, not an empty
event. Compaction needs space for both generations; held snapshots pin old file
space and writer locks. See [storage administration](STORAGE_COMPACTION.md).

## Backup and restore

For this preview use an **offline backup**: stop admission and mutations, finish
or abandon pending operations with their outcomes recorded, close every Store,
Realm and snapshot, then copy each data file and associated application state to
a protected backup directory. No cross-file transaction or online snapshot API
coordinates an ORM store and a separate realm ledger; quiesce them together.
Never remove a live `.lock` file or externally modify/truncate a mapped data file.

Restore into an empty, private directory while all writers are stopped. Copy the
data files, preserve their permissions and required host quotas, then open them
through Store/Realm and check expected revisions and reference policy before
reopening admission. New lock sidecars are created at the new paths. Verify the
backup in a disposable restore directory first. Do not serve two clones of the
same realm identity independently. Restoring an older backup can roll back
revocation and replay state; checksums do not provide rollback protection. No
automated lineage-safe backup service is supplied by this preview.

Only **RPROTO04** is accepted. Older development formats are rejected without
rewrite; there is no migration reader. Do not point a new binary at the only copy
of old development data expecting conversion. EAE's format is a different format
and cannot be opened in place as a Store.

## Repeatable qualification

Full checks also require Java 17, TLC 1.7.4, CMake, Ninja, a C++23 compiler and
Valgrind. Obtain the TLC release jar from the TLA+ project's `v1.7.4` release;
expected SHA-256 is
`936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88`.
Set `JAVA` to the Java executable and `TLA2TOOLS_JAR` to the jar path, or place
the jar at `target/tools/tla2tools.jar`. The bundle excludes downloaded executables and
registry/git caches; Cargo may download the locked dependencies.

```sh
cargo test --test release isolated_release_qualification -- --ignored --exact --nocapture
```

This reconstructs a source bundle beside the checkout and runs all native Cargo
checks, doctests and Clippy with fresh build outputs. It records source hashes
in `target/release-qualification/cargo-qualification.json` and creates the qualified archive
in `dist`. For the packaging round-trip alone, run
`cargo test --test release source_bundle_roundtrip`. For ordinary development,
run `cargo test --workspace`; see [TESTING.md](TESTING.md).

Custom crypto review, hostile-input fuzzing/soak, broader peer interoperability
and deployment qualification remain listed in [release acceptance](RELEASE_ACCEPTANCE.md).
Report suspected vulnerabilities privately to
[huynhderic@gmail.com](mailto:huynhderic@gmail.com); see the
[security policy](../SECURITY.md). Maintainers should also enable GitHub's private
reporting form using the [repository setup guide](GITHUB_SETUP.md).
