fn main() {
    // A fresh coverage run needs fresh build-script and code-generation counters.
    println!("cargo:rerun-if-env-changed=LLVM_PROFILE_FILE");
    println!("cargo:rerun-if-changed=preview.capnp");
    capnpc::CompilerCommand::new()
        .file("preview.capnp")
        .field_api(true)
        .run()
        .expect("generate the preview field API");
}
