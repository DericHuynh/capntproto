# capntproto-codegen

Rust binding generation from Cap’n Proto schema requests. This is a maintained first-party workspace crate derived from
[`capnpc`](https://github.com/capnproto/capnproto-rust), with the original MIT
[license](LICENSE) and [upstream changelog](CHANGELOG.md) preserved.
The upstream starting point and source hashes are recorded in
[provenance](../../vendor/provenance/capnpc-revision.json).

The Cargo package is `capntproto-codegen`. Its Rust library name remains `capnpc`
to preserve compatibility with generated bindings. Use a package alias when
selecting it directly; the root `capntproto` crate already selects this coordinated set.
All maintained crates share the root lockfile and are tested with
`cargo nextest run --workspace`. See the [workspace policy](../../docs/wiki/Fork-Policy.md).

Build scripts can use the bundled Rust frontend with one dependency and no
external `capnp` executable:

```toml
[build-dependencies]
capnpc = { package = "capntproto-codegen", version = "0.25" }
```

```rust
fn main() {
    capnpc::CompilerCommand::new()
        .src_prefix("schema")
        .file("schema/greeting.capnp")
        .run_native()
        .expect("compile greeting schema");
}
```

`run_native()` uses `OUT_DIR` by default, discovers schema/embed dependencies for
Cargo, and applies the same code generation settings as `run()`. Explicit import
roots take precedence over the bundled standard schemas, including
`/capnp/rust.capnp`. Disable standard imports with `no_standard_import()`.
The Rust frontend currently accepts one source prefix and supports the schema
language described by `capntproto-compiler`. `run()` retains the external C++
compiler path. Disable the default `native` feature for plugin-only builds.

When your runtime dependency is the root `capntproto` facade, add
`.capnp_root("::capntproto::capnp")` to the builder. With a dependency named `capnp`,
the default generated runtime path already matches.
