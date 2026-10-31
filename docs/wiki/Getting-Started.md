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
`capntproto-0.1.0-source.tar.gz`, verify its published SHA-256 against a trusted
delivery channel, and keep its `SOURCE_MANIFEST.json`. That manifest identifies
the exact files; a checksum alone does not authenticate the distributor.

Start with the part of the library your application uses. Rust **1.97** is the
manifest minimum; **1.97.0** is the reproducible development/CI pin. This is not
an assertion that newer compilers are rejected or qualified. Stable application
builds do not require nightly, Java, TLC or Valgrind.

| Build | Command | Additional build tools |
| --- | --- | --- |
| Serialization / schema compiler | `cargo build --locked -p capntproto-core -p capntproto-compiler` | Rust and platform linker |
| TCP RPC | `cargo build --locked -p capntproto --no-default-features` | C toolchain for ring |
| TCP with TLS/mTLS | `cargo build --locked -p capntproto --no-default-features --features tls` | C toolchain for ring; no BoringSSL/quiche |
| Storage and services | Add `--features storage,services` to a minimal build | Schemas compile with the workspace Rust compiler |
| Default / native TCP/mTLS | `cargo build --locked -p capntproto` | Native feature still builds quiche/BoringSSL: C/C++, CMake, libclang |
| Explicit two-party QUIC | `cargo build --locked -p capntproto --features quic` | C/C++, CMake, libclang for quiche/BoringSSL |

The Cap'n Proto C++ compiler and headers are needed for independent compatibility
checks, not for the library's Rust schema build. The full test suite additionally
uses C++ reference source, nightly analysis tools and Java/TLC; see [Testing](Testing.md).

TLS/TCP uses rustls with ring; quiche uses its BoringSSL integration. These are
separate TLS implementations because the unmodified upstream QUIC engine owns
its TLS handshake. Selecting only `tls` avoids the quiche/BoringSSL dependency
entirely. Default features retain both transports for API compatibility.

For a focused first run:

```sh
rustup toolchain install 1.97.0 --profile minimal --component rustfmt --component clippy
cargo install cargo-nextest --version 0.9.146 --locked
cargo nextest run --locked -p capntproto --no-default-features --features tls --test secure_rpc
```

For the full default-feature downstream example after installing QUIC tools:

```sh
bash scripts/setup-auditable.sh
export PATH="$PWD/target/auditable-tools/wrapper:$PWD/target/auditable-tools/bin:$PATH"
cargo nextest run --locked --test tooling external_consumer_default_features -- --exact
cargo run --locked --example native_store
```

The downstream check copies `examples/downstream` outside the workspace. It
generates the field API, starts two RPC systems, calls a returned capability
through a pipeline, and commits/reopens storage. It executes default and
optimized builds. Its manifest shows the dependency
recipe: use `capntproto` as the runtime dependency and access `capnp`, `capnp_rpc`
and `capnp_futures` through its re-exports. Schema generation uses the
`capntproto-codegen` build dependency (aliased as `capnpc`) and
`CompilerCommand::capnp_root("::capntproto::capnp")`. Published dependencies carry
registry versions; source checkouts also carry local paths. No consumer
`[patch.crates-io]` table is needed. Consumers own their Cargo.lock. The separate sample
uses in-memory duplex transport; `native_store` exercises the UDP/Native service.

All combinations of `native`, `storage`, and `services`, including none, are
supported by the build matrix. Defaults use TCP and include TLS/mTLS. The `quic`
feature explicitly enables standard two-party QUIC. The `tls` feature can be selected alone; `quic` transitively enables
`native` and `tls`. See [secure transports](TCP-TLS-and-QUIC.md). Optional nightly
`capnp/rpc_try` and alternate upstream crypto configurations are outside the
stable preview contract.

## Application contract

RPC clients, servers and route tasks belong to a Tokio `LocalSet`; keep the
drivers alive while using their capabilities. Configure explicit peer addresses
or delegate a [discovery directory](Discovery-and-Mobility.md); sessions still pin Native
identities. Introductions delegate capability rights and bind
the new connection; distribute private keys, tickets and SturdyRefs as secrets.
The native mutual TLS admission profile is experimental and rejects application
0-RTT. Quiche uses standard QUIC v1. See [Native setup](Native-Transports.md) and
[persistence](Persistence.md) for concrete constructors and provisioning.

Set application request deadlines and resource limits for message traversal,
pending calls, routes, bulk transfers, subscriptions and storage. Local cancellation
or timeout does not undo a remote method that already ran. A disconnected route
does not make an old live capability a fresh connection; reacquire the configured
bootstrap or restore an authorized SturdyRef. Persisted references remain subject
to owner, expiry and revocation checks. Never treat serialized capability pointers
as authority. There is no exactly-once application execution guarantee.

The [connection recovery policy](Connection-Recovery.md) offers bounded setup retries
and opt-in replacement of failed routes for newly acquired capabilities. Native
third-party answer setup has a configurable ten-second default deadline. Setup
expiry leaves already-executed remote effects intact and does not limit a method
after authenticated adoption.

Storage calls perform synchronous writes and `fsync`, including inside ORM
handlers. They can stall the local executor and other RPC traffic. This preview
does not promise latency isolation or hard realtime deadlines; qualify latency
under your workload before sharing a storage executor with deadline-sensitive work.

For privileged host code, the opt-in [storage worker](Storage-Worker.md) moves
open/recovery and storage operations to a dedicated thread with bounded admission,
queued cancellation and explicit shutdown. Existing ORM RPC handlers are not
automatically migrated, and borrowed snapshot reads can still fault on the caller.

After an ambiguous mutation/compaction failure, stop using the quarantined Store
or Realm, release its snapshots, and reopen successfully before serving again.
Recovery validates framing before using lengths, repairs only genuine incomplete
trailing records, then syncs the selected file and directory. Either sync failure
prevents a serving handle. A failed/lost reply may have committed: inspect the
recovered revision/receipt and use application operation identifiers where needed;
do not blindly replay a mutation. Existing snapshots remain readable but do not
grant authority after revocation. Default quotas are 16 MiB per entry and 256 MiB
per store; limits are host policy and must be supplied again when reopening.

This recovery contract assumes functioning filesystem durability barriers.
Real media/writeback errors require a separate integrity investigation; a
successful reopen and another `fsync` are not established repair for such errors.
The injected errors in unit tests occur at application I/O seams. They do not
emulate all kernel writeback/cache behavior. See the
[failure research and qualification plan](Storage-Resilience.md).

Select retention deliberately. `History` keeps available publications and drafts;
`Publishable` preserves eligible drafts; `Latest` can retire intermediate drafts
and advances the history floor. Expired history is an explicit gap, not an empty
event. Compaction needs space for both generations; held snapshots pin old file
space and writer locks. See [storage administration](Storage-Compaction.md).

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

Whole-entry `Store::open` accepts **RPROTO04**; the explicit
`Store::open_components` API accepts **RPROTO05**. Each rejects the other layout
without rewriting it. Older development formats are rejected without rewrite;
there is no migration reader. Do not point a new binary at the only copy
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
cargo nextest run --test release isolated_release_qualification -- --ignored --exact --nocapture
```

This reconstructs a source bundle beside the checkout and runs all native Cargo
checks, doctests and Clippy with fresh build outputs. It records source hashes
in `target/release-qualification/cargo-qualification.json` and creates the qualified archive
in `dist`. For the packaging round-trip alone, run
`cargo nextest run --test release source_bundle_roundtrip`. For ordinary development,
run `cargo nextest run --workspace`; see [Testing](Testing.md).

Native admission/security review, hostile-input fuzzing/soak, broader peer interoperability
and deployment qualification remain listed in [release acceptance](Release-Acceptance.md).
Report suspected vulnerabilities privately to
[huynhderic@gmail.com](mailto:huynhderic@gmail.com); see the
[security policy](../../SECURITY.md). Maintainers should also enable GitHub's private
reporting form using the [repository setup guide](GitHub-Setup.md).

## Publishing releases

The repository's `release.yml` workflow verifies the complete package set and
runs both simple and deep RPC examples before authenticating. Run it from `main`
with **publish** unchecked to check a release without uploading. Publishing also
requires a successful CI run for that exact commit.

For the first release, create a short-lived crates.io API token permitted to
publish the new crate names, then store it in this repository under **Settings →
Secrets and variables → Actions → New repository secret**, named
`CARGO_REGISTRY_TOKEN`. Do not put the token in source code, workflow inputs or
chat. Run **Release / crates.io** with **publish** and **bootstrap** checked.
Crates.io requires this initial token publication before trusted publishing can
be configured for a new crate.

After that first upload, open each crate's **Settings → Trusted Publishing** on
crates.io and add a GitHub publisher with owner `DericHuynh`, repository
`capntproto`, workflow filename `release.yml`, and no environment. Configure all
eleven packages: `capntproto`, `capntproto-core`, `capntproto-codegen`,
`capntproto-compiler`, `capntproto-futures`, `capntproto-rpc`,
`capntproto-rpc-engine`, `capntproto-rpc-wire`, `capntproto-compat` and
`capntproto-rustls` and `capntproto-transport`. Subsequent releases use **publish** checked and **bootstrap**
unchecked. GitHub OIDC then obtains a temporary token; the pinned official
[rust-lang action](https://github.com/rust-lang/crates-io-auth-action) revokes it
at job completion. Remove the bootstrap secret and revoke its token after the
trusted publisher is configured.

The `capntproto-rustls` package preserves our direct-output record encryption
backport and the upstream rustls licenses. Configure it through
`capntproto::rpc::security::rustls`; the official `rustls` crate has distinct config
types. See the [fork's provenance and API differences](../../vendor/rustls/README.capntproto.md).
