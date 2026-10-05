# Fork and compatibility policy

The project is unreleased 0.x. Private Rust APIs and Capntproto-only storage/transport
formats may break. Do not add forwarding aliases, format migrations or downgrade
fallbacks solely for earlier development snapshots. The realm loader accepts only
`reproto-realm/2`, and the explicit storage openers accept `RPROTO04` (whole entry) or
`RPROTO05` (components). Each rejects the other layout; malformed, incomplete
and obsolete inputs fail without rewriting.

This does not remove standard Cap'n Proto semantics: older-peer handling specified
by the RPC schema, unknown-field/schema evolution and capability ownership remain
part of interoperability. Compact and fragmented realtime encodings serve distinct
payload sizes; the compact path is not a compatibility shim. Direct and arbitrated
connections remain supported connection policies.

The wire runtime, RPC engine, async serialization and Rust generator are owned
workspace crates under `crates/capntproto-{core,rpc,futures,codegen}`. They share
the root lockfile. Their package names identify our maintained implementations;
Rust library names and generated binding paths remain `capnp`, `capnp_rpc`,
`capnp_futures` and `capnpc`. Use explicit Cargo `package` aliases for direct
consumers. Generated field APIs require this coordinated set.

Original MIT licenses, upstream changelogs, release manifests and source hashes
under `vendor/provenance/` retain attribution and the imported baseline. New
changes belong to the owning workspace crate and run in the workspace test,
lint, coverage and source-distribution gates.

Quiche is the unmodified crates.io package, pinned to `=0.30.0` with its registry
checksum in each consumer's lockfile. There is no source patch or local QUIC fork.
Our transport adapters use public APIs. Native QUIC receipt confirmations belong
to our protocol, not Quiche's internal stream state. Upstream currently supports
QUIC v1; explicit v2 requests fail without downgrade. Ordinary dependencies,
including the general `futures` crate and TLS libraries, remain Cargo-managed.

For an upgrade:

1. Record upstream provenance and retain licenses for imported code.
2. Preserve capability, budget, cancellation and output contracts in owned crates.
3. Regenerate bindings and compiler fixtures; check standard schema identity.
4. Run compiler rejection tests, C++ interoperability and relevant model replays.
5. For Quiche, update the registry pin and lockfiles, then run our encrypted
   transport tests, TLS/mTLS rejection tests and independent s2n-quic interoperability.
6. Run the workspace and source-distribution acceptance gates.

`cargo nextest run --test tooling external_consumer_feature_matrix -- --exact` builds a
separate client/server with the owned packages and no consumer root patches.
The C++ reference remains a pinned submodule with a source hash inventory, so
checks also work from an unpacked source release without Git metadata.
