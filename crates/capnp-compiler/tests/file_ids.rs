use capnp::{
    message,
    schema_loader::{
        dynamic::{Builder, Value},
        Limits,
    },
};
use capnp_compiler::{
    FileCompiler, ParseError, ParsedSchemas, SchemaParser, SourceCompiler, SourceFile,
    SourceProvider,
};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Read},
};

fn id(parsed: &ParsedSchemas, filename: &str) -> u64 {
    parsed.get_file(filename).unwrap().schema().id()
}

fn bytes(parsed: &ParsedSchemas) -> Vec<u8> {
    let mut message = message::Builder::new_default();
    message.set_root(parsed.request()).unwrap();
    capnp::serialize::write_message_to_words(&message)
}

#[test]
fn explicit_ids_are_required_by_default_and_after_reenabling_the_policy() {
    let mut parser = SchemaParser::default();
    parser
        .add_source("config.capnp", "struct Config {}")
        .unwrap();
    assert!(parser
        .parse(&["config.capnp"])
        .err()
        .unwrap()
        .message
        .contains("missing file ID"));
    assert!(parser.parse_session(&["config.capnp"]).is_err());
    parser.set_file_ids_required(false);
    assert!(parser.parse_schemas(&["config.capnp"]).is_ok());
    parser.set_file_ids_required(true);
    assert!(parser.parse_schemas(&["config.capnp"]).is_err());
    assert!(capnp_compiler::compile("config.capnp", "struct Config {}").is_err());
}

#[test]
fn generated_ids_are_fresh_and_explicit_declarations_keep_their_identity() {
    let mut parser = SchemaParser::new();
    parser.set_file_ids_required(false).add_source("config.capnp", "struct Config { value @0 :UInt32 = 41; } struct Fixed @0xcccccccccccccccc { struct Child {} }").unwrap();
    let mut roots = BTreeSet::new();
    let mut children = BTreeSet::new();
    let mut fixed_children = BTreeSet::new();
    for _ in 0..16 {
        let parsed = parser
            .parse_schemas(&["config.capnp", "./config.capnp"])
            .unwrap();
        assert_eq!(parsed.requested_files().len(), 1);
        let file = parsed.get_file("config.capnp").unwrap();
        let root = file.schema().id();
        assert_ne!(root >> 63, 0);
        assert!(roots.insert(root));
        let config = file.get_nested("Config").unwrap();
        assert!(children.insert(config.schema().id()));
        let fixed = file.get_nested("Fixed").unwrap();
        assert_eq!(fixed.schema().id(), 0xcccccccccccccccc);
        fixed_children.insert(fixed.get_nested("Child").unwrap().schema().id());
        let mut message = message::Builder::new_default();
        let value = Builder::init(message.init_root(), config.schema()).unwrap();
        assert!(matches!(
            value.as_reader().get_named("value").unwrap(),
            Value::UInt32(41)
        ));
    }
    assert_eq!(fixed_children.len(), 1);
    for text in [
        "@0xaaaaaaaaaaaaaaaa; struct S {}",
        "struct S {} @0xaaaaaaaaaaaaaaaa;",
    ] {
        let mut parser = SchemaParser::new();
        parser.add_source("explicit.capnp", text).unwrap();
        let required = parser.parse_schemas(&["explicit.capnp"]).unwrap();
        parser.set_file_ids_required(false);
        assert_eq!(
            bytes(&required),
            bytes(&parser.parse_schemas(&["explicit.capnp"]).unwrap())
        );
    }
}

#[test]
fn idless_empty_files_keep_zero_source_ranges_and_reject_invalid_explicit_ids() {
    let mut parser = SchemaParser::new();
    parser
        .set_file_ids_required(false)
        .add_source("empty.capnp", "# Just a comment.\n")
        .unwrap();
    let parsed = parser.parse_schemas(&["empty.capnp"]).unwrap();
    let file = parsed.get_file("empty.capnp").unwrap();
    assert_ne!(file.schema().id() >> 63, 0);
    assert_eq!(file.schema().get_proto().get_start_byte(), 0);
    assert_eq!(file.schema().get_proto().get_end_byte(), 0);
    assert_eq!(file.source_info().unwrap().get_start_byte(), 0);
    assert_eq!(file.source_info().unwrap().get_end_byte(), 0);
    for text in [
        "@0;",
        "@0x100;",
        "@0xaaaaaaaaaaaaaaaa; @0xbbbbbbbbbbbbbbbb;",
    ] {
        let mut parser = SchemaParser::new();
        parser
            .set_file_ids_required(false)
            .add_source("bad.capnp", text)
            .unwrap();
        assert!(parser.parse(&["bad.capnp"]).is_err());
    }
}

#[test]
fn cyclic_imports_share_ids_and_lazy_loading_preserves_existing_ids() {
    let mut parser = SchemaParser::new();
    parser.set_file_ids_required(false);
    parser
        .add_source(
            "main.capnp",
            "using I = import \"types.capnp\"; struct Root { item @0 :I.Item; }",
        )
        .unwrap();
    parser.add_source("types.capnp", "using M = import \"./main.capnp\"; struct Item { root @0 :M.Root; } struct Later { x @0 :UInt16; }").unwrap();
    let mut session = parser.parse_session(&["main.capnp"]).unwrap();
    let root_id = id(session.schemas(), "main.capnp");
    let types_id = session.get_nested(root_id, "I").unwrap().schema().id();
    assert_ne!(root_id, types_id);
    assert_eq!(
        session.get_nested(types_id, "M").unwrap().schema().id(),
        root_id
    );
    let item_id = session.get_nested(types_id, "Item").unwrap().schema().id();
    session.get_nested(types_id, "Later").unwrap();
    assert_eq!(id(session.schemas(), "main.capnp"), root_id);
    assert_eq!(
        session.get_nested(types_id, "Item").unwrap().schema().id(),
        item_id
    );
    let source = "using I = import \"types.capnp\"; struct Root { item @0 :I.Item; }";
    let mut strict_import = SchemaParser::new();
    strict_import
        .add_source("main.capnp", format!("@0xaaaaaaaaaaaaaaaa; {source}"))
        .unwrap();
    strict_import
        .add_source("types.capnp", "struct Item {}")
        .unwrap();
    assert!(strict_import
        .parse(&["main.capnp"])
        .err()
        .unwrap()
        .message
        .contains("missing file ID"));
}

#[test]
fn disk_sessions_capture_policy_and_failed_extensions_do_not_change_ids() {
    let directory = tempfile::tempdir().unwrap();
    let main = directory.path().join("main.capnp");
    fs::write(
        &main,
        "@0xaaaaaaaaaaaaaaaa; using I = import \"types.capnp\"; struct Root { item @0 :I.Item; }",
    )
    .unwrap();
    fs::write(
        directory.path().join("types.capnp"),
        "struct Item {} struct Later { dep @0 :import \"late.capnp\".Dep; }",
    )
    .unwrap();
    fs::write(
        directory.path().join("late.capnp"),
        "struct Dep { x @0 :Missing; }",
    )
    .unwrap();
    let mut compiler = FileCompiler::default();
    compiler.src_prefix(directory.path());
    assert!(compiler.parse_schemas(&[&main]).is_err());
    compiler.set_file_ids_required(false);
    let mut session = compiler.parse_session(&[&main]).unwrap();
    // The captured setting remains optional for an as-yet unread late file.
    compiler.set_file_ids_required(true);
    let types_id = session
        .get_nested(0xaaaaaaaaaaaaaaaa, "I")
        .unwrap()
        .schema()
        .id();
    let before = bytes(session.schemas());
    let dependencies = session.schemas().dependencies().to_vec();
    assert!(matches!(
        session.get_nested(types_id, "Later"),
        Err(ParseError::Compile(_))
    ));
    assert_eq!(bytes(session.schemas()), before);
    assert_eq!(session.schemas().dependencies(), dependencies);
    fs::remove_file(directory.path().join("types.capnp")).unwrap();
    fs::write(
        directory.path().join("late.capnp"),
        "struct Dep { x @0 :UInt16 = 8; }",
    )
    .unwrap();
    session.get_nested(types_id, "Later").unwrap();
    assert_eq!(
        session
            .get_nested(0xaaaaaaaaaaaaaaaa, "I")
            .unwrap()
            .schema()
            .id(),
        types_id
    );
    assert_eq!(id(session.schemas(), "main.capnp"), 0xaaaaaaaaaaaaaaaa);
    assert_eq!(session.schemas().dependencies().len(), 3);
}

struct Provider;
impl SourceProvider for Provider {
    fn resolve(&self, from: Option<&str>, path: &str) -> io::Result<Option<SourceFile>> {
        let key = match (from, path) {
            (None, "entry" | "alias") | (Some("types"), "cycle") => "main",
            (Some("main"), "types") => "types",
            _ => return Ok(None),
        };
        Ok(Some(SourceFile {
            identity: key.into(),
            filename: format!("{key}.capnp"),
        }))
    }
    fn open(&self, identity: &str) -> io::Result<Box<dyn Read + '_>> {
        let text = match identity {
            "main" => "using I = import \"types\"; struct Root { item @0 :I.Item; }",
            "types" => "using M = import \"cycle\"; struct Item { root @0 :M.Root; } struct Later { x @0 :Text; }",
            _ => return Err(io::ErrorKind::NotFound.into()),
        };
        Ok(Box::new(text.as_bytes()))
    }
}

#[test]
fn custom_provider_sessions_snapshot_policy_and_keep_ids_on_loader_rejection() {
    let provider = Provider;
    let mut compiler = SourceCompiler::new(&provider);
    assert!(compiler.parse_schemas(&["entry"]).is_err());
    assert!(compiler.parse_session(&["entry"]).is_err());
    compiler.set_file_ids_required(false);
    assert!(compiler.compile(&["entry"]).is_ok());
    let mut session = compiler
        .parse_session_with_limits(
            &["entry", "alias"],
            Limits {
                nodes: 4,
                ..Limits::default()
            },
        )
        .unwrap();
    compiler.set_file_ids_required(true);
    assert!(compiler.parse_schemas(&["entry"]).is_err());
    let root = id(session.schemas(), "main.capnp");
    let types = session.get_nested(root, "I").unwrap().schema().id();
    assert_eq!(session.get_nested(types, "M").unwrap().schema().id(), root);
    let before = bytes(session.schemas());
    for _ in 0..2 {
        assert!(matches!(
            session.get_nested(types, "Later"),
            Err(ParseError::Load(_))
        ));
        assert_eq!(bytes(session.schemas()), before);
    }
    assert_eq!(session.schemas().requested_files().len(), 1);
}

#[test]
fn command_line_compilation_continues_to_require_explicit_file_ids() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("config.capnp"), "struct Config {}").unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_capnp-compile"))
        .current_dir(directory.path())
        .arg("config.capnp")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("missing file ID"));
}
