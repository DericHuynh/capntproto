# capnp-compiler

Experimental Rust schema-language compiler for Cap'n Proto. Provides in-memory,
filesystem and callback sources, immutable reflection snapshots, lazy sessions,
concurrent parser caches and the `capnp-compile` CLI. It emits standard
`CodeGeneratorRequest` messages without invoking C++; `capnpc` generates Rust.

Use the coordinated source checkout and Rust 1.97.0. This crate is unpublished.
[Schema compiler guide](../../docs/wiki/Schema-Compiler.md) ·
[Corpus and limits](../../docs/wiki/Compiler-Qualification.md) ·
[Wiki home](../../docs/wiki/Home.md)

From the repository root, after the documented tool setup:

```sh
cargo test --locked -p capnp-compiler
```
