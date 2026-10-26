# Local rustls 0.23.46: upstream caller-provided encryption output

This directory is the checksum-verified published 0.23.46 package plus a targeted
backport of [rustls PR #3127](https://github.com/rustls/rustls/pull/3127), merged at
`73a793c90f2e8c8b651cd3f1131f4be27f41259b`. Original notices and licenses remain.
`UPSTREAM.json` records the package provenance and original file hashes;
`UPSTREAM-3127.json` records the upstream implementation and the 0.23 adaptations.
`LOCAL.patch` records the crypto implementation delta against the published crate.
The package manifest and README additionally identify the published fork as
`capntproto-rustls`; `Cargo.toml.orig` retains the original upstream manifest.

## Provider contract and data path

The previous local `MessageEncrypter::encrypt_into` extension is removed.
Following upstream, `MessageEncrypter::encrypt` now takes caller-owned ciphertext
storage and returns an `EncodedMessage` borrowing the written payload prefix.
The provider does not write the five-byte TLS header: the record layer encodes
it after encryption. `EncryptBuffer` gathers plaintext directly into the output
and appends suite-specific framing and the authentication tag. Ring and AWS-LC,
TLS 1.2 and TLS 1.3 use the upstream implementations of this contract.

**This breaks the crypto-provider trait API.** Custom `MessageEncrypter`
implementations must accept the output buffer and return borrowed ciphertext;
there is no owned-record fallback method. Application-level capntproto TLS APIs
retain their signatures. Vendored and registry rustls still have distinct crate
identities in downstream workspaces unless their dependency roots unify them.

The 0.23 Unbuffered state machine, whole-write size preflight, queued-control
ordering, key-refresh logic and record-sequence exhaustion checks remain intact.
`OutboundPlainMessage` is the 0.23 input representation. `EncodedMessage` and
`EncryptBuffer` are adapted from upstream; a short buffer uses the existing
`Error::EncryptError`, and the chunk iterator preserves 0.23's compiler support.
Buffered encryption also calls the same provider operation, with an owned output
vector. This is not an upgrade to the complete 0.24 split-connection API or its
buffer-recycling architecture.

Application ciphertext does not pass through a temporary allocation or copy.
Plaintext still enters output once. Received plaintext and queued handshake/control
records retain their 0.23 storage behavior. The root dependency aliases the `capntproto-rustls` package as `rustls`; the
maintained benchmark uses it through the root facade. The independent buffered
reference fixture uses official registry rustls. No consumer Cargo patch is required.

## Validation

Restore the complete workspace at the commit in `UPSTREAM.json`; the published
crate omits the upstream test fixtures. Apply `LOCAL.patch` in its `rustls/`
subdirectory and `UPSTREAM-TESTS.patch` at the workspace root. The latter updates
test-provider and direct-encryption plumbing for the new API without changing
protocol assertions. Verify the production files match this directory, then run:

```sh
cargo test --locked -p rustls --no-default-features --features ring,std,tls12,logging
cargo test --locked -p rustls
cargo test --locked -p rustls $(python3 admin/all-features-except fips rustls)
```

The backport passes 543 ring, 549 default AWS-LC and 950 all-non-FIPS test executions,
including upstream's buffer-cursor/range tests. Each run retains eight upstream-
ignored cases. BoGo, FIPS and the full platform/toolchain matrix are outside those
runs. Repository tests additionally check byte-for-byte equivalence against the
original owned ring sealing operation, zero allocations, short-buffer retries,
untouched tails, and actual buffered/unbuffered key updates for all three ciphers.
