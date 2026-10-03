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

The vendored core, RPC engine, async serialization and generator are a coordinated set. Generated field
APIs target this vendored core. Upstream Cargo versions alone do not express fork
compatibility; use Cargo.lock, the provenance files under `vendor/provenance/`, and the
current validation source hashes. Do not mix independently upgraded components.

For an upgrade:

1. Record the upstream revision and license in its provenance file.
2. Rebase changes preserving capability, budget, cancellation and output contracts.
3. Regenerate RPC bindings and compiler fixtures; check standard schema identity.
4. Run compiler rejection tests, C++ reference/interoperability tests and model replays.
5. For quiche, run `python3 scripts/update_quiche_patch.py --git-dir
   /path/to/pinned-upstream-checkout/.git`, then the Native profile check. It verifies
   that the archived patch reverses cleanly on the vendored source tree.
6. Run the complete acceptance gate on the resulting snapshot.

Fork extensions should expose explicit contracts at their owning layer: checked
root ownership in the core, output completion in two-party RPC, and acknowledged
send bytes in quiche. Higher layers must not infer these from implementation errors.

`vendor/quiche/` is ordinary vendored source, including its CI lockfile. Keep upstream
Git metadata outside this repository so an initial `git add` cannot turn the fork
into an embedded repository link. For patch maintenance, use a separate clone of
the repository in `vendor/provenance/quiche-revision.json`, checked out at its recorded
revision, or preserved Git metadata from that revision. The patch helper uses
that metadata with this repository's vendored source as its working tree; it does
not stage changes or modify the separate clone's source. With an isolated fork
checkout that still contains `vendor/quiche/.git`, the helper also works without the
`--git-dir` option.

The source-preview manifests use explicit paths throughout the coordinated
crates, including `capnp-futures`. Consumer root patches are unnecessary.
`cargo test --test tooling external_consumer_feature_matrix -- --exact` generates and executes a separate client/server.
The pinned reference C++ tree has a source hash inventory so checks also work
from an unpacked source release without `.git` metadata.
