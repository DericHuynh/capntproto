fn main() {
    println!("cargo:rerun-if-changed=legacy.capnp");
    let mut command = capnpc::CompilerCommand::new();
    if let Some(path) = std::env::var_os("CAPNP_INCLUDE_DIR") {
        command.import_path(path);
    }
    command.file("legacy.capnp").run().unwrap();
}
