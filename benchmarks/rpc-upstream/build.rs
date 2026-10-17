fn main() {
    println!("cargo:rustc-check-cfg=cfg(capntproto_upstream_api)");
    println!("cargo:rustc-cfg=capntproto_upstream_api");
    println!("cargo:rerun-if-changed=../rpc/echo.capnp");
    capnpc::CompilerCommand::new()
        .src_prefix("../rpc")
        .file("../rpc/echo.capnp")
        .run()
        .expect("compile shared benchmark schema with upstream capnpc");
}
