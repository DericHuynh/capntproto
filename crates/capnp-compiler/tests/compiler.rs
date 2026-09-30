use capnp::{
    message,
    schema_capnp::{code_generator_request, node},
    schema_loader::SchemaLoader,
};
use capnp_compiler::compile;

const SAMPLE: &str = include_str!("../examples/message.capnp");

#[test]
fn request_loads_and_resolves_recursive_nested_types() {
    let message = compile("message.capnp", SAMPLE).unwrap();
    let request = message
        .get_root_as_reader::<code_generator_request::Reader<'_>>()
        .unwrap();
    let mut loader = SchemaLoader::default();
    loader.load_request(request).unwrap();
    let nodes = request.get_nodes().unwrap();
    assert_eq!(nodes.len(), 5);
    assert_eq!(
        request.get_requested_files().unwrap().get(0).get_id(),
        0xdba1c3d4e5f60718
    );
    let node::Struct(s) = nodes.get(1).which().unwrap() else {
        panic!()
    };
    assert_eq!((s.get_data_word_count(), s.get_pointer_count()), (2, 6));
    assert_eq!(s.get_fields().unwrap().len(), 9);
    assert!(nodes.get(2).get_id() >> 63 == 1);
    assert_eq!(nodes.get(4).get_id(), 0xef0123456789abcd);
}

#[test]
fn compilation_is_deterministic_and_layout_uses_ordinals() {
    let source =
        "@0xabcdefabcdefabcd; struct S { c @2 :UInt16; b @1 :UInt64; a @0 :Bool; d @3 :UInt8; }";
    let first = compile("test.capnp", source).unwrap();
    let second = compile("test.capnp", source).unwrap();
    assert_eq!(
        capnp::serialize::write_message_to_words(&first),
        capnp::serialize::write_message_to_words(&second)
    );
    let request = first
        .get_root_as_reader::<code_generator_request::Reader<'_>>()
        .unwrap();
    let node::Struct(s) = request.get_nodes().unwrap().get(1).which().unwrap() else {
        panic!()
    };
    assert_eq!((s.get_data_word_count(), s.get_pointer_count()), (2, 0));
    let fields = s.get_fields().unwrap();
    let expected = [("a", 2, 0), ("b", 1, 1), ("c", 0, 1), ("d", 3, 1)];
    for (i, (name, order, offset)) in expected.into_iter().enumerate() {
        let field = fields.get(i as u32);
        assert_eq!(field.get_name().unwrap(), name);
        assert_eq!(field.get_code_order(), order);
        let capnp::schema_capnp::field::Slot(slot) = field.which().unwrap() else {
            panic!()
        };
        assert_eq!(slot.get_offset(), offset);
    }
}

#[test]
fn invalid_and_unsupported_inputs_report_source_locations() {
    for (body, expected) in [
        ("struct S { a @1 :Bool; }", "contiguous"),
        ("struct S { a @0 :Bool; b @0 :Bool; }", "contiguous"),
        ("struct S { a @0 :Bool; a @1 :Bool; }", "duplicate member"),
        ("struct S {} struct S {}", "duplicate declaration"),
        ("struct S @0xabcdefabcdefabcd {}", "duplicate schema ID"),
        ("struct S @5 {}", "high bit"),
        ("struct S { a @65536 :Bool; }", "UInt16"),
        ("struct S { a @0 :Missing; }", "unknown type"),
        ("struct S { a @0 :S.Missing; }", "unknown nested type"),
        ("struct s {}", "uppercase"),
        ("struct S { Bad @0 :Bool; }", "lowercase"),
        ("struct Bad_Name {}", "underscores"),
        ("struct S { a @0 :UInt8 = 256; }", "out of range"),
        ("struct S { a @0 :Int8 = -129; }", "out of range"),
        ("struct S { a @0 :UInt64 = -1; }", "out of range"),
        (
            "struct S { a @0 :UInt64 = 18446744073709551616; }",
            "out of range",
        ),
        ("struct S { a @0 :UInt32 = 09; }", "invalid octal digit"),
        ("struct S { a @0 :Bool = 1; }", "field type"),
        ("struct S { a @0 :Text = true; }", "field type"),
        ("struct S { a @0 :Float64 = NaN; }", "unknown type"),
        (
            "struct S { a @0 :E = missing; } enum E { ok @0; }",
            "unknown type",
        ),
        ("struct S { a @0 :List(AnyPointer); }", "List(AnyPointer)"),
        ("using X = import \"x.capnp\";", "not found"),
        ("interface I { f @1 (); }", "ordinal"),
        ("const x :UInt32;", "expected"),
        ("annotation a(unknown) :Text;", "invalid annotation target"),
        (
            "struct S(T) {} struct X { f @0 :S(Text, Data); }",
            "generic",
        ),
        ("struct S { a @0 :S(Text); }", "generic"),
        ("struct S { union { a @0 :Void; } }", "at least two"),
        ("struct S { g :group {} }", "at least one"),
        ("struct S { a @0 :List(Bool) = [1]; }", "field type"),
        ("struct S { a @0 :S = (missing = 1); }", "no field named"),
        (
            "struct S { a @0 :Text = \"\\u1234\"; }",
            "unsupported string escape",
        ),
        ("struct S { a @0 :Text = \"", "unterminated"),
        (
            "struct S { a @0 :Text = \"line\nline\"; }",
            "unescaped newline",
        ),
    ] {
        let source = format!("@0xabcdefabcdefabcd;\n# Unicode comment 🦀\n{body}");
        let error = compile("bad.capnp", &source).err().expect(body);
        assert!(error.message.contains(expected), "{body}: {error}");
        assert!(error.line >= 3);
        assert_eq!(error.filename, "bad.capnp");
        assert!(source.is_char_boundary(error.start));
        assert!(source.is_char_boundary(error.end));
    }
    let error = compile("bad.capnp", "@0xabcdefabcdefabcd;\n  🦀")
        .err()
        .unwrap();
    assert_eq!(
        (error.line, error.column, error.end - error.start),
        (2, 3, 4)
    );
}

#[test]
fn input_limits_fail_without_recursing_unboundedly() {
    assert!(compile(
        "large.capnp",
        &" ".repeat(capnp_compiler::MAX_SOURCE_BYTES + 1)
    )
    .is_err());
    let nested = format!(
        "@0xabcdefabcdefabcd; struct S {{ f @0 :{}Bool{}; }}",
        "List(".repeat(100),
        ")".repeat(100)
    );
    assert!(compile("deep.capnp", &nested)
        .err()
        .unwrap()
        .message
        .contains("nesting limit"));
    let nested = format!(
        "@0xabcdefabcdefabcd; {}{}",
        "struct S {".repeat(100),
        "}".repeat(100)
    );
    assert!(compile("deep.capnp", &nested)
        .err()
        .unwrap()
        .message
        .contains("nesting limit"));
    let mut many = String::from("@0xabcdefabcdefabcd;");
    for i in 0..4096 {
        many.push_str(&format!("struct S{i} {{}}"));
    }
    assert!(compile("many.capnp", &many)
        .err()
        .unwrap()
        .message
        .contains("node limit"));
    let tokens = format!("@0xabcdefabcdefabcd;{}", ";".repeat(262_144));
    assert!(compile("many.capnp", &tokens)
        .err()
        .unwrap()
        .message
        .contains("token limit"));
    assert!(compile("", SAMPLE).is_err());
    assert!(compile("nul\0.capnp", SAMPLE).is_err());
    let mut expanded = format!("@0xabcdefabcdefabcd; struct {} {{", "N".repeat(32 * 1024));
    for i in 0..260 {
        expanded.push_str(&format!("struct S{i} {{}}"));
    }
    expanded.push('}');
    assert!(compile("expansion.capnp", &expanded)
        .err()
        .unwrap()
        .message
        .contains("display-name limit"));
}

#[test]
fn every_truncated_prefix_is_handled_and_successful_requests_validate() {
    let source = "@0xabcdefabcdefabcd; struct S { text @0 :Text = \"hé🦀\"; next @1 :S; }";
    for end in (0..=source.len()).filter(|&i| source.is_char_boundary(i)) {
        if let Ok(message) = compile("prefix.capnp", &source[..end]) {
            SchemaLoader::default()
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
fn cli_emits_a_framed_request_and_keeps_diagnostics_off_stdout() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_capnp-compile"))
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/message.capnp"
        ))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let decoded =
        capnp::serialize::read_message(output.stdout.as_slice(), message::ReaderOptions::new())
            .unwrap();
    assert_eq!(
        decoded
            .get_root::<code_generator_request::Reader<'_>>()
            .unwrap()
            .get_nodes()
            .unwrap()
            .len(),
        5
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_capnp-compile"))
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}

#[test]
fn union_groups_load_with_shared_sizes_and_stable_ids_when_renamed() {
    let source = include_str!("../examples/choices.capnp");
    let compile_nodes = |source: &str| {
        let message = compile("choices.capnp", source).unwrap();
        let request = message
            .get_root_as_reader::<code_generator_request::Reader<'_>>()
            .unwrap();
        SchemaLoader::default().load_request(request).unwrap();
        let mut ids = Vec::new();
        let mut size = None;
        let mut groups = 0;
        for node in request.get_nodes().unwrap() {
            ids.push(node.get_id());
            if let node::Struct(s) = node.which().unwrap() {
                let current = (s.get_data_word_count(), s.get_pointer_count());
                if let Some(size) = size {
                    assert_eq!(current, size);
                } else {
                    size = Some(current);
                }
                if s.get_is_group() {
                    groups += 1;
                }
                if node.get_display_name().unwrap() == "choices.capnp:Message" {
                    assert_eq!(s.get_discriminant_count(), 4);
                    let fields = s.get_fields().unwrap();
                    assert_eq!(fields.len(), 6);
                    assert_eq!(fields.get(0).get_discriminant_value(), u16::MAX);
                    assert_eq!(fields.get(2).get_discriminant_value(), 0);
                    assert_eq!(fields.get(5).get_discriminant_value(), 3);
                }
            }
        }
        assert_eq!(groups, 4);
        ids
    };
    assert_eq!(
        compile_nodes(source),
        compile_nodes(&source.replace("reading", "sample"))
    );
}

#[test]
fn groups_unions_and_layout_work_are_bounded() {
    for source in [
        format!(
            "@0xabcdefabcdefabcd; struct S {{ {}x @0 :Void;{} }}",
            "g :group {".repeat(100),
            "}".repeat(100)
        ),
        format!(
            "@0xabcdefabcdefabcd; struct S {{ {}{} }}",
            "g :union { a @0 :Void; ".repeat(100),
            "}".repeat(100)
        ),
    ] {
        assert!(compile("deep.capnp", &source)
            .err()
            .unwrap()
            .message
            .contains("nesting limit"));
    }
    let mut source = String::from("@0xabcdefabcdefabcd; struct S { union { g :group {");
    for i in 0..1600 {
        source.push_str(&format!("f{i} @{i} :UInt64;"));
    }
    source.push_str("} other @1600 :Void; } }");
    assert!(compile("work.capnp", &source)
        .err()
        .unwrap()
        .message
        .contains("layout work limit"));
    let source = include_str!("../examples/choices.capnp");
    for end in 0..=source.len() {
        if let Ok(message) = compile("prefix.capnp", &source[..end]) {
            SchemaLoader::default()
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
fn historical_nested_union_layout_failure_is_a_source_diagnostic() {
    let source = "@0xabcdefabcdefabcd; struct S { union { a :group { union { x :group {f0 @8 :Bool;f1 @3 :Bool;f2 @6 :Text;} y :group {f3 @11 :UInt32;f4 @10 :UInt16;f5 @7 :Text;} } f6 @0 :UInt16;f7 @1 :Bool;} b :group {f8 @5 :UInt16;f9 @2 :Void;f10 @4 :Void;f11 @9 :UInt64;} } }";
    let error = compile("historical.capnp", source).err().unwrap();
    assert!(error.message.contains("issue #344"), "{error}");
    assert_eq!(error.filename, "historical.capnp");
    assert!(!source[error.start..error.end].is_empty());
}
