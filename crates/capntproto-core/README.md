# capntproto-core

Cap’n Proto wire runtime, checked message access, reflection and dynamic ownership. This is a maintained first-party workspace crate derived from
[`capnp`](https://github.com/capnproto/capnproto-rust), with the original MIT
[license](LICENSE) and [upstream changelog](CHANGELOG.md) preserved.
The upstream starting point and source hashes are recorded in
[provenance](../../vendor/provenance/capnp-revision.json).

The Cargo package is `capntproto-core`. Its Rust library name remains `capnp`
to preserve compatibility with generated bindings. Use a package alias when
selecting it directly; the root `capntproto` crate already selects this coordinated set.
All maintained crates share the root lockfile and are tested with
`cargo test --workspace`. See the [workspace policy](../../docs/wiki/Fork-Policy.md).
