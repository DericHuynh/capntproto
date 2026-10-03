fn main() {
    println!("cargo:rerun-if-env-changed=CAPNP_INCLUDE_DIR");
    for schema in ["rpc", "rpc-twoparty", "persistent"] {
        println!("cargo:rerun-if-changed=schema/{schema}.capnp");
        let mut command = capnpc::CompilerCommand::new();
        if let Some(directory) = std::env::var_os("CAPNP_INCLUDE_DIR") {
            command.import_path(directory);
        }
        command
            .src_prefix("schema")
            .import_path("schema")
            .file(format!("schema/{schema}.capnp"))
            .run()
            .expect("compile pinned Cap'n Proto RPC schemas");
    }
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
