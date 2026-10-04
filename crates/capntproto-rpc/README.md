# capntproto-rpc

Capability RPC, pipelining, flow control, cancellation and multiparty protocol machinery. This is a maintained first-party workspace crate derived from
[`capnp-rpc`](https://github.com/capnproto/capnproto-rust), with the original MIT
[license](LICENSE) and [upstream changelog](CHANGELOG.md) preserved.
The upstream starting point and source hashes are recorded in
[provenance](../../vendor/provenance/capnp-rpc-revision.json).

The Cargo package is `capntproto-rpc`. Its Rust library name remains `capnp_rpc`
to preserve compatibility with generated bindings. Use a package alias when
selecting it directly; the root `capntproto` crate already selects this coordinated set.
All maintained crates share the root lockfile and are tested with
`cargo nextest run --workspace`. See the [workspace policy](../../docs/wiki/Fork-Policy.md).
