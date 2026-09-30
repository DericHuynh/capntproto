use capnp::{
    message,
    schema_loader::{
        dynamic::{self, Value},
        Kind, Limits, Type,
    },
};
use capnp_compiler::{FileCompiler, ParseError, ParsedSchemas, SchemaParser};

fn parser() -> SchemaParser {
    // Keep this fixture stable under Git's Windows checkout conversion. Lexer
    // tests separately exercise preservation of actual CRLF source comments.
    let mut parser = SchemaParser::new();
    parser
        .add_source(
            "reflection.capnp",
            include_str!("../examples/reflection.capnp").replace("\r\n", "\n"),
        )
        .unwrap();
    parser
        .add_source(
            "reflection-common.capnp",
            include_str!("../examples/reflection-common.capnp").replace("\r\n", "\n"),
        )
        .unwrap();
    parser
        .add_file(
            "reflection.txt",
            include_str!("../examples/reflection.txt")
                .replace("\r\n", "\n")
                .into_bytes(),
        )
        .unwrap();
    parser
}

fn schemas() -> ParsedSchemas {
    parser()
        .parse_schemas(&["reflection.capnp", "reflection-common.capnp"])
        .unwrap()
}

#[test]
fn snapshot_owns_declarations_and_metadata_after_sources_are_dropped() {
    let parsed = schemas();
    assert!(parsed.dependencies().is_empty());
    assert_eq!(
        parsed
            .requested_files()
            .map(|(name, _)| name)
            .collect::<Vec<_>>(),
        ["reflection.capnp", "reflection-common.capnp"]
    );
    let file = parsed.get_file("reflection.capnp").unwrap();
    assert_eq!(file.schema().kind(), Kind::File);
    assert_eq!(
        file.source_info().unwrap().get_doc_comment().unwrap(),
        "Runtime reflection example.\n"
    );
    let children = file.get_all_nested().unwrap();
    assert_eq!(
        children
            .iter()
            .map(|child| child
                .schema()
                .short_display_name()
                .unwrap()
                .to_string()
                .unwrap())
            .collect::<Vec<_>>(),
        ["note", "limit", "Record", "Service"]
    );
    let record = file.get_nested("Record").unwrap();
    assert_eq!(
        record.source_info().unwrap().get_doc_comment().unwrap(),
        "Record documentation 🦀.\n"
    );
    for missing in ["", "record", "Record.Child", "Alias", "Missing", "Record\0"] {
        assert!(file.find_nested(missing).unwrap().is_none(), "{missing:?}");
        assert!(file.get_nested(missing).is_err());
    }
    for missing in ["ChildAlias", "count", "details", "fetch"] {
        assert!(record.find_nested(missing).unwrap().is_none());
    }
    assert_eq!(
        record.get_nested("Child").unwrap().schema().kind(),
        Kind::Struct
    );
    assert_eq!(
        record.get_nested("Status").unwrap().schema().kind(),
        Kind::Enum
    );
    assert!(parsed.get_file("./reflection.capnp").is_err());
    assert!(parsed.get(0).is_err());
    assert!(parsed.source_info(0).is_none());
    for declaration in parsed.get_all_loaded() {
        assert!(!declaration.schema().is_stub());
        assert_eq!(
            declaration.source_info().unwrap().get_id(),
            declaration.schema().id()
        );
    }
}

#[test]
fn parsed_brands_groups_and_methods_support_dynamic_wire_messages() {
    let parsed = schemas();
    let file = parsed.get_file("reflection.capnp").unwrap();
    let record = file.get_nested("Record").unwrap().schema();
    let Type::Struct(box_schema) = record.field("box").unwrap().get_type().unwrap() else {
        panic!()
    };
    assert!(matches!(
        box_schema.field("value").unwrap().get_type().unwrap(),
        Type::Text
    ));
    let Type::Struct(group) = record.field("details").unwrap().get_type().unwrap() else {
        panic!()
    };
    assert_eq!(
        parsed
            .source_info(group.id())
            .unwrap()
            .get_doc_comment()
            .unwrap(),
        "Inline details.\n"
    );
    let method = file
        .get_nested("Service")
        .unwrap()
        .schema()
        .method("fetch")
        .unwrap();
    for implicit in [method.params().unwrap(), method.results().unwrap()] {
        assert_eq!(
            parsed
                .get(implicit.id())
                .unwrap()
                .source_info()
                .unwrap()
                .get_members()
                .unwrap()
                .len(),
            1
        );
    }
    let mut message = message::Builder::new_default();
    let mut value = dynamic::Builder::init(message.init_root(), record.clone()).unwrap();
    assert!(matches!(
        value.as_reader().get_named("count").unwrap(),
        Value::UInt32(17)
    ));
    assert!(
        matches!(value.as_reader().get_named("label").unwrap(), Value::Text(s) if s == "embedded label\n")
    );
    value.set_named("count", Value::UInt32(99)).unwrap();
    value
        .reborrow()
        .init_struct("box")
        .unwrap()
        .set_named("value", Value::Text("boxed".into()))
        .unwrap();
    value
        .reborrow()
        .init_struct("details")
        .unwrap()
        .set_named("active", Value::Bool(false))
        .unwrap();
    value
        .reborrow()
        .init_struct("child")
        .unwrap()
        .set_named("label", Value::Text("child".into()))
        .unwrap();
    let mut entries = value.reborrow().init_list("entries", 2).unwrap();
    entries
        .reborrow()
        .get_struct(1)
        .unwrap()
        .set_named("value", Value::Int16(123))
        .unwrap();
    let bytes = capnp::serialize::write_message_to_words(&message);
    let message =
        capnp::serialize::read_message(bytes.as_slice(), message::ReaderOptions::new()).unwrap();
    let value = dynamic::Reader::new(message.get_root().unwrap(), record).unwrap();
    assert!(matches!(
        value.get_named("count").unwrap(),
        Value::UInt32(99)
    ));
    let Value::Struct(boxed) = value.get_named("box").unwrap() else {
        panic!()
    };
    assert!(matches!(boxed.get_named("value").unwrap(), Value::Text(s) if s == "boxed"));
    assert_eq!(
        value
            .which()
            .unwrap()
            .unwrap()
            .get_proto()
            .get_name()
            .unwrap(),
        "child"
    );
    let Value::List(entries) = value.get_named("entries").unwrap() else {
        panic!()
    };
    for (i, expected) in [(0, -7), (1, 123)] {
        let Value::Struct(entry) = entries.get(i).unwrap() else {
            panic!()
        };
        assert!(matches!(entry.get_named("value").unwrap(), Value::Int16(v) if v == expected));
    }
}

#[test]
fn dependency_selection_is_explicit_and_lookup_never_silently_omits_children() {
    let parser = parser();
    let parsed = parser
        .parse_schemas(&["reflection.capnp", "./reflection.capnp"])
        .unwrap();
    assert_eq!(parsed.requested_files().len(), 1);
    assert!(parsed.get_file("reflection-common.capnp").is_err());
    let imported = parsed.get(0x9dba12d9d7a1c4e0).unwrap();
    assert!(imported.get_nested("Entry").is_ok());
    assert!(imported.find_nested("Missing").unwrap().is_none());
    assert!(imported.find_nested("Unused").is_err());
    assert!(imported.get_all_nested().is_err());
    let boxed = imported.get_nested("Box").unwrap();
    assert!(boxed.find_nested("Unused").is_err());
    // An explicit request includes all children without changing the old snapshot.
    let expanded = schemas();
    assert!(expanded
        .get_file("reflection-common.capnp")
        .unwrap()
        .get_nested("Unused")
        .is_ok());
    assert!(boxed.find_nested("Unused").is_err());
}

#[test]
fn loader_failures_and_compile_errors_are_distinct_and_do_not_poison_retries() {
    let parser = parser();
    let previous = parser.parse_schemas(&["reflection.capnp"]).unwrap();
    let error = parser
        .parse_schemas_with_limits(
            &["reflection.capnp"],
            Limits {
                nodes: 1,
                ..Limits::default()
            },
        )
        .err()
        .unwrap();
    assert!(matches!(error, ParseError::Load(_)));
    assert!(std::error::Error::source(&error).is_some());
    assert!(error.to_string().contains("runtime schema validation"));
    assert!(matches!(
        parser.parse_schemas(&["missing.capnp"]),
        Err(ParseError::Compile(_))
    ));
    assert!(previous
        .get_file("reflection.capnp")
        .unwrap()
        .get_nested("Record")
        .is_ok());
    assert!(parser.parse_schemas(&["reflection.capnp"]).is_ok());
    // Requests can be emitted for cyclic inheritance, but runtime validation
    // must reject them before exposing any parsed handles.
    let mut invalid = SchemaParser::new();
    invalid
        .add_source(
            "cycle.capnp",
            "@0xaaaaaaaaaaaaaaaa; interface A extends(B) {} interface B extends(A) {}",
        )
        .unwrap();
    assert!(invalid.parse(&["cycle.capnp"]).is_ok());
    assert!(matches!(
        invalid.parse_schemas(&["cycle.capnp"]),
        Err(ParseError::Load(_))
    ));
}

#[test]
fn disk_snapshots_keep_input_tracking_and_survive_deletion_and_recompilation() {
    let directory = tempfile::tempdir().unwrap();
    let schema = directory.path().join("main.capnp");
    let embed = directory.path().join("label.txt");
    std::fs::write(
        &schema,
        "@0xaaaaaaaaaaaaaaaa; struct S { value @0 :Text = embed \"label.txt\"; }",
    )
    .unwrap();
    std::fs::write(&embed, "old").unwrap();
    let mut compiler = FileCompiler::new();
    compiler.src_prefix(directory.path());
    let old = compiler.parse_schemas(&[&schema]).unwrap();
    assert!(old.dependencies().contains(&schema.canonicalize().unwrap()));
    assert!(old.dependencies().contains(&embed.canonicalize().unwrap()));
    std::fs::write(&embed, "new").unwrap();
    let new = compiler.parse_schemas(&[&schema]).unwrap();
    drop(directory);
    for (parsed, expected) in [(&old, "old"), (&new, "new")] {
        let schema = parsed
            .get_file("main.capnp")
            .unwrap()
            .get_nested("S")
            .unwrap()
            .schema();
        let mut message = message::Builder::new_default();
        let value = dynamic::Builder::init(message.init_root(), schema).unwrap();
        assert!(
            matches!(value.as_reader().get_named("value").unwrap(), Value::Text(s) if s == expected)
        );
    }
}
