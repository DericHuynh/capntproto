fn main() {
    // A fresh coverage run needs fresh build-script and code-generation counters.
    println!("cargo:rerun-if-env-changed=LLVM_PROFILE_FILE");
    println!("cargo:rerun-if-changed=echo.capnp");
    println!("cargo:rerun-if-changed=echo.proto");
    capnpc::CompilerCommand::new()
        .file("echo.capnp")
        .run()
        .unwrap();
    tonic_prost_build::compile_protos("echo.proto").unwrap();
}
