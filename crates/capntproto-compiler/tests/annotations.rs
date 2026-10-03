use capnp::{
    schema_capnp::{code_generator_request, node},
    schema_loader::{dynamic::Value, SchemaLoader},
};
use capntproto_compiler::{compile, SchemaParser};

const SAMPLE: &str = include_str!("../examples/annotations.capnp");

fn find<'a>(request: code_generator_request::Reader<'a>, name: &str) -> node::Reader<'a> {
    request
        .get_nodes()
        .unwrap()
        .iter()
        .find(|n| n.get_display_name().unwrap() == name)
        .unwrap()
}

#[test]
fn annotations_load_with_declarations_types_targets_and_ordered_values() {
    let message = compile("annotations.capnp", SAMPLE).unwrap();
    let request = message
        .get_root_as_reader::<code_generator_request::Reader<'_>>()
        .unwrap();
    let mut loader = SchemaLoader::default();
    loader.load_request(request).unwrap();
    let schema = |name: &str| loader.get(find(request, name).get_id()).unwrap();
    let label = find(request, "annotations.capnp:label");
    assert_eq!(label.get_id(), 0xf1a0235c49bd786e);
    let node::Annotation(label) = label.which().unwrap() else {
        panic!()
    };
    assert!([
        label.get_targets_file(),
        label.get_targets_const(),
        label.get_targets_enum(),
        label.get_targets_enumerant(),
        label.get_targets_struct(),
        label.get_targets_field(),
        label.get_targets_union(),
        label.get_targets_group(),
        label.get_targets_interface(),
        label.get_targets_method(),
        label.get_targets_param(),
        label.get_targets_annotation()
    ]
    .into_iter()
    .all(|v| v));
    let node::Annotation(marker) = find(request, "annotations.capnp:marker").which().unwrap()
    else {
        panic!()
    };
    assert!(marker.get_targets_group() && marker.get_targets_union() && marker.get_targets_field());
    assert!(
        !marker.get_targets_struct() && !marker.get_targets_enum() && !marker.get_targets_const()
    );
    let annotations = schema("annotations.capnp").annotations().unwrap();
    assert_eq!(annotations.len(), 2);
    assert!(
        matches!(annotations.get(0).unwrap().get_value().unwrap(), Value::Text(t) if t == "annotation example")
    );
    assert!(matches!(
        annotations.get(1).unwrap().get_value().unwrap(),
        Value::Void
    ));
    let message = schema("annotations.capnp:Message");
    let annotations = message.annotations().unwrap();
    assert_eq!(annotations.len(), 3);
    for (i, expected) in ["first", "second"].into_iter().enumerate() {
        assert!(
            matches!(annotations.get(i as u32).unwrap().get_value().unwrap(), Value::Text(t) if t == expected)
        );
    }
    let Value::Struct(rule) = annotations.get(2).unwrap().get_value().unwrap() else {
        panic!()
    };
    assert!(matches!(
        rule.get_named("limit").unwrap(),
        Value::UInt32(12)
    ));
    let Value::List(tags) = rule.get_named("tags").unwrap() else {
        panic!()
    };
    assert_eq!(tags.len(), 2);
    assert!(matches!(tags.get(1).unwrap(), Value::Text(t) if t == "portable"));
    let payload = message.field("payload").unwrap().annotations().unwrap();
    assert!(matches!(
        payload.get(0).unwrap().get_value().unwrap(),
        Value::Void
    ));
    assert!(
        matches!(payload.get(1).unwrap().get_value().unwrap(), Value::Text(t) if t == "wire bytes")
    );
    for field in ["metadata", "choice"] {
        assert_eq!(
            message
                .field(field)
                .unwrap()
                .annotations()
                .unwrap()
                .get(0)
                .unwrap()
                .id(),
            0xc735a02e1498bf6d
        );
        // Group annotations belong to the containing field, not the auxiliary node.
        assert!(schema(&format!("annotations.capnp:Message.{field}"))
            .annotations()
            .unwrap()
            .is_empty());
    }
    let state = schema("annotations.capnp:State");
    assert!(
        matches!(state.enumerant("ready").unwrap().annotations().unwrap().get(0).unwrap().get_value().unwrap(), Value::Text(t) if t == "running")
    );
    assert!(
        matches!(schema("annotations.capnp:bytes").annotations().unwrap().get(0).unwrap().get_value().unwrap(), Value::Text(t) if t == "signature")
    );
}

#[test]
fn annotation_imports_keep_declarations_and_inline_value_only_dependencies() {
    let mut parser = SchemaParser::new();
    parser
        .add_source(
            "main.capnp",
            "@0xaaaaaaaaaaaaaaaa; $import \"tags.capnp\".label(import \"values.capnp\".text);",
        )
        .unwrap();
    parser.add_source("tags.capnp", "@0xbbbbbbbbbbbbbbbb; annotation label(file) :Text; annotation unused(file) :import \"missing.capnp\".Type;").unwrap();
    parser
        .add_source(
            "values.capnp",
            "@0xcccccccccccccccc; const text :Text = \"imported\";",
        )
        .unwrap();
    let message = parser.parse(&["main.capnp"]).unwrap();
    let request = message
        .get_root_as_reader::<code_generator_request::Reader<'_>>()
        .unwrap();
    let names: Vec<_> = request
        .get_nodes()
        .unwrap()
        .iter()
        .map(|n| n.get_display_name().unwrap().to_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["main.capnp", "tags.capnp", "tags.capnp:label"]);
    let imports = request
        .get_requested_files()
        .unwrap()
        .get(0)
        .get_imports()
        .unwrap();
    assert_eq!(imports.len(), 1);
    assert_eq!(imports.get(0).get_name().unwrap(), "tags.capnp");
    let mut loader = SchemaLoader::default();
    loader.load_request(request).unwrap();
    let annotation = loader
        .get(0xaaaaaaaaaaaaaaaa)
        .unwrap()
        .annotations()
        .unwrap()
        .get(0)
        .unwrap();
    assert!(matches!(annotation.get_value().unwrap(), Value::Text(t) if t == "imported"));
    assert!(parser.parse(&["tags.capnp"]).is_err());
    assert!(parser.parse(&["main.capnp"]).is_ok());
}

#[test]
fn invalid_annotations_report_source_spans_including_transitive_cycles() {
    for (body, expected) in [
        (
            "annotation a(field,field) :Void;",
            "duplicate annotation target",
        ),
        ("annotation a(*,file) :Void;", "mixed wildcard"),
        ("annotation a(missing) :Void;", "invalid annotation target"),
        (
            "annotation a(field) :Void; $a;",
            "cannot be applied to file",
        ),
        ("annotation a(file) :Text; $a;", "requires a value"),
        ("annotation a(file) :UInt8; $a(256);", "out of range"),
        ("annotation a(annotation) :Void $a;", "cyclic annotation"),
        (
            "annotation a(annotation) :Void $b; annotation b(annotation) :Void $a;",
            "cyclic annotation",
        ),
        (
            "annotation a(const) :UInt32; const x :UInt32 = 1 $a(.x);",
            "cyclic constant",
        ),
        ("const x :UInt32 = 1; $x;", "not an annotation"),
        (
            "annotation a(file) :Void; struct S { x @0 :a; }",
            "annotation is not a field type",
        ),
    ] {
        let source = format!("@0xabcdefabcdefabcd;\n# 🦀\n{body}");
        let error = compile("bad.capnp", &source).err().expect(body);
        assert!(error.message.contains(expected), "{body}: {error}");
        assert_eq!(error.filename, "bad.capnp");
        assert!(error.line >= 3);
        assert!(source.is_char_boundary(error.start) && source.is_char_boundary(error.end));
    }
    let mut parser = SchemaParser::new();
    parser
        .add_source(
            "a.capnp",
            "@0xaaaaaaaaaaaaaaaa; annotation a(annotation) :Void $import \"b.capnp\".b;",
        )
        .unwrap();
    parser
        .add_source(
            "b.capnp",
            "@0xbbbbbbbbbbbbbbbb; annotation b(annotation) :Void $import \"a.capnp\".a;",
        )
        .unwrap();
    assert!(parser
        .parse(&["a.capnp"])
        .err()
        .unwrap()
        .message
        .contains("cyclic annotation"));
}

#[test]
fn annotation_recursion_and_repeated_payloads_are_bounded() {
    let mut source = String::from("@0xabcdefabcdefabcd;");
    for i in 0..80 {
        source.push_str(&format!("annotation a{i}(annotation) :Void $a{};", i + 1));
    }
    source.push_str("annotation a80(annotation) :Void;");
    assert!(compile("deep.capnp", &source)
        .err()
        .unwrap()
        .message
        .contains("nesting limit"));
    let source = format!(
        "@0xabcdefabcdefabcd; annotation a(file) :Text; const text :Text = \"{}\"; {}",
        "x".repeat(1024 * 1024),
        "$a(.text);".repeat(17)
    );
    assert!(compile("large.capnp", &source)
        .err()
        .unwrap()
        .message
        .contains("expanded value size limit"));
    let source = format!(
        "@0xabcdefabcdefabcd; annotation a(file) :Void; ${}a{};",
        "(".repeat(100),
        ")".repeat(100)
    );
    assert!(compile("syntax.capnp", &source)
        .err()
        .unwrap()
        .message
        .contains("nesting limit"));
}

#[test]
fn every_truncated_annotation_prefix_is_handled_without_panicking() {
    let source = "@0xabcdefabcdefabcd; annotation a(*) :List(Text); $(a([\"hé🦀\", \"text\"])); struct S $a([]) { x @0 :Bool $a([\"field\"]); }";
    for end in (0..=source.len()).filter(|&i| source.is_char_boundary(i)) {
        match compile("prefix.capnp", &source[..end]) {
            Ok(message) => SchemaLoader::default()
                .load_request(
                    message
                        .get_root_as_reader::<code_generator_request::Reader<'_>>()
                        .unwrap(),
                )
                .unwrap(),
            Err(error) => assert!(
                error.start <= error.end
                    && error.end <= end
                    && source.is_char_boundary(error.start)
                    && source.is_char_boundary(error.end)
            ),
        }
    }
}
