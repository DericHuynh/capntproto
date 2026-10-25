fn main() {
    // The persistent owned API is absent in the allocator-free profile.
    if std::env::var_os("CARGO_FEATURE_ALLOC").is_none() {
        return;
    }
    // A fresh coverage run needs fresh build-script and code-generation counters.
    println!("cargo:rerun-if-env-changed=LLVM_PROFILE_FILE");
    println!("cargo:rerun-if-env-changed=CAPNP_INCLUDE_DIR");
    println!("cargo:rerun-if-changed=schema/persistent.capnp");
    let mut command = capnpc::CompilerCommand::new();
    if let Some(directory) = std::env::var_os("CAPNP_INCLUDE_DIR") {
        command.import_path(directory);
    }
    command
        .src_prefix("schema")
        .import_path("schema")
        .file("schema/persistent.capnp")
        .run()
        .expect("compile pinned persistent capability schema");
    // capnpc maps both Persistent and the persistent annotation to the same
    // Rust module. Rename only the generated annotation module, preserving
    // the authoritative schema and both wire IDs.
    let path =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("persistent_capnp.rs");
    let code = std::fs::read_to_string(&path).unwrap();
    let old = "\npub mod persistent {\n    pub const ID:";
    assert_eq!(code.matches(old).count(), 1);
    std::fs::write(
        path,
        code.replace(old, "\npub mod persistent_annotation {\n    pub const ID:"),
    )
    .unwrap();
}
