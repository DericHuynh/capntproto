fn main() {
    println!("cargo:rerun-if-changed=../rpc/echo.capnp");
    capnpc::CompilerCommand::new()
        .src_prefix("../rpc")
        .file("../rpc/echo.capnp")
        .run()
        .expect("compile shared benchmark schema with upstream capnpc");
}
