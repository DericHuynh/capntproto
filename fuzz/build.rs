fn main() {
    println!("cargo:rerun-if-changed=schemas/lifecycle.capnp");
    capnpc::CompilerCommand::new()
        .src_prefix("schemas")
        .file("schemas/lifecycle.capnp")
        .run()
        .expect("compile stateful RPC fuzz fixture");
}
