# Cap'n Proto code generation for Rust

[![crates.io](https://img.shields.io/crates/v/capnpc.svg)](https://crates.io/crates/capnpc)

[documentation](https://docs.rs/capnpc/)

The generated code depends on the [capnproto-rust runtime library](https://github.com/capnproto/capnproto-rust).

Code generation can be customized through the annotations defined in [`rust.capnp`](rust.capnp).

If `capnp` is renamed in your dependencies, set the runtime path in your build
script. For `runtime = { package = "capnp", version = "0.25" }`:

```rust
fn main() {
    capnpc::CompilerCommand::new()
        .capnp_root("::runtime")
        .file("schema.capnp")
        .run()
        .expect("compile schema");
}
```

The default is `::capnp`. A module that re-exports the runtime can use
`.capnp_root("crate::wire")`; bindings generated inside the runtime itself use
`.capnp_root("crate")`. The option applies to lists, constants, interfaces and
the optional field API, native values and projections. When supplying a
`CodeGeneratorRequest` directly, set the same option on
`capnpc::codegen::CodeGenerationCommand`.
