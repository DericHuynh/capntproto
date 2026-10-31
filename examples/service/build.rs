fn main() {
    capnpc::CompilerCommand::new()
        .src_prefix("../webtransport/schema")
        .file("../webtransport/schema/greeting.capnp")
        .run_native()
        .expect("compile service schema in Rust");
}
