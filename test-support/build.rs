fn compile(files: &[&str], facade: bool) {
    let requested: Vec<_> = files
        .iter()
        .map(|s| format!("../schemas/{s}.capnp"))
        .collect();
    let compiled = capnp_compiler::FileCompiler::new()
        .src_prefix("../schemas")
        .import_path("../vendor")
        .import_path("../vendor/capnpc")
        .compile_with_dependencies(&requested)
        .expect("compile test fixtures in Rust");
    for dependency in compiled.dependencies {
        println!("cargo:rerun-if-changed={}", dependency.display());
    }
    let bytes = capnp::serialize::write_message_to_words(&compiled.message);
    capnpc::codegen::CodeGenerationCommand::new()
        .output_directory(std::env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"))
        .field_api_values(facade)
        .field_api_projections(facade)
        .run(bytes.as_slice())
        .expect("generate test fixture bindings");
}

fn main() {
    compile(&["field-api", "enum-brand"], true);
    compile(
        &[
            "runtime-test",
            "cancellation-policy",
            "dynamic-test",
            "presence",
            "conversion",
            "reflection-lookup",
            "native-list",
            "native-rpc",
            "membrane-copy",
        ],
        false,
    );
}
