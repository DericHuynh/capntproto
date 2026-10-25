fn main() {
    let compiled = capntproto_compiler::FileCompiler::new()
        .import_path("../../schemas/imports")
        .compile_with_dependencies(&["legacy.capnp"])
        .expect("compile legacy invocation schema");
    for path in compiled.dependencies {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let bytes = capnp::serialize::write_message_to_words(&compiled.message);
    capnpc::codegen::CodeGenerationCommand::new()
        .output_directory(std::env::var_os("OUT_DIR").unwrap())
        .bounded_rpc(true)
        .run(bytes.as_slice())
        .expect("generate caller-owned service bindings");
}
