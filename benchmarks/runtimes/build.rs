fn main() {
    println!("cargo:rerun-if-changed=service.capnp");
    println!("cargo:rerun-if-changed=measure.c");
    println!("cargo:rerun-if-env-changed=CAPNTPROTO_VALGRIND_INCLUDE");
    println!("cargo:rerun-if-env-changed=CAPNTPROTO_ALLOCATION_TRACE");
    println!("cargo:rustc-check-cfg=cfg(benchmark_allocation_trace)");
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
    if std::env::var("CAPNTPROTO_ALLOCATION_TRACE").as_deref() == Ok("1") {
        println!("cargo:rustc-cfg=benchmark_allocation_trace");
        c.define("CAPNTPROTO_ALLOCATION_TRACE", "1");
    }
    if let Some(path) = std::env::var_os("CAPNTPROTO_VALGRIND_INCLUDE") {
        c.include(path);
    }
    c.compile("runtime_measure");
}
