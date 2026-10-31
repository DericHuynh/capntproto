fn main() {
    let file = "schema/greeting.capnp";
    let compiled = capntproto_compiler::FileCompiler::new()
        .src_prefix("schema")
        .compile_with_dependencies(&[file])
        .unwrap();
    for dependency in compiled.dependencies {
        println!("cargo:rerun-if-changed={}", dependency.display());
    }
    let words = capnp::serialize::write_message_to_words(&compiled.message);
    capnpc::codegen::CodeGenerationCommand::new()
        .capnp_root("::capntproto::capnp")
        .output_directory(std::env::var_os("OUT_DIR").unwrap())
        .run(words.as_slice())
        .unwrap();
}
