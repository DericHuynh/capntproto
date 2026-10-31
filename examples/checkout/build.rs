fn main() {
    capnpc::CompilerCommand::new()
        .src_prefix("schema")
        .file("schema/checkout.capnp")
        .capnp_root("::capntproto::capnp")
        .run_native()
        .expect("compile checkout schema");
}
