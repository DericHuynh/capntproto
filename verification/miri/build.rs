fn main() {
    // A fresh coverage run needs fresh build-script and code-generation counters.
    println!("cargo:rerun-if-env-changed=LLVM_PROFILE_FILE");
    println!("cargo:rerun-if-changed=../../schemas/field-api.capnp");
    println!("cargo:rerun-if-changed=../../crates/capntproto-codegen/rust.capnp");
    capnpc::CompilerCommand::new()
        .src_prefix("../../schemas")
        .import_path("../../crates/capntproto-codegen")
        .file("../../schemas/field-api.capnp")
        .field_api(true)
        .field_api_values(true)
        .field_api_projections(true)
        .run()
        .expect("compile the production field API fixture");
}
