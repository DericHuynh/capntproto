fn main() {
    let mut compiler = capnp_compiler::FileCompiler::new();
    compiler
        .src_prefix("schema/capnp/compat")
        .import_path("schema");
    let files = ["byte-stream", "http-over-capnp", "json"]
        .map(|name| format!("schema/capnp/compat/{name}.capnp"));
    let compiled = compiler
        .compile_with_dependencies(&files)
        .expect("compile compatibility schemas");
    for dependency in compiled.dependencies {
        println!("cargo:rerun-if-changed={}", dependency.display());
    }
    capnpc::codegen::CodeGenerationCommand::new()
        .output_directory(std::env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"))
        .run(capnp::serialize::write_message_to_words(&compiled.message).as_slice())
        .expect("generate compatibility bindings");
}
