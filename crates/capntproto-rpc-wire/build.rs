fn main() {
    println!("cargo:rerun-if-env-changed=LLVM_PROFILE_FILE");
    // Compile on the host with the Rust compiler; embedded targets do not need
    // the C++ executable or a checked-out vendor submodule.
    let compiled = capntproto_compiler::FileCompiler::new()
        .src_prefix("schema")
        .import_path(capntproto_compiler::standard_imports())
        .compile_with_dependencies(&["schema/rpc.capnp", "schema/rpc-twoparty.capnp"])
        .expect("compile pinned portable RPC schemas");
    for dependency in compiled.dependencies {
        println!("cargo:rerun-if-changed={}", dependency.display());
    }
    let bytes = capnp::serialize::write_message_to_words(&compiled.message);
    capnpc::codegen::CodeGenerationCommand::new()
        .output_directory(std::env::var_os("OUT_DIR").unwrap())
        .field_api(true)
        .run(bytes.as_slice())
        .expect("generate portable RPC wire types");
}
