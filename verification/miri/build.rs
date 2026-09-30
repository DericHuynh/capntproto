fn main() {
    println!("cargo:rerun-if-changed=../../schemas/field-api.capnp");
    println!("cargo:rerun-if-changed=../../vendor/capnpc/rust.capnp");
    capnpc::CompilerCommand::new()
        .src_prefix("../../schemas")
        .import_path("../../vendor/capnpc")
        .file("../../schemas/field-api.capnp")
        .field_api(true)
        .field_api_values(true)
        .field_api_projections(true)
        .run()
        .expect("compile the production field API fixture");
}
