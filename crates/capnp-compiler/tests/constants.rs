use capnp::schema_capnp::{code_generator_request, field, node, value};
use capnp_compiler::{compile, SchemaParser};

const SAMPLE: &str = include_str!("../examples/constants.capnp");

fn find<'a>(request: code_generator_request::Reader<'a>, name: &str) -> node::Reader<'a> {
    request
        .get_nodes()
        .unwrap()
        .iter()
        .find(|n| n.get_display_name().unwrap() == name)
        .unwrap()
}

fn data(value: value::Reader<'_>) -> &[u8] {
    let value::Data(data) = value.which().unwrap() else {
        panic!()
    };
    data.unwrap()
}

#[test]
fn constants_emit_typed_values_and_defaults_preserve_rounding_and_bytes() {
    let message = compile("constants.capnp", SAMPLE).unwrap();
    let request = message
        .get_root_as_reader::<code_generator_request::Reader<'_>>()
        .unwrap();
    capnp::schema_loader::SchemaLoader::default()
        .load_request(request)
        .unwrap();
    let node::Const(value) = find(request, "constants.capnp:signature").which().unwrap() else {
        panic!()
    };
    assert_eq!(data(value.get_value().unwrap()), [0, 255, 126, 128]);
    let node::Const(value) = find(request, "constants.capnp:preferred").which().unwrap() else {
        panic!()
    };
    assert!(matches!(
        value.get_value().unwrap().which().unwrap(),
        value::Enum(1)
    ));
    assert_eq!(
        find(request, "constants.capnp:Settings.maxCount").get_id(),
        0xd45c17e9ac3042b8
    );
    let node::Struct(settings) = find(request, "constants.capnp:Settings").which().unwrap() else {
        panic!()
    };
    let defaults: Vec<_> = settings
        .get_fields()
        .unwrap()
        .iter()
        .map(|f| {
            let field::Slot(slot) = f.which().unwrap() else {
                panic!()
            };
            assert!(slot.get_had_explicit_default());
            slot.get_default_value().unwrap()
        })
        .collect();
    assert!(matches!(defaults[0].which().unwrap(), value::Uint64(64)));
    let value::Text(text) = defaults[1].which().unwrap() else {
        panic!()
    };
    assert_eq!(text.unwrap(), "Rust frontend");
    assert_eq!(data(defaults[2]), [0, 255, 126, 128]);
    assert!(matches!(defaults[3].which().unwrap(), value::Float64(n) if n == f64::from(0.1f32)));
    assert!(matches!(defaults[4].which().unwrap(), value::Enum(1)));
}

#[test]
fn constant_imports_inline_values_without_selecting_unused_declarations() {
    let mut parser = SchemaParser::new();
    parser.add_source("main.capnp", "@0xabcdefabcdefabcd; const x :Data = import \"mid.capnp\".blob; struct S { x @0 :Data = .x; }").unwrap();
    parser.add_source("mid.capnp", "@0xbbbbbbbbbbbbbbbb; const blob :Data = import \"leaf.capnp\".blob; const unused :UInt32 = import \"missing.capnp\".bad;").unwrap();
    parser
        .add_source(
            "leaf.capnp",
            "@0xcccccccccccccccc; const blob :Data = 0x\"00FF\";",
        )
        .unwrap();
    let message = parser.parse(&["main.capnp"]).unwrap();
    let request = message
        .get_root_as_reader::<code_generator_request::Reader<'_>>()
        .unwrap();
    assert_eq!(request.get_nodes().unwrap().len(), 3);
    let node::Const(constant) = find(request, "main.capnp:x").which().unwrap() else {
        panic!()
    };
    assert_eq!(data(constant.get_value().unwrap()), [0, 255]);
    assert_eq!(
        request
            .get_requested_files()
            .unwrap()
            .get(0)
            .get_imports()
            .unwrap()
            .len(),
        0
    );
    assert!(parser
        .parse(&["mid.capnp"])
        .err()
        .unwrap()
        .message
        .contains("not found"));
    assert!(parser.parse(&["main.capnp"]).is_ok());
}

#[test]
fn cycles_aliases_types_and_integer_overflow_produce_source_diagnostics() {
    for (body, expected) in [
        ("const x :UInt32 = .x;", "cyclic constant"),
        (
            "const x :UInt32 = .y; const y :UInt32 = .x;",
            "cyclic constant",
        ),
        ("const x :UInt32 = 1; const y :UInt32 = x;", "qualified"),
        (
            "const x :UInt32 = 1; using y = .x; const z :y = 2;",
            "not a field type",
        ),
        ("const x :UInt64 = 18446744073709551616;", "out of range"),
        ("const x :UInt64 = 0x10000000000000000;", "out of range"),
        ("const x :Float64 = -9223372036854775809;", "out of range"),
        ("const x :Float64 = 1.0; const y :UInt8 = .x;", "field type"),
        ("const x :Data = 0x\"a\";", "pairs of hexadecimal"),
        ("const x :Data = 0x\"é\";", "pairs of hexadecimal"),
        ("const x :Text = \"a\"; const y :Data = .x;", "field type"),
        ("using x = UInt32;", "uppercase"),
        ("const x :UInt32 = 1; using X = .x;", "lowercase"),
        (
            "struct S { const x :UInt32 = 1; x @0 :Bool; }",
            "duplicate declaration",
        ),
        ("const x :List(UInt8) = [256];", "out of range"),
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
            "@0xaaaaaaaaaaaaaaaa; const a :UInt32 = import \"b.capnp\".b;",
        )
        .unwrap();
    parser
        .add_source(
            "b.capnp",
            "@0xbbbbbbbbbbbbbbbb; const b :UInt32 = import \"a.capnp\".a;",
        )
        .unwrap();
    assert!(parser
        .parse(&["a.capnp"])
        .err()
        .unwrap()
        .message
        .contains("cyclic constant"));
}

#[test]
fn constant_chains_expansion_and_truncated_literals_are_bounded() {
    let mut chain = String::from("@0xabcdefabcdefabcd;");
    for i in 0..100 {
        chain.push_str(&format!("const c{i} :UInt32 = .c{};", i + 1));
    }
    chain.push_str("const c100 :UInt32 = 1;");
    assert!(compile("chain.capnp", &chain)
        .err()
        .unwrap()
        .message
        .contains("nesting limit"));
    let mut expansion = format!(
        "@0xabcdefabcdefabcd; const large :Text = \"{}\";",
        "a".repeat(1024 * 1024)
    );
    for i in 0..17 {
        expansion.push_str(&format!("const c{i} :Text = .large;"));
    }
    assert!(compile("large.capnp", &expansion)
        .err()
        .unwrap()
        .message
        .contains("expanded value size limit"));
    let source = "@0xabcdefabcdefabcd; const text :Text = \"hé🦀\" \"next\"; const bytes :Data = 0x\"01 ff\"; struct S { x @0 :Text = .text; }";
    for end in (0..=source.len()).filter(|&n| source.is_char_boundary(n)) {
        if let Ok(message) = compile("prefix.capnp", &source[..end]) {
            capnp::schema_loader::SchemaLoader::default()
                .load_request(
                    message
                        .get_root_as_reader::<code_generator_request::Reader<'_>>()
                        .unwrap(),
                )
                .unwrap();
        }
    }
}

#[test]
fn empty_explicit_data_and_text_defaults_are_distinct_from_null() {
    let message = compile("empty.capnp", "@0xabcdefabcdefabcd; struct S { a @0 :Data; b @1 :Data = \"\"; c @2 :Text; d @3 :Text = \"\"; }").unwrap();
    let request = message
        .get_root_as_reader::<code_generator_request::Reader<'_>>()
        .unwrap();
    let node::Struct(s) = find(request, "empty.capnp:S").which().unwrap() else {
        panic!()
    };
    for (i, field) in s.get_fields().unwrap().iter().enumerate() {
        let field::Slot(slot) = field.which().unwrap() else {
            panic!()
        };
        let value = slot.get_default_value().unwrap();
        let present = match value.which().unwrap() {
            value::Data(_) => value.has_data(),
            value::Text(_) => value.has_text(),
            _ => panic!(),
        };
        assert_eq!(present, i % 2 == 1);
        assert_eq!(slot.get_had_explicit_default(), i % 2 == 1);
    }
}
