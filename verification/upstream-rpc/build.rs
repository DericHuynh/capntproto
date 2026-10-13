fn main() {
    println!("cargo:rerun-if-changed=../../schemas/interop.capnp");
    println!("cargo:rerun-if-changed=../../schemas/runtime-test.capnp");
    println!("cargo:rerun-if-env-changed=CAPNP");
    let mut command = capnpc::CompilerCommand::new();
    if let Some(executable) = std::env::var_os("CAPNP") {
        command.capnp_executable(executable);
    }
    command
        .src_prefix("../../schemas")
        .import_path("../../schemas/imports")
        .file("../../schemas/interop.capnp")
        .file("../../schemas/runtime-test.capnp")
        .run()
        .expect("compile interoperability schema with upstream capnpc");
}
