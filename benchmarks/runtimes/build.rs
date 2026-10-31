fn main() {
    println!("cargo:rerun-if-changed=service.capnp");
    println!("cargo:rerun-if-changed=measure.c");
    println!("cargo:rerun-if-env-changed=CAPNTPROTO_VALGRIND_INCLUDE");
    let schema = capntproto_compiler::FileCompiler::new()
        .src_prefix(".")
        .compile_with_dependencies(&["service.capnp"])
        .unwrap();
    capnpc::codegen::CodeGenerationCommand::new()
        .output_directory(std::env::var_os("OUT_DIR").unwrap())
        .run(capnp::serialize::write_message_to_words(&schema.message).as_slice())
        .unwrap();
    let mut c = cc::Build::new();
    c.file("measure.c");
    if let Some(path) = std::env::var_os("CAPNTPROTO_VALGRIND_INCLUDE") {
        c.include(path);
    }
    c.compile("runtime_measure");
}
