fn main() {
    println!("cargo:rerun-if-changed=preview.capnp");
    capnpc::CompilerCommand::new()
        .file("preview.capnp")
        .field_api(true)
        .run()
        .expect("generate the preview field API");
}
