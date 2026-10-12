fn main() {
    let compiled = capntproto_compiler::FileCompiler::new()
        .src_prefix("schema")
        .import_path("../../schemas/imports")
        .compile_with_dependencies(&["schema/portable.capnp"])
        .expect("compile portable data schema on the host");
    for dependency in compiled.dependencies {
        println!("cargo:rerun-if-changed={}", dependency.display());
    }
    let bytes = capnp::serialize::write_message_to_words(&compiled.message);
    capnpc::codegen::CodeGenerationCommand::new()
        .output_directory(std::env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"))
        .field_api(true)
        .bounded_rpc(true)
        .run(bytes.as_slice())
        .expect("generate borrowed views for the allocation-free target");
}
