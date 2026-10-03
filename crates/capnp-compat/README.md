# capnp-compat

Optional, unpublished compatibility adapters for the coordinated Cap'n Proto
Rust runtime: JSON/text codecs, ByteStream, HTTP-over-capabilities, WebSocket
message framing and JSON-RPC. Applications supply their executor and transport
integrations. Ordinary `reproto` RPC does not depend on this crate.

[Adapter guide and limits](../../docs/wiki/Compatibility-Adapters.md) ·
[Wiki home](../../docs/wiki/Home.md) · [Upstream attribution](NOTICE)

From the repository root with Rust 1.97.0 and the documented tool setup:

```sh
cargo test --locked -p capnp-compat
```
