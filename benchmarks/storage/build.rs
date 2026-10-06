fn main() {
    // A fresh coverage run needs fresh build-script and code-generation counters.
    println!("cargo:rerun-if-env-changed=LLVM_PROFILE_FILE");
    println!("cargo:rerun-if-changed=entry.capnp");
    capnpc::CompilerCommand::new()
        .file("entry.capnp")
        .field_api(true)
        .run()
        .expect("generate EAE integration fixture");
}
