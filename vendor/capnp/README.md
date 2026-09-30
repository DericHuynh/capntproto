# Cap'n Proto runtime library for Rust

This is the ReProto fork of the runtime, requiring **Rust 1.97.0 or newer**.
See [fork changes and scope](REPROTO.md). The links below describe the upstream
crate.

[![crates.io](https://img.shields.io/crates/v/capnp.svg)](https://crates.io/crates/capnp)

[documentation](https://docs.rs/capnp/)

The optional `rpc_try` feature requires nightly Rust (tested with
`nightly-2026-08-29`). With `alloc` enabled, it permits `Result?` inside functions
returning `capnp::capability::Promise<T, capnp::Error>` and converts errors using
`From`. It does not await promises: use `promise.await?` in async code.
The previous panic-only `Try` implementation has been removed, so direct
`promise?` is now a compile error. Default features continue to use stable Rust.
