fn main() {
    println!("cargo:rerun-if-changed=entry.capnp");
    capnpc::CompilerCommand::new()
        .file("entry.capnp")
        .field_api(true)
        .run()
        .expect("generate EAE integration fixture");
}
