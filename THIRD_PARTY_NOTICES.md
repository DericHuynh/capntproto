# Third-party source and dependency notices

Capntproto's original code is covered by the root MIT license. Included upstream
code retains its original notices and licenses; the root license does not
replace them.

| Included component | Notice | Provenance |
|---|---|---|
| Coordinated `capnp`, `capnpc`, `capnp-rpc` forks | Each directory's `LICENSE` under `vendor/` (MIT) | Corresponding `vendor/provenance/*-revision.json` |
| Coordinated `capnp-futures` fork | `vendor/capnp-futures/LICENSE` (MIT) | `vendor/provenance/capnp-futures-revision.json` records the upstream base; async framing and queue changes are maintained locally |
| QUIC v1/v2 quiche fork | `vendor/quiche/COPYING` (BSD-2-Clause), source notices | `vendor/provenance/quiche-revision.json` and `quiche-native.patch` |
| C++ reference submodule and normative schemas | `vendor/capnproto/LICENSE`, schema notices | `.gitmodules` and `vendor/provenance/revision.json` |
| RFC 9369 packet vectors | `vendor/quiche/LICENSE-RFC9369` (Revised BSD) | RFC 9369 Appendix A; protocol regression tests |
| rustls and tokio-rustls | Apache-2.0 OR MIT, obtained through Cargo | TLS-over-TCP implementation; exact versions and checksums in Cargo.lock |
| BoringSSL / boring and x509-parser | Respective crate license notices, obtained through Cargo | quiche TLS and pinned certificate verification; exact versions in Cargo.lock |
| rcgen | Apache-2.0 OR MIT, obtained through Cargo | Native identity and test certificate generation; version and checksum in Cargo.lock |
| Shuttle 0.9.4 | Apache-2.0, obtained through Cargo | Development-only schedule testing; exact version and component checksums in Cargo.lock |
| EAE research archive | Archive package metadata declares MIT | `research/EAE-Reconstruction.zip`; benchmark-only input, not a runtime dependency |

Cargo.lock records the complete dependency resolution. The source release
manifest inventories included file hashes. Cargo downloads retain their own
license notices in the dependency sources; this bundle is not a fully vendored
offline registry. EAE's archived source/reports remain separate research inputs.
