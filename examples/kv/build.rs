fn main() {
    let compiled = capntproto_compiler::FileCompiler::new()
        .src_prefix(".")
        .compile_with_dependencies(&["key_value.capnp"])
        .expect("compile the service schema in Rust");
    for input in compiled.dependencies {
        println!("cargo:rerun-if-changed={}", input.display());
    }
    capnpc::codegen::CodeGenerationCommand::new()
        .capnp_root("::capntproto::capnp")
        .output_directory(std::env::var_os("OUT_DIR").unwrap())
        .run(capnp::serialize::write_message_to_words(&compiled.message).as_slice())
        .expect("generate service types");
}
