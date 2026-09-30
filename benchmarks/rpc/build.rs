fn main() {
    println!("cargo:rerun-if-changed=echo.capnp");
    println!("cargo:rerun-if-changed=echo.proto");
    capnpc::CompilerCommand::new()
        .file("echo.capnp")
        .run()
        .unwrap();
    tonic_prost_build::compile_protos("echo.proto").unwrap();
}
