# Third-party source and dependency notices

Capn't Proto's original code is covered by the root MIT license. Included upstream
code retains its original notices and licenses; the root license does not
replace them.

| Included component | Notice | Provenance |
|---|---|---|
| Coordinated `capnp`, `capnpc`, `capnp-rpc` forks | Each directory's `LICENSE` under `vendor/` (MIT) | Corresponding `vendor/provenance/*-revision.json` |
| Coordinated `capnp-futures` fork | `vendor/capnp-futures/LICENSE` (MIT) | `vendor/provenance/capnp-futures-revision.json` records the upstream base; async framing and queue changes are maintained locally |
| Noise/quiche fork | `vendor/quiche/COPYING` (BSD-2-Clause), source notices | `vendor/provenance/quiche-revision.json` and `quiche-noise.patch` |
| C++ reference submodule and normative schemas | `vendor/capnproto/LICENSE`, schema notices | `.gitmodules` and `vendor/provenance/revision.json` |
| Snow | Apache-2.0 OR MIT, obtained through Cargo | `vendor/provenance/snow-revision.json`, exact git revision in Cargo.lock |
| Shuttle 0.9.4 | Apache-2.0, obtained through Cargo | Development-only schedule testing; exact version and component checksums in Cargo.lock |
| EAE research archive | Archive package metadata declares MIT | `research/EAE-Reconstruction.zip`; benchmark-only input, not a runtime dependency |

Cargo.lock records the complete dependency resolution. The source release
manifest inventories included file hashes. Cargo downloads retain their own
license notices in the dependency sources; this bundle is not a fully vendored
offline registry. EAE's archived source/reports remain separate research inputs.
