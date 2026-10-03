fn compile(files: &[&str], facade: bool, structured: bool) {
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
    let mut output =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    let mut command = capnpc::codegen::CodeGenerationCommand::new();
    if structured {
        output.push("structured");
        std::fs::create_dir_all(&output).unwrap();
        command.default_parent_module(vec!["structured".into()]);
    }
    command
        .structured_replies(structured)
        .output_directory(output)
        .field_api_values(facade)
        .field_api_projections(facade)
        .run(bytes.as_slice())
        .expect("generate test fixture bindings");
}

fn main() {
    compile(&["field-api", "enum-brand"], true, false);
    compile(&["runtime-test", "rpc-api"], true, true);
    compile(
        &[
            "runtime-test",
            "rpc-api",
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
        false,
    );
}
