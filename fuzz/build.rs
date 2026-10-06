fn main() {
    // A fresh coverage run needs fresh build-script and code-generation counters.
    println!("cargo:rerun-if-env-changed=LLVM_PROFILE_FILE");
    println!("cargo:rerun-if-changed=schemas/lifecycle.capnp");
    capnpc::CompilerCommand::new()
        .src_prefix("schemas")
        .file("schemas/lifecycle.capnp")
        .run()
        .expect("compile stateful RPC fuzz fixture");
}
