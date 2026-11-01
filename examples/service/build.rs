fn main() {
    capnpc::CompilerCommand::new()
        .src_prefix("schema")
        .file("schema/greeting.capnp")
        .run_native()
        .expect("compile service schema in Rust");
}
