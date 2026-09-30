#![allow(dead_code)]
use capnp::schema_loader::{Schema, SchemaLoader};
pub fn schemas() -> capnp_compiler::ParsedSchemas {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut compiler = capnp_compiler::FileCompiler::new();
    compiler
        .src_prefix(root.join("tests"))
        .import_path(root.join("schema"));
    compiler
        .parse_schemas(&[root.join("tests/compat.capnp")])
        .unwrap()
}
pub fn schema<'a>(schemas: &'a capnp_compiler::ParsedSchemas, name: &str) -> Schema<'a> {
    schemas
        .get_file("compat.capnp")
        .unwrap()
        .get_nested(name)
        .unwrap()
        .schema()
}
pub fn loader() -> std::rc::Rc<SchemaLoader> {
    let parsed = schemas();
    let mut loader = SchemaLoader::default();
    loader.load_request(parsed.request()).unwrap();
    std::rc::Rc::new(loader)
}
pub fn wire(
    message: &capnp::message::Builder<capnp::message::HeapAllocator>,
    schema: Schema<'_>,
) -> Vec<u8> {
    let reader =
        capnp::schema_loader::dynamic::Reader::new(message.get_root_as_reader().unwrap(), schema)
            .unwrap();
    capnp::Word::words_to_bytes(
        &capnp::any_struct::Reader::from_reader(reader)
            .canonicalize()
            .unwrap(),
    )
    .to_vec()
}
